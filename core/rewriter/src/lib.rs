//! Rewriter: 決定的変換の最終段（設計書 §4.13）。
//!
//! LLM を使わない決定的な表記変換（全角/半角、カタカナ/半角カナ 等）を候補として提供する。
//! trait でプラグイン追加可能。v1 は全角/半角英数記号・半角カナのみ（日付・簡易計算は対象外）。

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
}
