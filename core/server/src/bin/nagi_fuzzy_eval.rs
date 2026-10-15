//! `nagi-fuzzy-eval`: 曖昧入力（タイプミス補正の候補、M1）の評価。
//!
//! Mozc の `evaluation.tsv`（`cargo xtask fetch-dict` で取得）の読みを辞書の文節に分け、各文節を
//! ローマ字の打鍵列に戻してノイズ（隣接キー置換・脱落・挿入・転置を、1 打鍵ごとに確率 p）を注入し、
//! Composer と同じ規則で読みに戻したものを `FuzzyEngine::suggest` に渡す。測るもの:
//!
//! 1. 補正候補の上位 N 件（`--max-candidates`、既定 5）に正解の表記（ノイズ前の読みの最良変換）が入る率
//! 2. クリーンな入力（ノイズ無し）で補正候補が出てしまう率（誤訂正率。0 に近いほど良い）
//! 3. 提案の所要時間（展開 + 辞書変換）の p50 / p95
//!
//! ```text
//! cargo run --release -p nagi-server --bin nagi-fuzzy-eval -- --data-dir ../data \
//!     [--seed 1] [--strength normal] [--max-candidates 5] [--limit N] [--verbose]
//! ```
//!
//! 注意: ノイズは補正側と同じ隣接表から作るので、実際の打ち間違いに対する値より楽観的。
//! データ（辞書・evaluation.tsv）が無ければ skip して正常終了する。

use nagi_config::FuzzyStrength;
use nagi_dict::{Dictionary, Overlay};
use nagi_fuzzy::{Adjacency, EditKinds, ReverseRomaji};
use nagi_romaji::{Emit, RomajiConverter, RomajiTable};
use nagi_server::fuzzy::{FuzzyEngine, FuzzySettings};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 決定的な擬似乱数（SplitMix64）
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// 打鍵列にノイズを入れる。1 打鍵ごとに確率 `p` で、置換・脱落・挿入・転置のいずれかを注入する
fn add_noise(typed: &str, p: f64, adj: &Adjacency, rng: &mut Rng) -> String {
    let t: Vec<char> = typed.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < t.len() {
        let c = t[i];
        if !c.is_ascii_lowercase() || rng.unit() >= p {
            out.push(c);
            i += 1;
            continue;
        }
        match rng.below(4) {
            // 置換: 隣のキーを打つ
            0 => {
                let nb = adj.neighbors(c);
                if nb.is_empty() {
                    out.push(c);
                } else {
                    out.push(nb[rng.below(nb.len())].label);
                }
                i += 1;
            }
            // 脱落: 打ち忘れ
            1 => i += 1,
            // 挿入: 直前の文字の重複か、隣のキーが余計に入る
            2 => {
                out.push(c);
                let nb = adj.neighbors(c);
                if nb.is_empty() || rng.below(2) == 0 {
                    out.push(c);
                } else {
                    out.push(nb[rng.below(nb.len())].label);
                }
                i += 1;
            }
            // 転置
            _ => {
                if i + 1 < t.len() && t[i + 1] != c {
                    out.push(t[i + 1]);
                    out.push(c);
                    i += 2;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
        }
    }
    out
}

/// 打鍵列を Composer と同じ規則で読みにする（変換できずに残った英字はそのまま残す）
fn to_reading(typed: &str, table: &std::sync::Arc<RomajiTable>) -> String {
    let mut conv = RomajiConverter::new(table.clone());
    let mut out = String::new();
    let mut push = |emits: Vec<Emit>| {
        for e in emits {
            match e {
                Emit::Converted(s) => out.push_str(&s),
                Emit::Raw(c) => out.push(c),
            }
        }
    };
    for c in typed.chars() {
        push(conv.feed(c));
    }
    push(conv.flush());
    out
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx]
}

fn is_kana_reading(s: &str) -> bool {
    s.chars().all(|c| ('ぁ'..='ん').contains(&c) || c == 'ー')
}

fn main() {
    let mut data_dir: Option<PathBuf> = None;
    let mut seed = 1u64;
    let mut strength = FuzzyStrength::Normal;
    let mut max_candidates = 5usize;
    let mut limit = usize::MAX;
    let mut verbose = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--seed" => seed = args.next().and_then(|v| v.parse().ok()).unwrap_or(1),
            "--strength" => {
                strength = match args.next().as_deref() {
                    Some("gentle") => FuzzyStrength::Gentle,
                    Some("aggressive") => FuzzyStrength::Aggressive,
                    _ => FuzzyStrength::Normal,
                }
            }
            "--max-candidates" => {
                max_candidates = args.next().and_then(|v| v.parse().ok()).unwrap_or(5)
            }
            "--limit" => {
                limit = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(usize::MAX)
            }
            "--verbose" => verbose = true,
            other => {
                eprintln!("不明な引数: {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(dir) = data_dir else {
        eprintln!("--data-dir が必要です");
        std::process::exit(2);
    };
    let dict_path = dir.join("dict/nagi-dict.bin");
    let tsv_path = dir.join("dict/mozc/evaluation.tsv");
    let (Ok(dict), Ok(text)) = (
        Dictionary::load(&dict_path),
        std::fs::read_to_string(&tsv_path),
    ) else {
        eprintln!(
            "skip: {} / {} がありません（cargo xtask fetch-dict → build-dict）",
            dict_path.display(),
            tsv_path.display()
        );
        return;
    };

    // 評価の文節: 評価データの読みを辞書の Viterbi で文節に分けたもの（正解表記 = 最良変換）
    let overlay = Overlay::default();
    let mut segments: Vec<(String, String)> = Vec::new();
    let mut sentences: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 5 || !cols[3].starts_with("Conversion") {
            continue;
        }
        let reading = cols[1];
        if !is_kana_reading(reading) {
            continue;
        }
        sentences.push(reading.to_string());
        for s in dict.convert(reading) {
            let n = s.reading.chars().count();
            if (3..=24).contains(&n) && is_kana_reading(&s.reading) {
                segments.push((s.reading, s.surface));
            }
        }
    }
    segments.truncate(limit);
    sentences.truncate(limit);
    segments.dedup();
    eprintln!("文節 {} 件、文 {} 件", segments.len(), sentences.len());

    let table = RomajiTable::standard();
    let reverse = ReverseRomaji::new(&table);
    let adjacency = Adjacency::qwerty();
    let engine = FuzzyEngine::new();
    let settings = FuzzySettings {
        enabled: true,
        max_cost: strength.max_cost(),
        max_candidates,
        kinds: EditKinds::ALL,
    };
    let budget = Duration::from_millis(150);
    println!(
        "strength={} max_cost={} max_candidates={} seed={seed}",
        strength, settings.max_cost, max_candidates
    );

    // (2) クリーンな入力での誤訂正
    let mut clean_shown = 0usize;
    let mut clean_total = 0usize;
    let mut clean_multi_shown = 0usize;
    let mut clean_multi_total = 0usize;
    for (reading, _) in &segments {
        let s = engine.suggest(&dict, &overlay, reading, &settings, false, budget);
        clean_total += 1;
        let exact = !dict.lookup(reading).is_empty();
        if !exact {
            clean_multi_total += 1;
        }
        if !s.is_empty() {
            clean_shown += 1;
            if !exact {
                clean_multi_shown += 1;
            }
            if verbose {
                println!(
                    "CLEAN {reading} -> {:?}",
                    s.iter()
                        .map(|x| (&x.reading, &x.surface))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    println!(
        "クリーン文節: 補正候補が出た {clean_shown} / {clean_total} ({:.2}%)、辞書語そのものでない文節に限ると {clean_multi_shown} / {clean_multi_total} ({:.2}%)",
        pct(clean_shown, clean_total),
        pct(clean_multi_shown, clean_multi_total)
    );
    // 文全体（入力中の候補が読み全体に対して出す場合）
    let mut sent_shown = 0usize;
    let mut sent_total = 0usize;
    for reading in &sentences {
        if reading.chars().count() > 24 {
            continue;
        }
        sent_total += 1;
        if !engine
            .suggest(&dict, &overlay, reading, &settings, false, budget)
            .is_empty()
        {
            sent_shown += 1;
        }
    }
    println!(
        "クリーンな文（24 文字以下）: 補正候補が出た {sent_shown} / {sent_total} ({:.2}%)",
        pct(sent_shown, sent_total)
    );

    // (1)(3) ノイズ入り
    for p in [0.02, 0.05, 0.10] {
        let mut rng = Rng(seed ^ (p * 1000.0) as u64);
        let (mut noisy, mut hit1, mut hit_n, mut any, mut skipped) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        let mut latencies: Vec<f64> = Vec::new();
        let mut expand_latencies: Vec<f64> = Vec::new();
        for (reading, surface) in &segments {
            let Some(typed) = reverse.to_romaji(reading) else {
                continue;
            };
            let noisy_typed = add_noise(&typed, p, &adjacency, &mut rng);
            let noisy_reading = to_reading(&noisy_typed, &table);
            if noisy_reading == *reading {
                continue; // ノイズが入らなかった（または結果が同じ）
            }
            // 通常の変換でそのまま正解になっていれば、補正の対象外
            let direct = dict
                .segment_candidates_with(&noisy_reading, 1, Some(&overlay))
                .first()
                .map(|c| c.surface.clone());
            if direct.as_deref() == Some(surface.as_str()) {
                skipped += 1;
                continue;
            }
            noisy += 1;
            let started = Instant::now();
            let _ = engine.expand(&dict, &noisy_reading, &settings, budget);
            expand_latencies.push(started.elapsed().as_secs_f64() * 1000.0);
            let started = Instant::now();
            let s = engine.suggest(&dict, &overlay, &noisy_reading, &settings, false, budget);
            latencies.push(started.elapsed().as_secs_f64() * 1000.0);
            if !s.is_empty() {
                any += 1;
            }
            if s.first().is_some_and(|x| &x.surface == surface) {
                hit1 += 1;
            }
            if s.iter().any(|x| &x.surface == surface) {
                hit_n += 1;
            } else if verbose {
                println!(
                    "MISS p={p} {reading}({surface}) typed={noisy_typed} read={noisy_reading} -> {:?}",
                    s.iter()
                        .map(|x| (&x.reading, &x.surface))
                        .collect::<Vec<_>>()
                );
            }
        }
        latencies.sort_by(f64::total_cmp);
        expand_latencies.sort_by(f64::total_cmp);
        println!(
            "p={p:.2}: ノイズ入り {noisy} 件（通常変換で正解済み {skipped} 件を除く）: 正解が上位 {max_candidates} 件に {hit_n} ({:.1}%)、1 位に {hit1} ({:.1}%)、何か出た {any} ({:.1}%) / 提案全体 p50={:.2}ms p95={:.2}ms max={:.2}ms / 展開のみ p50={:.2}ms p95={:.2}ms",
            pct(hit_n, noisy),
            pct(hit1, noisy),
            pct(any, noisy),
            percentile(&latencies, 0.5),
            percentile(&latencies, 0.95),
            latencies.last().copied().unwrap_or(0.0),
            percentile(&expand_latencies, 0.5),
            percentile(&expand_latencies, 0.95),
        );
    }
}
