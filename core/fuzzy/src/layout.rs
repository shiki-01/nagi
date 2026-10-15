//! キー配置と隣接表。
//!
//! 配置は矩形の集合（`KeyLayout`）で表し、そこから各キーの近傍キーと距離順位を計算する。
//! 標準 QWERTY の矩形配置を内蔵し、フロントが `set_layout_geometry` で渡す実レイアウト
//! （iOS キーボードの実寸、QWERTY 以外・カスタム配列）も、同じ `KeyLayout` に載せて使える（M2）。
//! 距離は「キーの並びの間隔（ピッチ）」を 1 とする（キー間の隙間を含めた間隔なので、
//! 縦横比の違うキーでも隣が 1 前後になる）。

use std::collections::BTreeMap;

/// 近傍とみなす距離の既定値（キー間隔の何倍まで。中心間距離）。
///
/// 1.3 にすると、QWERTY（行ごとに 0.25 / 0.5 キーずれた段）で、横の 2 つ（距離 1.0）と
/// 上下の斜め 2+2 つ（1.03〜1.25）が入り、2 つ隣（2.0）や 1 段飛ばし（2.0 以上）は入らない。
/// 例: j の近傍は h・k・u・n・m・i の 6 つ
pub const DEFAULT_REACH: f32 = 1.3;

/// 1 つのキー
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    /// キーの識別子（フロントのキー ID。補正には使わない）
    pub id: String,
    /// このキーで入力される文字
    pub label: char,
    /// 左端の座標
    pub x: f32,
    /// 上端の座標
    pub y: f32,
    /// 幅
    pub w: f32,
    /// 高さ
    pub h: f32,
}

impl Key {
    fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// キー配置（矩形の集合）
#[derive(Debug, Clone, PartialEq)]
pub struct KeyLayout {
    /// キー
    pub keys: Vec<Key>,
}

impl KeyLayout {
    /// 標準 QWERTY の矩形配置（アルファベット 26 キー。段のずれは ANSI 配列の 0 / 0.25 / 0.75）
    pub fn qwerty() -> Self {
        const ROWS: [(&str, f32); 3] =
            [("qwertyuiop", 0.0), ("asdfghjkl", 0.25), ("zxcvbnm", 0.75)];
        let mut keys = Vec::new();
        for (row, (letters, offset)) in ROWS.iter().enumerate() {
            for (col, label) in letters.chars().enumerate() {
                keys.push(Key {
                    id: label.to_string(),
                    label,
                    x: offset + col as f32,
                    y: row as f32,
                    w: 1.0,
                    h: 1.0,
                });
            }
        }
        KeyLayout { keys }
    }

    /// 文字のキー
    pub fn key(&self, label: char) -> Option<&Key> {
        self.keys.iter().find(|k| k.label == label)
    }

    /// 横・縦のキー間隔（隣のキーの中心までの距離。隙間を含む）。
    /// 横は同じ段で隣り合うキーの中心間距離の中央値、縦は段の中心の間隔の中央値。
    /// 求められなければ（1 段しかない等）キーの平均の幅・高さ
    pub fn pitch(&self) -> (f32, f32) {
        let n = self.keys.len().max(1) as f32;
        let mean_w = self.keys.iter().map(|k| k.w).sum::<f32>() / n;
        let mean_h = self.keys.iter().map(|k| k.h).sum::<f32>() / n;
        let fallback = (
            if mean_w > 0.0 { mean_w } else { 1.0 },
            if mean_h > 0.0 { mean_h } else { 1.0 },
        );
        // 中心の y で段にまとめる
        let mut order: Vec<&Key> = self.keys.iter().collect();
        order.sort_by(|a, b| a.center().1.total_cmp(&b.center().1));
        let mut rows: Vec<Vec<&Key>> = Vec::new();
        for k in order {
            match rows.last_mut() {
                Some(row) if (k.center().1 - row[0].center().1).abs() < mean_h / 2.0 => row.push(k),
                _ => rows.push(vec![k]),
            }
        }
        let mut xs: Vec<f32> = Vec::new();
        for row in &mut rows {
            row.sort_by(|a, b| a.center().0.total_cmp(&b.center().0));
            for pair in row.windows(2) {
                let gap = pair[1].center().0 - pair[0].center().0;
                // 間のキーが抜けている（空白キー等で飛んだ）ものは除く
                if gap > 0.0 && gap < mean_w * 1.6 {
                    xs.push(gap);
                }
            }
        }
        let row_y: Vec<f32> = rows
            .iter()
            .map(|r| r.iter().map(|k| k.center().1).sum::<f32>() / r.len() as f32)
            .collect();
        let mut ys: Vec<f32> = row_y.windows(2).map(|w| w[1] - w[0]).collect();
        ys.retain(|g| *g > 0.0 && *g < mean_h * 1.6);
        let median = |v: &mut Vec<f32>, default: f32| {
            if v.is_empty() {
                default
            } else {
                v.sort_by(f32::total_cmp);
                v[v.len() / 2]
            }
        };
        (median(&mut xs, fallback.0), median(&mut ys, fallback.1))
    }
}

/// タッチ位置から求める置換コストのモデル（M2）。
///
/// 押されたキー（`pressed`）の矩形内の相対位置 `rel`（0..1）から、本来は隣の `intended` を狙って
/// いた尤度を、ガウス分布で評価する: 狙ったキーの中心からの指の位置のばらつきを、横・縦とも
/// 標準偏差 `sigma`（キー間隔を 1 とする）の等方ガウスとみなすと、押されたキーを狙った場合と
/// 隣を狙った場合の対数尤度の差は `(d_intended² - d_pressed²) / (2σ²)`（d は指の位置からキー中心までの
/// 距離）。これに、置換という誤りそのものの事前コスト [`TOUCH_PRIOR`] を足す。
/// 境界に近いほど差が小さく、置換が安くなる（単調）。
#[derive(Debug, Clone)]
pub struct TouchModel {
    keys: BTreeMap<char, Key>,
    pitch: (f32, f32),
}

/// タッチ位置による置換コストの下限（指が 2 つのキーの境界の真上にあるとき）。
/// 離散コスト（隣接順位ごとに 2.0〜3.0）の最小より少し軽くし、境界の打鍵は離散の最良の隣より
/// 補正しやすくする。評価（`docs/decisions.md` H-25）で調整した値
pub const TOUCH_PRIOR: f32 = 1.2;

/// タッチ位置による置換コストの上限（これを超えるなら実質的に補正しない）
pub const TOUCH_MAX_COST: f32 = 6.0;

impl TouchModel {
    /// 配置から作る
    pub fn new(layout: &KeyLayout) -> Self {
        TouchModel {
            keys: layout.keys.iter().map(|k| (k.label, k.clone())).collect(),
            pitch: layout.pitch(),
        }
    }

    /// `pressed` の矩形内の相対位置 `rel` を押したとき、本来は `intended` を狙っていたとする置換コスト。
    /// どちらかが配置に無ければ `None`（呼び出し側は離散コストを使う）
    pub fn substitute_cost(
        &self,
        pressed: char,
        intended: char,
        rel: (f32, f32),
        sigma: f32,
    ) -> Option<f32> {
        let p = self.keys.get(&pressed)?;
        let k = self.keys.get(&intended)?;
        let rx = rel.0.clamp(0.0, 1.0);
        let ry = rel.1.clamp(0.0, 1.0);
        let (x, y) = (p.x + rx * p.w, p.y + ry * p.h);
        let dist2 = |key: &Key| {
            let (cx, cy) = key.center();
            ((x - cx) / self.pitch.0).powi(2) + ((y - cy) / self.pitch.1).powi(2)
        };
        let sigma = sigma.max(0.05);
        let delta = (dist2(k) - dist2(p)).max(0.0) / (2.0 * sigma * sigma);
        Some((TOUCH_PRIOR + delta).min(TOUCH_MAX_COST))
    }
}

/// 近傍の 1 キー
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbor {
    /// 近傍キーの文字
    pub label: char,
    /// 近い順の順位（0 が最も近い）
    pub rank: usize,
    /// 中心間距離（キー間隔を 1 とする）
    pub distance: f32,
}

/// 隣接表: 各キーの近傍キー（近い順）
#[derive(Debug, Clone, Default)]
pub struct Adjacency {
    // 決定的な順序のため BTreeMap
    map: BTreeMap<char, Vec<Neighbor>>,
}

impl Adjacency {
    /// 配置から隣接表を作る。`reach` は近傍とみなす中心間距離の上限（キー幅の何倍か。
    /// 既定 [`DEFAULT_REACH`]）。距離が等しいキーは文字の順
    pub fn from_layout(layout: &KeyLayout, reach: f32) -> Self {
        let (px, py) = layout.pitch();
        let limit = reach;
        let mut map = BTreeMap::new();
        for a in &layout.keys {
            let (ax, ay) = a.center();
            let mut near: Vec<(f32, char)> = layout
                .keys
                .iter()
                .filter(|b| b.label != a.label)
                .filter_map(|b| {
                    let (bx, by) = b.center();
                    let d = (((ax - bx) / px).powi(2) + ((ay - by) / py).powi(2)).sqrt();
                    (d <= limit + 1e-4).then_some((d, b.label))
                })
                .collect();
            near.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
            let neighbors = near
                .into_iter()
                .enumerate()
                .map(|(rank, (distance, label))| Neighbor {
                    label,
                    rank,
                    distance,
                })
                .collect();
            map.insert(a.label, neighbors);
        }
        Adjacency { map }
    }

    /// 標準 QWERTY の隣接表
    pub fn qwerty() -> Self {
        Self::from_layout(&KeyLayout::qwerty(), DEFAULT_REACH)
    }

    /// `key` の近傍キー（近い順）。配置に無いキーは空
    pub fn neighbors(&self, key: char) -> &[Neighbor] {
        self.map.get(&key).map_or(&[], Vec::as_slice)
    }

    /// `a` から見た `b` の近さの順位。近傍でなければ `None`
    pub fn rank(&self, a: char, b: char) -> Option<usize> {
        self.neighbors(a)
            .iter()
            .find(|n| n.label == b)
            .map(|n| n.rank)
    }

    /// 2 つのキーが隣り合っているか（どちらから見ても）
    pub fn is_adjacent(&self, a: char, b: char) -> bool {
        self.rank(a, b).is_some() || self.rank(b, a).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn j_neighbors_in_qwerty() {
        let adj = Adjacency::qwerty();
        let j: Vec<char> = adj.neighbors('j').iter().map(|n| n.label).collect();
        for c in ['k', 'u', 'n', 'm', 'h', 'i'] {
            assert!(j.contains(&c), "{c} が j の近傍にない: {j:?}");
        }
        assert_eq!(j.len(), 6, "{j:?}");
        // 横の 2 つが最も近い
        assert_eq!(&j[..2], ['h', 'k']);
        assert!(!j.contains(&'y') && !j.contains(&'l'));
    }

    #[test]
    fn pitch_of_qwerty_and_phone_like_layout() {
        assert_eq!(KeyLayout::qwerty().pitch(), (1.0, 1.0));
        // 隙間つき・縦長のキー（実機の QWERTY に近い）でも、縦横の隣が近傍に入る
        let layout = phone_layout();
        let (px, py) = layout.pitch();
        assert!(
            (px - 42.0).abs() < 0.01 && (py - 64.0).abs() < 0.01,
            "{px} {py}"
        );
        let adj = Adjacency::from_layout(&layout, DEFAULT_REACH);
        let j: Vec<char> = adj.neighbors('j').iter().map(|n| n.label).collect();
        for c in ['h', 'k', 'u', 'i', 'n', 'm'] {
            assert!(j.contains(&c), "{c} が j の近傍にない: {j:?}");
        }
    }

    /// 幅 36・高さ 54、横の間隔 42・縦の間隔 64 の QWERTY（段のずれは QWERTY と同じ）
    pub(crate) fn phone_layout() -> KeyLayout {
        let mut layout = KeyLayout::qwerty();
        for k in &mut layout.keys {
            k.x *= 42.0;
            k.y *= 64.0;
            k.w = 36.0;
            k.h = 54.0;
        }
        layout
    }

    #[test]
    fn touch_cost_is_cheaper_near_the_boundary() {
        let model = TouchModel::new(&KeyLayout::qwerty());
        // j を押した。右端（k 側）に近いほど、本来は k だった可能性が高い
        let costs: Vec<f32> = [0.5, 0.7, 0.9, 1.0]
            .iter()
            .map(|&rx| model.substitute_cost('j', 'k', (rx, 0.5), 0.35).unwrap())
            .collect();
        assert!(costs.windows(2).all(|w| w[0] > w[1]), "{costs:?}");
        // 反対側（左）に寄っているのに k だったとするのは高い
        let away = model.substitute_cost('j', 'k', (0.1, 0.5), 0.35).unwrap();
        assert!(away > costs[0]);
        // 下限は事前コスト
        assert!(costs[3] >= TOUCH_PRIOR - 1e-6 && costs[3] < TOUCH_PRIOR + 0.5);
        // σ が大きいほど（指がぶれる）安い
        let loose = model.substitute_cost('j', 'k', (0.5, 0.5), 0.6).unwrap();
        let tight = model.substitute_cost('j', 'k', (0.5, 0.5), 0.3).unwrap();
        assert!(loose < tight);
        // 配置に無いキーは None
        assert!(model.substitute_cost('j', '1', (0.5, 0.5), 0.35).is_none());
    }

    #[test]
    fn corner_and_symmetry() {
        let adj = Adjacency::qwerty();
        let q: Vec<char> = adj.neighbors('q').iter().map(|n| n.label).collect();
        assert_eq!(q, ['w', 'a']);
        for a in 'a'..='z' {
            for n in adj.neighbors(a) {
                assert!(adj.rank(n.label, a).is_some(), "{a}-{} が非対称", n.label);
            }
        }
        assert!(adj.neighbors('1').is_empty());
        assert!(adj.is_adjacent('n', 'm') && !adj.is_adjacent('q', 'p'));
    }

    #[test]
    fn custom_layout_with_reach() {
        let layout = KeyLayout {
            keys: ["a", "b", "c"]
                .iter()
                .enumerate()
                .map(|(i, s)| Key {
                    id: (*s).to_string(),
                    label: s.chars().next().unwrap(),
                    x: i as f32 * 2.0,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                })
                .collect(),
        };
        let adj = Adjacency::from_layout(&layout, 1.2);
        assert_eq!(adj.neighbors('b').len(), 2);
        assert_eq!(adj.neighbors('a').len(), 1, "a-c は離れている");
    }
}
