//! キー配置と隣接表。
//!
//! 配置は矩形の集合（`KeyLayout`）で表し、そこから各キーの近傍キーと距離順位を計算する。
//! M1 は標準 QWERTY の矩形配置を内蔵する。将来はタッチ座標（M2）やフロントが渡す実レイアウト
//! （iOS キーボードの実寸）も、同じ `KeyLayout` に載せて渡せる。

use std::collections::BTreeMap;

/// 近傍とみなす距離の既定値（キー幅の何倍まで。中心間距離）。
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
}

/// 近傍の 1 キー
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbor {
    /// 近傍キーの文字
    pub label: char,
    /// 近い順の順位（0 が最も近い）
    pub rank: usize,
    /// 中心間距離（キー幅を 1 とする）
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
        let mean = if layout.keys.is_empty() {
            1.0
        } else {
            layout.keys.iter().map(|k| (k.w + k.h) / 2.0).sum::<f32>() / layout.keys.len() as f32
        };
        let limit = reach * mean;
        let mut map = BTreeMap::new();
        for a in &layout.keys {
            let (ax, ay) = a.center();
            let mut near: Vec<(f32, char)> = layout
                .keys
                .iter()
                .filter(|b| b.label != a.label)
                .filter_map(|b| {
                    let (bx, by) = b.center();
                    let d = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
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
                    distance: distance / mean,
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
