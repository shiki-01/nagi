//! 表示型（Preedit・Candidates・InputMode）

use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// 入力モード（ひらがな / 英数）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    /// ひらがな入力モード
    Hiragana,
    /// 英数入力モード
    Alphanumeric,
}

/// セグメントの種別
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    /// 日本語（変換対象）
    Japanese,
    /// 英字・数字・記号（surface は変わらないが、推論の文脈には含める）
    Literal,
    /// 部分確定済み（変換できない）
    Fixed,
    /// スタイル変換結果（reading を持たず、Undo 用に元を保持）
    Styled,
}

/// UTF-16 コード単位による半開区間 `[start, end)`
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RangeU16 {
    /// 開始位置（UTF-16 コード単位）
    pub start: u32,
    /// 終了位置（UTF-16 コード単位）
    pub end: u32,
}

impl RangeU16 {
    /// 新しい Range を作成する
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// 長さを返す
    pub const fn len(&self) -> u32 {
        self.end - self.start
    }

    /// 空かどうか
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// セグメント（文節相当）の表示情報
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentView {
    /// 表示文字列（変換結果またはそのまま）
    pub surface: String,
    /// ひらがな読み（正）
    pub reading: String,
    /// セグメント種別
    pub kind: SegmentKind,
    /// PreeditView.text 上での位置（UTF-16）
    pub range: RangeU16,
}

/// プリエディット（変換中のテキスト）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PreeditView {
    /// 表示文字列全体（保留ローマ字をカーソル位置に含む）
    pub text: String,

    /// セグメント列。保留部分は含まない（segments の range の隙間が保留）
    pub segments: Vec<SegmentView>,

    /// text 上の UTF-16 コード単位による カーソル位置
    pub cursor: u32,

    /// 保留中のローマ字が text 上で占める範囲。None なら保留なし
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pending: Option<RangeU16>,

    /// Converting 状態の注目文節インデックス。None なら注目なし
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focus: Option<u32>,

    /// RangeSelecting 状態での複数文節選択範囲（text 上の位置）。None なら選択なし
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub selection: Option<RangeU16>,
}

/// 候補の出所（出元のモデル・辞書等）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    /// 学習キャッシュ
    Learned,
    /// ユーザー辞書
    UserDict,
    /// LLM 推論
    Llm,
    /// システム辞書
    SystemDict,
    /// ひらがな・カタカナ変換
    Kana,
    /// Rewriter（正規化・変換済み）
    Rewriter,
    /// 曖昧入力（タイプミス補正の「もしかして」候補。`input.fuzzy.enabled`）
    Fuzzy,
}

/// 単一の候補
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateView {
    /// 表示文字列
    pub surface: String,
    /// 出所のリスト（複数の出所から同じ候補が来た場合）
    pub sources: Vec<CandidateSource>,
    /// 入力中（変換前）の候補で、先頭から何文字（Unicode スカラー）の読みを置き換えるか。
    /// `None` は読み全体（変換中の候補は常に `None`）。短い値の候補を選ぶと、その分だけ確定し、
    /// 残りの読みは入力中のまま続く（部分変換）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub covers: Option<u32>,
    /// 曖昧入力（出所 `Fuzzy`）の候補で、補正後の読み（「もしかして: かんじ」の表示用）。
    /// 選ぶとこの読みで確定し、学習する。補正でない候補は `None`
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reading: Option<String>,
}

/// 候補ウィンドウの状態
///
/// 未入力（Direct）で `visible` かつ `items` があるときは、確定直後の次語予測
/// （`predict.next_word`）。各候補は `select_candidate` で選ぶとそのまま挿入される
/// （`commit`）。入力が始まるか別の操作をすると、`visible = false` の差分で消える
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CandidatesView {
    /// 候補のリスト
    pub items: Vec<CandidateView>,

    /// 現在選択中の候補のインデックス
    pub selected: u32,

    /// 1 ページあたりの表示候補数
    pub page_size: u32,

    /// ウィンドウが表示されているか
    pub visible: bool,
}
