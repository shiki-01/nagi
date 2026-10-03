//! 設定とコスト関数。

/// 取り違えやすい子音の対（`(打たれた文字, 本来の文字)`）。キー配列ではなく音の類似に基づく。
///
/// `si`/`shi`・`ti`/`chi`・`tu`/`tsu`・`hu`/`fu` 系はローマ字テーブルが両方受けるので含めない。
pub const CONSONANT_CONFUSIONS: &[(char, char)] = &[
    ('l', 'r'),
    ('r', 'l'),
    ('c', 'k'),
    ('k', 'c'),
    ('c', 's'),
    ('s', 'c'),
    ('b', 'v'),
    ('v', 'b'),
    ('j', 'z'),
    ('z', 'j'),
];

/// 曖昧入力の設定。コストの単位は nat（暫定値。実機較正で更新する）
#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyConfig {
    /// 編集操作（欠落補完・余剰削除・入れ替え・紛れ）の最大回数。0 で編集を無効化
    pub max_edits: u8,
    /// この打鍵数以上の入力では `max_edits` を 2 以上に引き上げる（`max_edits` が 0 のときは 0 のまま）
    pub long_input_len: usize,
    /// 編集を適用する末尾からの打鍵数。それより前は打たれたとおりに固定する
    pub window: usize,
    /// 位置ごとに残す状態数
    pub beam: usize,
    /// 補正仮説の cost 上限（第一仮説は対象外）
    pub max_cost: f32,
    /// 返す仮説の最大数（第一仮説を含む）
    pub max_hypotheses: usize,
    /// 入れ替えのコスト
    pub cost_transposition: f32,
    /// 欠落補完（1 文字挿入）のコスト
    pub cost_insertion: f32,
    /// 余剰削除のコスト
    pub cost_deletion: f32,
    /// 子音の紛れのコスト
    pub cost_confusion: f32,
    /// 距離→コスト変換の標準偏差（キー幅比）。`cost = 0.5 * (distance / sigma)^2`
    pub sigma: f32,
    /// これを超える距離の代替は捨てる（キー幅比）
    pub max_distance: f32,
    /// 子音の紛れ（[`CONSONANT_CONFUSIONS`]）を使うか
    pub enable_confusions: bool,
    /// 辞書にない語として許す連続ひらがな数の上限。0 で未知語を許さない
    pub unknown_budget: usize,
}

impl Default for FuzzyConfig {
    fn default() -> Self {
        Self {
            max_edits: 1,
            long_input_len: 6,
            window: 8,
            beam: 8,
            max_cost: 6.0,
            max_hypotheses: 3,
            cost_transposition: 2.0,
            cost_insertion: 3.0,
            cost_deletion: 3.0,
            cost_confusion: 2.5,
            sigma: 0.35,
            max_distance: 1.0,
            enable_confusions: true,
            unknown_budget: 0,
        }
    }
}

/// 距離（キー幅正規化）をコスト（nat）に変換する。二次形式 `0.5 * (distance / sigma)^2`。
///
/// 距離→コストの変換はこの関数だけに閉じている。
pub fn distance_cost(distance: f32, cfg: &FuzzyConfig) -> f32 {
    let d = distance.max(0.0) / cfg.sigma.max(f32::EPSILON);
    0.5 * d * d
}
