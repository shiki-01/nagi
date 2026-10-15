//! 編集操作・コスト・探索パラメータ。
//!
//! コストは「負の対数尤度風」の相対値（小さいほどありそうな打ち間違い）。論文値ではなく、
//! 一般的なタイプミスの頻度の順序（転置・隣接キー ＞ 脱落・余計な打鍵）に合わせた初期値で、
//! 評価（`docs/decisions.md` H-24）で調整する前提。操作名は**利用者の打ち間違い**の側から付ける
//! （脱落 = 打つべき文字を打ち忘れた → 補正は 1 文字を足す）。

use std::time::Duration;

/// 隣接キーへの置換の最小コスト（最も近い隣接キー）。
/// 隣接キーの打ち間違いは最も頻度が高い誤りの一つなので、全操作の中で最小の 2.0 にする
pub const COST_SUBSTITUTE_MIN: f32 = 2.0;
/// 隣接キーへの置換の最大コスト（近傍の中で順位が低い・遠いキー）
pub const COST_SUBSTITUTE_MAX: f32 = 3.0;
/// 隣接順位 1 つごとに加えるコスト（順位 5 以降は [`COST_SUBSTITUTE_MAX`] で頭打ち）
pub const COST_SUBSTITUTE_RANK_STEP: f32 = 0.2;
/// 脱落（打ち忘れ。補正は任意の 1 文字の挿入）。候補の幅が広い（26 通り）ので置換より重くする
pub const COST_DELETE: f32 = 3.0;
/// 挿入（余計な打鍵。補正は 1 文字の削除）。直前文字の重複と、前後のキーに隣接する文字だけ許す
pub const COST_INSERT: f32 = 3.0;
/// 隣り合う 2 文字の転置（ `kanzi` → `kanzi` を `knazi` と打つ類）。同時打鍵の順序ミスで頻度が高い
pub const COST_TRANSPOSE: f32 = 2.0;
/// 濁点・半濁点・小書きの付け忘れ / 付け過ぎ（かな直接入力）。キー操作 1 回の差なので置換より軽い
pub const COST_VOICING: f32 = 1.5;
/// フリックの隣り合う方向への誤り（中央↔各方向、周回で隣の方向）
pub const COST_ROW_NEAR: f32 = 2.5;
/// フリックの角度が隣の方向との境界にあるときの取り違えのコスト（境界から離れるほど
/// [`COST_ROW_NEAR`] に近づく）。どちらの方向とも言える動きなので、通常より軽い
pub const COST_FLICK_BOUNDARY: f32 = 1.5;
/// フリックの反対方向への誤り
pub const COST_ROW_FAR: f32 = 3.0;

/// タッチ位置のばらつき（標準偏差）の既定。キー間隔を 1 とした値。
///
/// 実際の指のばらつき（親指の QWERTY で概ねキー間隔の 0.2〜0.35 倍）より大きい 0.5 にしている。
/// 評価（`nagi-fuzzy-eval`、`docs/decisions.md` H-25）で、モデルの σ をノイズの σ と同じにすると
/// 累積コスト上限（4.5）に対して座標つきの置換が高くなり過ぎ、ノイズが大きい（0.32）ときに
/// M1（離散）より悪化した。σ を 0.25 → 0.35 → 0.45 → 0.55 → 0.7 と変えると、上位 5 件の正解率は
/// ノイズ 0.25 で 48 → 59 → 64 → 70 → 70%、0.32 で 23 → 36 → 44 → 50 → 55%（M1 は 49 / 36%）と
/// 単調に上がる一方、クリーン入力の誤訂正は 0.55 から現れる（0.13%、0.7 で 0.25%）。
/// 誤訂正率が 0% のまま正解率が頭打ちに近い手前の 0.5 を採った。ばらつきを広めに見る方が安全
/// （境界から遠い打鍵も、辞書で成立すれば補正候補になる）
pub const DEFAULT_TOUCH_SIGMA: f32 = 0.5;

/// 累積コスト上限の既定（置換 2 回、または転置 + 置換 程度）
pub const DEFAULT_MAX_COST: f32 = 4.5;

/// 隣接順位に応じた置換コスト（2.0〜3.0）
pub fn substitute_cost(rank: usize) -> f32 {
    (COST_SUBSTITUTE_MIN + COST_SUBSTITUTE_RANK_STEP * rank as f32).min(COST_SUBSTITUTE_MAX)
}

/// 編集操作の種類（`input.fuzzy.kinds` の名前と対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// 隣接キー（かな直接入力では同じ行内の隣の文字）への置換
    Substitute,
    /// 脱落（打ち忘れ。補正は文字の追加）
    Delete,
    /// 挿入（余計な打鍵。補正は文字の削除）
    Insert,
    /// 隣り合う 2 文字の転置
    Transpose,
    /// 濁点・半濁点・小書きの付け忘れ / 付け過ぎ
    Voicing,
}

/// 有効にする操作の集合
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditKinds {
    /// 置換
    pub substitute: bool,
    /// 脱落
    pub delete: bool,
    /// 挿入
    pub insert: bool,
    /// 転置
    pub transpose: bool,
    /// 濁点・小書き
    pub voicing: bool,
}

impl Default for EditKinds {
    fn default() -> Self {
        EditKinds::ALL
    }
}

impl EditKinds {
    /// すべて有効
    pub const ALL: EditKinds = EditKinds {
        substitute: true,
        delete: true,
        insert: true,
        transpose: true,
        voicing: true,
    };

    /// カンマ区切りの名前（`substitute,delete,insert,transpose,voicing`）から作る。
    /// 空白は無視し、未知の名前は読み飛ばす。名前が 1 つも有効でなければ全部オフ
    pub fn parse(csv: &str) -> Self {
        let mut kinds = EditKinds {
            substitute: false,
            delete: false,
            insert: false,
            transpose: false,
            voicing: false,
        };
        for name in csv.split(',') {
            match name.trim() {
                "substitute" => kinds.substitute = true,
                "delete" => kinds.delete = true,
                "insert" => kinds.insert = true,
                "transpose" => kinds.transpose = true,
                "voicing" => kinds.voicing = true,
                _ => {}
            }
        }
        kinds
    }

    /// 1 つでも有効か
    pub fn any(&self) -> bool {
        self.substitute || self.delete || self.insert || self.transpose || self.voicing
    }
}

/// 1 回の編集操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditOp {
    /// 種類
    pub kind: EditKind,
    /// 打鍵列（かな入力では読み）の文字位置
    pub pos: usize,
    /// 実際に打たれた文字（脱落では `None`）
    pub typed: Option<char>,
    /// 本来打つつもりだった文字（挿入では `None`。転置では先頭側）
    pub intended: Option<char>,
}

/// 補正された読みの 1 候補
#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    /// 補正後の読み（ひらがな）
    pub reading: String,
    /// 累積コスト（小さいほどありそうな補正）
    pub cost: f32,
    /// 適用した編集操作
    pub ops: Vec<EditOp>,
}

/// 展開のパラメータ
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// 累積コストの上限
    pub max_cost: f32,
    /// 返す補正読みの最大数（コストの小さい順）
    pub max_results: usize,
    /// 探索するノード数の上限（決定的な打ち切り。レイテンシの上限を兼ねる）
    pub max_nodes: usize,
    /// 実時間の打ち切り（`None` なら無し。時間による打ち切りは結果が実行環境に依存するので、
    /// 決定性が要るテストでは `None` にする）
    pub deadline: Option<Duration>,
    /// 有効にする操作
    pub kinds: EditKinds,
    /// 打鍵のタッチ座標を置換コストに使うか（`input.fuzzy.use_touch_coords`）。座標のない打鍵は常に離散コスト
    pub use_touch: bool,
    /// タッチ位置のばらつき（指の位置の標準偏差。キー間隔を 1 とする）。[`DEFAULT_TOUCH_SIGMA`]
    pub touch_sigma: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            max_cost: DEFAULT_MAX_COST,
            max_results: 12,
            max_nodes: 2500,
            deadline: Some(Duration::from_millis(2)),
            kinds: EditKinds::ALL,
            use_touch: true,
            touch_sigma: DEFAULT_TOUCH_SIGMA,
        }
    }
}

/// コストを整数（ミリ単位）にする。ヒープの順序を厳密・決定的にするため
pub(crate) fn milli(cost: f32) -> u32 {
    (cost * 1000.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_cost_range() {
        assert_eq!(substitute_cost(0), 2.0);
        assert!((substitute_cost(2) - 2.4).abs() < 1e-6);
        assert_eq!(substitute_cost(5), 3.0);
        assert_eq!(substitute_cost(40), 3.0);
    }

    #[test]
    fn kinds_parse() {
        assert_eq!(
            EditKinds::parse("substitute,delete,insert,transpose,voicing"),
            EditKinds::ALL
        );
        let k = EditKinds::parse(" transpose , bogus,voicing");
        assert!(k.transpose && k.voicing && !k.substitute && !k.delete && !k.insert);
        assert!(!EditKinds::parse("").any());
    }
}
