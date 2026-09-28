//! 品詞（`id.def` の各行）の分類。文節区切りと未知語処理で使う。

/// 主分類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PosClass {
    /// BOS/EOS（ID 0）
    BosEos,
    /// 名詞
    Noun,
    /// 動詞
    Verb,
    /// 形容詞
    Adjective,
    /// 副詞
    Adverb,
    /// 連体詞
    Adnominal,
    /// 接続詞
    Conjunction,
    /// 感動詞
    Interjection,
    /// フィラー
    Filler,
    /// 助詞
    Particle,
    /// 助動詞
    AuxVerb,
    /// 接頭詞
    Prefix,
    /// 記号
    Symbol,
    /// その他
    Other,
}

/// 細分類 1 のうち文節区切りに関わるもの
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PosSub {
    /// 上記以外
    Other,
    /// 非自立
    Dependent,
    /// 接尾
    Suffix,
    /// 特殊（助動詞語幹など）
    Special,
    /// 数
    Number,
    /// 句点
    Period,
    /// 読点
    Comma,
    /// 括弧開
    BracketOpen,
    /// 括弧閉
    BracketClose,
}

/// 1 つの品詞 ID の分類結果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    /// 主分類
    pub class: PosClass,
    /// 細分類 1
    pub sub: PosSub,
}

impl Pos {
    /// `id.def` の品詞文字列（例 `名詞,一般,*,*,*,*,*`）を分類する
    pub fn parse(def: &str) -> Pos {
        let mut it = def.split(',');
        let major = it.next().unwrap_or("");
        let sub1 = it.next().unwrap_or("*");
        let class = match major {
            "BOS/EOS" => PosClass::BosEos,
            "名詞" => PosClass::Noun,
            "動詞" => PosClass::Verb,
            "形容詞" => PosClass::Adjective,
            "副詞" => PosClass::Adverb,
            "連体詞" => PosClass::Adnominal,
            "接続詞" => PosClass::Conjunction,
            "感動詞" => PosClass::Interjection,
            "フィラー" => PosClass::Filler,
            "助詞" => PosClass::Particle,
            "助動詞" => PosClass::AuxVerb,
            "接頭詞" => PosClass::Prefix,
            "記号" => PosClass::Symbol,
            _ => PosClass::Other,
        };
        let sub = match sub1 {
            "非自立" => PosSub::Dependent,
            "接尾" => PosSub::Suffix,
            "特殊" => PosSub::Special,
            "数" => PosSub::Number,
            "句点" => PosSub::Period,
            "読点" => PosSub::Comma,
            "括弧開" => PosSub::BracketOpen,
            "括弧閉" => PosSub::BracketClose,
            _ => PosSub::Other,
        };
        Pos { class, sub }
    }
}

/// SudachiDict の品詞1・2要素（POS1, POS2）から、`Pos::parse` が正しく解釈できる
/// Mozc `id.def` 互換の品詞文字列を組み立てる。
pub fn sudachi_pos_to_mozc_like(pos1: &str, pos2: &str) -> String {
    match (pos1, pos2) {
        ("代名詞", _) => "名詞,代名詞,*,*,*,*,*".to_string(),
        ("名詞", "固有名詞") => "名詞,固有名詞,*,*,*,*,*".to_string(),
        ("名詞", "数詞") => "名詞,数,*,*,*,*,*".to_string(),
        ("名詞", _) => "名詞,一般,*,*,*,*,*".to_string(),
        ("動詞", "非自立可能") => "動詞,非自立,*,*,*,*,*".to_string(),
        ("動詞", _) => "動詞,自立,*,*,*,*,*".to_string(),
        ("形容詞", "非自立可能") => "形容詞,非自立,*,*,*,*,*".to_string(),
        ("形容詞", _) => "形容詞,自立,*,*,*,*,*".to_string(),
        ("形状詞", _) => "名詞,形容動詞語幹,*,*,*,*,*".to_string(),
        ("連体詞", _) => "連体詞,*,*,*,*,*,*".to_string(),
        ("副詞", _) => "副詞,一般,*,*,*,*,*".to_string(),
        ("接続詞", _) => "接続詞,*,*,*,*,*,*".to_string(),
        ("感動詞", "フィラー") => "フィラー,*,*,*,*,*,*".to_string(),
        ("感動詞", _) => "感動詞,*,*,*,*,*,*".to_string(),
        ("助詞", _) => format!("助詞,{pos2},*,*,*,*,*"),
        ("助動詞", _) => "助動詞,*,*,*,*,*,*".to_string(),
        ("接頭辞", _) => "接頭詞,*,*,*,*,*,*".to_string(),
        ("接尾辞", "動詞的") => "動詞,接尾,*,*,*,*,*".to_string(),
        ("接尾辞", "形容詞的") => "形容詞,接尾,*,*,*,*,*".to_string(),
        ("接尾辞", _) => "名詞,接尾,*,*,*,*,*".to_string(),
        ("補助記号", "句点") => "記号,句点,*,*,*,*,。".to_string(),
        ("補助記号", "読点") => "記号,読点,*,*,*,*,、".to_string(),
        ("補助記号", "括弧開") => "記号,括弧開,*,*,*,*,*".to_string(),
        ("補助記号", "括弧閉") => "記号,括弧閉,*,*,*,*,*".to_string(),
        ("補助記号" | "記号", _) => "記号,一般,*,*,*,*,*".to_string(),
        ("空白", _) => "記号,空白,*,*,*,*,*".to_string(),
        _ => "名詞,一般,*,*,*,*,*".to_string(),
    }
}
