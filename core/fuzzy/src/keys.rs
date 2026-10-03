//! 位置ごとの代替キー。

use crate::config::{FuzzyConfig, distance_cost};

/// 1 つの代替（文字とコスト）。コスト 0 が実際に打たれたキー
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyAlternative {
    /// 文字
    pub ch: char,
    /// 打たれたキーに対する相対コスト（nat。小さいほど尤もらしい）
    pub cost: f32,
}

/// 1 打鍵位置の代替列。先頭が打たれたキー（コスト 0）で、残りはコスト昇順
#[derive(Debug, Clone, PartialEq)]
pub struct KeyAlternatives {
    items: Vec<KeyAlternative>,
}

impl KeyAlternatives {
    /// 代替なし（打たれたキーのみ）
    pub fn exact(typed: char) -> Self {
        Self {
            items: vec![KeyAlternative {
                ch: typed,
                cost: 0.0,
            }],
        }
    }

    /// 打たれたキーと、タッチ位置から各キー/方向ゾーン中心までの距離（キー幅正規化）から作る。
    ///
    /// `max_distance` を超える代替は捨てる。`alts` に `typed` 自身が含まれる場合は、
    /// その距離のコストを基準にした相対コストにする。同じ文字は小さい方を残す。
    pub fn from_distances(typed: char, alts: &[(char, f32)], cfg: &FuzzyConfig) -> Self {
        let base = alts
            .iter()
            .filter(|(c, _)| *c == typed)
            .map(|(_, d)| distance_cost(*d, cfg))
            .fold(f32::INFINITY, f32::min);
        let base = if base.is_finite() { base } else { 0.0 };
        let mut items: Vec<KeyAlternative> = Vec::new();
        for &(ch, d) in alts {
            if ch == typed || d > cfg.max_distance || d.is_nan() {
                continue;
            }
            let cost = (distance_cost(d, cfg) - base).max(0.0);
            match items.iter_mut().find(|a| a.ch == ch) {
                Some(a) => a.cost = a.cost.min(cost),
                None => items.push(KeyAlternative { ch, cost }),
            }
        }
        items.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        items.insert(
            0,
            KeyAlternative {
                ch: typed,
                cost: 0.0,
            },
        );
        Self { items }
    }

    /// 打たれたキー
    pub fn typed(&self) -> char {
        self.items[0].ch
    }

    /// 全代替（先頭が打たれたキー）
    pub fn items(&self) -> &[KeyAlternative] {
        &self.items
    }
}
