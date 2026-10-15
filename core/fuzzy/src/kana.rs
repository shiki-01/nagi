//! かな直接入力（12 キー・フリック）の誤りの展開用の表。
//!
//! 表のデータ（各行の文字、濁点・小書きの循環）は engine の `touch.rs`（`key_table`）が唯一の情報源で、
//! ここは受け取った表を引きやすい形にするだけ（二重定義を避ける）。

use crate::edit::{COST_FLICK_BOUNDARY, COST_ROW_FAR, COST_ROW_NEAR, COST_VOICING};
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
    /// 文字 → 行内の方向の添字（中央 0・左・上・右・下）
    direction: BTreeMap<char, usize>,
}

/// 方向の添字に対応するフリックの角度（度。右 0、上 90、左 180、下 270。反時計回り）。中央はタップ
fn direction_angle(dir: usize) -> Option<f32> {
    match dir {
        LEFT => Some(180.0),
        UP => Some(90.0),
        RIGHT => Some(0.0),
        DOWN => Some(270.0),
        _ => None,
    }
}

/// 2 つの角度（度）の差（0..180）
fn angle_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// フリックの方向が、隣の方向との境界（隣り合う 2 方向の中間、45°）から何度以内なら割り引くか
const FLICK_WINDOW_DEG: f32 = 45.0;

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
        let mut direction: BTreeMap<char, usize> = BTreeMap::new();
        for chars in rows {
            for (i, &c) in chars.iter().enumerate() {
                direction.insert(c, i);
            }
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
        KanaTable {
            voicing,
            row,
            direction,
        }
    }

    /// 濁点・半濁点・小書きの別形（コストは [`COST_VOICING`]）
    pub fn voicing_variants(&self, c: char) -> &[char] {
        self.voicing.get(&c).map_or(&[], Vec::as_slice)
    }

    /// 同じ行内の隣の文字とコスト
    pub fn row_neighbors(&self, c: char) -> &[(char, f32)] {
        self.row.get(&c).map_or(&[], Vec::as_slice)
    }

    /// フリックの角度（度）と距離から、`typed` を `intended`（同じ行の隣の方向の文字）と取り違えた
    /// 置換コスト。方向が 2 方向の境界（隣り合う方向の中間）に近いほど安く、境界で
    /// [`COST_FLICK_BOUNDARY`]、境界から 45° 離れれば通常の [`COST_ROW_NEAR`] になる（それ以上は
    /// 割り引かない）。方向どうしが隣り合わない（反対方向）・中央との取り違えは `None`（通常のコスト）。
    /// 距離は現状使わない（角度のみ）
    pub fn flick_cost(&self, typed: char, intended: char, flick: (f32, f32)) -> Option<f32> {
        let a = direction_angle(*self.direction.get(&typed)?)?;
        let b = direction_angle(*self.direction.get(&intended)?)?;
        if angle_diff(a, b) > 90.0 + 1e-3 || angle_diff(a, b) < 1e-3 {
            return None;
        }
        // 2 方向の中間の角度（90° 離れているので円周上の中点は曖昧にならない）
        let rad = |d: f32| d.to_radians();
        let mid = (rad(a).sin() + rad(b).sin())
            .atan2(rad(a).cos() + rad(b).cos())
            .to_degrees();
        let off = angle_diff(flick.0, mid);
        if off >= FLICK_WINDOW_DEG {
            return None;
        }
        let t = off / FLICK_WINDOW_DEG;
        Some(COST_FLICK_BOUNDARY + (COST_ROW_NEAR - COST_FLICK_BOUNDARY) * t)
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

    #[test]
    fn flick_cost_is_cheaper_near_the_direction_boundary() {
        let t = table();
        // い(左 180°) と う(上 90°) の境界は 135°
        let at = |deg: f32| t.flick_cost('い', 'う', (deg, 0.8)).unwrap();
        assert!((at(135.0) - COST_FLICK_BOUNDARY).abs() < 1e-4);
        assert!(at(135.0) < at(150.0) && at(150.0) < at(170.0));
        // 左のど真ん中（180°）は割り引かない
        assert!(t.flick_cost('い', 'う', (180.0, 0.8)).is_none());
        // 反対方向（い-え）と中央（い-あ）は対象外
        assert!(t.flick_cost('い', 'え', (135.0, 0.8)).is_none());
        assert!(t.flick_cost('い', 'あ', (135.0, 0.8)).is_none());
        // 右(0°) と下(270°) の境界は 315°
        assert!(t.flick_cost('え', 'お', (-45.0, 0.8)).unwrap() < COST_ROW_NEAR);
    }
}
