//! Rewriter: 決定的変換の最終段（設計書 §4.13）。
//!
//! LLM を使わない決定的な表記変換（全角/半角、カタカナ/半角カナ 等）を候補として提供する。
//! trait でプラグイン追加可能。v1 は全角/半角英数記号・半角カナ・漢数字（digit-for-digit・位取り）・日付（YYYYMMDD）・簡易な二項演算。複雑な式や曖昧な日付は対象外。

/// 決定的な表記変換を提供するプラグイン
pub trait Rewriter: Send + Sync {
    /// 識別子（診断・ログ用）
    fn id(&self) -> &str;

    /// `reading` に対する変換結果の候補を返す（対象外・変換不能なら空を返す）
    fn rewrite(&self, reading: &str) -> Vec<String>;
}

/// 半角英数字・半角記号（ASCII 0x21-0x7E）を全角に変換する
#[derive(Debug, Clone, Copy, Default)]
pub struct ZenkakuAlnum;

impl Rewriter for ZenkakuAlnum {
    fn id(&self) -> &str {
        "zenkaku_alnum"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        if reading.is_empty() || !reading.chars().all(is_halfwidth_ascii) {
            return Vec::new();
        }
        vec![reading.chars().map(to_fullwidth_ascii).collect()]
    }
}

/// 全角英数字・全角記号（U+FF01-FF5E）を半角に変換する
#[derive(Debug, Clone, Copy, Default)]
pub struct HankakuAlnum;

impl Rewriter for HankakuAlnum {
    fn id(&self) -> &str {
        "hankaku_alnum"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        if reading.is_empty() || !reading.chars().all(is_fullwidth_ascii) {
            return Vec::new();
        }
        vec![reading.chars().map(to_halfwidth_ascii).collect()]
    }
}

/// 全角カタカナ（濁点・半濁点・記号含む）を半角カナに変換する
#[derive(Debug, Clone, Copy, Default)]
pub struct HankakuKatakana;

impl Rewriter for HankakuKatakana {
    fn id(&self) -> &str {
        "hankaku_katakana"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        if reading.is_empty() || !reading.chars().all(is_convertible_to_hankaku_katakana) {
            return Vec::new();
        }
        vec![to_hankaku_katakana(reading)]
    }
}

/// アラビア数字を1文字ずつ漢数字に置き換える（桁の重みは考慮しない。例: "2026" → "二〇二六"）
#[derive(Debug, Clone, Copy, Default)]
pub struct KanjiDigits;

impl Rewriter for KanjiDigits {
    fn id(&self) -> &str {
        "kanji_digits"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        if reading.is_empty() || !reading.chars().all(|c| c.is_ascii_digit()) {
            return Vec::new();
        }
        let out: String = reading
            .chars()
            .map(|c| KANJI_DIGIT_CHARS[c.to_digit(10).expect("ASCII digit") as usize])
            .collect();
        vec![out]
    }
}

/// アラビア数字の位取り漢数字表記（例: "2026" → "二千二十六"、"10000" → "一万"）。
/// 先頭に無意味なゼロがある場合（"0" 単独を除く）や、兆の桁（10^16 = 京 以上）を超える場合は
/// 位取りが定義できない・対象外として変換しない
#[derive(Debug, Clone, Copy, Default)]
pub struct KanjiPositionalNumeral;

impl Rewriter for KanjiPositionalNumeral {
    fn id(&self) -> &str {
        "kanji_positional_numeral"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        match to_positional_kanji(reading) {
            Some(s) => vec![s],
            None => Vec::new(),
        }
    }
}

/// 0〜9 の漢数字（インデックス = 数字）
const KANJI_DIGIT_CHARS: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// アラビア数字の文字列を位取り漢数字に変換する。変換できない場合は `None`
fn to_positional_kanji(digits: &str) -> Option<String> {
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if digits == "0" {
        return Some("〇".to_string());
    }
    // 先頭ゼロは位取り表記が定義できない（"0" 単独は上で処理済み）
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    // 兆の桁（10^16 = 京）を超える場合は対象外
    if digits.len() > 16 {
        return None;
    }

    const SMALL_UNITS: [&str; 4] = ["", "十", "百", "千"];
    const BIG_UNITS: [&str; 4] = ["", "万", "億", "兆"];

    // 4桁ずつのグループに分割するため、内部処理用に先頭を '0' で埋める（表記には出さない）
    let pad_len = (4 - digits.len() % 4) % 4;
    let padded = format!("{}{}", "0".repeat(pad_len), digits);
    let padded_bytes = padded.as_bytes();
    let groups: Vec<&[u8]> = padded_bytes.chunks(4).collect();
    let group_count = groups.len();

    let mut out = String::new();
    for (gi, group) in groups.iter().enumerate() {
        let big_unit_index = group_count - 1 - gi;
        if big_unit_index >= BIG_UNITS.len() {
            return None;
        }
        let group_digits: Vec<u32> = group.iter().map(|&b| (b - b'0') as u32).collect();
        if group_digits.iter().all(|&d| d == 0) {
            continue;
        }
        let mut group_str = String::new();
        for (i, &d) in group_digits.iter().enumerate() {
            let place = 3 - i;
            if d == 0 {
                continue;
            }
            if !(d == 1 && place > 0) {
                group_str.push(KANJI_DIGIT_CHARS[d as usize]);
            }
            group_str.push_str(SMALL_UNITS[place]);
        }
        out.push_str(&group_str);
        out.push_str(BIG_UNITS[big_unit_index]);
    }
    Some(out)
}

/// 8桁の数字（YYYYMMDD）を日付らしい表記に変換する（例: "20260927" → "2026/09/27", "2026年9月27日"）。
/// 月・日として妥当でない場合（うるう年考慮）は変換しない
#[derive(Debug, Clone, Copy, Default)]
pub struct DigitsToDate;

impl Rewriter for DigitsToDate {
    fn id(&self) -> &str {
        "digits_to_date"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        if reading.chars().count() != 8 || !reading.chars().all(|c| c.is_ascii_digit()) {
            return Vec::new();
        }
        let digits: Vec<u32> = reading.chars().map(|c| c.to_digit(10).unwrap()).collect();
        let year = digits[0] * 1000 + digits[1] * 100 + digits[2] * 10 + digits[3];
        let month = digits[4] * 10 + digits[5];
        let day = digits[6] * 10 + digits[7];
        if !is_valid_date(year, month, day) {
            return Vec::new();
        }
        vec![
            format!("{year}/{month:02}/{day:02}"),
            format!("{year}年{month}月{day}日"),
        ]
    }
}

/// うるう年か
fn is_leap_year(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// 年・月・日として妥当か（月は1〜12、日は月の日数の範囲内。うるう年を考慮する）
fn is_valid_date(year: u32, month: u32, day: u32) -> bool {
    if !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => unreachable!(),
    };
    day <= days_in_month
}

/// 単純な二項演算（`数値 演算子 数値` の1回だけ。括弧・複数演算は対象外）を計算する。
/// 演算子は `+` `-` `*` `/`。数値は非負の整数または小数（先頭の符号無し）
#[derive(Debug, Clone, Copy, Default)]
pub struct SimpleCalculator;

impl Rewriter for SimpleCalculator {
    fn id(&self) -> &str {
        "simple_calculator"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        match evaluate_simple_expression(reading) {
            Some(result) => vec![result],
            None => Vec::new(),
        }
    }
}

/// 妥当な非負整数・小数の文字列か（桁数の上限あり。オーバーフロー・極端な精度低下を避ける）
fn is_plausible_number(s: &str) -> bool {
    if s.is_empty() || s.len() > 18 {
        return false;
    }
    let mut seen_dot = false;
    let mut has_digit = false;
    for c in s.chars() {
        if c == '.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
        } else if c.is_ascii_digit() {
            has_digit = true;
        } else {
            return false;
        }
    }
    has_digit
}

/// `数値 演算子 数値` の形の文字列を計算する。パース不能・0除算・非有限の結果は `None`
fn evaluate_simple_expression(reading: &str) -> Option<String> {
    let op_index = reading.find(['+', '-', '*', '/'])?;
    if op_index == 0 {
        return None; // 先頭の符号は対象外
    }
    let (left, rest) = reading.split_at(op_index);
    let op = rest.chars().next()?;
    let right = &rest[1..];
    if !is_plausible_number(left) || !is_plausible_number(right) {
        return None;
    }
    let a: f64 = left.parse().ok()?;
    let b: f64 = right.parse().ok()?;
    let result = match op {
        '+' => a + b,
        '-' => a - b,
        '*' => a * b,
        '/' => {
            if b == 0.0 {
                return None;
            }
            a / b
        }
        _ => return None,
    };
    if !result.is_finite() {
        return None;
    }
    Some(format_calc_result(result))
}

/// 計算結果を表示用に整形する（整数ならそのまま、小数は小数点以下4桁に丸めて末尾の0を除去）
fn format_calc_result(x: f64) -> String {
    let x = if x == 0.0 { 0.0 } else { x }; // -0.0 を 0.0 に正規化
    if x == x.trunc() && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let rounded = (x * 10000.0).round() / 10000.0;
        let s = format!("{rounded:.4}");
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

/// 半角 ASCII の印字可能文字（0x21-0x7E）か
fn is_halfwidth_ascii(c: char) -> bool {
    ('\u{0021}'..='\u{007e}').contains(&c)
}

/// 半角 ASCII を全角に変換する（対象外の文字はそのまま返す）
fn to_fullwidth_ascii(c: char) -> char {
    if is_halfwidth_ascii(c) {
        char::from_u32(c as u32 + 0xfee0).unwrap_or(c)
    } else {
        c
    }
}

/// 全角英数記号（U+FF01-FF5E）か
fn is_fullwidth_ascii(c: char) -> bool {
    ('\u{ff01}'..='\u{ff5e}').contains(&c)
}

/// 全角英数記号を半角に変換する（対象外の文字はそのまま返す）
fn to_halfwidth_ascii(c: char) -> char {
    if is_fullwidth_ascii(c) {
        char::from_u32(c as u32 - 0xfee0).unwrap_or(c)
    } else {
        c
    }
}

/// 全角カタカナ（濁点・半濁点付き含む）→ 半角カナの対応表。
/// Unicode の公式分解（`<narrow>` タグ付き分解）から機械的に導出した値で、手入力による誤りはない
/// （左: 全角1文字、右: 半角の文字列。濁点・半濁点は半角では2文字になる）
const HANKAKU_KATAKANA_TABLE: &[(char, &str)] = &[
    ('ァ', "ｧ"),
    ('ア', "ｱ"),
    ('ィ', "ｨ"),
    ('イ', "ｲ"),
    ('ゥ', "ｩ"),
    ('ウ', "ｳ"),
    ('ェ', "ｪ"),
    ('エ', "ｴ"),
    ('ォ', "ｫ"),
    ('オ', "ｵ"),
    ('カ', "ｶ"),
    ('ガ', "ｶﾞ"),
    ('キ', "ｷ"),
    ('ギ', "ｷﾞ"),
    ('ク', "ｸ"),
    ('グ', "ｸﾞ"),
    ('ケ', "ｹ"),
    ('ゲ', "ｹﾞ"),
    ('コ', "ｺ"),
    ('ゴ', "ｺﾞ"),
    ('サ', "ｻ"),
    ('ザ', "ｻﾞ"),
    ('シ', "ｼ"),
    ('ジ', "ｼﾞ"),
    ('ス', "ｽ"),
    ('ズ', "ｽﾞ"),
    ('セ', "ｾ"),
    ('ゼ', "ｾﾞ"),
    ('ソ', "ｿ"),
    ('ゾ', "ｿﾞ"),
    ('タ', "ﾀ"),
    ('ダ', "ﾀﾞ"),
    ('チ', "ﾁ"),
    ('ヂ', "ﾁﾞ"),
    ('ッ', "ｯ"),
    ('ツ', "ﾂ"),
    ('ヅ', "ﾂﾞ"),
    ('テ', "ﾃ"),
    ('デ', "ﾃﾞ"),
    ('ト', "ﾄ"),
    ('ド', "ﾄﾞ"),
    ('ナ', "ﾅ"),
    ('ニ', "ﾆ"),
    ('ヌ', "ﾇ"),
    ('ネ', "ﾈ"),
    ('ノ', "ﾉ"),
    ('ハ', "ﾊ"),
    ('バ', "ﾊﾞ"),
    ('パ', "ﾊﾟ"),
    ('ヒ', "ﾋ"),
    ('ビ', "ﾋﾞ"),
    ('ピ', "ﾋﾟ"),
    ('フ', "ﾌ"),
    ('ブ', "ﾌﾞ"),
    ('プ', "ﾌﾟ"),
    ('ヘ', "ﾍ"),
    ('ベ', "ﾍﾞ"),
    ('ペ', "ﾍﾟ"),
    ('ホ', "ﾎ"),
    ('ボ', "ﾎﾞ"),
    ('ポ', "ﾎﾟ"),
    ('マ', "ﾏ"),
    ('ミ', "ﾐ"),
    ('ム', "ﾑ"),
    ('メ', "ﾒ"),
    ('モ', "ﾓ"),
    ('ャ', "ｬ"),
    ('ヤ', "ﾔ"),
    ('ュ', "ｭ"),
    ('ユ', "ﾕ"),
    ('ョ', "ｮ"),
    ('ヨ', "ﾖ"),
    ('ラ', "ﾗ"),
    ('リ', "ﾘ"),
    ('ル', "ﾙ"),
    ('レ', "ﾚ"),
    ('ロ', "ﾛ"),
    ('ワ', "ﾜ"),
    ('ヲ', "ｦ"),
    ('ン', "ﾝ"),
    ('ヴ', "ｳﾞ"),
    ('ヷ', "ﾜﾞ"),
    ('ヺ', "ｦﾞ"),
    ('ー', "ｰ"),
    ('、', "､"),
    ('。', "｡"),
    ('「', "｢"),
    ('」', "｣"),
    ('・', "･"),
];

/// 半角カナへの対応表にある文字か（無いものは変換不能。半角カナ自体が無い「ヮヰヱヵヶヸヹ」等）
fn is_convertible_to_hankaku_katakana(c: char) -> bool {
    HANKAKU_KATAKANA_TABLE.iter().any(|&(full, _)| full == c)
}

/// 全角カタカナ文字列を半角カナに変換する（`is_convertible_to_hankaku_katakana` で全文字確認済みの前提）
fn to_hankaku_katakana(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match HANKAKU_KATAKANA_TABLE.iter().find(|&&(full, _)| full == c) {
            Some(&(_, half)) => out.push_str(half),
            None => out.push(c),
        }
    }
    out
}

/// 複数の [`Rewriter`] をまとめて呼び出す
pub struct RewriterChain {
    rewriters: Vec<Box<dyn Rewriter>>,
}

impl RewriterChain {
    /// 既定の Rewriter（全角/半角英数記号、半角カナ）を登録したチェーンを作る
    pub fn default_chain() -> Self {
        RewriterChain {
            rewriters: vec![
                Box::new(ZenkakuAlnum),
                Box::new(HankakuAlnum),
                Box::new(HankakuKatakana),
                Box::new(KanjiDigits),
                Box::new(KanjiPositionalNumeral),
                Box::new(DigitsToDate),
                Box::new(SimpleCalculator),
            ],
        }
    }

    /// 空のチェーン（テスト・カスタム構成用）
    pub fn empty() -> Self {
        RewriterChain {
            rewriters: Vec::new(),
        }
    }

    /// Rewriter を追加する
    pub fn push(&mut self, rewriter: Box<dyn Rewriter>) {
        self.rewriters.push(rewriter);
    }

    /// 登録された全 Rewriter の候補をまとめて返す（重複除去はしない。呼び出し側の候補統合処理に委ねる）
    pub fn candidates(&self, reading: &str) -> Vec<String> {
        self.rewriters
            .iter()
            .flat_map(|r| r.rewrite(reading))
            .collect()
    }
}

impl Default for RewriterChain {
    fn default() -> Self {
        Self::default_chain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zenkaku_alnum_converts_ascii() {
        let r = ZenkakuAlnum;
        assert_eq!(r.rewrite("Nagi123!"), vec!["Ｎａｇｉ１２３！".to_string()]);
    }

    #[test]
    fn zenkaku_alnum_rejects_non_ascii() {
        let r = ZenkakuAlnum;
        assert_eq!(r.rewrite("こんにちは"), Vec::<String>::new());
        assert_eq!(r.rewrite(""), Vec::<String>::new());
    }

    #[test]
    fn hankaku_alnum_converts_fullwidth() {
        let r = HankakuAlnum;
        assert_eq!(r.rewrite("Ｎａｇｉ１２３！"), vec!["Nagi123!".to_string()]);
    }

    #[test]
    fn hankaku_alnum_rejects_halfwidth() {
        let r = HankakuAlnum;
        assert_eq!(r.rewrite("Nagi123"), Vec::<String>::new());
    }

    #[test]
    fn zenkaku_and_hankaku_alnum_round_trip() {
        let z = ZenkakuAlnum;
        let h = HankakuAlnum;
        // ZenkakuAlnum/HankakuAlnum は半角スペース（0x20）を対象外とする（全角スペースは
        // U+3000 であり、0x21-0x7E の +0xFEE0 変換では表現できないため）。往復テストの
        // 対象文字列にはスペースを含めない
        let original = "Hello,World!2026-09-27(Test)";
        let full = z.rewrite(original).remove(0);
        let back = h.rewrite(&full).remove(0);
        assert_eq!(back, original);
    }

    #[test]
    fn hankaku_katakana_converts_basic_words() {
        let r = HankakuKatakana;
        assert_eq!(r.rewrite("カタカナ"), vec!["ｶﾀｶﾅ".to_string()]);
        assert_eq!(r.rewrite("パソコン"), vec!["ﾊﾟｿｺﾝ".to_string()]);
        assert_eq!(r.rewrite("ガギグゲゴ"), vec!["ｶﾞｷﾞｸﾞｹﾞｺﾞ".to_string()]);
        assert_eq!(r.rewrite("ラーメン"), vec!["ﾗｰﾒﾝ".to_string()]);
    }

    #[test]
    fn hankaku_katakana_rejects_non_katakana() {
        let r = HankakuKatakana;
        assert_eq!(r.rewrite("こんにちは"), Vec::<String>::new());
        assert_eq!(r.rewrite("abc"), Vec::<String>::new());
    }

    #[test]
    fn hankaku_katakana_rejects_unmappable_characters() {
        // 半角カナが存在しない文字（小書きの「ヮ」等）を含む場合は変換しない
        let r = HankakuKatakana;
        assert_eq!(r.rewrite("クヮンサイ"), Vec::<String>::new());
    }

    #[test]
    fn rewriter_chain_default_collects_all_applicable_candidates() {
        let chain = RewriterChain::default_chain();
        // 半角英数字なので ZenkakuAlnum のみ該当（HankakuAlnum・HankakuKatakana は空を返す）
        assert_eq!(chain.candidates("abc123"), vec!["ａｂｃ１２３".to_string()]);
    }

    #[test]
    fn rewriter_chain_empty_never_produces_candidates() {
        let chain = RewriterChain::empty();
        assert_eq!(chain.candidates("abc123"), Vec::<String>::new());
    }

    #[test]
    fn kanji_digits_replaces_each_digit() {
        let r = KanjiDigits;
        assert_eq!(r.rewrite("2026"), vec!["二〇二六".to_string()]);
        assert_eq!(r.rewrite("0"), vec!["〇".to_string()]);
        assert_eq!(r.rewrite(""), Vec::<String>::new());
        assert_eq!(r.rewrite("12a"), Vec::<String>::new());
    }

    #[test]
    fn kanji_positional_numeral_basic_cases() {
        let r = KanjiPositionalNumeral;
        assert_eq!(r.rewrite("0"), vec!["〇".to_string()]);
        assert_eq!(r.rewrite("10"), vec!["十".to_string()]);
        assert_eq!(r.rewrite("11"), vec!["十一".to_string()]);
        assert_eq!(r.rewrite("100"), vec!["百".to_string()]);
        assert_eq!(r.rewrite("1000"), vec!["千".to_string()]);
        assert_eq!(r.rewrite("2026"), vec!["二千二十六".to_string()]);
        assert_eq!(r.rewrite("10000"), vec!["一万".to_string()]);
        assert_eq!(r.rewrite("100000000"), vec!["一億".to_string()]);
        assert_eq!(
            r.rewrite("9999999999999999"),
            vec!["九千九百九十九兆九千九百九十九億九千九百九十九万九千九百九十九".to_string()]
        );
    }

    #[test]
    fn kanji_positional_numeral_rejects_ambiguous_or_out_of_range() {
        let r = KanjiPositionalNumeral;
        // 先頭ゼロ（"0" 単独を除く）は位取りが定義できない
        assert_eq!(r.rewrite("007"), Vec::<String>::new());
        assert_eq!(r.rewrite("00"), Vec::<String>::new());
        // 空・数字以外
        assert_eq!(r.rewrite(""), Vec::<String>::new());
        assert_eq!(r.rewrite("12a"), Vec::<String>::new());
        // 兆の桁（10^16 = 京）を超える17桁は対象外
        assert_eq!(r.rewrite("99999999999999999"), Vec::<String>::new());
    }

    #[test]
    fn rewriter_chain_default_includes_kanji_numerals() {
        let chain = RewriterChain::default_chain();
        let candidates = chain.candidates("2026");
        assert!(
            candidates.contains(&"二〇二六".to_string()),
            "{candidates:?}"
        );
        assert!(
            candidates.contains(&"二千二十六".to_string()),
            "{candidates:?}"
        );
    }

    #[test]
    fn digits_to_date_converts_valid_date() {
        let r = DigitsToDate;
        assert_eq!(
            r.rewrite("20260927"),
            vec!["2026/09/27".to_string(), "2026年9月27日".to_string()]
        );
        assert_eq!(
            r.rewrite("20000101"),
            vec!["2000/01/01".to_string(), "2000年1月1日".to_string()]
        );
    }

    #[test]
    fn digits_to_date_rejects_invalid_date() {
        let r = DigitsToDate;
        // 2026年は平年（うるう年ではない）なので2月は28日まで
        assert_eq!(r.rewrite("20260230"), Vec::<String>::new());
        // 13月は存在しない
        assert_eq!(r.rewrite("20261301"), Vec::<String>::new());
        // 0日は存在しない
        assert_eq!(r.rewrite("20260100"), Vec::<String>::new());
    }

    #[test]
    fn digits_to_date_accepts_leap_year_feb_29() {
        let r = DigitsToDate;
        // 2024年はうるう年
        assert_eq!(
            r.rewrite("20240229"),
            vec!["2024/02/29".to_string(), "2024年2月29日".to_string()]
        );
    }

    #[test]
    fn digits_to_date_rejects_wrong_length_or_non_digits() {
        let r = DigitsToDate;
        assert_eq!(r.rewrite("2026927"), Vec::<String>::new());
        assert_eq!(r.rewrite("202609271"), Vec::<String>::new());
        assert_eq!(r.rewrite("2026092a"), Vec::<String>::new());
        assert_eq!(r.rewrite(""), Vec::<String>::new());
    }

    #[test]
    fn simple_calculator_basic_arithmetic() {
        let r = SimpleCalculator;
        assert_eq!(r.rewrite("3+4"), vec!["7".to_string()]);
        assert_eq!(r.rewrite("10-3"), vec!["7".to_string()]);
        assert_eq!(r.rewrite("3*4"), vec!["12".to_string()]);
        assert_eq!(r.rewrite("7/2"), vec!["3.5".to_string()]);
        assert_eq!(r.rewrite("10/3"), vec!["3.3333".to_string()]);
        assert_eq!(r.rewrite("1/3"), vec!["0.3333".to_string()]);
        assert_eq!(r.rewrite("0/5"), vec!["0".to_string()]);
        assert_eq!(r.rewrite("2.5+2.5"), vec!["5".to_string()]);
        assert_eq!(r.rewrite("1.1+2.2"), vec!["3.3".to_string()]);
    }

    #[test]
    fn simple_calculator_rejects_invalid_or_unsafe_input() {
        let r = SimpleCalculator;
        // 0除算
        assert_eq!(r.rewrite("5/0"), Vec::<String>::new());
        // 先頭の符号は対象外
        assert_eq!(r.rewrite("-3+4"), Vec::<String>::new());
        // 演算子が無い・数字以外
        assert_eq!(r.rewrite("abc"), Vec::<String>::new());
        assert_eq!(r.rewrite(""), Vec::<String>::new());
        // 演算子の前後が空
        assert_eq!(r.rewrite("+4"), Vec::<String>::new());
        assert_eq!(r.rewrite("4+"), Vec::<String>::new());
    }

    #[test]
    fn rewriter_chain_default_includes_date_and_calculator() {
        let chain = RewriterChain::default_chain();
        assert!(
            chain
                .candidates("20260927")
                .contains(&"2026/09/27".to_string()),
            "{:?}",
            chain.candidates("20260927")
        );
        assert!(
            chain.candidates("3+4").contains(&"7".to_string()),
            "{:?}",
            chain.candidates("3+4")
        );
    }
}
