//! かな直接入力（12 キー・フリック）の誤りの展開用の表。
//!
//! 表のデータ（各行の文字、濁点・小書きの循環）は engine の `touch.rs`（`key_table`）が唯一の情報源で、
//! ここは受け取った表を引きやすい形にするだけ（二重定義を避ける）。

use crate::edit::{COST_ROW_FAR, COST_ROW_NEAR, COST_VOICING};
use std::collections::BTreeMap;

/// フリック行の文字の並び（中央 0・左・上・右・下）のうち、方向の添字
const LEFT: usize = 1;
const UP: usize = 2;
const RIGHT: usize = 3;
const DOWN: usize = 4;

fn is_kana(c: char) -> bool {
    ('ぁ'..='ん').contains(&c)
}

/// かなの誤り展開の表
#[derive(Debug, Clone, Default)]
pub struct KanaTable {
    /// 文字 → 同じ循環に属する他の文字（濁点・半濁点・小書き）
    voicing: BTreeMap<char, Vec<char>>,
    /// 文字 → (同じ行内の隣の文字, コスト)
    row: BTreeMap<char, Vec<(char, f32)>>,
}

impl KanaTable {
    /// `rows` は 12 キーの各キーの文字 `[中央, 左, 上, 右, 下]`、`cycles` は濁点・小書きの循環。
    /// ひらがなでない文字（括弧・長音・記号）は展開の対象外
    pub fn new(rows: &[[char; 5]], cycles: &[Vec<char>]) -> Self {
        let mut voicing: BTreeMap<char, Vec<char>> = BTreeMap::new();
        for cycle in cycles {
            for &c in cycle {
                let others: Vec<char> = cycle.iter().copied().filter(|&o| o != c).collect();
                voicing.entry(c).or_default().extend(others);
            }
        }
        let mut row: BTreeMap<char, Vec<(char, f32)>> = BTreeMap::new();
        for chars in rows {
            // 中央と各方向は隣り合い、方向どうしは周回で隣（左-上-右-下-左）。反対方向だけが遠い
            let far =
                |a: usize, b: usize| matches!((a.min(b), a.max(b)), (LEFT, RIGHT) | (UP, DOWN));
            for (i, &a) in chars.iter().enumerate() {
                if !is_kana(a) {
                    continue;
                }
                for (j, &b) in chars.iter().enumerate() {
                    if i == j || !is_kana(b) {
                        continue;
                    }
                    let cost = if far(i, j) {
                        COST_ROW_FAR
                    } else {
                        COST_ROW_NEAR
                    };
                    row.entry(a).or_default().push((b, cost));
                }
            }
        }
        KanaTable { voicing, row }
    }

    /// 濁点・半濁点・小書きの別形（コストは [`COST_VOICING`]）
    pub fn voicing_variants(&self, c: char) -> &[char] {
        self.voicing.get(&c).map_or(&[], Vec::as_slice)
    }

    /// 同じ行内の隣の文字とコスト
    pub fn row_neighbors(&self, c: char) -> &[(char, f32)] {
        self.row.get(&c).map_or(&[], Vec::as_slice)
    }

    /// 濁点・小書きの展開コスト
    pub const fn voicing_cost() -> f32 {
        COST_VOICING
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> KanaTable {
        KanaTable::new(
            &[
                ['あ', 'い', 'う', 'え', 'お'],
                ['や', '（', 'ゆ', '）', 'よ'],
            ],
            &[
                vec!['は', 'ば', 'ぱ'],
                vec!['つ', 'っ', 'づ'],
                vec!['か', 'が'],
            ],
        )
    }

    #[test]
    fn voicing_cycles() {
        let t = table();
        assert_eq!(t.voicing_variants('か'), ['が']);
        assert_eq!(t.voicing_variants('ば'), ['は', 'ぱ']);
        assert_eq!(t.voicing_variants('つ'), ['っ', 'づ']);
        assert!(t.voicing_variants('あ').is_empty());
    }

    #[test]
    fn row_neighbors_costs() {
        let t = table();
        let a: BTreeMap<char, f32> = t.row_neighbors('あ').iter().copied().collect();
        assert_eq!(a.len(), 4);
        assert_eq!(a[&'い'], COST_ROW_NEAR, "中央から各方向");
        // い(左) の隣は う(上) と お(下)、反対の え(右) は遠い
        let i: BTreeMap<char, f32> = t.row_neighbors('い').iter().copied().collect();
        assert_eq!(i[&'あ'], COST_ROW_NEAR);
        assert_eq!(i[&'う'], COST_ROW_NEAR);
        assert_eq!(i[&'お'], COST_ROW_NEAR);
        assert_eq!(i[&'え'], COST_ROW_FAR);
        // 括弧は対象外
        let ya: Vec<char> = t.row_neighbors('や').iter().map(|p| p.0).collect();
        assert_eq!(ya, ['ゆ', 'よ']);
    }
}
