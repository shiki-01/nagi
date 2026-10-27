//! `nagi-live-bench`: ライブ変換の 1 打鍵あたりの所要時間の計測。
//!
//! Mozc の `evaluation.tsv`（`cargo xtask fetch-dict` で取得）の読みをつないで、目標の長さ（既定 30 / 60 / 90 / 120
//! 文字）の文を作り、ローマ字の打鍵列に戻して 1 打鍵ずつ `process_key` で打つ。打鍵のたびに、サーバーが
//! デバウンス後に呼ぶのと同じ `Engine::live_convert` を同期で呼び、その時間を入力中の読みの長さ別に集計する
//! （デバウンスで間引かれない最悪の場合。実際の速打ちでは呼ばれる回数は減る）。測るもの:
//!
//! 1. 打鍵（`process_key`。ひらがなの反映）の p50 / p95 / max
//! 2. 末尾に打ち足したときのライブ変換（`live_convert`）の p50 / p95 / max（読みの長さ別）
//! 3. 打ち終えた文の途中を編集したとき（← で 10 文字戻って 1 文字打つ）のライブ変換
//! 4. 同じ読みの 2 回目のライブ変換（推論の結果のキャッシュに当たるとき）
//!
//! ```text
//! cargo run --release -p nagi-server --features llama --bin nagi-live-bench -- --data-dir ../data \
//!     [--no-llm] [--profile balanced|quality|light|minimal|auto] [--sync-budget-ms N] [--sentences N]
//!     [--lengths 30,60,90,120] [--user-dir DIR]
//! ```
//!
//! `--profile` の既定は `balanced`（自動切替なし）。LLM が遅いと実行時の自動切替で軽いプロファイルへ落ち、
//! 以後は辞書だけで変換されて計測にならないため、固定する（`auto` で実運用の自動判定・自動切替のまま）。
//! `--sync-budget-ms` の既定は 10000（推論を待ち切って本当の所要時間を測る）。実運用の待ち時間で測るときは
//! 明示する（超えた分は辞書の結果を先に返し、推論は裏で続く）。`--user-dir` を省くと空の一時フォルダを使う
//! （利用者の学習データを読まない・汚さない）。データが無ければ skip して正常終了する。

use nagi_engine::{ConversionBackend, Engine, parse_key};
use nagi_fuzzy::ReverseRomaji;
use nagi_romaji::RomajiTable;
use nagi_server::setup::{ServerOptions, build, engine_config_from};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SESSION: &str = "bench";

fn main() {
    let mut options = ServerOptions {
        sync_budget_ms: Some(10_000),
        ..ServerOptions::default()
    };
    let mut sentences = 5usize;
    let mut lengths: Vec<usize> = vec![30, 60, 90, 120];
    let mut profile = "balanced".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => options.data_dir = args.next().map(PathBuf::from),
            "--no-llm" => options.use_llm = false,
            "--user-dir" => options.user_dir = args.next().map(PathBuf::from),
            "--sync-budget-ms" => {
                options.sync_budget_ms = args.next().and_then(|v| v.parse().ok());
            }
            "--profile" => profile = args.next().unwrap_or_default(),
            "--sentences" => sentences = args.next().and_then(|v| v.parse().ok()).unwrap_or(5),
            "--lengths" => {
                lengths = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
            }
            other => {
                eprintln!("不明な引数: {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(dir) = options.data_dir.clone() else {
        eprintln!("--data-dir が必要です");
        std::process::exit(2);
    };
    let tsv = dir.join("dict/mozc/evaluation.tsv");
    if !dir.join("dict/nagi-dict.bin").exists() || !tsv.exists() {
        eprintln!(
            "辞書または evaluation.tsv が無いので省略（cargo xtask fetch-dict / build-dict）"
        );
        return;
    }
    let temp_user_dir = options.user_dir.is_none().then(|| {
        let d = std::env::temp_dir().join(format!("nagi-live-bench-{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("一時フォルダを作れません");
        d
    });
    if let Some(d) = &temp_user_dir {
        options.user_dir = Some(d.clone());
    }
    // プロファイルの固定は、一時フォルダの設定ファイルで行う（利用者の設定ファイルは書き換えない）
    let config_file =
        std::env::temp_dir().join(format!("nagi-live-bench-{}.toml", std::process::id()));
    if profile != "auto" {
        std::fs::write(
            &config_file,
            format!("[profile]\nmode = \"{profile}\"\nauto_switch = false\n"),
        )
        .expect("設定ファイルを書けません");
        options.config_path = Some(config_file.clone());
    }

    let readings = load_readings(&tsv);
    let built = build(&options);
    for note in &built.notes {
        eprintln!("{note}");
    }
    let (config, _) = engine_config_from(&built.config);
    eprintln!(
        "backend={} profile={} chunk_len={} sync_budget_ms={:?}",
        built.pipeline.name(),
        built.runtime.profile().as_str(),
        config.composing_chunk_len,
        options.sync_budget_ms
    );
    let reverse = ReverseRomaji::new(&RomajiTable::standard());

    let mut keys_ms: Vec<f64> = Vec::new();
    // 読みの長さ（30 文字ごとの区間）→ 末尾に打ち足したときのライブ変換の時間
    let mut append: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    let mut edit: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    let mut repeat: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for &target in &lengths {
        for text in make_sentences(&readings, target, sentences) {
            let Some(romaji) = reverse.to_romaji(&text) else {
                continue;
            };
            // 文ごとに作り直す（左文脈・学習・キャッシュの持ち越しは、実際の入力に近い形で残す）
            let mut engine = Engine::new(built.pipeline.clone(), config.clone());
            for ch in romaji.chars() {
                let key = parse_key(&ch.to_string()).expect("打鍵");
                let started = Instant::now();
                engine.process_key(SESSION, &key);
                keys_ms.push(ms(started.elapsed()));
                if let Some(reading) = engine.composing_reading(SESSION) {
                    let started = Instant::now();
                    engine.live_convert(SESSION, &reading);
                    append
                        .entry(bucket(&reading))
                        .or_default()
                        .push(ms(started.elapsed()));
                }
            }
            // 同じ読みをもう一度（推論の結果のキャッシュに当たる）
            if let Some(reading) = engine.composing_reading(SESSION) {
                let started = Instant::now();
                engine.live_convert(SESSION, &reading);
                repeat
                    .entry(bucket(&reading))
                    .or_default()
                    .push(ms(started.elapsed()));
            }
            // 途中の編集: ← で 10 文字戻って 1 文字（`a`）打つ
            for _ in 0..10 {
                engine.process_key(SESSION, &parse_key("Left").expect("←"));
            }
            engine.process_key(SESSION, &parse_key("a").expect("a"));
            if let Some(reading) = engine.composing_reading(SESSION) {
                let started = Instant::now();
                engine.live_convert(SESSION, &reading);
                edit.entry(bucket(&reading))
                    .or_default()
                    .push(ms(started.elapsed()));
            }
            engine.process_key(SESSION, &parse_key("Escape").expect("Esc"));
            engine.process_key(SESSION, &parse_key("Escape").expect("Esc"));
        }
    }

    println!("打鍵（process_key）: {}", summary(&mut keys_ms));
    println!("ライブ変換（末尾に打ち足す。読みの長さ別）:");
    for (b, v) in &mut append {
        println!(
            "  {:>3}〜{:>3} 文字: {}",
            b * 30 + 1,
            (b + 1) * 30,
            summary(v)
        );
    }
    println!("ライブ変換（同じ読みの 2 回目）:");
    for (b, v) in &mut repeat {
        println!(
            "  {:>3}〜{:>3} 文字: {}",
            b * 30 + 1,
            (b + 1) * 30,
            summary(v)
        );
    }
    println!("ライブ変換（途中を編集。← 10 文字 + 1 文字）:");
    for (b, v) in &mut edit {
        println!(
            "  {:>3}〜{:>3} 文字: {}",
            b * 30 + 1,
            (b + 1) * 30,
            summary(v)
        );
    }
    println!(
        "終了時: profile={} LLM={}",
        built.runtime.profile().as_str(),
        built.runtime.effective().use_llm
    );
    if let Some(d) = temp_user_dir {
        let _ = std::fs::remove_dir_all(d);
    }
    let _ = std::fs::remove_file(config_file);
}

/// evaluation.tsv の変換の読み（重複なし、ひらがなだけのもの）
fn load_readings(tsv: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(tsv).expect("evaluation.tsv を読めません");
    let mut seen = std::collections::HashSet::new();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 5 && c[3].starts_with("Conversion")).then(|| c[1].to_string())
        })
        .filter(|r| {
            r.chars()
                .all(|c| ('\u{3041}'..='\u{3096}').contains(&c) || c == 'ー')
        })
        .filter(|r| seen.insert(r.clone()))
        .collect()
}

/// 読みを順につないで、`target` 文字以上になった文を `count` 個作る（決定的）
fn make_sentences(readings: &[String], target: usize, count: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    // 長さごとに出だしをずらす（同じ文の接頭辞ばかりにならないように）
    for r in readings.iter().cycle().skip(target).take(readings.len()) {
        current.push_str(r);
        if current.chars().count() >= target {
            out.push(current.chars().take(target).collect());
            current.clear();
            if out.len() == count {
                break;
            }
        }
    }
    out
}

fn bucket(reading: &str) -> usize {
    reading.chars().count().saturating_sub(1) / 30
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn summary(v: &mut [f64]) -> String {
    if v.is_empty() {
        return "（なし）".to_string();
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
    format!(
        "n={} p50={:.1}ms p95={:.1}ms max={:.1}ms",
        v.len(),
        at(0.5),
        at(0.95),
        v[v.len() - 1]
    )
}
