//! 曖昧入力の合成ノイズ評価（`nagi-eval --noise touch|typo`。設計: `docs/fuzzy-input-design.md` §10-3）。
//!
//! 評価データの読み（ひらがな）からローマ字の打鍵列を作り、ノイズを混ぜて `Engine` に入力する。
//! 補正なし（`fuzzy.enabled = false`）と補正あり（`true`）で、Space 変換の候補リストに期待表記が
//! 出るか・レイテンシ・追加仮説を走らせた割合を比べる。
//!
//! **QWERTY 配列・タップ誤差・子音の紛れはこの評価ツール内部の仮定**で、コア（`nagi-fuzzy` /
//! `nagi-engine`）はレイアウトを持たない。コアへはフロントと同じ形（`alternatives`: 文字と
//! キー幅で正規化した距離）で渡す。乱数は自前の xorshift で、同じ `seed` なら結果は再現する。

use nagi_engine::backend::{
    BackendCandidate, BackendSegment, BackendSentence, Completion, ReadingQuality, ScoredSentence,
};
use nagi_engine::{ConversionBackend, Engine, parse_key};
use nagi_fuzzy::{CONSONANT_CONFUSIONS, PrefixOracle, ReadingHypothesis};
use nagi_protocol::methods::InputAlternative;
use nagi_romaji::{Emit, RomajiConverter, RomajiTable};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

// ---- 乱数 ----

/// xorshift64* による擬似乱数（外部依存なし。暗号用途ではない）
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// `seed` から作る（0 は内部で別の定数に置き換える）
    pub fn new(seed: u64) -> Self {
        // splitmix64 で撹拌して、近い seed でも別の列にする
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng(if z == 0 { 0x2545_F491_4F6C_DD1D } else { z })
    }

    /// 次の 64 ビット
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `[0, 1)` の一様乱数
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `0..n` の一様乱数（`n = 0` は 0）
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// 標準正規分布（Box-Muller 法）
    pub fn gauss(&mut self) -> f64 {
        let u1 = self.next_f64().max(f64::MIN_POSITIVE);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

// ---- ローマ字の逆引き ----

/// かな → ローマ字（最短の綴り）の逆引き。標準テーブルの `entries` から作る
#[derive(Debug, Clone)]
pub struct Romanizer {
    table: Arc<RomajiTable>,
    /// かな（1〜3 文字）→ 最短の綴り
    best: HashMap<String, String>,
}

impl Romanizer {
    /// テーブルから逆引きを作る。綴りは「短い → c/q/l/x/v を含まない → 辞書順」で選ぶ
    pub fn new(table: Arc<RomajiTable>) -> Self {
        let mut best: HashMap<String, String> = HashMap::new();
        for (input, output, pending) in table.entries() {
            if !pending.is_empty() || output.is_empty() || !output.chars().all(is_hiragana) {
                continue;
            }
            let rank = |s: &str| (s.len(), s.chars().filter(|c| "cqlxv".contains(*c)).count());
            match best.get(output) {
                Some(cur) if (rank(cur), cur.as_str()) <= (rank(input), input) => {}
                _ => {
                    best.insert(output.to_string(), input.to_string());
                }
            }
        }
        // 単独の n はテーブルに無い（変換器が扱う）ので、ん は常に nn
        best.insert("ん".to_string(), "nn".to_string());
        Romanizer { table, best }
    }

    /// 読み（ひらがな）→ ローマ字の打鍵列。変換器で再生して読みに戻らなければ `None`
    pub fn romanize(&self, reading: &str) -> Option<String> {
        if reading.is_empty() || !reading.chars().all(is_hiragana) {
            return None;
        }
        // 促音は次の子音の重ね打ちが基本。戻らなければ xtu で打つ
        for double in [true, false] {
            if let Some(keys) = self.try_romanize(reading, double)
                && self.replay(&keys).as_deref() == Some(reading)
            {
                return Some(keys);
            }
        }
        None
    }

    fn try_romanize(&self, reading: &str, double_sokuon: bool) -> Option<String> {
        let chars: Vec<char> = reading.chars().collect();
        let mut keys = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == 'っ' {
                let next = (i + 1 < chars.len()).then(|| self.longest(&chars, i + 1));
                match next {
                    Some((_, k)) if double_sokuon => {
                        let c = k.chars().next()?;
                        if "aiueon".contains(c) {
                            keys.push_str("xtu");
                        } else {
                            keys.push(c);
                        }
                    }
                    _ => keys.push_str("xtu"),
                }
                i += 1;
                continue;
            }
            let (len, k) = self.longest(&chars, i);
            if len == 0 {
                return None;
            }
            keys.push_str(&k);
            i += len;
        }
        Some(keys)
    }

    /// `chars[i..]` の先頭から最長で逆引きできるかなの長さと綴り（見つからなければ長さ 0）
    fn longest(&self, chars: &[char], i: usize) -> (usize, String) {
        for len in (1..=3.min(chars.len() - i)).rev() {
            let s: String = chars[i..i + len].iter().collect();
            if let Some(k) = self.best.get(&s) {
                return (len, k.clone());
            }
        }
        (0, String::new())
    }

    /// 打鍵列を変換器に通した読み（Raw が出たら `None`）
    fn replay(&self, keys: &str) -> Option<String> {
        let mut conv = RomajiConverter::new(self.table.clone());
        let mut out = String::new();
        for c in keys.chars() {
            for e in conv.feed(c) {
                match e {
                    Emit::Converted(t) => out.push_str(&t),
                    Emit::Raw(_) => return None,
                }
            }
        }
        for e in conv.flush() {
            match e {
                Emit::Converted(t) => out.push_str(&t),
                Emit::Raw(_) => return None,
            }
        }
        Some(out)
    }
}

fn is_hiragana(c: char) -> bool {
    ('ぁ'..='ゖ').contains(&c)
}

// ---- ノイズ ----

/// ノイズの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseKind {
    /// タッチ位置誤差（隣接キーへの置換 + 距離付き代替）
    Touch,
    /// タイポ（入れ替え・欠落・余剰・子音の紛れ。代替なし）
    Typo,
}

impl NoiseKind {
    /// `--noise` の値から
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "touch" => Some(NoiseKind::Touch),
            "typo" => Some(NoiseKind::Typo),
            _ => None,
        }
    }
}

/// 打鍵 1 回（キーと、フロントが送る代替）
#[derive(Debug, Clone, PartialEq)]
pub struct TypedKey {
    /// 打たれたキー
    pub key: char,
    /// 代替 `(文字, キー幅で正規化した距離)`。打たれたキー自身の距離を含む。代替なしなら空
    pub alts: Vec<(char, f32)>,
}

impl TypedKey {
    fn plain(key: char) -> Self {
        TypedKey {
            key,
            alts: Vec::new(),
        }
    }
}

/// ノイズ生成の設定
#[derive(Debug, Clone, Copy)]
pub struct NoiseConfig {
    /// 種類
    pub kind: NoiseKind,
    /// 1 打鍵あたりのノイズ確率（0..=1）
    pub rate: f64,
    /// 1 入力に混ぜるノイズの最大数
    pub max_noise: usize,
    /// ノイズを混ぜる範囲（末尾からの打鍵数。0 なら全体）。補正の `window`（既定 8）より
    /// 手前のノイズは補正できないので、機能そのものの効果を見るには `window` 以下にする
    pub tail: usize,
    /// タップ位置の誤差の標準偏差（キー幅比。touch のみ）
    pub tap_sigma: f64,
    /// 代替として送る距離の上限（キー幅比。フロントの `max_distance` に相当）
    pub max_distance: f32,
    /// 代替として送る打たれたキー以外の最大数
    pub max_alts: usize,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        NoiseConfig {
            kind: NoiseKind::Touch,
            rate: 0.0,
            max_noise: 1,
            tail: 0,
            tap_sigma: 0.25,
            max_distance: 1.0,
            max_alts: 3,
        }
    }
}

/// 評価用 QWERTY の段（先頭キーの中心の x 座標（キー幅単位）と並び）
const QWERTY_ROWS: [(f64, &str); 3] = [(0.0, "qwertyuiop"), (0.25, "asdfghjkl"), (0.75, "zxcvbnm")];

fn qwerty_position(c: char) -> Option<(usize, usize)> {
    QWERTY_ROWS
        .iter()
        .enumerate()
        .find_map(|(r, (_, keys))| keys.find(c).map(|i| (r, i)))
}

fn key_center(row: usize, index: usize) -> f64 {
    QWERTY_ROWS[row].0 + index as f64
}

/// 打鍵列（小文字 ASCII）に、タップ位置の誤差を模したノイズを混ぜる。
///
/// 各打鍵は、確率 `rate`（かつ `max_noise` 未満）で「隣接キー（同じ段）を押した」ことにする:
/// タップ位置は狙ったキーの中心から `0.5 + |N(0, tap_sigma)|`（キー幅）だけ左右にずれる。
/// それ以外の打鍵は、`N(0, tap_sigma)`（±0.49 に制限）のずれで狙ったキーを押す。
/// どちらもフロントと同じく、タップ位置から `max_distance` 以内の同じ段のキーを距離つきで
/// 代替に載せる（打たれたキー自身の距離を含む。縦方向のずれは扱わない）。
/// 戻り値の 2 つ目はノイズ（打たれたキーが狙いと違う）の数
pub fn touch_noise(keys: &str, cfg: &NoiseConfig, rng: &mut Rng) -> (Vec<TypedKey>, usize) {
    let mut out = Vec::with_capacity(keys.len());
    let mut noised = 0;
    let total = keys.chars().count();
    for (pos, c) in keys.chars().enumerate() {
        let Some((row, idx)) = qwerty_position(c) else {
            out.push(TypedKey::plain(c));
            continue;
        };
        let eligible = cfg.tail == 0 || pos + cfg.tail >= total;
        let row_len = QWERTY_ROWS[row].1.len();
        let center = key_center(row, idx);
        let mut tap = None;
        if eligible && noised < cfg.max_noise && rng.next_f64() < cfg.rate {
            // 隣接キーのある向きだけ選ぶ
            let mut dirs = Vec::new();
            if idx > 0 {
                dirs.push(-1.0);
            }
            if idx + 1 < row_len {
                dirs.push(1.0);
            }
            if !dirs.is_empty() {
                let sign = dirs[rng.below(dirs.len())];
                let d = (0.5 + (rng.gauss() * cfg.tap_sigma).abs()).min(1.49);
                tap = Some(center + sign * d);
            }
        }
        let tap = tap.unwrap_or_else(|| center + (rng.gauss() * cfg.tap_sigma).clamp(-0.49, 0.49));
        // タップ位置に最も近いキーが打たれたキー
        let mut near: Vec<(char, f64)> = QWERTY_ROWS[row]
            .1
            .chars()
            .enumerate()
            .map(|(i, k)| (k, (tap - key_center(row, i)).abs()))
            .collect();
        near.sort_by(|a, b| a.1.total_cmp(&b.1));
        let typed = near[0].0;
        if typed != c {
            noised += 1;
        }
        let mut alts = vec![(typed, near[0].1 as f32)];
        alts.extend(
            near[1..]
                .iter()
                .filter(|(_, d)| *d <= cfg.max_distance as f64)
                .take(cfg.max_alts)
                .map(|&(k, d)| (k, d as f32)),
        );
        out.push(TypedKey { key: typed, alts });
    }
    (out, noised)
}

/// 打鍵列に、タイポ（入れ替え・欠落・余剰・子音の紛れ）を混ぜる。代替は付けない。
///
/// 各打鍵は確率 `rate`（かつ `max_noise` 未満）で、次のいずれかを均等に選んで適用する:
/// 次の打鍵との入れ替え / その打鍵の欠落 / 余剰（同じキーの重ね打ち、または母音の混入）/
/// 子音の紛れ（`CONSONANT_CONFUSIONS` の逆: 本来 `r` を `l` と打つ）。
/// 適用できない操作（末尾の入れ替え、紛れの無い文字）は選び直さず、その打鍵はノイズなしとする。
/// 戻り値の 2 つ目は適用したノイズの数
pub fn typo_noise(keys: &str, cfg: &NoiseConfig, rng: &mut Rng) -> (Vec<TypedKey>, usize) {
    let chars: Vec<char> = keys.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len() + 2);
    let mut noised = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let eligible = cfg.tail == 0 || i + cfg.tail >= chars.len();
        if eligible && noised < cfg.max_noise && rng.next_f64() < cfg.rate {
            match rng.below(4) {
                0 if i + 1 < chars.len() && chars[i + 1] != c => {
                    out.push(chars[i + 1]);
                    out.push(c);
                    i += 2;
                    noised += 1;
                    continue;
                }
                1 => {
                    i += 1;
                    noised += 1;
                    continue;
                }
                2 => {
                    out.push(c);
                    if rng.below(2) == 0 {
                        out.push(c);
                    } else {
                        out.push(*b"aiueo".get(rng.below(5)).unwrap_or(&b'a') as char);
                    }
                    i += 1;
                    noised += 1;
                    continue;
                }
                3 => {
                    // 本来 `to` のところを `from` と打つ
                    let confused: Vec<char> = CONSONANT_CONFUSIONS
                        .iter()
                        .filter(|(_, to)| *to == c)
                        .map(|(from, _)| *from)
                        .collect();
                    if !confused.is_empty() {
                        out.push(confused[rng.below(confused.len())]);
                        i += 1;
                        noised += 1;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(c);
        i += 1;
    }
    (out.into_iter().map(TypedKey::plain).collect(), noised)
}

/// 種類に応じてノイズを混ぜる
pub fn add_noise(keys: &str, cfg: &NoiseConfig, rng: &mut Rng) -> (Vec<TypedKey>, usize) {
    match cfg.kind {
        NoiseKind::Touch => touch_noise(keys, cfg, rng),
        NoiseKind::Typo => typo_noise(keys, cfg, rng),
    }
}

/// 打鍵列の文字列（ログ・失敗例の表示用）
pub fn keys_string(keys: &[TypedKey]) -> String {
    keys.iter().map(|k| k.key).collect()
}

// ---- 追加仮説の計測用バックエンド ----

/// `convert_hypotheses` の呼び出し回数を数えるバックエンド（追加仮説を走らせた割合の計測用）。
/// それ以外は内側のバックエンドへそのまま委譲する
pub struct CountingBackend {
    inner: Arc<dyn ConversionBackend>,
    hypothesis_calls: AtomicUsize,
}

impl CountingBackend {
    /// 内側のバックエンドを包む
    pub fn new(inner: Arc<dyn ConversionBackend>) -> Self {
        CountingBackend {
            inner,
            hypothesis_calls: AtomicUsize::new(0),
        }
    }

    /// `convert_hypotheses` が呼ばれた回数
    pub fn hypothesis_calls(&self) -> usize {
        self.hypothesis_calls.load(Ordering::Relaxed)
    }
}

impl ConversionBackend for CountingBackend {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn convert_sentence(&self, reading: &str, left_context: &str) -> Vec<BackendSegment> {
        self.inner.convert_sentence(reading, left_context)
    }
    fn segment_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendCandidate> {
        self.inner.segment_candidates(reading, left_context, limit)
    }
    fn predict(&self, prefix: &str, left_context: &str, limit: usize) -> Vec<BackendCandidate> {
        self.inner.predict(prefix, left_context, limit)
    }
    fn reconvert(&self, surface: &str) -> Vec<BackendSegment> {
        self.inner.reconvert(surface)
    }
    fn sentence_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendSentence> {
        self.inner.sentence_candidates(reading, left_context, limit)
    }
    fn prefix_oracle(&self) -> Option<&dyn PrefixOracle> {
        self.inner.prefix_oracle()
    }
    fn quality(&self, reading: &str) -> Option<ReadingQuality> {
        self.inner.quality(reading)
    }
    fn convert_hypotheses(
        &self,
        hyps: &[ReadingHypothesis],
        left_context: &str,
        limit: usize,
    ) -> Vec<ScoredSentence> {
        self.hypothesis_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.convert_hypotheses(hyps, left_context, limit)
    }
    fn take_completions(&self) -> Vec<Completion> {
        self.inner.take_completions()
    }
    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.inner.set_waker(waker);
    }
}

// ---- 評価ループ ----

/// 評価 1 件（読みと期待表記）
#[derive(Debug, Clone)]
pub struct EvalCase {
    /// 読み（ひらがな）
    pub reading: String,
    /// 期待する表記
    pub expected: String,
}

/// 1 回の入力 → Space 変換の結果
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// 候補リスト（表記と、補正由来か）。先頭が第 1 候補
    pub candidates: Vec<(String, bool)>,
    /// Space 変換にかかった時間（ミリ秒。1 回目で候補ウィンドウが出なければ 2 回目まで）
    pub millis: f64,
}

impl Outcome {
    /// 第 1 候補
    pub fn top1(&self) -> &str {
        self.candidates.first().map_or("", |c| c.0.as_str())
    }

    /// 上位 `n` 件に `expected` があるか
    pub fn in_top(&self, expected: &str, n: usize) -> bool {
        self.candidates.iter().take(n).any(|c| c.0 == expected)
    }

    /// 補正由来を除いた候補リスト（劣化チェックで補正なしと比べる）
    fn base_list(&self) -> Vec<&str> {
        self.candidates
            .iter()
            .filter(|c| !c.1)
            .map(|c| c.0.as_str())
            .collect()
    }
}

/// 打鍵列を入力して Space 変換し、候補リストを返す。終わったらセッションをリセットする
pub fn run_input(engine: &mut Engine, session: &str, keys: &[TypedKey]) -> Outcome {
    use nagi_protocol::view::CandidateSource;
    for k in keys {
        let Ok(event) = parse_key(&k.key.to_string()) else {
            continue;
        };
        let alts: Vec<InputAlternative> = k
            .alts
            .iter()
            .map(|&(c, d)| InputAlternative {
                text: c.to_string(),
                distance: d,
            })
            .collect();
        engine.process_key_with_alternatives(session, &event, &alts);
    }
    let space = parse_key("Space").expect("Space");
    let started = Instant::now();
    let mut result = engine.process_key(session, &space);
    let mut preedit = result.preedit.as_ref().map(|p| p.text.clone());
    if !result.candidates.as_ref().is_some_and(|c| c.visible) {
        // 文節ごとの変換などで、1 回目では候補ウィンドウが開かない場合
        result = engine.process_key(session, &space);
        preedit = preedit.or_else(|| result.preedit.as_ref().map(|p| p.text.clone()));
    }
    let millis = started.elapsed().as_secs_f64() * 1000.0;
    let candidates: Vec<(String, bool)> = match result.candidates {
        Some(c) if c.visible => c
            .items
            .into_iter()
            .map(|i| (i.surface, i.sources.contains(&CandidateSource::Corrected)))
            .collect(),
        _ => preedit.into_iter().map(|p| (p, false)).collect(),
    };
    engine.reset(session);
    Outcome { candidates, millis }
}

/// 劣化チェックで補正なしの結果を取る回数（補正なし側の揺れを拾うため）
const BASELINE_RUNS: usize = 6;

/// 評価の設定
#[derive(Debug, Clone)]
pub struct EvalSettings {
    /// ノイズ
    pub noise: NoiseConfig,
    /// 乱数の種
    pub seed: u64,
    /// 上位何件を「候補に出た」とみなすか
    pub top_n: usize,
    /// 失敗例を `Report::examples` に集める
    pub verbose: bool,
}

/// 1 条件（補正なし / あり）の集計
#[derive(Debug, Clone, Default)]
pub struct Tally {
    /// 第 1 候補が期待表記
    pub top1: usize,
    /// 上位 N 件に期待表記
    pub top_n: usize,
    /// 候補リスト全体に期待表記
    pub any: usize,
    /// Space 変換のレイテンシ（ミリ秒）
    pub millis: Vec<f64>,
}

/// 評価結果
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// 入力した評価データの件数
    pub cases: usize,
    /// 読みをローマ字に直せず（ひらがな以外を含む等）対象外にした件数
    pub skipped: usize,
    /// 補正なしで、ノイズ無し入力の第 1 候補が期待表記だった件数（以降の集計はこの集合）
    pub clean_correct: usize,
    /// そのうち、実際にノイズが入った件数（`off`・`on`・`hypotheses_run` の集計はこの集合）
    pub noised: usize,
    /// 補正なし（ノイズ入り入力）
    pub off: Tally,
    /// 補正あり（ノイズ入り入力）
    pub on: Tally,
    /// 補正ありで追加仮説の変換（`convert_hypotheses`）を走らせた件数（ノイズ入り入力）
    pub hypotheses_run: usize,
    /// 補正ありで追加仮説を走らせた件数（ノイズ無し・代替なしの入力。評価データ全体）
    pub clean_hypotheses_run: usize,
    /// 補正ありで追加仮説を走らせた件数（ノイズ無し・タップ誤差なしの代替つき入力。評価データ全体）
    pub tap_hypotheses_run: usize,
    /// 補正なしの結果が実行ごとに揺れた件数（同点候補の順。情報用）
    pub unstable_baseline: usize,
    /// 劣化チェックの対象件数（評価データ全体 × 代替なし / タップ誤差なしの代替つき）
    pub degradation_checked: usize,
    /// 劣化した件数（補正あり/なしで、第 1 候補または補正由来を除く候補リストが変わった）
    pub degraded: usize,
    /// 失敗例（verbose のとき）
    pub examples: Vec<String>,
}

fn pick<'a>(cases: &'a [EvalCase], romanizer: &Romanizer) -> (Vec<(&'a EvalCase, String)>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0;
    for c in cases {
        match romanizer.romanize(&c.reading) {
            Some(keys) => out.push((c, keys)),
            None => skipped += 1,
        }
    }
    (out, skipped)
}

/// 評価を実行する。`off` は補正なし、`on` は補正ありのエンジン（同じバックエンド・設定で
/// `fuzzy.enabled` だけが違うこと）。`counter` は `on` のバックエンドの [`CountingBackend`]
pub fn evaluate(
    cases: &[EvalCase],
    romanizer: &Romanizer,
    off: &mut Engine,
    on: &mut Engine,
    counter: &CountingBackend,
    settings: &EvalSettings,
) -> Report {
    let (typeable, skipped) = pick(cases, romanizer);
    let mut report = Report {
        cases: cases.len(),
        skipped,
        ..Report::default()
    };
    let n = settings.top_n;
    let clean_touch = NoiseConfig {
        kind: NoiseKind::Touch,
        rate: 0.0,
        ..settings.noise
    };
    for (idx, (case, keys)) in typeable.iter().enumerate() {
        let plain: Vec<TypedKey> = keys.chars().map(TypedKey::plain).collect();
        // 補正ありの結果が補正なしと一致するか（ノイズ無し。代替なし / タップ誤差なしの代替つき）。
        // 補正なし（= 従来の変換）側に、同点の候補の順が実行ごとに変わる揺れがあるので（`HashMap` の
        // 反復順）、補正なしを `BASELINE_RUNS` 回走らせ（補正なしでは代替は無視される）、
        // 補正ありの結果がそのどれとも違うときだけ劣化とする
        let off_clean = run_input(off, "off", &plain);
        let off_more: Vec<Outcome> = (1..BASELINE_RUNS)
            .map(|_| run_input(off, "off", &plain))
            .collect();
        let before = counter.hypothesis_calls();
        let on_clean = run_input(on, "on", &plain);
        report.clean_hypotheses_run += usize::from(counter.hypothesis_calls() > before);
        let mut rng_clean = Rng::new(settings.seed ^ 0xC1EA_5000 ^ (idx as u64) << 8);
        let (tapped, _) = touch_noise(keys, &clean_touch, &mut rng_clean);
        let off_tap = run_input(off, "off", &tapped);
        let before = counter.hypothesis_calls();
        let on_tap = run_input(on, "on", &tapped);
        report.tap_hypotheses_run += usize::from(counter.hypothesis_calls() > before);
        let baselines: Vec<&Outcome> = std::iter::once(&off_clean)
            .chain(off_more.iter())
            .chain(std::iter::once(&off_tap))
            .collect();
        let key_of = |o: &Outcome| (o.top1().to_string(), o.base_list().join("\u{1}"));
        if baselines.iter().any(|b| key_of(b) != key_of(&off_clean)) {
            report.unstable_baseline += 1;
        }
        for (label, b) in [("plain", &on_clean), ("tap", &on_tap)] {
            report.degradation_checked += 1;
            if baselines.iter().all(|a| key_of(a) != key_of(b)) {
                report.degraded += 1;
                if settings.verbose {
                    report.examples.push(format!(
                        "DEGRADED[{label}] {} -> off={:?} on={:?}",
                        keys,
                        off_clean
                            .candidates
                            .iter()
                            .take(3)
                            .map(|c| &c.0)
                            .collect::<Vec<_>>(),
                        b.candidates
                            .iter()
                            .take(3)
                            .map(|c| &c.0)
                            .collect::<Vec<_>>(),
                    ));
                }
            }
        }
        // ノイズ入り入力の集計は、ノイズ無しで補正なしが正解のケースに限る
        if off_clean.top1() != case.expected {
            continue;
        }
        report.clean_correct += 1;
        let mut rng = Rng::new(settings.seed ^ (idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (noisy, noised) = add_noise(keys, &settings.noise, &mut rng);
        if noised == 0 {
            continue;
        }
        report.noised += 1;
        let a = run_input(off, "off", &noisy);
        let before = counter.hypothesis_calls();
        let b = run_input(on, "on", &noisy);
        report.hypotheses_run += usize::from(counter.hypothesis_calls() > before);
        for (tally, o) in [(&mut report.off, &a), (&mut report.on, &b)] {
            tally.top1 += usize::from(o.top1() == case.expected);
            tally.top_n += usize::from(o.in_top(&case.expected, n));
            tally.any += usize::from(o.in_top(&case.expected, usize::MAX));
            tally.millis.push(o.millis);
        }
        if settings.verbose && !b.in_top(&case.expected, n) {
            report.examples.push(format!(
                "NG {} (typed {}) want {:?}; off top3={:?} on top3={:?}",
                keys,
                keys_string(&noisy),
                case.expected,
                a.candidates
                    .iter()
                    .take(3)
                    .map(|c| &c.0)
                    .collect::<Vec<_>>(),
                b.candidates
                    .iter()
                    .take(3)
                    .map(|c| &c.0)
                    .collect::<Vec<_>>(),
            ));
        }
    }
    report
}

/// パーセンタイル（`p` は 0..=1。空なら 0）
pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: NoiseKind, rate: f64) -> NoiseConfig {
        NoiseConfig {
            kind,
            rate,
            max_noise: usize::MAX,
            ..NoiseConfig::default()
        }
    }

    fn keys_of(keys: &[TypedKey]) -> String {
        keys.iter().map(|k| k.key).collect()
    }

    #[test]
    fn rng_is_reproducible_and_roughly_uniform() {
        let a: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..8).map(|_| r.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..8).map(|_| r.next_u64()).collect()
        };
        assert_eq!(a, b);
        let c: Vec<u64> = {
            let mut r = Rng::new(8);
            (0..8).map(|_| r.next_u64()).collect()
        };
        assert_ne!(a, c);
        // seed 0 でも停止しない
        let mut r = Rng::new(0);
        assert_ne!(r.next_u64(), r.next_u64());
        let mut r = Rng::new(1);
        let n = 20000;
        let mean: f64 = (0..n).map(|_| r.next_f64()).sum::<f64>() / n as f64;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
        let g: Vec<f64> = (0..n).map(|_| r.gauss()).collect();
        let gm = g.iter().sum::<f64>() / n as f64;
        let gv = g.iter().map(|x| (x - gm).powi(2)).sum::<f64>() / n as f64;
        assert!(gm.abs() < 0.05 && (gv - 1.0).abs() < 0.05, "{gm} {gv}");
    }

    #[test]
    fn romanizer_round_trips_through_the_converter() {
        let r = Romanizer::new(RomajiTable::standard());
        for reading in [
            "にほんご",
            "がっこう",
            "きょうは",
            "ほんとう",
            "んあ",
            "なんでも",
            "いっちょう",
            "しんにゅう",
            "ぱっと",
            "ふぁいる",
            "てぃー",
        ] {
            // 「ー」を含む読みは対象外（記号キーは打鍵ログを無効にする）
            let keys = r.romanize(reading);
            if reading.contains('ー') {
                assert!(keys.is_none(), "{reading}");
                continue;
            }
            let keys = keys.unwrap_or_else(|| panic!("{reading}"));
            assert_eq!(r.replay(&keys).as_deref(), Some(reading), "{keys}");
            assert!(keys.chars().all(|c| c.is_ascii_lowercase()), "{keys}");
        }
        assert_eq!(r.romanize("がっこう").unwrap(), "gakkou");
        assert_eq!(r.romanize("ん").unwrap(), "nn");
        // 最短の綴り
        assert_eq!(r.romanize("し").unwrap().len(), 2);
        // ひらがな以外・空は None
        assert!(r.romanize("漢字").is_none());
        assert!(r.romanize("").is_none());
    }

    #[test]
    fn touch_noise_without_noise_keeps_keys_and_gives_plausible_alternatives() {
        let mut rng = Rng::new(3);
        let config = cfg(NoiseKind::Touch, 0.0);
        let (keys, noised) = touch_noise("nihongokanzihenkan", &config, &mut rng);
        assert_eq!(noised, 0);
        assert_eq!(keys_of(&keys), "nihongokanzihenkan");
        for k in &keys {
            // 先頭は打たれたキー自身（キーの内側 = 距離 0.5 以内）
            assert_eq!(k.alts[0].0, k.key);
            assert!(k.alts[0].1 <= 0.5 + 1e-6, "{:?}", k.alts);
            // 距離の昇順、上限内、打たれたキー + 最大 max_alts
            assert!(k.alts.windows(2).all(|w| w[0].1 <= w[1].1));
            assert!(k.alts.iter().all(|a| a.1 <= config.max_distance));
            assert!(k.alts.len() <= 1 + config.max_alts);
            // 重複なし
            let mut chars: Vec<char> = k.alts.iter().map(|a| a.0).collect();
            chars.sort_unstable();
            chars.dedup();
            assert_eq!(chars.len(), k.alts.len());
        }
    }

    #[test]
    fn touch_noise_miss_lands_on_adjacent_key_with_intended_key_nearby() {
        let mut rng = Rng::new(11);
        let config = cfg(NoiseKind::Touch, 1.0);
        let intended = "kanjihenkan";
        let (keys, noised) = touch_noise(intended, &config, &mut rng);
        assert_eq!(noised, intended.len());
        for (k, c) in keys.iter().zip(intended.chars()) {
            assert_ne!(k.key, c);
            // 同じ段の隣
            let (r1, i1) = qwerty_position(k.key).unwrap();
            let (r2, i2) = qwerty_position(c).unwrap();
            assert_eq!(r1, r2);
            assert_eq!(i1.abs_diff(i2), 1, "{} -> {}", c, k.key);
            // 打たれたキーの距離は 0.5 未満、狙ったキーは 0.5 以上（あれば）
            assert!(k.alts[0].1 < 0.5 + 1e-6);
            if let Some(a) = k.alts.iter().find(|a| a.0 == c) {
                assert!(a.1 >= 0.5 - 1e-6 && a.1 <= 1.0, "{:?}", k.alts);
            }
        }
    }

    #[test]
    fn touch_noise_rate_cap_tail_and_reproducibility() {
        let text = "abcdefghijklmnopqrstuvwxyz".repeat(80);
        let mut rng = Rng::new(5);
        let (keys, noised) = touch_noise(&text, &cfg(NoiseKind::Touch, 0.3), &mut rng);
        let changed = keys
            .iter()
            .zip(text.chars())
            .filter(|(k, c)| k.key != *c)
            .count();
        assert_eq!(changed, noised);
        let rate = noised as f64 / text.len() as f64;
        assert!((rate - 0.3).abs() < 0.04, "{rate}");
        // 同じ seed なら同じ結果
        let mut rng2 = Rng::new(5);
        assert_eq!(
            touch_noise(&text, &cfg(NoiseKind::Touch, 0.3), &mut rng2).0,
            keys
        );
        // 最大数
        let capped = NoiseConfig {
            max_noise: 2,
            ..cfg(NoiseKind::Touch, 1.0)
        };
        assert_eq!(touch_noise(&text, &capped, &mut Rng::new(5)).1, 2);
        // 末尾 4 打鍵だけ
        let tail = NoiseConfig {
            tail: 4,
            ..cfg(NoiseKind::Touch, 1.0)
        };
        let (keys, noised) = touch_noise("kanjihenkan", &tail, &mut Rng::new(5));
        assert_eq!(noised, 4);
        assert_eq!(&keys_of(&keys)[..7], "kanjihe");
    }

    #[test]
    fn typo_noise_applies_one_edit_without_alternatives() {
        let text = "watasihagakkouniikimasu";
        let one = NoiseConfig {
            max_noise: 1,
            ..cfg(NoiseKind::Typo, 1.0)
        };
        let mut edits = std::collections::HashSet::new();
        for seed in 0..200 {
            let (keys, noised) = typo_noise(text, &one, &mut Rng::new(seed));
            assert!(keys.iter().all(|k| k.alts.is_empty()));
            if noised == 0 {
                // 選んだ操作が適用できなかった（末尾付近の入れ替えなど）場合だけ
                assert_eq!(keys_of(&keys), text);
                continue;
            }
            assert_eq!(noised, 1);
            let typed = keys_of(&keys);
            assert_ne!(typed, text);
            let (a, b) = (text.chars().count(), typed.chars().count());
            edits.insert(match b.cmp(&a) {
                std::cmp::Ordering::Less => "drop",
                std::cmp::Ordering::Greater => "extra",
                std::cmp::Ordering::Equal => "swap-or-confuse",
            });
            // 同じ seed なら同じ結果
            assert_eq!(keys, typo_noise(text, &one, &mut Rng::new(seed)).0);
        }
        assert_eq!(edits.len(), 3, "{edits:?}");
        // 末尾 3 打鍵だけ
        let tail = NoiseConfig {
            tail: 3,
            ..cfg(NoiseKind::Typo, 1.0)
        };
        for seed in 0..50 {
            let (keys, _) = typo_noise(text, &tail, &mut Rng::new(seed));
            assert!(keys_of(&keys).starts_with(&text[..text.len() - 3]));
        }
        // 紛れは本来 r を l と打つ向き
        let confuse = (0..200).any(|seed| {
            let (keys, n) = typo_noise("r", &one, &mut Rng::new(seed));
            n == 1 && keys_of(&keys) == "l"
        });
        assert!(confuse);
    }

    #[test]
    fn percentile_picks_sorted_value() {
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 0.5), 2.0);
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 1.0), 3.0);
    }
}
