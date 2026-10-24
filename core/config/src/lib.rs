//! Nagi 設定: config.toml の読み込み・検証・行番号付きエラー。
//!
//! # 概要
//!
//! `config.toml` から設定を読み込み、TOML パースエラー、不明キー、範囲外の値を
//! 行番号付きで報告する。各項目は `Option` で未指定を区別でき、未指定ならプロファイルが決める。
//!
//! # 使用例
//!
//! ```ignore
//! let (config, warnings) = nagi_config::parse(source)?;
//! let (config, warnings) = nagi_config::load_file(path)?;
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub mod settings;

/// エラー・警告の重大度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 警告（値が無視されるが、処理は続行）。
    Warning,
    /// エラー（処理を中止）。
    Error,
}

/// エラー・警告の情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// 行番号（1 始まり。特定できなければ 0）。
    pub line: u32,
    /// 重大度。
    pub severity: Severity,
    /// メッセージ。
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "(不明):: {}: {}", self.severity, self.message)?;
        } else {
            write!(f, "{}:: {}: {}", self.line, self.severity, self.message)?;
        }
        Ok(())
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

/// 環境プロファイル。品質と軽さのトレードオフを表現。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileMode {
    /// 自動判定（既定）。
    Auto,
    /// 精度最優先。
    Quality,
    /// 精度と軽さのバランス。
    Balanced,
    /// 軽さ優先。
    Light,
    /// 辞書のみ（モデルなし）。
    Minimal,
}

impl ProfileMode {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            ProfileMode::Auto => "auto",
            ProfileMode::Quality => "quality",
            ProfileMode::Balanced => "balanced",
            ProfileMode::Light => "light",
            ProfileMode::Minimal => "minimal",
        }
    }
}

impl fmt::Display for ProfileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 変換の初期表示形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InitialView {
    /// 文全体の N-best を表示（既定）。
    Sentence,
    /// 先頭文節の候補を表示。
    Segment,
}

impl InitialView {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            InitialView::Sentence => "sentence",
            InitialView::Segment => "segment",
        }
    }
}

impl fmt::Display for InitialView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Shift + 英字の動作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftLetter {
    /// 大文字を Literal で挿入し、後続も英字で続ける（既定）。
    LiteralContinue,
    /// 大文字 1 文字だけを Literal で挿入。
    SingleUpper,
}

impl ShiftLetter {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            ShiftLetter::LiteralContinue => "literal_continue",
            ShiftLetter::SingleUpper => "single_upper",
        }
    }
}

impl fmt::Display for ShiftLetter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// フリック入力の濁点・半濁点・小書きキー（゛゜小）の循環順（タッチ入力のみ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DakutenCycle {
    /// 小書きを先にする（既定。つ→っ→づ、う→ぅ→ゔ。iOS 標準と同じ）。
    SmallFirst,
    /// 濁点を先にする（つ→づ→っ、う→ゔ→ぅ）。
    DakutenFirst,
}

impl DakutenCycle {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            DakutenCycle::SmallFirst => "small_first",
            DakutenCycle::DakutenFirst => "dakuten_first",
        }
    }
}

impl fmt::Display for DakutenCycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 句読点の種類。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Punctuation {
    /// 句点（。/ ．）。
    pub period: char,
    /// 読点（、/ ，）。
    pub comma: char,
}

impl Punctuation {
    /// `"、。"` から `Punctuation` を解析。
    fn from_str(s: &str) -> Result<Self, String> {
        if s == "、。" {
            Ok(Punctuation {
                period: '。',
                comma: '、',
            })
        } else if s == "，．" {
            Ok(Punctuation {
                period: '．',
                comma: '，',
            })
        } else if s == "，。" {
            Ok(Punctuation {
                period: '。',
                comma: '，',
            })
        } else {
            Err(format!(
                "句読点は '、。', '，．', '，。' のいずれかである必要があります: '{}'",
                s
            ))
        }
    }
}

impl Default for Punctuation {
    fn default() -> Self {
        Punctuation {
            period: '。',
            comma: '、',
        }
    }
}

// TOML の文字列から Punctuation をデシリアライズする。
mod punct_serde {
    use super::Punctuation;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Punctuation, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("{}{}", value.comma, value.period);
        s.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Punctuation, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Punctuation::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// 環境プロファイルセクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileSection {
    /// プロファイルモード。既定: Auto。
    pub mode: ProfileMode,
    /// 環境の変化で自動切替するかどうか。既定: true。
    pub auto_switch: bool,
    /// ユーザーのペルソナ（スタイル変換等で使用）。既定: ""。
    pub persona: String,
}

impl Default for ProfileSection {
    fn default() -> Self {
        ProfileSection {
            mode: ProfileMode::Auto,
            auto_switch: true,
            persona: String::new(),
        }
    }
}

/// 変換セクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversionSection {
    /// ライブ変換を有効にするかどうか。既定: true。
    pub live_conversion: bool,
    /// 変換の初期表示形式。既定: Sentence。
    pub initial_view: InitialView,
    /// ライブ変換の時のチャンク長。既定: 30。
    pub composing_chunk_len: usize,
    /// LLM ビームサーチの幅。未指定 = プロファイルに従う。既定: None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beam_width: Option<usize>,
    /// 候補ウィンドウ 1 ページの候補数。既定: 9。
    pub num_candidates: usize,
    /// 推論スレッド数。0 = 自動。既定: 0。
    pub n_threads: usize,
    /// モデル ID または GGUF パス。未指定 = プロファイルに従う。既定: None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// LLM 推論を使うかどうか。false なら辞書のみで変換する。既定: true。
    pub use_llm: bool,
    /// 文脈を使用するかどうか。既定: true。
    pub use_context: bool,
    /// 文脈の最大文字数。未指定 = プロファイルに従う。既定: None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_length: Option<usize>,
    /// 推論の最大レイテンシ（ミリ秒）。既定: 100。
    pub max_latency_ms: u64,
    /// 同期応答の最大予算（ミリ秒）。既定: 150。
    pub sync_budget_ms: u64,
    /// ライブ変換の入力から実際に変換するまでの遅延（ミリ秒）。既定: 228（Mozkey の実測値を参考）。
    pub live_conversion_delay_ms: u64,
    /// ライブ変換を試みる最小文字数（読み・Unicodeスカラー単位）。既定: 2。
    pub live_conversion_min_key_length: usize,
}

impl Default for ConversionSection {
    fn default() -> Self {
        ConversionSection {
            live_conversion: true,
            initial_view: InitialView::Sentence,
            composing_chunk_len: 30,
            beam_width: None,
            num_candidates: 9,
            n_threads: 0,
            model: None,
            use_llm: true,
            use_context: true,
            max_context_length: None,
            max_latency_ms: 100,
            sync_budget_ms: 150,
            live_conversion_delay_ms: 228,
            live_conversion_min_key_length: 2,
        }
    }
}

/// 曖昧入力（タイプミス補正）の強さ。補正として許す打ち間違いの量（累積コスト上限）を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FuzzyStrength {
    /// 控えめ（1 操作まで程度）。
    Gentle,
    /// 標準（2 操作程度）。既定。
    Normal,
    /// 強め（3 操作程度。誤補正が増える）。
    Aggressive,
}

impl FuzzyStrength {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            FuzzyStrength::Gentle => "gentle",
            FuzzyStrength::Normal => "normal",
            FuzzyStrength::Aggressive => "aggressive",
        }
    }

    /// 補正の累積コスト上限（`nagi-fuzzy` の編集コストと同じ尺度。置換 2.0〜3.0・転置 2.0 など）。
    pub fn max_cost(&self) -> f32 {
        match self {
            FuzzyStrength::Gentle => 3.0,
            FuzzyStrength::Normal => 4.5,
            FuzzyStrength::Aggressive => 6.5,
        }
    }
}

impl fmt::Display for FuzzyStrength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 曖昧入力（`[input.fuzzy]`）。キー配置に応じたタイプミスを「もしかして」候補として混ぜる。
/// 入力中の読みは打った通りに保ち、自動で置き換えない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FuzzySection {
    /// 曖昧入力（補正候補）を使うかどうか。既定: false。
    pub enabled: bool,
    /// 補正の強さ。既定: Normal。
    pub strength: FuzzyStrength,
    /// 候補に混ぜる補正候補の最大数。1〜5。既定: 3。
    pub max_candidates: usize,
    /// 使う補正の種類（`substitute,delete,insert,transpose,voicing` のカンマ区切り）。既定: 全部。
    pub kinds: String,
    /// タッチ座標（キー内の位置・フリックの角度）を補正のコストに使うか。既定: true。
    /// 座標を送らない入力（物理キーボード）には影響しない。
    pub use_touch_coords: bool,
}

/// `input.fuzzy.kinds` の既定値（全種類）。
pub const FUZZY_KINDS_ALL: &str = "substitute,delete,insert,transpose,voicing";

impl Default for FuzzySection {
    fn default() -> Self {
        FuzzySection {
            enabled: false,
            strength: FuzzyStrength::Normal,
            max_candidates: 3,
            kinds: FUZZY_KINDS_ALL.to_string(),
            use_touch_coords: true,
        }
    }
}

/// 英語の自動判定の強さ（`[input.auto_english] mode`）。ローマ字のまま打った英単語を、
/// 日本語入力を切り替えずに英語として入力できるようにする（`docs/plan-auto-english.md`）。
/// 段階 1（Space の候補に ASCII を足す）だけが実装済みで、`Standard` と `Aggressive` は
/// 段階 2 以降（先頭の候補を ASCII にする・入力中に切り替える）の予約として、
/// 現状は `Conservative` と同じ動作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoEnglishMode {
    /// 判定しない。
    Off,
    /// 慎重。自信が高いときだけ、Space の候補の上位に ASCII を置く。自動では変えない。既定。
    Conservative,
    /// 標準（予約。現状は慎重と同じ）。
    Standard,
    /// 積極的（予約。現状は慎重と同じ）。
    Aggressive,
}

impl AutoEnglishMode {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            AutoEnglishMode::Off => "off",
            AutoEnglishMode::Conservative => "conservative",
            AutoEnglishMode::Standard => "standard",
            AutoEnglishMode::Aggressive => "aggressive",
        }
    }
}

impl fmt::Display for AutoEnglishMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 英語の自動判定（`[input.auto_english]`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoEnglishSection {
    /// 判定の強さ。既定: Conservative（段階 1 のみ）。
    pub mode: AutoEnglishMode,
}

impl Default for AutoEnglishSection {
    fn default() -> Self {
        AutoEnglishSection {
            mode: AutoEnglishMode::Conservative,
        }
    }
}

/// 入力セクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputSection {
    /// Shift + 英字の動作。既定: LiteralContinue。
    pub shift_letter: ShiftLetter,
    /// 句読点の種類。既定: 、。。
    #[serde(with = "punct_serde")]
    pub punctuation: Punctuation,
    /// 濁点・小書きキーの循環順（タッチ入力のみ）。既定: SmallFirst（従来の動作）。
    pub dakuten_cycle: DakutenCycle,
    /// Backspace で文字を消したとき、残った未変換のローマ字を保留に戻して続けて入力できるようにする（H-23）。既定: true。
    pub romaji_carryover: bool,
    /// 曖昧入力（タイプミス補正の候補）。
    pub fuzzy: FuzzySection,
    /// 英語の自動判定。
    pub auto_english: AutoEnglishSection,
}

impl Default for InputSection {
    fn default() -> Self {
        InputSection {
            shift_letter: ShiftLetter::LiteralContinue,
            punctuation: Punctuation::default(),
            dakuten_cycle: DakutenCycle::SmallFirst,
            romaji_carryover: true,
            fuzzy: FuzzySection::default(),
            auto_english: AutoEnglishSection::default(),
        }
    }
}

/// ユーザー辞書セクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserdictSection {
    /// ユーザー辞書の語を使うかどうか（無効でも登録内容は消えない）。既定: true。
    pub enabled: bool,
}

impl Default for UserdictSection {
    fn default() -> Self {
        UserdictSection { enabled: true }
    }
}

/// スニペットセクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnippetSection {
    /// スニペット（短い読みで定型文）を使うかどうか。既定: true。
    pub enabled: bool,
}

impl Default for SnippetSection {
    fn default() -> Self {
        SnippetSection { enabled: true }
    }
}

/// ランキングセクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RankingSection {
    /// 候補の優先順序（出所の順序）。既定: ["learned","user_dict","llm","system_dict","kana","rewriter"]。
    pub source_order: Vec<String>,
}

impl Default for RankingSection {
    fn default() -> Self {
        RankingSection {
            source_order: vec![
                "learned".to_string(),
                "user_dict".to_string(),
                "llm".to_string(),
                "system_dict".to_string(),
                "kana".to_string(),
                "rewriter".to_string(),
            ],
        }
    }
}

/// 学習セクション。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningSection {
    /// 学習を有効にするかどうか。既定: true。
    pub enabled: bool,
    /// 学習データの最大エントリ数。既定: 10000。
    pub max_entries: usize,
    /// 学習の強さ（ランキングへの加点の倍率）。0.0〜2.0。既定: 1.0。
    pub strength: f64,
    /// 最終使用からこの日数を過ぎた学習を忘れる。0 = 忘れない。0〜3650。既定: 0。
    pub forget_days: u32,
    /// 学習を無効にするアプリのリスト。既定: パスワードマネージャ等。
    pub disabled_apps: Vec<String>,
}

impl Default for LearningSection {
    fn default() -> Self {
        LearningSection {
            enabled: true,
            max_entries: 10000,
            strength: 1.0,
            forget_days: 0,
            disabled_apps: vec![
                "com.1password.1password".to_string(),
                "com.agilebits.onepassword7".to_string(),
                "com.bitwarden.desktop".to_string(),
                "org.keepassxc.keepassxc".to_string(),
                "com.lastpass.LastPass".to_string(),
            ],
        }
    }
}

/// 予測変換セクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PredictSection {
    /// 予測変換を行うかどうか。既定: true。
    pub enabled: bool,
    /// 予測変換を表示する最小の読みの文字数。既定: 1。
    pub min_chars: usize,
    /// 確定直後の次語予測（学習した「直前の語 → 次の語」から候補を出す）。既定: false。
    pub next_word: bool,
    /// 次語予測の候補数。既定: 5。
    pub next_word_count: usize,
}

impl Default for PredictSection {
    fn default() -> Self {
        PredictSection {
            enabled: true,
            min_chars: 1,
            next_word: false,
            next_word_count: 5,
        }
    }
}

/// Rewriter セクション（決定的な表記変換の候補）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RewriterSection {
    /// 読みから絵文字の候補を出すかどうか。既定: true。
    pub emoji: bool,
    /// 読みから顔文字の候補を出すかどうか。既定: true。
    pub kaomoji: bool,
}

impl Default for RewriterSection {
    fn default() -> Self {
        RewriterSection {
            emoji: true,
            kaomoji: true,
        }
    }
}

/// 入力中の候補に出す記号（`かっこ` → `「」` など）の位置（`convert.symbol_position`。H-63）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolPosition {
    /// 候補の先頭。
    First,
    /// 最良の変換の直後（既定。従来どおり）。
    AfterBest,
    /// 候補の末尾。
    Last,
    /// 入力中の候補には出さない（Space の候補窓の Rewriter は従来どおり）。
    Off,
}

impl SymbolPosition {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolPosition::First => "first",
            SymbolPosition::AfterBest => "after_best",
            SymbolPosition::Last => "last",
            SymbolPosition::Off => "off",
        }
    }
}

impl fmt::Display for SymbolPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 変換結果の表記に関するセクション（`docs/ios-settings.md` §2 の `convert.*`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConvertSection {
    /// 電卓候補（`1+2` → `3`）を出すかどうか。既定: true。
    pub calculator: bool,
    /// カタカナ語の原語の候補（`[convert.loanword]`）。
    pub loanword: LoanwordSection,
    /// 入力中の候補（サジェスト）の最大件数。読みの接頭辞だけを置き換える候補
    /// （フロントが対応を宣言したときだけ出る。H-58）の総数の上限。既定: 120。
    pub suggest_max: usize,
    /// 入力中の候補に出す記号（`かっこ` → `「」` など）の位置。既定: `after_best`（最良の変換の直後）。
    pub symbol_position: SymbolPosition,
}

impl Default for ConvertSection {
    fn default() -> Self {
        ConvertSection {
            calculator: true,
            loanword: LoanwordSection::default(),
            suggest_max: 120,
            symbol_position: SymbolPosition::AfterBest,
        }
    }
}

/// カタカナ語の原語の候補（`[convert.loanword]`。`docs/plan-loanword-translation.md`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoanwordSection {
    /// カタカナ語の原語（コンピューター → computer、アルバイト → Arbeit）を変換候補に出すか。
    /// 既定: true。データ（`data/loanword/loanword.fst`）が無ければ出ない。
    pub enabled: bool,
}

impl Default for LoanwordSection {
    fn default() -> Self {
        LoanwordSection { enabled: true }
    }
}

/// `[convert.loanword]` の既知キー。
const LOANWORD_KEYS: &[&str] = &["enabled"];

/// アプリの種類（`[apps]` の分類）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppClass {
    /// コードエディタ・ターミナル。
    Code,
    /// チャット・メッセンジャー。
    Chat,
    /// それ以外。
    General,
}

/// フォーカスを得たときの入力モード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppDefaultMode {
    /// 変えない（前回のまま）。
    Keep,
    /// ひらがな。
    Hiragana,
    /// 英数。
    Alphanumeric,
}

impl AppDefaultMode {
    /// 文字列表現を返す。
    pub fn as_str(&self) -> &'static str {
        match self {
            AppDefaultMode::Keep => "keep",
            AppDefaultMode::Hiragana => "hiragana",
            AppDefaultMode::Alphanumeric => "alphanumeric",
        }
    }
}

/// 同梱のアプリ分類表（パターン → 種類）。末尾が `*` なら前方一致、それ以外は完全一致。
/// 大文字小文字は区別しない（Windows の実行ファイル名のため）。macOS は bundle id、Windows は実行ファイル名。
pub const DEFAULT_APP_CLASSES: &[(&str, AppClass)] = &[
    // macOS: ターミナル・コードエディタ
    ("com.apple.Terminal", AppClass::Code),
    ("com.googlecode.iterm2", AppClass::Code),
    ("com.mitchellh.ghostty", AppClass::Code),
    ("net.kovidgoyal.kitty", AppClass::Code),
    ("org.alacritty", AppClass::Code),
    ("com.github.wez.wezterm", AppClass::Code),
    ("dev.warp.Warp-Stable", AppClass::Code),
    ("co.zeit.hyper", AppClass::Code),
    ("com.microsoft.VSCode", AppClass::Code),
    ("com.microsoft.VSCodeInsiders", AppClass::Code),
    ("com.todesktop.230313mzl4w4u92", AppClass::Code),
    ("com.vscodium", AppClass::Code),
    ("com.jetbrains.*", AppClass::Code),
    ("com.apple.dt.Xcode", AppClass::Code),
    ("dev.zed.Zed", AppClass::Code),
    ("com.sublimetext.*", AppClass::Code),
    ("com.panic.Nova", AppClass::Code),
    ("org.vim.MacVim", AppClass::Code),
    // Windows: 実行ファイル名
    ("WindowsTerminal.exe", AppClass::Code),
    ("cmd.exe", AppClass::Code),
    ("powershell.exe", AppClass::Code),
    ("pwsh.exe", AppClass::Code),
    ("wezterm-gui.exe", AppClass::Code),
    ("alacritty.exe", AppClass::Code),
    ("Code.exe", AppClass::Code),
    ("Cursor.exe", AppClass::Code),
    ("devenv.exe", AppClass::Code),
    ("idea64.exe", AppClass::Code),
    ("pycharm64.exe", AppClass::Code),
    ("webstorm64.exe", AppClass::Code),
    ("sublime_text.exe", AppClass::Code),
    // チャット
    ("com.tinyspeck.slackmacgap", AppClass::Chat),
    ("com.hnc.Discord", AppClass::Chat),
    ("jp.naver.line.mac", AppClass::Chat),
    ("com.apple.MobileSMS", AppClass::Chat),
    ("ru.keepcoder.Telegram", AppClass::Chat),
    ("com.microsoft.teams2", AppClass::Chat),
    ("us.zoom.xos", AppClass::Chat),
    ("Slack.exe", AppClass::Chat),
    ("Discord.exe", AppClass::Chat),
    ("LINE.exe", AppClass::Chat),
    ("Telegram.exe", AppClass::Chat),
];

/// アプリごとの既定モード・素通しのセクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppsSection {
    /// `code` のアプリでフォーカスを得たときのモード。既定: alphanumeric。
    pub code_mode: AppDefaultMode,
    /// `chat` のアプリでフォーカスを得たときのモード。既定: keep。
    pub chat_mode: AppDefaultMode,
    /// `general`（上記以外）のアプリでフォーカスを得たときのモード。既定: keep。
    pub general_mode: AppDefaultMode,
    /// アプリごとに、手で切り替えたモードを覚えるか。既定: true。
    pub remember_mode: bool,
    /// 未入力で先頭が対象の記号のとき、その語を日本語変換せず ASCII で入力するか。既定: true。
    pub passthrough: bool,
    /// 素通しの先頭記号（ASCII の記号を並べた文字列）。既定: "/$@!:#"。
    pub passthrough_chars: String,
    /// 利用者の分類表（パターン → 種類）。同梱の表より優先する。既定: 空。
    pub classes: std::collections::HashMap<String, AppClass>,
}

impl Default for AppsSection {
    fn default() -> Self {
        AppsSection {
            code_mode: AppDefaultMode::Alphanumeric,
            chat_mode: AppDefaultMode::Keep,
            general_mode: AppDefaultMode::Keep,
            remember_mode: true,
            passthrough: true,
            passthrough_chars: "/$@!:#".to_string(),
            classes: Default::default(),
        }
    }
}

impl AppsSection {
    /// 分類の規則を優先順に並べる。利用者の表が同梱の表より先で、それぞれ完全一致が前方一致より先、
    /// 前方一致は長いパターンが先。大文字小文字は区別しないため、パターンは小文字にして返す
    pub fn rules(&self) -> Vec<(String, AppClass)> {
        fn ordered(mut rules: Vec<(String, AppClass)>) -> Vec<(String, AppClass)> {
            rules.sort_by(|a, b| {
                let prefix = |p: &str| p.ends_with('*');
                prefix(&a.0)
                    .cmp(&prefix(&b.0))
                    .then(b.0.len().cmp(&a.0.len()))
                    .then(a.0.cmp(&b.0))
            });
            rules
        }
        let user = self
            .classes
            .iter()
            .map(|(k, v)| (k.trim().to_lowercase(), *v))
            .filter(|(k, _)| !k.is_empty())
            .collect();
        let builtin = DEFAULT_APP_CLASSES
            .iter()
            .map(|(k, v)| (k.to_lowercase(), *v))
            .collect();
        let mut rules = ordered(user);
        rules.extend(ordered(builtin));
        rules
    }

    /// `app_id` の種類。どの規則にも当たらなければ `General`
    pub fn classify(&self, app_id: &str) -> AppClass {
        let id = app_id.to_lowercase();
        for (pattern, class) in self.rules() {
            let hit = match pattern.strip_suffix('*') {
                Some(prefix) => id.starts_with(prefix),
                None => id == pattern,
            };
            if hit {
                return class;
            }
        }
        AppClass::General
    }

    /// 種類ごとの、フォーカスを得たときのモード
    pub fn mode_for(&self, class: AppClass) -> AppDefaultMode {
        match class {
            AppClass::Code => self.code_mode,
            AppClass::Chat => self.chat_mode,
            AppClass::General => self.general_mode,
        }
    }
}

/// キーマップセクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeymapSection {
    /// キーマップのプリセット名。既定: "default"。
    pub preset: String,
    /// ローマ字入力中（Composing）のキー上書き。キー: キー仕様文字列（"Ctrl+j" 等）、
    /// 値: Action 仕様文字列。既定: 空。
    pub composing: std::collections::HashMap<String, String>,
    /// 変換中（Converting）のキー上書き。既定: 空。
    pub converting: std::collections::HashMap<String, String>,
}

impl Default for KeymapSection {
    fn default() -> Self {
        KeymapSection {
            preset: "default".to_string(),
            composing: Default::default(),
            converting: Default::default(),
        }
    }
}

/// アップデートセクション。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSection {
    /// 自動的にアップデートをチェックするかどうか。既定: true。
    pub auto_check: bool,
    /// アップデート確認マニフェストの URL。未指定なら既定の URL
    /// （`nagi_server::update_check::default_check_url()`。OS ごと）を使う。既定: 未指定。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_url: Option<String>,
}

impl Default for UpdateSection {
    fn default() -> Self {
        UpdateSection {
            auto_check: true,
            check_url: None,
        }
    }
}

/// Nagi の全設定。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 環境プロファイルセクション。
    pub profile: ProfileSection,
    /// 変換セクション。
    pub conversion: ConversionSection,
    /// 入力セクション。
    pub input: InputSection,
    /// ランキングセクション。
    pub ranking: RankingSection,
    /// 学習セクション。
    pub learning: LearningSection,
    /// 予測変換セクション。
    pub predict: PredictSection,
    /// Rewriter セクション。
    pub rewriter: RewriterSection,
    /// 変換結果の表記セクション。
    pub convert: ConvertSection,
    /// ユーザー辞書セクション。
    pub userdict: UserdictSection,
    /// スニペットセクション。
    pub snippet: SnippetSection,
    /// キーマップセクション。
    pub keymap: KeymapSection,
    /// アップデートセクション。
    pub update: UpdateSection,
    /// アプリごとの既定モード・素通しのセクション。
    pub apps: AppsSection,
}

impl Config {
    /// 設定の値の範囲を検証。エラーのリストを返す。
    #[allow(clippy::collapsible_if)]
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // beam_width: 1 <= beam_width <= 16
        if let Some(bw) = self.conversion.beam_width {
            if !(1..=16).contains(&bw) {
                errors.push(format!(
                    "conversion.beam_width は 1 から 16 の間である必要があります: {}",
                    bw
                ));
            }
        }

        // composing_chunk_len: 1 <= composing_chunk_len <= 200
        if !(1..=200).contains(&self.conversion.composing_chunk_len) {
            errors.push(format!(
                "conversion.composing_chunk_len は 1 から 200 の間である必要があります: {}",
                self.conversion.composing_chunk_len
            ));
        }

        // num_candidates: 1 <= num_candidates <= 50
        if !(1..=50).contains(&self.conversion.num_candidates) {
            errors.push(format!(
                "conversion.num_candidates は 1 から 50 の間である必要があります: {}",
                self.conversion.num_candidates
            ));
        }

        // max_context_length: 1 <= max_context_length <= 200
        if let Some(mcl) = self.conversion.max_context_length {
            if !(1..=200).contains(&mcl) {
                errors.push(format!(
                    "conversion.max_context_length は 1 から 200 の間である必要があります: {}",
                    mcl
                ));
            }
        }

        // max_latency_ms: 10 <= max_latency_ms <= 10000
        if !(10..=10000).contains(&self.conversion.max_latency_ms) {
            errors.push(format!(
                "conversion.max_latency_ms は 10 から 10000 の間である必要があります: {}",
                self.conversion.max_latency_ms
            ));
        }

        // sync_budget_ms: 10 <= sync_budget_ms <= 5000
        if !(10..=5000).contains(&self.conversion.sync_budget_ms) {
            errors.push(format!(
                "conversion.sync_budget_ms は 10 から 5000 の間である必要があります: {}",
                self.conversion.sync_budget_ms
            ));
        }

        // input.fuzzy.max_candidates: 1 <= max_candidates <= 5
        if !(1..=5).contains(&self.input.fuzzy.max_candidates) {
            errors.push(format!(
                "input.fuzzy.max_candidates は 1 から 5 の間である必要があります: {}",
                self.input.fuzzy.max_candidates
            ));
        }

        // live_conversion_delay_ms: 0 <= live_conversion_delay_ms <= 5000
        if !(0..=5000).contains(&self.conversion.live_conversion_delay_ms) {
            errors.push(format!(
                "conversion.live_conversion_delay_ms は 0 から 5000 の間である必要があります: {}",
                self.conversion.live_conversion_delay_ms
            ));
        }

        // live_conversion_min_key_length: 1 <= live_conversion_min_key_length <= 20
        if !(1..=20).contains(&self.conversion.live_conversion_min_key_length) {
            errors.push(format!(
                "conversion.live_conversion_min_key_length は 1 から 20 の間である必要があります: {}",
                self.conversion.live_conversion_min_key_length
            ));
        }

        // learning.max_entries: 100 <= max_entries <= 1000000
        if !(100..=1_000_000).contains(&self.learning.max_entries) {
            errors.push(format!(
                "learning.max_entries は 100 から 1000000 の間である必要があります: {}",
                self.learning.max_entries
            ));
        }

        // learning.strength: 0.0 <= strength <= 2.0
        if !(self.learning.strength.is_finite() && (0.0..=2.0).contains(&self.learning.strength)) {
            errors.push(format!(
                "learning.strength は 0.0 から 2.0 の間である必要があります: {}",
                self.learning.strength
            ));
        }

        // learning.forget_days: 0 <= forget_days <= 3650
        if self.learning.forget_days > 3650 {
            errors.push(format!(
                "learning.forget_days は 0 から 3650 の間である必要があります: {}",
                self.learning.forget_days
            ));
        }

        // predict.min_chars: 1 <= min_chars <= 20
        if !(1..=20).contains(&self.predict.min_chars) {
            errors.push(format!(
                "predict.min_chars は 1 から 20 の間である必要があります: {}",
                self.predict.min_chars
            ));
        }

        // predict.next_word_count: 1 <= next_word_count <= 10
        if !(1..=10).contains(&self.predict.next_word_count) {
            errors.push(format!(
                "predict.next_word_count は 1 から 10 の間である必要があります: {}",
                self.predict.next_word_count
            ));
        }

        // convert.suggest_max: 12 <= suggest_max <= 500
        if !(12..=500).contains(&self.convert.suggest_max) {
            errors.push(format!(
                "convert.suggest_max は 12 から 500 の間である必要があります: {}",
                self.convert.suggest_max
            ));
        }

        // ranking.source_order: 既知の名前のみ、重複なし、空でない
        const KNOWN_SOURCES: &[&str] = &[
            "learned",
            "user_dict",
            "llm",
            "system_dict",
            "kana",
            "rewriter",
        ];
        if self.ranking.source_order.is_empty() {
            errors.push("ranking.source_order は空にすることはできません".to_string());
        } else {
            let mut seen = std::collections::HashSet::new();
            for source in &self.ranking.source_order {
                if !KNOWN_SOURCES.contains(&source.as_str()) {
                    errors.push(format!(
                        "ranking.source_order に未知の出所があります: '{}'",
                        source
                    ));
                }
                if !seen.insert(source) {
                    errors.push(format!(
                        "ranking.source_order に重複がある出所があります: '{}'",
                        source
                    ));
                }
            }
        }

        errors
    }
}

/// デフォルトの TOML 設定（埋め込み）。
pub const DEFAULT_TOML: &str = include_str!("../../../config/default.toml");

/// バイト位置から 1 始まりの行番号を求める
fn line_at(source: &str, pos: usize) -> u32 {
    let pos = pos.min(source.len());
    source.as_bytes()[..pos]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
        + 1
}

/// `section.key`（または `section`）が書かれている行番号（1 始まり）。`[section]` ヘッダを
/// 追跡して同名キーの取り違えを避ける。見つからなければ 0
fn locate(source: &str, path: &str) -> u32 {
    let (section, key) = match path.rsplit_once('.') {
        Some((s, k)) => (s, Some(k)),
        None => (path, None),
    };
    let mut current = String::new();
    for (i, raw) in source.lines().enumerate() {
        let line = raw.trim_start_matches('\u{feff}').trim();
        if let Some(header) = line.strip_prefix('[')
            && !line.starts_with("[[")
            && let Some(name) = header.split(']').next()
        {
            current = name.trim().to_string();
            if key.is_none() && current == section {
                return i as u32 + 1;
            }
            continue;
        }
        if let Some(key) = key
            && current == section
            && let Some(rest) = line.strip_prefix(key)
            && rest.trim_start().starts_with('=')
        {
            return i as u32 + 1;
        }
    }
    0
}

/// 既知のトップレベルキー。
const KNOWN_KEYS: &[&str] = &[
    "profile",
    "conversion",
    "input",
    "ranking",
    "learning",
    "predict",
    "rewriter",
    "convert",
    "userdict",
    "snippet",
    "keymap",
    "update",
    "apps",
];

/// `[input.fuzzy]` の既知キー。
const FUZZY_KEYS: &[&str] = &[
    "enabled",
    "strength",
    "max_candidates",
    "kinds",
    "use_touch_coords",
];

/// `[input.auto_english]` の既知キー。
const AUTO_ENGLISH_KEYS: &[&str] = &["mode"];

/// 各セクションの既知キー。
fn get_known_section_keys(section: &str) -> &'static [&'static str] {
    match section {
        "profile" => &["mode", "auto_switch", "persona"],
        "conversion" => &[
            "live_conversion",
            "initial_view",
            "composing_chunk_len",
            "beam_width",
            "num_candidates",
            "n_threads",
            "model",
            "use_llm",
            "use_context",
            "max_context_length",
            "max_latency_ms",
            "sync_budget_ms",
            "live_conversion_delay_ms",
            "live_conversion_min_key_length",
        ],
        "input" => &[
            "shift_letter",
            "punctuation",
            "dakuten_cycle",
            "romaji_carryover",
            "fuzzy",
            "auto_english",
        ],
        "ranking" => &["source_order"],
        "learning" => &[
            "enabled",
            "max_entries",
            "strength",
            "forget_days",
            "disabled_apps",
        ],
        "predict" => &["enabled", "min_chars", "next_word", "next_word_count"],
        "rewriter" => &["emoji", "kaomoji"],
        "convert" => &["calculator", "loanword", "suggest_max", "symbol_position"],
        "userdict" | "snippet" => &["enabled"],
        "keymap" => &["preset", "composing", "converting"],
        "update" => &["auto_check", "check_url"],
        "apps" => &[
            "code_mode",
            "chat_mode",
            "general_mode",
            "remember_mode",
            "passthrough",
            "passthrough_chars",
            "classes",
        ],
        _ => &[],
    }
}

/// 不明なキー・セクション（`section` または `section.key` の形）を検出する
fn find_unknown_keys(table: &toml::Table) -> Vec<String> {
    let mut unknown = Vec::new();
    for (key, value) in table {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            unknown.push(key.clone());
            continue;
        }
        if let Some(section) = value.as_table() {
            let known = get_known_section_keys(key);
            for sub in section.keys() {
                if !known.contains(&sub.as_str()) {
                    unknown.push(format!("{key}.{sub}"));
                }
            }
            if key == "convert"
                && let Some(table) = section.get("loanword").and_then(|v| v.as_table())
            {
                for sub in table.keys() {
                    if !LOANWORD_KEYS.contains(&sub.as_str()) {
                        unknown.push(format!("convert.loanword.{sub}"));
                    }
                }
            }
            // 入れ子のテーブル（`[input.fuzzy]` `[input.auto_english]`）
            if key == "input" {
                for (nested, known) in [("fuzzy", FUZZY_KEYS), ("auto_english", AUTO_ENGLISH_KEYS)]
                {
                    if let Some(table) = section.get(nested).and_then(|v| v.as_table()) {
                        for sub in table.keys() {
                            if !known.contains(&sub.as_str()) {
                                unknown.push(format!("input.{nested}.{sub}"));
                            }
                        }
                    }
                }
            }
        }
    }
    unknown
}

/// TOML 文字列から設定を読み込む。エラーがあれば Err、警告のみなら Ok。
/// 複数のエラー・警告を全部返す（構文エラーの場合は構文エラー 1 件だけ）。
pub fn parse(source: &str) -> Result<(Config, Vec<Issue>), Vec<Issue>> {
    let source = source.trim_start_matches('\u{feff}');
    let syntax_error = |e: &toml::de::Error, what: &str| Issue {
        line: e.span().map_or(0, |s| line_at(source, s.start)),
        severity: Severity::Error,
        message: format!("{what}: {}", e.message()),
    };

    // 1. 構文（不明キーの検出にも使う）
    let table: toml::Table = match toml::from_str(source) {
        Ok(t) => t,
        Err(e) => return Err(vec![syntax_error(&e, "TOML の構文エラー")]),
    };
    // 2. 型・値（enum の未知の値など）。span から値の行が分かる
    let config: Config = match toml::from_str(source) {
        Ok(c) => c,
        Err(e) => return Err(vec![syntax_error(&e, "設定の値が不正です")]),
    };

    let mut issues = Vec::new();
    // 3. 不明キー（警告）
    for unknown in find_unknown_keys(&table) {
        issues.push(Issue {
            line: locate(source, &unknown),
            severity: Severity::Warning,
            message: format!("未知のキーです（無視します）: {unknown}"),
        });
    }
    // 4. 値の範囲検査（メッセージの先頭が `section.key`）
    for message in config.validate() {
        let path = message.split([' ', '　']).next().unwrap_or("");
        issues.push(Issue {
            line: locate(source, path),
            severity: Severity::Error,
            message,
        });
    }

    if issues.iter().any(|i| i.severity == Severity::Error) {
        Err(issues)
    } else {
        Ok((config, issues))
    }
}

/// ファイルから設定を読み込む。
/// ファイルが存在しなければ Ok((Config::default(), vec![]))。
/// 読めない場合は Err（line=0）。
pub fn load_file(path: &Path) -> Result<(Config, Vec<Issue>), Vec<Issue>> {
    match std::fs::read_to_string(path) {
        Ok(source) => parse(&source),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // ファイルが存在しない場合は既定値を返す
            Ok((Config::default(), vec![]))
        }
        Err(e) => {
            // その他のエラー
            Err(vec![Issue {
                line: 0,
                severity: Severity::Error,
                message: format!("設定ファイルが読み込めません: {} ({})", path.display(), e),
            }])
        }
    }
}

/// 設定ファイルのディレクトリを返す。
/// Windows: %APPDATA%\Nagi
/// macOS: ~/Library/Application Support/com.shiki.nagi
/// Linux: $XDG_CONFIG_HOME/nagi または ~/.config/nagi
/// 環境変数 NAGI_CONFIG_DIR があれば最優先。
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(&|k| std::env::var(k).ok(), std::env::consts::OS)
}

/// テスト用：環境変数を関数で受け取り、OS を明示的に指定。
#[doc(hidden)]
pub fn config_dir_from<F>(env: &F, os: &str) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    // 環境変数 NAGI_CONFIG_DIR が最優先
    if let Some(path) = env("NAGI_CONFIG_DIR") {
        return Some(PathBuf::from(path));
    }

    match os {
        "windows" => {
            // %APPDATA%\Nagi
            // Windows の区切り文字は実行 OS に依存させない（他 OS 上での単体テストでも同じ結果にする）
            env("APPDATA").map(|app_data| {
                PathBuf::from(format!("{}\\Nagi", app_data.trim_end_matches(['\\', '/'])))
            })
        }
        "macos" => {
            // ~/Library/Application Support/com.shiki.nagi
            env("HOME").map(|home| {
                let mut p = PathBuf::from(home);
                p.push("Library/Application Support/com.shiki.nagi");
                p
            })
        }
        _ => {
            // Linux: $XDG_CONFIG_HOME/nagi または ~/.config/nagi
            if let Some(xdg) = env("XDG_CONFIG_HOME") {
                let mut p = PathBuf::from(xdg);
                p.push("nagi");
                return Some(p);
            }

            env("HOME").map(|home| {
                let mut p = PathBuf::from(home);
                p.push(".config/nagi");
                p
            })
        }
    }
}

/// 設定ファイルのパスを返す。config_dir() / config.toml。
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|mut p| {
        p.push("config.toml");
        p
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let default = Config::default();
        assert_eq!(default.profile.mode, ProfileMode::Auto);
        assert!(default.profile.auto_switch);
        assert!(default.conversion.live_conversion);
        assert_eq!(default.conversion.initial_view, InitialView::Sentence);
        assert_eq!(default.conversion.composing_chunk_len, 30);
        assert_eq!(default.conversion.num_candidates, 9);
        assert_eq!(default.conversion.live_conversion_delay_ms, 228);
        assert_eq!(default.conversion.live_conversion_min_key_length, 2);
        assert!(default.input.punctuation.comma == '、');
        assert!(default.input.punctuation.period == '。');
    }

    #[test]
    fn test_parse_default_toml() {
        let (config, warnings) = parse(DEFAULT_TOML).expect("default.toml should parse");
        assert_eq!(config, Config::default());
        // 警告があってもよい（コメント行やスペースによる未知キーなど）
        assert!(
            warnings
                .iter()
                .all(|i| i.severity == Severity::Warning || i.severity == Severity::Error)
        );
    }

    #[test]
    fn test_roundtrip_serialization() {
        let default = Config::default();
        let toml_str = toml::to_string_pretty(&default).expect("should serialize");
        let (parsed, _) = parse(&toml_str).expect("should parse roundtrip");
        assert_eq!(parsed, default);
    }

    #[test]
    fn test_partial_config() {
        let source = r#"
[profile]
mode = "quality"

[conversion]
beam_width = 5
"#;
        let (config, _) = parse(source).expect("should parse partial config");
        assert_eq!(config.profile.mode, ProfileMode::Quality);
        assert_eq!(config.conversion.beam_width, Some(5));
        assert!(config.conversion.live_conversion); // 既定
        assert_eq!(config.conversion.num_candidates, 9); // 既定
    }

    #[test]
    fn test_option_unspecified() {
        let source = r#"
[conversion]
num_candidates = 12
"#;
        let (config, _) = parse(source).expect("should parse");
        assert_eq!(config.conversion.beam_width, None);
        assert_eq!(config.conversion.model, None);
        assert_eq!(config.conversion.max_context_length, None);
    }

    #[test]
    fn test_type_error() {
        let source = r#"
[conversion]
num_candidates = "nine"
"#;
        let result = parse(source);
        assert!(result.is_err());
        let issues = result.unwrap_err();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].line, 3, "値の書かれた行が分かる: {:?}", issues[0]);
        assert!(issues[0].message.contains("不正"), "{}", issues[0].message);
    }

    #[test]
    fn test_invalid_enum() {
        let source = r#"
[profile]
mode = "unknown_mode"
"#;
        let result = parse(source);
        assert!(result.is_err());
        let issues = result.unwrap_err();
        assert!(issues.iter().any(|i| i.severity == Severity::Error));
    }

    #[test]
    fn test_unknown_key_warning() {
        let source = r#"
[conversion]
unknown_setting = 42
num_candidates = 9
"#;
        let result = parse(source);
        assert!(result.is_ok());
        let (_, warnings) = result.unwrap();
        assert!(
            warnings
                .iter()
                .any(|i| i.severity == Severity::Warning && i.message.contains("unknown_setting"))
        );
    }

    #[test]
    fn test_unknown_section_warning() {
        let source = r#"
[unknown_section]
foo = 42
"#;
        let result = parse(source);
        assert!(result.is_ok());
        let (_, warnings) = result.unwrap();
        assert!(
            warnings
                .iter()
                .any(|i| i.severity == Severity::Warning && i.message.contains("unknown_section"))
        );
    }

    #[test]
    fn test_line_numbers() {
        let source = "# comment\n[profile]\nmode = \"quality\"\n[conversion]\nbeam_width = 999\n";
        let result = parse(source);
        assert!(result.is_err());
        let issues = result.unwrap_err();
        // beam_width のエラーがあり、行番号が 5 のはず
        let error = issues
            .iter()
            .find(|i| i.message.contains("beam_width") && i.severity == Severity::Error);
        assert!(error.is_some());
        assert_eq!(error.unwrap().line, 5);
    }

    #[test]
    fn test_validate_beam_width() {
        let mut config = Config::default();
        config.conversion.beam_width = Some(0);
        let errors = config.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("beam_width") && e.contains("1"))
        );

        config.conversion.beam_width = Some(17);
        let errors = config.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("beam_width") && e.contains("16"))
        );
    }

    #[test]
    fn test_validate_composing_chunk_len() {
        let mut config = Config::default();
        config.conversion.composing_chunk_len = 0;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("composing_chunk_len")));

        config.conversion.composing_chunk_len = 201;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("composing_chunk_len")));
    }

    #[test]
    fn test_validate_num_candidates() {
        let mut config = Config::default();
        config.conversion.num_candidates = 0;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("num_candidates")));

        config.conversion.num_candidates = 51;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("num_candidates")));
    }

    #[test]
    fn test_validate_max_context_length() {
        let mut config = Config::default();
        config.conversion.max_context_length = Some(0);
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("max_context_length")));

        config.conversion.max_context_length = Some(201);
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("max_context_length")));
    }

    #[test]
    fn test_validate_latency_ranges() {
        let mut config = Config::default();
        config.conversion.max_latency_ms = 9;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("max_latency_ms")));

        config.conversion.max_latency_ms = 10001;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("max_latency_ms")));

        config.conversion.sync_budget_ms = 9;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("sync_budget_ms")));

        config.conversion.sync_budget_ms = 5001;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("sync_budget_ms")));
    }

    #[test]
    fn test_validate_live_conversion_delay_ms() {
        let mut config = Config::default();
        config.conversion.live_conversion_delay_ms = 5001;
        let errors = config.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("live_conversion_delay_ms"))
        );

        // 下限は 0（許容される）
        config.conversion.live_conversion_delay_ms = 0;
        let errors = config.validate();
        assert!(
            !errors
                .iter()
                .any(|e| e.contains("live_conversion_delay_ms"))
        );
    }

    #[test]
    fn test_validate_live_conversion_min_key_length() {
        let mut config = Config::default();
        config.conversion.live_conversion_min_key_length = 0;
        let errors = config.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("live_conversion_min_key_length"))
        );

        config.conversion.live_conversion_min_key_length = 21;
        let errors = config.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("live_conversion_min_key_length"))
        );
    }

    #[test]
    fn test_validate_learning_strength_and_forget_days() {
        let mut config = Config::default();
        assert_eq!(config.learning.strength, 1.0);
        assert_eq!(config.learning.forget_days, 0);
        assert!(config.validate().iter().all(|e| !e.contains("learning.")));
        config.learning.strength = 2.1;
        assert!(config.validate().iter().any(|e| e.contains("strength")));
        config.learning.strength = -0.1;
        assert!(config.validate().iter().any(|e| e.contains("strength")));
        config.learning.strength = 0.0;
        config.learning.forget_days = 3651;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("forget_days")));
        assert!(!errors.iter().any(|e| e.contains("strength")));
    }

    #[test]
    fn test_validate_learning_max_entries() {
        let mut config = Config::default();
        config.learning.max_entries = 99;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("max_entries")));

        config.learning.max_entries = 1_000_001;
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("max_entries")));
    }

    #[test]
    fn test_validate_source_order_empty() {
        let mut config = Config::default();
        config.ranking.source_order = vec![];
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("source_order")));
    }

    #[test]
    fn test_validate_source_order_unknown() {
        let mut config = Config::default();
        config.ranking.source_order = vec!["learned".to_string(), "unknown".to_string()];
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("unknown")));
    }

    #[test]
    fn test_validate_source_order_duplicate() {
        let mut config = Config::default();
        config.ranking.source_order = vec!["learned".to_string(), "learned".to_string()];
        let errors = config.validate();
        assert!(errors.iter().any(|e| e.contains("重複")));
    }

    #[test]
    fn test_multiple_errors_collected() {
        let source = r#"
[conversion]
beam_width = 999
num_candidates = 0
max_latency_ms = 5
"#;
        let result = parse(source);
        assert!(result.is_err());
        let issues = result.unwrap_err();
        assert!(issues.len() >= 3);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("beam_width") && i.severity == Severity::Error)
        );
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("num_candidates") && i.severity == Severity::Error)
        );
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("max_latency_ms") && i.severity == Severity::Error)
        );
    }

    #[test]
    fn test_load_file_not_found() {
        let path = std::path::Path::new("/nonexistent/path/config.toml");
        let (config, warnings) = load_file(path).expect("should return default");
        assert_eq!(config, Config::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_load_file_valid() {
        // テンポラリファイルを作成
        let tmp_dir = std::env::temp_dir();
        let tmp_file = tmp_dir.join("nagi_test_config.toml");

        let source = r#"
[profile]
mode = "balanced"

[conversion]
num_candidates = 15
"#;

        std::fs::write(&tmp_file, source).expect("should write temp file");

        let (config, _) = load_file(&tmp_file).expect("should load");
        assert_eq!(config.profile.mode, ProfileMode::Balanced);
        assert_eq!(config.conversion.num_candidates, 15);

        std::fs::remove_file(&tmp_file).ok();
    }

    #[test]
    fn test_punctuation_variants() {
        let p1: Punctuation = toml::from_str("p = \"、。\"").unwrap_or_default();
        assert_eq!(p1, Punctuation::default());

        let source = r#"
[input]
punctuation = "，．"
"#;
        let (config, _) = parse(source).expect("should parse");
        assert_eq!(config.input.punctuation.comma, '，');
        assert_eq!(config.input.punctuation.period, '．');

        let source = r#"
[input]
punctuation = "，。"
"#;
        let (config, _) = parse(source).expect("should parse");
        assert_eq!(config.input.punctuation.comma, '，');
        assert_eq!(config.input.punctuation.period, '。');
    }

    #[test]
    fn test_punctuation_invalid() {
        let source = r#"
[input]
punctuation = "invalid"
"#;
        let result = parse(source);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_dir_with_env() {
        // テスト用に環境変数をシミュレート
        let env_fn = |k: &str| {
            if k == "NAGI_CONFIG_DIR" {
                Some("/custom/nagi/config".to_string())
            } else {
                None
            }
        };

        let dir = config_dir_from(&env_fn, "linux");
        assert_eq!(dir, Some(PathBuf::from("/custom/nagi/config")));
    }

    #[test]
    fn test_config_dir_windows() {
        let env_fn = |k: &str| {
            if k == "APPDATA" {
                Some("C:\\Users\\Test\\AppData\\Roaming".to_string())
            } else {
                None
            }
        };

        let dir = config_dir_from(&env_fn, "windows");
        assert_eq!(
            dir,
            Some(PathBuf::from("C:\\Users\\Test\\AppData\\Roaming\\Nagi"))
        );
    }

    #[test]
    fn test_config_dir_macos() {
        let env_fn = |k: &str| {
            if k == "HOME" {
                Some("/Users/test".to_string())
            } else {
                None
            }
        };

        let dir = config_dir_from(&env_fn, "macos");
        assert_eq!(
            dir,
            Some(PathBuf::from(
                "/Users/test/Library/Application Support/com.shiki.nagi"
            ))
        );
    }

    #[test]
    fn test_config_dir_linux_xdg() {
        let env_fn = |k: &str| {
            if k == "XDG_CONFIG_HOME" {
                Some("/home/test/.config".to_string())
            } else {
                None
            }
        };

        let dir = config_dir_from(&env_fn, "linux");
        assert_eq!(dir, Some(PathBuf::from("/home/test/.config/nagi")));
    }

    #[test]
    fn test_config_dir_linux_home() {
        let env_fn = |k: &str| {
            if k == "HOME" {
                Some("/home/test".to_string())
            } else {
                None
            }
        };

        let dir = config_dir_from(&env_fn, "linux");
        assert_eq!(dir, Some(PathBuf::from("/home/test/.config/nagi")));
    }

    #[test]
    fn test_config_path() {
        let env_fn = |k: &str| {
            if k == "NAGI_CONFIG_DIR" {
                Some("/custom/nagi".to_string())
            } else {
                None
            }
        };

        let _orig_dir = config_dir_from(&env_fn, "linux");

        // config_path は実際の config_dir を使うので、テストは困難
        // ここでは存在チェックだけ
        if let Some(path) = config_path() {
            assert!(path.ends_with("config.toml"));
        }
    }

    #[test]
    fn test_keymap_overrides_parse() {
        let source = r#"
[keymap]
preset = "default"

[keymap.composing]
"Ctrl+j" = "ToKatakana"

[keymap.converting]
"Tab" = "NextCand"
"#;
        let (config, warnings) = parse(source).expect("should parse keymap overrides");
        assert_eq!(
            config.keymap.composing.get("Ctrl+j").map(String::as_str),
            Some("ToKatakana")
        );
        assert_eq!(
            config.keymap.converting.get("Tab").map(String::as_str),
            Some("NextCand")
        );
        // [keymap.composing] / [keymap.converting] 自体は既知のサブセクションなので警告は出ない
        assert!(
            warnings
                .iter()
                .all(|i| !i.message.contains("composing") && !i.message.contains("converting")),
            "{warnings:?}"
        );
    }

    #[test]
    fn test_update_check_url_parse() {
        let source = r#"
[update]
auto_check = false
check_url = "https://example.com/latest.json"
"#;
        let (config, _) = parse(source).expect("should parse update.check_url");
        assert!(!config.update.auto_check);
        assert_eq!(
            config.update.check_url.as_deref(),
            Some("https://example.com/latest.json")
        );
    }

    #[test]
    fn test_predict_section_parse() {
        let source = r#"
[predict]
enabled = false
min_chars = 3
"#;
        let (config, _) = parse(source).expect("should parse predict section");
        assert!(!config.predict.enabled);
        assert_eq!(config.predict.min_chars, 3);
    }

    #[test]
    fn test_convert_symbol_position_default_and_values() {
        assert_eq!(
            Config::default().convert.symbol_position,
            SymbolPosition::AfterBest
        );
        for (text, expected) in [
            ("first", SymbolPosition::First),
            ("after_best", SymbolPosition::AfterBest),
            ("last", SymbolPosition::Last),
            ("off", SymbolPosition::Off),
        ] {
            let (config, issues) =
                parse(&format!("[convert]\nsymbol_position = \"{text}\"\n")).expect("parse");
            assert!(issues.is_empty(), "{issues:?}");
            assert_eq!(config.convert.symbol_position, expected);
            assert_eq!(expected.as_str(), text);
        }
        // 知らない値は読み込みエラー
        assert!(parse("[convert]\nsymbol_position = \"middle\"\n").is_err());
    }

    #[test]
    fn test_convert_suggest_max_default_and_range() {
        let (config, _) = parse("[convert]\nsuggest_max = 200\n").expect("parse");
        assert_eq!(config.convert.suggest_max, 200);
        assert_eq!(Config::default().convert.suggest_max, 120);
        assert!(Config::default().validate().is_empty());
        for bad in [0, 11, 501] {
            let mut config = Config::default();
            config.convert.suggest_max = bad;
            assert!(
                config
                    .validate()
                    .iter()
                    .any(|e| e.contains("convert.suggest_max")),
                "{bad}"
            );
        }
    }

    #[test]
    fn test_validate_predict_min_chars() {
        let mut config = Config::default();
        config.predict.min_chars = 0;
        assert!(
            config
                .validate()
                .iter()
                .any(|e| e.contains("predict.min_chars") && e.contains("1"))
        );
        config.predict.min_chars = 21;
        assert!(
            config
                .validate()
                .iter()
                .any(|e| e.contains("predict.min_chars") && e.contains("20"))
        );
    }

    #[test]
    fn test_userdict_snippet_dakuten_sections() {
        let d = Config::default();
        assert!(d.userdict.enabled && d.snippet.enabled);
        assert_eq!(d.input.dakuten_cycle, DakutenCycle::SmallFirst);
        assert!(d.input.romaji_carryover);
        let source = "[userdict]\nenabled = false\n[snippet]\nenabled = false\n[input]\ndakuten_cycle = \"dakuten_first\"\n";
        let (c, issues) = parse(source).expect("parse");
        assert!(issues.is_empty(), "{issues:?}");
        assert!(!c.userdict.enabled && !c.snippet.enabled);
        assert_eq!(c.input.dakuten_cycle, DakutenCycle::DakutenFirst);
        assert_eq!(format!("{}", DakutenCycle::SmallFirst), "small_first");
    }

    #[test]
    fn test_crlf_handling() {
        let source = "[profile]\r\nmode = \"quality\"\r\n[conversion]\r\nbeam_width = 5\r\n";
        let (config, _) = parse(source).expect("should handle CRLF");
        assert_eq!(config.profile.mode, ProfileMode::Quality);
        assert_eq!(config.conversion.beam_width, Some(5));
    }

    #[test]
    fn test_display_severity() {
        assert_eq!(format!("{}", Severity::Error), "error");
        assert_eq!(format!("{}", Severity::Warning), "warning");
    }

    #[test]
    fn test_display_profile_mode() {
        assert_eq!(format!("{}", ProfileMode::Auto), "auto");
        assert_eq!(format!("{}", ProfileMode::Quality), "quality");
        assert_eq!(format!("{}", ProfileMode::Balanced), "balanced");
        assert_eq!(format!("{}", ProfileMode::Light), "light");
        assert_eq!(format!("{}", ProfileMode::Minimal), "minimal");
    }

    #[test]
    fn test_display_initial_view() {
        assert_eq!(format!("{}", InitialView::Sentence), "sentence");
        assert_eq!(format!("{}", InitialView::Segment), "segment");
    }

    #[test]
    fn test_display_shift_letter() {
        assert_eq!(
            format!("{}", ShiftLetter::LiteralContinue),
            "literal_continue"
        );
        assert_eq!(format!("{}", ShiftLetter::SingleUpper), "single_upper");
    }

    #[test]
    fn auto_english_defaults_parse_and_validate() {
        let d = Config::default();
        assert_eq!(d.input.auto_english.mode, AutoEnglishMode::Conservative);
        assert_eq!(AutoEnglishMode::Aggressive.to_string(), "aggressive");
        let (c, issues) = parse("[input.auto_english]\nmode = \"off\"\n").expect("parse");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(c.input.auto_english.mode, AutoEnglishMode::Off);
        for (v, m) in [
            ("conservative", AutoEnglishMode::Conservative),
            ("standard", AutoEnglishMode::Standard),
            ("aggressive", AutoEnglishMode::Aggressive),
        ] {
            let (c, _) = parse(&format!("[input.auto_english]\nmode = \"{v}\"\n")).expect("parse");
            assert_eq!(c.input.auto_english.mode, m);
        }
        assert!(parse("[input.auto_english]\nmode = \"x\"\n").is_err());
        let (_, issues) = parse("[input.auto_english]\nbogus = 1\n").expect("parse");
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("input.auto_english.bogus") && i.line == 2),
            "{issues:?}"
        );
    }

    #[test]
    fn fuzzy_section_defaults_parse_and_validate() {
        let d = Config::default();
        assert!(!d.input.fuzzy.enabled, "既定はオフ");
        assert_eq!(d.input.fuzzy.strength, FuzzyStrength::Normal);
        assert_eq!(d.input.fuzzy.max_candidates, 3);
        assert_eq!(d.input.fuzzy.kinds, FUZZY_KINDS_ALL);
        assert_eq!(FuzzyStrength::Gentle.max_cost(), 3.0);
        assert_eq!(FuzzyStrength::Normal.max_cost(), 4.5);
        assert_eq!(FuzzyStrength::Aggressive.max_cost(), 6.5);
        assert_eq!(FuzzyStrength::Aggressive.to_string(), "aggressive");

        let source = "[input.fuzzy]\nenabled = true\nstrength = \"aggressive\"\nmax_candidates = 5\nkinds = \"voicing\"\n";
        let (c, issues) = parse(source).expect("parse");
        assert!(issues.is_empty(), "{issues:?}");
        assert!(c.input.fuzzy.enabled);
        assert_eq!(c.input.fuzzy.strength, FuzzyStrength::Aggressive);
        assert_eq!(c.input.fuzzy.max_candidates, 5);
        assert_eq!(c.input.fuzzy.kinds, "voicing");

        // 範囲外はエラー（行番号付き）、入れ子の未知キーは警告
        let bad = "[input]\nromaji_carryover = true\n[input.fuzzy]\nmax_candidates = 6\n";
        let issues = parse(bad).expect_err("範囲外");
        assert_eq!(issues[0].line, 4, "{issues:?}");
        assert!(issues[0].message.contains("input.fuzzy.max_candidates"));
        let (_, issues) = parse("[input.fuzzy]\nbogus = 1\n").expect("parse");
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("input.fuzzy.bogus") && i.line == 2),
            "{issues:?}"
        );
        assert!(parse("[input.fuzzy]\nstrength = \"x\"\n").is_err());
    }

    #[test]
    fn locate_tracks_sections_and_ignores_same_named_keys() {
        let source = "\
[profile]
mode = \"auto\"
[learning]
enabled = true
[conversion]
enabled = 1
model = \"x\"
[[other]]
model = \"y\"
";
        assert_eq!(locate(source, "profile.mode"), 2);
        assert_eq!(locate(source, "learning.enabled"), 4);
        assert_eq!(
            locate(source, "conversion.enabled"),
            6,
            "同名のキーでもセクションで区別する"
        );
        assert_eq!(locate(source, "conversion.model"), 7);
        assert_eq!(
            locate(source, "conversion"),
            5,
            "セクション名だけならヘッダの行"
        );
        assert_eq!(locate(source, "conversion.nope"), 0);
        // CRLF・BOM・インデント
        assert_eq!(locate("\u{feff}[a]\r\n  k = 1\r\n", "a.k"), 2);
    }

    #[test]
    fn errors_report_the_line_of_the_offending_value() {
        let source = "\
[conversion]
beam_width = 3
num_candidates = 999
sync_budget_ms = 1

[input]
shift_letter = \"nope\"
";
        // enum の不正値は値の行（7）。範囲エラーは構文が通ってから検査するので別の呼び出しで確認する
        let err = parse(source).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].line, 7);
        let source = source.replace("shift_letter = \"nope\"", "shift_letter = \"single_upper\"");
        let err = parse(&source).unwrap_err();
        let lines: Vec<u32> = err.iter().map(|i| i.line).collect();
        assert_eq!(
            lines,
            vec![3, 4],
            "num_candidates と sync_budget_ms の 2 件がまとめて返る: {err:?}"
        );
    }

    #[test]
    fn unknown_keys_get_lines_within_their_section() {
        let source = "\
[conversion]
model = \"m\"
beam_widht = 3
[typo_section]
x = 1
";
        let (config, issues) = parse(source).expect("警告だけなら Ok");
        assert_eq!(config.conversion.model.as_deref(), Some("m"));
        let lines: Vec<(u32, Severity)> = issues.iter().map(|i| (i.line, i.severity)).collect();
        assert!(lines.contains(&(3, Severity::Warning)), "{issues:?}");
        assert!(lines.contains(&(4, Severity::Warning)), "{issues:?}");
    }

    #[test]
    fn bom_and_crlf_files_parse() {
        let source = "\u{feff}[conversion]\r\nnum_candidates = 7\r\n";
        let (config, issues) = parse(source).unwrap();
        assert_eq!(config.conversion.num_candidates, 7);
        assert!(issues.is_empty());
        let err = parse("\u{feff}[conversion]\r\nnum_candidates = 0\r\n").unwrap_err();
        assert_eq!(err[0].line, 2);
    }

    #[test]
    fn apps_default_classification_table() {
        let a = AppsSection::default();
        assert_eq!(a.classify("com.apple.Terminal"), AppClass::Code);
        assert_eq!(a.classify("com.jetbrains.intellij"), AppClass::Code);
        assert_eq!(a.classify("COM.MICROSOFT.VSCODE"), AppClass::Code);
        assert_eq!(a.classify("WindowsTerminal.exe"), AppClass::Code);
        assert_eq!(a.classify("com.tinyspeck.slackmacgap"), AppClass::Chat);
        assert_eq!(a.classify("com.apple.Safari"), AppClass::General);
        assert_eq!(a.mode_for(AppClass::Code), AppDefaultMode::Alphanumeric);
        assert_eq!(a.mode_for(AppClass::General), AppDefaultMode::Keep);
        assert!(a.remember_mode && a.passthrough);
        assert_eq!(a.passthrough_chars, "/$@!:#");
    }

    #[test]
    fn apps_section_parses_and_user_table_wins() {
        let src = r#"
[apps]
code_mode = "hiragana"
remember_mode = false
passthrough = false
passthrough_chars = "/"

[apps.classes]
"com.apple.Terminal" = "general"
"com.example.*" = "code"
"com.example.chat" = "chat"
"#;
        let (c, issues) = parse(src).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(c.apps.code_mode, AppDefaultMode::Hiragana);
        assert!(!c.apps.remember_mode && !c.apps.passthrough);
        assert_eq!(c.apps.passthrough_chars, "/");
        // 利用者の表が同梱の表より優先
        assert_eq!(c.apps.classify("com.apple.Terminal"), AppClass::General);
        // 前方一致より完全一致が先
        assert_eq!(c.apps.classify("com.example.foo"), AppClass::Code);
        assert_eq!(c.apps.classify("com.example.chat"), AppClass::Chat);
        // 同梱の表は引き続き効く
        assert_eq!(c.apps.classify("com.googlecode.iterm2"), AppClass::Code);
    }

    #[test]
    fn apps_unknown_key_warns_and_bad_value_errors() {
        let (_, issues) = parse("[apps]\nbogus = 1\n").unwrap();
        assert!(
            issues.iter().any(|i| i.message.contains("apps.bogus")),
            "{issues:?}"
        );
        assert!(parse("[apps]\ncode_mode = \"x\"\n").is_err());
    }
}
