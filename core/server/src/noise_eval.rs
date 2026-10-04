//! 曖昧入力の合成ノイズ評価（`nagi-eval --noise touch|typo|flick`。設計: `docs/fuzzy-input-design.md` §10-3）。
//!
//! 評価データの読み（ひらがな）からローマ字の打鍵列（`flick` はフリック 12 キーの操作列）を作り、
//! ノイズを混ぜて `Engine` に入力する。
//! 補正なし（`fuzzy.enabled = false`）と補正あり（`true`）で、Space 変換の候補リストに期待表記が
//! 出るか・レイテンシ・追加仮説を走らせた割合を比べる。
//!
//! **QWERTY 配列・12 キー配列と方向ゾーンの幾何・タップ誤差・子音の紛れはこの評価ツール内部の仮定**で、
//! コア（`nagi-fuzzy` / `nagi-engine`）はレイアウトを持たない。コアへはフロントと同じ形（`alternatives`: 文字と
//! キー幅で正規化した距離）で渡す。乱数は自前の xorshift で、同じ `seed` なら結果は再現する。

use nagi_engine::backend::{
    BackendCandidate, BackendSegment, BackendSentence, Completion, ReadingQuality, ScoredSentence,
};
use nagi_engine::{ConversionBackend, Engine, parse_key};
use nagi_fuzzy::{CONSONANT_CONFUSIONS, PrefixOracle, ReadingHypothesis};
use nagi_protocol::methods::InputAlternative;
use nagi_romaji::{Emit, RomajiConverter, RomajiTable};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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

/// 打鍵ログが代替として受け付けるかな（ひらがなと長音。`KanaText.isLoggableKana`）
fn is_loggable_kana(c: char) -> bool {
    is_hiragana(c) || c == 'ー'
}

// ---- ノイズ ----

/// ノイズの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseKind {
    /// タッチ位置誤差（隣接キーへの置換 + 距離付き代替）
    Touch,
    /// タイポ（入れ替え・欠落・余剰・子音の紛れ。代替なし）
    Typo,
    /// フリック 12 キーの誤判定（方向の取り違え・隣接キーへのずれ + 距離付きかな代替）
    Flick,
}

impl NoiseKind {
    /// `--noise` の値から
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "touch" => Some(NoiseKind::Touch),
            "typo" => Some(NoiseKind::Typo),
            "flick" => Some(NoiseKind::Flick),
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
    /// フリックで、確定したゾーン自身の距離も代替に含める（既定 true = iOS フロントと同じ。
    /// 含めると、コアは確定したゾーンとの相対コストで換算する。false は旧フロントの模擬）
    pub flick_typed_distance: bool,
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
            flick_typed_distance: true,
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
        // フリックはローマ字の打鍵列ではない（`flick_input`）。ここではノイズなしの打鍵列を返す
        NoiseKind::Flick => (keys.chars().map(TypedKey::plain).collect(), 0),
    }
}

/// 打鍵列の文字列（ログ・失敗例の表示用）
pub fn keys_string(keys: &[TypedKey]) -> String {
    keys.iter().map(|k| k.key).collect()
}

// ---- フリック 12 キー ----

/// 入力 1 ステップ（フロントがコアへ送る操作）
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// `process_key`（ローマ字キー + 代替）
    Key(TypedKey),
    /// `insert_reading`（かな 1 文字 + 距離付き代替）
    Reading {
        /// 確定した方向のかな
        text: char,
        /// 代替 `(文字, キー幅で正規化した距離)`
        alts: Vec<(char, f32)>,
    },
    /// Backspace（`process_key`。「小゛゜」で直前のかなを消す）
    Backspace,
}

/// 12 キーの列数（両端は機能キー）。`frontend-ios/Layouts/flick-12.json` と同じ
const FLICK_COLUMNS: usize = 5;
/// キーの高さ / 幅。iPhone（幅 390pt → キー幅 78pt）で行の高さ 52pt
/// （`KeyboardViewController.rowHeight`）の想定
const FLICK_ASPECT: f64 = 52.0 / 78.0;
/// 方向ゾーン中心をキー中心から離す幅（キーの短辺比）。`FlickGeometry.spreadRatio` と同じ値
/// （テストで Swift のソースと一致を確認する）
const FLICK_SPREAD_RATIO: f64 = 0.8;
/// `FlickGeometry` の代替の件数の上限と距離の上限（`TouchGeometry.defaultMaxCount` / `defaultMaxDistance`）
const FLICK_MAX_COUNT: usize = 3;

/// かなキー（行・列・`中央 左 上 右 下`。`-` はそのゾーンなし）。`flick-12.json` のかなキーと同じ
const FLICK_TABLE: [(usize, usize, &str); 11] = [
    (0, 1, "あいうえお"),
    (0, 2, "かきくけこ"),
    (0, 3, "さしすせそ"),
    (1, 1, "たちつてと"),
    (1, 2, "なにぬねの"),
    (1, 3, "はひふへほ"),
    (2, 1, "まみむめも"),
    (2, 2, "や(ゆ)よ"),
    (2, 3, "らりるれろ"),
    (3, 2, "わをんー-"),
    (3, 3, "、。？！-"),
];

/// 「小゛゜」の循環（`KanaTransform.cycles` と同じ。テストで Swift のソースと一致を確認する）
const KANA_CYCLES: [&str; 29] = [
    "あぁ",
    "いぃ",
    "うぅゔ",
    "えぇ",
    "おぉ",
    "かが",
    "きぎ",
    "くぐ",
    "けげ",
    "こご",
    "さざ",
    "しじ",
    "すず",
    "せぜ",
    "そぞ",
    "ただ",
    "ちぢ",
    "つっづ",
    "てで",
    "とど",
    "はばぱ",
    "ひびぴ",
    "ふぶぷ",
    "へべぺ",
    "ほぼぽ",
    "やゃ",
    "ゆゅ",
    "よょ",
    "わゎ",
];

/// 「小゛゜」で `c` の次の形（循環に無ければ `None`）
pub fn next_kana(c: char) -> Option<char> {
    KANA_CYCLES.iter().find_map(|cycle| {
        let chars: Vec<char> = cycle.chars().collect();
        chars
            .iter()
            .position(|&x| x == c)
            .map(|i| chars[(i + 1) % chars.len()])
    })
}

/// 方向ゾーン
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// 中央（タップ）
    Center,
    /// 左
    Left,
    /// 上
    Up,
    /// 右
    Right,
    /// 下
    Down,
}

const DIRS: [Dir; 5] = [Dir::Center, Dir::Left, Dir::Up, Dir::Right, Dir::Down];

/// フリックキー 1 つ（ゾーンの並びは中央・左・上・右・下で、`FlickGeometry.zones` と同じ）
#[derive(Debug, Clone)]
pub struct FlickKey {
    row: usize,
    col: usize,
    zones: Vec<(Dir, char)>,
}

type Point = (f64, f64);

fn dist(a: Point, b: Point) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

impl FlickKey {
    fn center_char(&self) -> char {
        self.zones[0].1
    }

    fn center(&self) -> Point {
        (
            self.col as f64 + 0.5,
            (self.row as f64 + 0.5) * FLICK_ASPECT,
        )
    }

    /// ゾーン中心（`FlickGeometry.zoneCenter`。キー幅 = 1）
    fn zone_center(&self, dir: Dir) -> Point {
        let (cx, cy) = self.center();
        let s = FLICK_ASPECT.min(1.0) * FLICK_SPREAD_RATIO;
        match dir {
            Dir::Center => (cx, cy),
            Dir::Left => (cx - s, cy),
            Dir::Right => (cx + s, cy),
            Dir::Up => (cx, cy - s),
            Dir::Down => (cx, cy + s),
        }
    }

    fn zone_char(&self, dir: Dir) -> Option<char> {
        self.zones.iter().find(|z| z.0 == dir).map(|z| z.1)
    }

    /// 離した位置 `end` に最も近いゾーン（同距離なら先のゾーン。`FlickGeometry.decide` と同じ）
    fn nearest_zone(&self, end: Point) -> (Dir, char, f64) {
        let mut best = (self.zones[0].0, self.zones[0].1, f64::INFINITY);
        for &(dir, ch) in &self.zones {
            let d = dist(end, self.zone_center(dir));
            if d < best.2 {
                best = (dir, ch, d);
            }
        }
        best
    }
}

/// フリックの判定結果（`FlickDecision`）
#[derive(Debug, Clone, PartialEq)]
pub struct FlickDecision {
    /// 確定した方向
    pub dir: Dir,
    /// 確定したかな
    pub text: char,
    /// 代替 `(かな, 距離)`。他のゾーン・キーのひらがな・長音のみ（近い順、最大 3 件）。
    /// `typed_distance` なら、確定したかな自身を末尾に加える。確定がひらがな・長音以外なら空
    pub alts: Vec<(char, f32)>,
    /// 確定したゾーン中心までの距離
    pub chosen_distance: f32,
}

/// 評価の対象にできる読みの 1 文字に対応するフリック操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlickUnit {
    /// キーの添字
    pub key: usize,
    /// 方向
    pub dir: Dir,
    /// 基本かなのあとに「小゛゜」を押す回数（濁点・半濁点・小書き）
    pub presses: usize,
    /// 目的のかな
    pub target: char,
}

/// 12 キー配列（評価ツール内の仮定。`frontend-ios/Layouts/flick-12.json` のかなキーに合わせる）
#[derive(Debug, Clone)]
pub struct FlickLayout {
    keys: Vec<FlickKey>,
    base: HashMap<char, (usize, Dir)>,
}

impl FlickLayout {
    /// 標準の 12 キー
    pub fn standard() -> Self {
        let mut keys = Vec::new();
        let mut base = HashMap::new();
        for (i, &(row, col, spec)) in FLICK_TABLE.iter().enumerate() {
            let chars: Vec<char> = spec.chars().collect();
            let mut zones = Vec::new();
            for (dir, &c) in DIRS.iter().zip(chars.iter()) {
                if c != '-' {
                    zones.push((*dir, c));
                    // 入力できる読みは、ひらがなと長音（打鍵ログに載るもの）だけ
                    if is_hiragana(c) || c == 'ー' {
                        base.insert(c, (i, *dir));
                    }
                }
            }
            keys.push(FlickKey { row, col, zones });
        }
        FlickLayout { keys, base }
    }

    /// 読みを操作列に直す。直せない文字（記号・ひらがなで作れない文字）があればその文字を返す
    pub fn plan(&self, reading: &str) -> Result<Vec<FlickUnit>, char> {
        let mut units = Vec::new();
        for c in reading.chars() {
            if let Some(&(key, dir)) = self.base.get(&c) {
                units.push(FlickUnit {
                    key,
                    dir,
                    presses: 0,
                    target: c,
                });
                continue;
            }
            // 濁点・小書き: 循環の先頭（基本かな）+ 「小゛゜」を i 回
            let found = KANA_CYCLES.iter().find_map(|cycle| {
                let chars: Vec<char> = cycle.chars().collect();
                chars.iter().position(|&x| x == c).map(|i| (chars[0], i))
            });
            match found {
                Some((b, presses)) if presses > 0 => {
                    let &(key, dir) = self.base.get(&b).ok_or(c)?;
                    units.push(FlickUnit {
                        key,
                        dir,
                        presses,
                        target: c,
                    });
                }
                _ => return Err(c),
            }
        }
        Ok(units)
    }

    /// 開始キー `start` を `start_pt` で押し、`end` で離したときの判定（`FlickGeometry.decide` と同じ規則）:
    /// 確定するのは開始キーのゾーンのうち `end` に最も近いもの。代替は、開始キーの他のゾーン
    /// （`end` からの距離）と、他のかなキーの中央ゾーン（`start_pt` からの距離）のうち距離 <= 1.0 のものを
    /// 近い順（同距離は文字の昇順）に最大 3 件。代替はひらがな・長音のみ（「(」「、」などは除いてから
    /// 上位 3 件を取る）。`typed_distance` なら確定したゾーン自身も加える。確定がひらがな・長音以外
    /// （記号キー）なら代替は空
    pub fn decide(
        &self,
        start: usize,
        start_pt: Point,
        end: Point,
        max_distance: f64,
        typed_distance: bool,
    ) -> FlickDecision {
        let key = &self.keys[start];
        let (dir, text, chosen_d) = key.nearest_zone(end);
        let mut best: Vec<(char, f64)> = Vec::new();
        let mut consider = |c: char, d: f64| {
            if c == text || !is_loggable_kana(c) || d > max_distance {
                return;
            }
            match best.iter_mut().find(|b| b.0 == c) {
                Some(b) if b.1 <= d => {}
                Some(b) => b.1 = d,
                None => best.push((c, d)),
            }
        };
        for &(zd, zc) in &key.zones {
            consider(zc, dist(end, key.zone_center(zd)));
        }
        for (i, other) in self.keys.iter().enumerate() {
            if i != start {
                consider(other.center_char(), dist(start_pt, other.center()));
            }
        }
        best.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        best.truncate(FLICK_MAX_COUNT);
        // 確定したのがかな以外（記号キー）なら、代替は一切送らない
        if !is_loggable_kana(text) {
            best.clear();
        }
        let mut alts: Vec<(char, f32)> = best.into_iter().map(|(c, d)| (c, d as f32)).collect();
        if typed_distance && is_loggable_kana(text) {
            alts.push((text, chosen_d as f32));
        }
        FlickDecision {
            dir,
            text,
            alts,
            chosen_distance: chosen_d as f32,
        }
    }

    /// 隣のかなキー（上下左右。機能キー・空きは除く）
    fn neighbors(&self, key: usize) -> Vec<(usize, i32, i32)> {
        let k = &self.keys[key];
        let mut out = Vec::new();
        for (dc, dr) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (c, r) = (k.col as i32 + dc, k.row as i32 + dr);
            if let Some(i) = self
                .keys
                .iter()
                .position(|o| o.col as i32 == c && o.row as i32 == r)
            {
                out.push((i, dc, dr));
            }
        }
        out
    }

    /// 開始位置が属するキー（かなキーの外は `None`）
    fn key_at(&self, p: Point) -> Option<usize> {
        let col = p.0.floor();
        let row = (p.1 / FLICK_ASPECT).floor();
        if col < 0.0 || row < 0.0 || col >= FLICK_COLUMNS as f64 {
            return None;
        }
        self.keys
            .iter()
            .position(|k| k.col as f64 == col && k.row as f64 == row)
    }
}

/// 方向ゾーンの隣（取り違えやすい相手）。中央は全方向、方向ゾーンは中央と直交する 2 方向
fn adjacent_dirs(dir: Dir) -> &'static [Dir] {
    match dir {
        Dir::Center => &[Dir::Left, Dir::Up, Dir::Right, Dir::Down],
        Dir::Left | Dir::Right => &[Dir::Center, Dir::Up, Dir::Down],
        Dir::Up | Dir::Down => &[Dir::Center, Dir::Left, Dir::Right],
    }
}

/// 1 つのフリック操作（基本かな）の離し位置と、判定結果を作る。
///
/// - ノイズなし: タッチ開始は狙ったキー中心から `N(0, tap_sigma)`（±0.49 に制限）、離す位置は狙ったゾーン
///   中心から `N(0, tap_sigma)` ずれる。狙ったゾーンが最も近くなるまで誤差を半分にする（偶然の誤判定を
///   ノイズに数えない）
/// - 方向の取り違え（`noisy` の半分）: 離す位置が、狙ったゾーンから隣のゾーンへ向かって半分を
///   `|N(0, tap_sigma)|`（ゾーン間隔で頭打ち）超えた位置になる
/// - 隣接キーへのずれ（残り半分。隣が無ければ方向の取り違え）: タッチ開始が隣のかなキーの内側
///   （境界から `|N(0, tap_sigma)|` だけ内側）になり、離す位置は開始位置から狙った方向へ同じだけ動いた位置
///   （＋誤差）。隣のキーのゾーンで判定される
fn flick_decision(
    layout: &FlickLayout,
    unit: &FlickUnit,
    noisy: bool,
    cfg: &NoiseConfig,
    rng: &mut Rng,
) -> FlickDecision {
    let key = &layout.keys[unit.key];
    let sigma = cfg.tap_sigma;
    let intended = key.zone_center(unit.dir);
    let (cx, cy) = key.center();
    let max_d = cfg.max_distance as f64;
    if noisy {
        let neighbors = layout.neighbors(unit.key);
        let slip_key = rng.below(2) == 0 && !neighbors.is_empty();
        if slip_key {
            let (nk, dc, dr) = neighbors[rng.below(neighbors.len())];
            // 境界から隣のキーの内側へ `|N|` だけ入った位置
            let inside = (rng.gauss() * sigma).abs().min(0.45);
            let (sx, sy) = if dc != 0 {
                (
                    cx + dc as f64 * (0.5 + inside),
                    cy + (rng.gauss() * sigma).clamp(-0.4, 0.4) * FLICK_ASPECT,
                )
            } else {
                (
                    cx + (rng.gauss() * sigma).clamp(-0.4, 0.4),
                    cy + dr as f64 * (0.5 + inside) * FLICK_ASPECT,
                )
            };
            let disp = (intended.0 - cx, intended.1 - cy);
            let end = (
                sx + disp.0 + rng.gauss() * sigma * 0.5,
                sy + disp.1 + rng.gauss() * sigma * 0.5,
            );
            if layout.key_at((sx, sy)) == Some(nk) {
                return layout.decide(nk, (sx, sy), end, max_d, cfg.flick_typed_distance);
            }
        }
        // 方向の取り違え
        let others: Vec<Dir> = adjacent_dirs(unit.dir)
            .iter()
            .copied()
            .filter(|d| key.zone_char(*d).is_some())
            .collect();
        if !others.is_empty() {
            let wrong = others[rng.below(others.len())];
            let target = key.zone_center(wrong);
            let gap = dist(intended, target);
            let t = 0.5 + ((rng.gauss() * sigma).abs() / gap).min(0.45);
            let end = (
                intended.0 + (target.0 - intended.0) * t,
                intended.1 + (target.1 - intended.1) * t,
            );
            let start = (cx, cy);
            return layout.decide(unit.key, start, end, max_d, cfg.flick_typed_distance);
        }
    }
    // ノイズなし
    let start = (
        cx + (rng.gauss() * sigma).clamp(-0.49, 0.49),
        cy + (rng.gauss() * sigma).clamp(-0.49, 0.49) * FLICK_ASPECT,
    );
    let (mut ex, mut ey) = (rng.gauss() * sigma, rng.gauss() * sigma);
    let mut end;
    loop {
        end = if unit.dir == Dir::Center {
            // タップ: 指は動かない
            (start.0 + ex * 0.4, start.1 + ey * 0.4)
        } else {
            (intended.0 + ex, intended.1 + ey)
        };
        if key.nearest_zone(end).1 == key.zone_char(unit.dir).unwrap_or(' ') {
            break;
        }
        ex *= 0.5;
        ey *= 0.5;
        if ex.abs() < 1e-6 && ey.abs() < 1e-6 {
            end = intended;
            break;
        }
    }
    let start_pt = if unit.dir == Dir::Center { end } else { start };
    layout.decide(unit.key, start_pt, end, max_d, cfg.flick_typed_distance)
}

/// 「小゛゜」で代替を引き継ぐ: 各代替のかなを `next_kana` で写し、距離はそのまま。写せないものは落とす
fn carry_alts(alts: &[(char, f32)]) -> Vec<(char, f32)> {
    alts.iter()
        .filter_map(|&(c, d)| next_kana(c).map(|n| (n, d)))
        .collect()
}

/// 読みの操作列（`plan`）をフロントの操作列に直す。ノイズ・代替は `cfg` に従う。
///
/// 各かなは基本かなのフリック（`Step::Reading`、代替つき）と、濁点・小書きなら続く
/// 「`Backspace` + `Reading`」× 回数（iOS の `applyDakuten` と同じ）。「小゛゜」の `Reading` の代替は、
/// 直前の `Reading` の代替（確定したかな自身の距離を含む）を同じ循環（`next_kana`）で写して距離はそのまま付ける
/// （写せない代替は落とす。`LastFlick.transformed`）。
/// 戻り値は 手順、ノイズ（確定したかなが狙いと違う）の数、打たれたとおりの読み
pub fn flick_input(
    layout: &FlickLayout,
    plan: &[FlickUnit],
    cfg: &NoiseConfig,
    rng: &mut Rng,
) -> (Vec<Step>, usize, String) {
    let mut steps = Vec::new();
    let mut noised = 0;
    let mut shown = String::new();
    for (i, unit) in plan.iter().enumerate() {
        let eligible = cfg.tail == 0 || i + cfg.tail >= plan.len();
        let noisy = eligible && noised < cfg.max_noise && rng.next_f64() < cfg.rate;
        let d = flick_decision(layout, unit, noisy, cfg, rng);
        let intended = layout.keys[unit.key].zone_char(unit.dir);
        if Some(d.text) != intended {
            noised += 1;
        }
        let alts_sent = d.alts.clone();
        steps.push(Step::Reading {
            text: d.text,
            alts: d.alts,
        });
        let mut last = d.text;
        let mut last_alts = alts_sent;
        for _ in 0..unit.presses {
            // 循環に無いかなでは「小゛゜」は何もしない
            let Some(next) = next_kana(last) else { break };
            last_alts = carry_alts(&last_alts);
            steps.push(Step::Backspace);
            steps.push(Step::Reading {
                text: next,
                alts: last_alts.clone(),
            });
            last = next;
        }
        shown.push(last);
    }
    (steps, noised, shown)
}

/// 代替なしで、狙いどおりの操作列にする
pub fn flick_plain(plan: &[FlickUnit], layout: &FlickLayout) -> Vec<Step> {
    let mut steps = Vec::new();
    for unit in plan {
        let base = layout.keys[unit.key].zone_char(unit.dir).unwrap_or(' ');
        steps.push(Step::Reading {
            text: base,
            alts: Vec::new(),
        });
        let mut last = base;
        for _ in 0..unit.presses {
            let Some(next) = next_kana(last) else { break };
            steps.push(Step::Backspace);
            steps.push(Step::Reading {
                text: next,
                alts: Vec::new(),
            });
            last = next;
        }
    }
    steps
}

// ---- 追加仮説の計測用バックエンド ----

/// `convert_hypotheses` の呼び出し回数を数えるバックエンド（追加仮説を走らせた割合の計測用）。
/// それ以外は内側のバックエンドへそのまま委譲する
pub struct CountingBackend {
    inner: Arc<dyn ConversionBackend>,
    hypothesis_calls: AtomicUsize,
    hypothesis_nanos: AtomicU64,
}

impl CountingBackend {
    /// 内側のバックエンドを包む
    pub fn new(inner: Arc<dyn ConversionBackend>) -> Self {
        CountingBackend {
            inner,
            hypothesis_calls: AtomicUsize::new(0),
            hypothesis_nanos: AtomicU64::new(0),
        }
    }

    /// `convert_hypotheses` が呼ばれた回数
    pub fn hypothesis_calls(&self) -> usize {
        self.hypothesis_calls.load(Ordering::Relaxed)
    }

    /// `convert_hypotheses` に費やした時間の合計（ミリ秒。残りの補正コストは `expand` など）
    pub fn hypothesis_millis(&self) -> f64 {
        self.hypothesis_nanos.load(Ordering::Relaxed) as f64 / 1e6
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
        let started = Instant::now();
        let out = self.inner.convert_hypotheses(hyps, left_context, limit);
        self.hypothesis_nanos
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        out
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
    let steps: Vec<Step> = keys.iter().cloned().map(Step::Key).collect();
    run_steps(engine, session, &steps)
}

fn wire(alts: &[(char, f32)]) -> Vec<InputAlternative> {
    alts.iter()
        .map(|&(c, d)| InputAlternative {
            text: c.to_string(),
            distance: d,
        })
        .collect()
}

/// 操作列（キー・`insert_reading`・Backspace）を入力して Space 変換し、候補リストを返す。
/// 終わったらセッションをリセットする
pub fn run_steps(engine: &mut Engine, session: &str, steps: &[Step]) -> Outcome {
    use nagi_protocol::view::CandidateSource;
    for step in steps {
        match step {
            Step::Key(k) => {
                let Ok(event) = parse_key(&k.key.to_string()) else {
                    continue;
                };
                engine.process_key_with_alternatives(session, &event, &wire(&k.alts));
            }
            Step::Reading { text, alts } => {
                engine.insert_reading(session, &text.to_string(), &wire(alts));
            }
            Step::Backspace => {
                engine.process_key(session, &parse_key("BackSpace").expect("BackSpace"));
            }
        }
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
    /// 補正候補（`Corrected`）が 1 件以上出た件数（ノイズ無し・タップ誤差なしの代替つき入力。評価データ全体）。
    /// 正しい入力で候補欄に補正が混ざる頻度
    pub tap_corrected_cases: usize,
    /// 上の入力で出た補正候補の総数
    pub tap_corrected_items: usize,
    /// 補正なしの結果が実行ごとに揺れた件数（同点候補の順。情報用）
    pub unstable_baseline: usize,
    /// 劣化チェックの対象件数（評価データ全体 × 代替なし / タップ誤差なしの代替つき）
    pub degradation_checked: usize,
    /// 劣化した件数（補正あり/なしで、第 1 候補または補正由来を除く候補リストが変わった）
    pub degraded: usize,
    /// 対象外にした理由（入力に直せなかった文字 → その文字を含む読みの件数。`flick` のみ。
    /// 1 つの読みに複数あれば最初の 1 文字で数える）
    pub unsupported: BTreeMap<char, usize>,
    /// 評価対象のうち、濁点・小書きなど「小゛゜」を使う読みの件数（`flick` のみ）
    pub with_presses: usize,
    /// 失敗例（verbose のとき）
    pub examples: Vec<String>,
}

/// 1 件の入力の元（ローマ字の打鍵列、またはフリックの操作列）
enum Target {
    Keys(String),
    Flick(Vec<FlickUnit>),
}

impl Target {
    /// 代替なし・ノイズなしの操作列
    fn plain(&self, layout: &FlickLayout) -> Vec<Step> {
        match self {
            Target::Keys(k) => k.chars().map(|c| Step::Key(TypedKey::plain(c))).collect(),
            Target::Flick(plan) => flick_plain(plan, layout),
        }
    }

    /// ノイズ（`cfg.rate`）つきの操作列・ノイズの数・打たれたとおりの表示
    fn noisy(
        &self,
        layout: &FlickLayout,
        cfg: &NoiseConfig,
        rng: &mut Rng,
    ) -> (Vec<Step>, usize, String) {
        match self {
            Target::Keys(k) => {
                let (keys, n) = add_noise(k, cfg, rng);
                let shown = keys_string(&keys);
                (keys.into_iter().map(Step::Key).collect(), n, shown)
            }
            Target::Flick(plan) => flick_input(layout, plan, cfg, rng),
        }
    }

    /// 打鍵の誤差なし（`rate = 0`）で、代替つきの操作列（タップ誤差なしの代替つき入力）
    fn accurate(&self, layout: &FlickLayout, cfg: &NoiseConfig, rng: &mut Rng) -> Vec<Step> {
        let clean = NoiseConfig {
            kind: if matches!(self, Target::Flick(_)) {
                NoiseKind::Flick
            } else {
                NoiseKind::Touch
            },
            rate: 0.0,
            ..*cfg
        };
        match self {
            Target::Keys(k) => touch_noise(k, &clean, rng)
                .0
                .into_iter()
                .map(Step::Key)
                .collect(),
            Target::Flick(plan) => flick_input(layout, plan, &clean, rng).0,
        }
    }

    /// 失敗例の表示用（ローマ字、または目的の読み）
    fn label(&self) -> String {
        match self {
            Target::Keys(k) => k.clone(),
            Target::Flick(plan) => plan.iter().map(|u| u.target).collect(),
        }
    }
}

fn pick<'a>(
    cases: &'a [EvalCase],
    romanizer: &Romanizer,
    layout: &FlickLayout,
    kind: NoiseKind,
    report: &mut Report,
) -> Vec<(&'a EvalCase, Target)> {
    let mut out = Vec::new();
    for c in cases {
        if kind == NoiseKind::Flick {
            match layout.plan(&c.reading) {
                Ok(plan) if !plan.is_empty() => {
                    report.with_presses += usize::from(plan.iter().any(|u| u.presses > 0));
                    out.push((c, Target::Flick(plan)));
                }
                Ok(_) => {
                    report.skipped += 1;
                    *report.unsupported.entry(' ').or_default() += 1;
                }
                Err(ch) => {
                    report.skipped += 1;
                    *report.unsupported.entry(ch).or_default() += 1;
                }
            }
        } else {
            match romanizer.romanize(&c.reading) {
                Some(keys) => out.push((c, Target::Keys(keys))),
                None => report.skipped += 1,
            }
        }
    }
    out
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
    let layout = FlickLayout::standard();
    let mut report = Report {
        cases: cases.len(),
        ..Report::default()
    };
    let typeable = pick(cases, romanizer, &layout, settings.noise.kind, &mut report);
    let n = settings.top_n;
    for (idx, (case, target)) in typeable.iter().enumerate() {
        let label = target.label();
        let plain = target.plain(&layout);
        // 補正ありの結果が補正なしと一致するか（ノイズ無し。代替なし / タップ誤差なしの代替つき）。
        // 補正なし（= 従来の変換）側に、同点の候補の順が実行ごとに変わる揺れがあるので（`HashMap` の
        // 反復順）、補正なしを `BASELINE_RUNS` 回走らせ（補正なしでは代替は無視される）、
        // 補正ありの結果がそのどれとも違うときだけ劣化とする
        let off_clean = run_steps(off, "off", &plain);
        let off_more: Vec<Outcome> = (1..BASELINE_RUNS)
            .map(|_| run_steps(off, "off", &plain))
            .collect();
        let before = counter.hypothesis_calls();
        let on_clean = run_steps(on, "on", &plain);
        report.clean_hypotheses_run += usize::from(counter.hypothesis_calls() > before);
        let mut rng_clean = Rng::new(settings.seed ^ 0xC1EA_5000 ^ (idx as u64) << 8);
        let tapped = target.accurate(&layout, &settings.noise, &mut rng_clean);
        let off_tap = run_steps(off, "off", &tapped);
        let before = counter.hypothesis_calls();
        let on_tap = run_steps(on, "on", &tapped);
        report.tap_hypotheses_run += usize::from(counter.hypothesis_calls() > before);
        let shown = on_tap.candidates.iter().filter(|c| c.1).count();
        report.tap_corrected_cases += usize::from(shown > 0);
        report.tap_corrected_items += shown;
        let baselines: Vec<&Outcome> = std::iter::once(&off_clean)
            .chain(off_more.iter())
            .chain(std::iter::once(&off_tap))
            .collect();
        let key_of = |o: &Outcome| (o.top1().to_string(), o.base_list().join("\u{1}"));
        if baselines.iter().any(|b| key_of(b) != key_of(&off_clean)) {
            report.unstable_baseline += 1;
        }
        for (what, b) in [("plain", &on_clean), ("tap", &on_tap)] {
            report.degradation_checked += 1;
            if baselines.iter().all(|a| key_of(a) != key_of(b)) {
                report.degraded += 1;
                if settings.verbose {
                    report.examples.push(format!(
                        "DEGRADED[{what}] {} -> off={:?} on={:?}",
                        label,
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
        let (noisy, noised, shown) = target.noisy(&layout, &settings.noise, &mut rng);
        if noised == 0 {
            continue;
        }
        report.noised += 1;
        let a = run_steps(off, "off", &noisy);
        let before = counter.hypothesis_calls();
        let b = run_steps(on, "on", &noisy);
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
                label,
                shown,
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

    // ---- フリック ----

    fn flick_cfg(rate: f64) -> NoiseConfig {
        NoiseConfig {
            kind: NoiseKind::Flick,
            rate,
            max_noise: usize::MAX,
            tap_sigma: 0.15,
            ..NoiseConfig::default()
        }
    }

    fn repo_file(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Swift の `static let name: Type = <number>` の数値
    fn swift_constant(source: &str, name: &str) -> f64 {
        let line = source
            .lines()
            .find(|l| l.contains(&format!("static let {name}")))
            .unwrap_or_else(|| panic!("{name} が見つからない"));
        line.rsplit('=')
            .next()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or_else(|| panic!("{name} の値を読めない: {line}"))
    }

    #[test]
    fn flick_geometry_constants_match_the_ios_frontend() {
        let geometry = repo_file("frontend-ios/Shared/Layout/TouchGeometry.swift");
        assert_eq!(swift_constant(&geometry, "spreadRatio"), FLICK_SPREAD_RATIO);
        assert_eq!(
            swift_constant(&geometry, "defaultMaxCount") as usize,
            FLICK_MAX_COUNT
        );
        assert_eq!(
            swift_constant(&geometry, "defaultMaxDistance") as f32,
            NoiseConfig::default().max_distance
        );
        // 「小゛゜」の循環（`KanaTransform.cycles`）
        let transform = repo_file("frontend-ios/Shared/Layout/KanaTransform.swift");
        let marker = "let strings: [String] = [";
        let start = transform.find(marker).expect("cycles") + marker.len();
        let body = &transform[start..];
        let body = &body[..body.find(']').expect("]")];
        let cycles: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
        assert_eq!(cycles, KANA_CYCLES);
    }

    #[test]
    fn flick_table_matches_flick_12_json() {
        let json: serde_json::Value =
            serde_json::from_str(&repo_file("frontend-ios/Layouts/flick-12.json")).expect("json");
        let mut expected: Vec<(usize, usize, String)> = Vec::new();
        for (row, r) in json["rows"].as_array().unwrap().iter().enumerate() {
            let keys = r["keys"].as_array().unwrap();
            assert_eq!(keys.len(), FLICK_COLUMNS, "列数");
            for (col, k) in keys.iter().enumerate() {
                // 幅はすべて 1（省略 = 1）
                assert!(k["width"].is_null() || k["width"] == 1.0, "{k}");
                if k["tap"]["type"] != "kana" {
                    continue;
                }
                let mut spec = k["tap"]["text"].as_str().unwrap().to_string();
                for dir in ["left", "up", "right", "down"] {
                    spec.push_str(k["flick"][dir].as_str().unwrap_or("-"));
                }
                expected.push((row, col, spec));
            }
        }
        let actual: Vec<(usize, usize, String)> = FLICK_TABLE
            .iter()
            .map(|&(r, c, spec)| (r, c, spec.to_string()))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn flick_plan_maps_kana_and_reports_unsupported_chars() {
        let layout = FlickLayout::standard();
        let plan = layout.plan("かんじゃっ").expect("plan");
        let summary: Vec<(char, usize)> = plan.iter().map(|u| (u.target, u.presses)).collect();
        assert_eq!(
            summary,
            [('か', 0), ('ん', 0), ('じ', 1), ('ゃ', 1), ('っ', 1)]
        );
        // 基本かなは行キーの方向（ん は わ の上、ー は 右）
        let u = layout.plan("んー").unwrap();
        assert_eq!(
            (layout.keys[u[0].key].center_char(), u[0].dir),
            ('わ', Dir::Up)
        );
        assert_eq!(u[1].dir, Dir::Right);
        // ゔ は う を 2 回
        assert_eq!(layout.plan("ゔ").unwrap()[0].presses, 2);
        // 記号・漢字・ASCII は対象外（最初の 1 文字を返す）。かなキーの記号（、）・括弧も同じ
        assert_eq!(layout.plan("か、"), Err('、'));
        assert_eq!(layout.plan("漢字"), Err('漢'));
        assert_eq!(layout.plan("あ(い"), Err('('));
        assert_eq!(layout.plan("aあ"), Err('a'));
    }

    #[test]
    fn flick_decide_uses_zone_centers_normalised_by_key_width() {
        let layout = FlickLayout::standard();
        let a = 0; // あ
        let key = &layout.keys[a];
        let s = (FLICK_ASPECT.min(1.0) * FLICK_SPREAD_RATIO) as f32;
        // 中央をタップ: あ。代替は他の方向ゾーン（距離 = spread）と隣のキーの中央（距離 1.0）のうち近い 3 件
        let c = key.center();
        let d = layout.decide(a, c, c, 1.0, false);
        assert_eq!(d.text, 'あ');
        assert_eq!(d.dir, Dir::Center);
        // 距離は同じ（浮動小数の丸めで並びが前後するので、集合で比べる）
        let mut texts: Vec<char> = d.alts.iter().map(|a| a.0).collect();
        texts.sort_unstable();
        assert_eq!(texts, ['い', 'う', 'え']);
        assert!(
            d.alts.iter().all(|a| (a.1 - s).abs() < 1e-5),
            "{:?}",
            d.alts
        );
        // 確定したかなは alts に含まれない。typed_distance で含めると距離 0
        assert!(d.alts.iter().all(|a| a.0 != 'あ'));
        let d = layout.decide(a, c, c, 1.0, true);
        assert_eq!(d.alts.last(), Some(&('あ', 0.0)));
        assert_eq!(d.alts.len(), 4);
        // 左ゾーンと中央の境目の少し左で離す: い。あ（中央）との距離の差が小さい
        let end = (c.0 - 0.55 * s as f64, c.1);
        let d = layout.decide(a, c, end, 1.0, false);
        assert_eq!(d.text, 'い');
        assert_eq!(d.dir, Dir::Left);
        assert_eq!(d.alts[0].0, 'あ');
        assert!(
            (d.alts[0].1 - 0.55 * s).abs() < 1e-5 && (d.chosen_distance - 0.45 * s).abs() < 1e-5,
            "{d:?}"
        );
        // 上限 1.0 を超える距離は含まれない（max_distance = 0.5 では何も残らない）
        let d = layout.decide(a, c, c, 0.5, false);
        assert!(d.alts.is_empty());
        // 隣のキー（か）の近くを押し始めると、か の中央ゾーンが距離つきで代替に入る
        let start = (key.center().0 + 0.45, key.center().1);
        let d = layout.decide(a, start, start, 1.0, false);
        let ka = d.alts.iter().find(|a| a.0 == 'か').expect("か");
        assert!((ka.1 - 0.55).abs() < 1e-5, "{ka:?}");
    }

    #[test]
    fn flick_decide_sends_only_kana_alternatives_and_none_for_symbol_keys() {
        let layout = FlickLayout::standard();
        // や（中央）: 左 ( と右 ) はかな以外なので、除いてから上位 3 件を取る（ゆ・よ のあとに隣のキー）
        let ya = layout
            .keys
            .iter()
            .position(|k| k.center_char() == 'や')
            .expect("や");
        let c = layout.keys[ya].center();
        for typed_distance in [false, true] {
            let d = layout.decide(ya, c, c, 1.0, typed_distance);
            assert_eq!(d.text, 'や');
            let others: Vec<char> = d
                .alts
                .iter()
                .take(FLICK_MAX_COUNT)
                .map(|a| a.0)
                .filter(|&x| x != 'や')
                .collect();
            assert_eq!(others.len(), FLICK_MAX_COUNT, "{:?}", d.alts);
            assert!(d.alts.iter().all(|a| is_loggable_kana(a.0)), "{:?}", d.alts);
            assert!(
                others.contains(&'ゆ') && others.contains(&'よ'),
                "{:?}",
                d.alts
            );
            assert_eq!(d.alts.len(), FLICK_MAX_COUNT + usize::from(typed_distance));
        }
        // 、 キー（記号）を確定したら、近くにかなキーがあっても代替は送らない
        let comma = layout
            .keys
            .iter()
            .position(|k| k.center_char() == '、')
            .expect("、");
        let c = layout.keys[comma].center();
        for typed_distance in [false, true] {
            let d = layout.decide(comma, c, c, 1.0, typed_distance);
            assert_eq!(d.text, '、');
            assert!(d.alts.is_empty(), "{:?}", d.alts);
        }
        // ー はかな扱い（わ キーの右）
        let wa = layout
            .keys
            .iter()
            .position(|k| k.center_char() == 'わ')
            .expect("わ");
        let end = layout.keys[wa].zone_center(Dir::Right);
        let d = layout.decide(wa, end, end, 1.0, true);
        assert_eq!(d.text, 'ー');
        assert_eq!(d.alts.last(), Some(&('ー', 0.0)));
    }

    #[test]
    fn dakuten_carries_alternatives_with_the_same_cycle() {
        // き・く は ぎ・ぐ へ。循環に無い ん は落ち、距離はそのまま
        assert_eq!(
            carry_alts(&[('き', 0.3), ('ん', 0.5), ('か', 0.1)]),
            [('ぎ', 0.3), ('が', 0.1)]
        );
        // は → ば → ぱ → は
        let mut alts = vec![('ひ', 0.5), ('は', 0.1)];
        let mut seen = Vec::new();
        for _ in 0..3 {
            alts = carry_alts(&alts);
            seen.push(alts.clone());
        }
        assert_eq!(seen[0], [('び', 0.5), ('ば', 0.1)]);
        assert_eq!(seen[1], [('ぴ', 0.5), ('ぱ', 0.1)]);
        assert_eq!(seen[2], [('ひ', 0.5), ('は', 0.1)]);
        // 操作列でも、が の insert_reading の代替は か の代替の写像で、確定したゾーンの距離も同じ
        let layout = FlickLayout::standard();
        let plan = layout.plan("が").unwrap();
        for seed in 0..20 {
            let (steps, _, shown) =
                flick_input(&layout, &plan, &flick_cfg(0.0), &mut Rng::new(seed));
            assert_eq!(shown, "が");
            let [
                Step::Reading { alts: a, text: ka },
                Step::Backspace,
                Step::Reading { alts: b, text: ga },
            ] = &steps[..]
            else {
                panic!("{steps:?}");
            };
            assert_eq!((*ka, *ga), ('か', 'が'));
            assert_eq!(b, &carry_alts(a));
            assert_eq!(a.last().map(|x| x.1), b.last().map(|x| x.1));
            assert_eq!(b.last().map(|x| x.0), Some('が'));
        }
        // 確定ゾーンの距離を含めない模擬（旧フロント）でも、代替は写る
        let cfg = NoiseConfig {
            flick_typed_distance: false,
            ..flick_cfg(0.0)
        };
        let (steps, _, _) = flick_input(&layout, &plan, &cfg, &mut Rng::new(1));
        let [
            Step::Reading { alts: a, .. },
            Step::Backspace,
            Step::Reading { alts: b, .. },
        ] = &steps[..]
        else {
            panic!("{steps:?}");
        };
        assert!(a.iter().all(|x| x.0 != 'か'));
        assert_eq!(b, &carry_alts(a));
    }

    #[test]
    fn flick_input_without_noise_confirms_the_intended_kana_with_plausible_alternatives() {
        let layout = FlickLayout::standard();
        let reading = "にほんごきょうはがっこうでぱぴぷぺぽわをんー";
        let plan = layout.plan(reading).unwrap();
        let mut presses = 0;
        for seed in 0..100 {
            let (steps, noised, shown) =
                flick_input(&layout, &plan, &flick_cfg(0.0), &mut Rng::new(seed));
            assert_eq!(noised, 0);
            assert_eq!(shown, reading);
            presses = steps.iter().filter(|s| **s == Step::Backspace).count();
            for (i, step) in steps.iter().enumerate() {
                let Step::Reading { text, alts } = step else {
                    continue;
                };
                // 「小゛゜」の `insert_reading`（直前は Backspace）: 直前のフリックの代替を同じ循環で写したもの
                if i > 0 && steps[i - 1] == Step::Backspace {
                    let Step::Reading { alts: prev, .. } = &steps[i - 2] else {
                        panic!("{:?}", steps[i - 2]);
                    };
                    assert_eq!(alts, &carry_alts(prev));
                    assert_eq!(alts.last().map(|a| a.0), Some(*text));
                    continue;
                }
                // 代替: 末尾が確定したかな自身（距離つき）。それ以外は確定したかなを含まず、ひらがな・長音のみ、
                // 距離の昇順・上限内・最大 3 件
                let (typed, others) = alts.split_last().expect("typed");
                assert_eq!(typed.0, *text);
                assert!(others.len() <= FLICK_MAX_COUNT, "{alts:?}");
                assert!(
                    others
                        .iter()
                        .all(|a| a.0 != *text && is_loggable_kana(a.0) && a.1 <= 1.0)
                );
                assert!(others.windows(2).all(|w| w[0].1 <= w[1].1));
            }
            // 同じ seed なら同じ操作列
            assert_eq!(
                steps,
                flick_input(&layout, &plan, &flick_cfg(0.0), &mut Rng::new(seed)).0
            );
        }
        // が ぱ ぴ ぷ ぺ ぽ っ... の押下回数 = 濁点・半濁点の回数
        let expected: usize = plan.iter().map(|u| u.presses).sum();
        assert_eq!(presses, expected);
        assert!(expected > 5);
    }

    #[test]
    fn flick_noise_misflicks_and_alternatives_often_contain_the_intended_kana() {
        let layout = FlickLayout::standard();
        // 濁点・小書きなし（基本かなだけ）の列
        let reading =
            "かきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをん";
        let plan = layout.plan(reading).unwrap();
        let (mut total, mut contained, mut centered) = (0, 0, 0);
        for seed in 0..60 {
            let (steps, noised, shown) =
                flick_input(&layout, &plan, &flick_cfg(1.0), &mut Rng::new(seed));
            // rate 1: すべてのフリックが狙いと違うかなに判定される
            assert_eq!(noised, plan.len(), "{shown}");
            for (step, want) in steps.iter().zip(reading.chars()) {
                let Step::Reading { text, alts } = step else {
                    panic!("{step:?}")
                };
                assert_ne!(*text, want);
                total += 1;
                if alts.iter().any(|a| a.0 == want) {
                    contained += 1;
                    // 狙ったゾーンまでの距離は、確定したゾーンより遠いか同程度（上限 1.0 以内）
                    let d = alts.iter().find(|a| a.0 == want).unwrap().1;
                    assert!((0.0..=1.0).contains(&d));
                }
                if layout
                    .keys
                    .iter()
                    .any(|k| k.center_char() == want && !alts.is_empty())
                {
                    centered += 1;
                }
            }
        }
        assert!(centered > 0);
        let ratio = contained as f64 / total as f64;
        // 方向の取り違えの大半と、中央（タップ）を狙った隣接キーへのずれが代替に入る
        assert!(ratio > 0.35, "{ratio}");
        assert!(
            ratio < 0.95,
            "隣接キーへのずれで方向ゾーンの狙いは代替に入らない: {ratio}"
        );
    }

    #[test]
    fn flick_noise_respects_max_noise_tail_and_rate() {
        let layout = FlickLayout::standard();
        let reading = "かんじへんかんにほんごへ";
        let plan = layout.plan(reading).unwrap();
        let capped = NoiseConfig {
            max_noise: 2,
            ..flick_cfg(1.0)
        };
        let (_, noised, _) = flick_input(&layout, &plan, &capped, &mut Rng::new(3));
        assert_eq!(noised, 2);
        // 末尾 3 つのフリックだけ
        let tail = NoiseConfig {
            tail: 3,
            ..flick_cfg(1.0)
        };
        let (_, noised, shown) = flick_input(&layout, &plan, &tail, &mut Rng::new(3));
        assert_eq!(noised, 3);
        let n = reading.chars().count();
        assert_eq!(
            shown.chars().take(n - 3).collect::<String>(),
            reading.chars().take(n - 3).collect::<String>()
        );
        // 確率: 長い入力でおよそ rate
        let long = layout.plan(&"あかさたなはまやらわ".repeat(60)).unwrap();
        let (_, noised, _) = flick_input(&layout, &long, &flick_cfg(0.2), &mut Rng::new(5));
        let rate = noised as f64 / long.len() as f64;
        assert!((rate - 0.2).abs() < 0.05, "{rate}");
    }

    #[test]
    fn flick_plain_has_no_alternatives_and_the_same_text() {
        let layout = FlickLayout::standard();
        let plan = layout.plan("がっこう").unwrap();
        let steps = flick_plain(&plan, &layout);
        assert!(steps.iter().all(|s| match s {
            Step::Reading { alts, .. } => alts.is_empty(),
            _ => true,
        }));
        // か → が、こ、う、つ → っ
        let texts: Vec<char> = steps
            .iter()
            .filter_map(|s| match s {
                Step::Reading { text, .. } => Some(*text),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ['か', 'が', 'つ', 'っ', 'こ', 'う']);
    }

    #[test]
    fn percentile_picks_sorted_value() {
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 0.5), 2.0);
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 1.0), 3.0);
    }
}
