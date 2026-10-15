//! Nagi 曖昧入力（M1）: キー配置に応じたタイプミスの補正候補の展開。
//!
//! 読み（ひらがな）が真実で、入力中の読みは打った通りに保つ。ここは「もしかして」候補のための
//! **補正読みの列挙**だけを行う（置換はしない）。M1 はキーの近接（隣接表）と辞書だけを使い、
//! タッチ座標（M2）も LLM（M3）も使わない。
//!
//! - [`layout`] — キー配置から隣接表（近傍キーと距離順位）を作る
//! - [`edit`] — 編集操作の種類・コスト・探索パラメータ
//! - [`index`] — 辞書の trie による枝刈り（[`ReadingIndex`]）
//! - [`kana`] — かな直接入力（12 キー）の濁点・小書き・同じ行内の隣の文字
//! - [`reverse`] — 読みから打鍵列（ローマ字）の復元
//!
//! 補正読みから表記を作る・元の読みより良いか判定する（辞書の Viterbi コスト）のは呼び出し側。

pub mod edit;
pub mod index;
pub mod kana;
pub mod layout;
pub mod reverse;
mod search;

pub use edit::{Correction, EditKind, EditKinds, EditOp, Params};
pub use index::{ReadingIndex, WordSet, covered, viable_prefix};
pub use kana::KanaTable;
pub use layout::{Adjacency, DEFAULT_REACH, Key, KeyLayout, Neighbor};
pub use reverse::ReverseRomaji;

use nagi_romaji::RomajiTable;
use std::sync::Arc;

/// 展開する読みの最小・最大の文字数。短すぎると誤補正が増え（1〜2 文字は何でも辞書語になる）、
/// 長すぎると探索が重い（長い文は文節に分かれてから呼ばれる）
pub const MIN_READING_CHARS: usize = 3;
/// 展開する読みの最大文字数
pub const MAX_READING_CHARS: usize = 24;

/// 曖昧入力の展開器
#[derive(Debug, Clone)]
pub struct Fuzzy {
    adjacency: Adjacency,
    kana: KanaTable,
    table: Arc<RomajiTable>,
    reverse: ReverseRomaji,
}

impl Fuzzy {
    /// 配置・かな表・ローマ字テーブルから作る
    pub fn new(layout: &KeyLayout, kana: KanaTable, table: Arc<RomajiTable>) -> Self {
        Fuzzy {
            adjacency: Adjacency::from_layout(layout, DEFAULT_REACH),
            kana,
            reverse: ReverseRomaji::new(&table),
            table,
        }
    }

    /// 標準 QWERTY と標準ローマ字テーブルで作る
    pub fn qwerty(kana: KanaTable) -> Self {
        Self::new(&KeyLayout::qwerty(), kana, RomajiTable::standard())
    }

    /// 隣接表
    pub fn adjacency(&self) -> &Adjacency {
        &self.adjacency
    }

    /// 入力中の読み `reading`（ひらがなと、未変換のまま残った英小文字だけ）の補正読みを、
    /// コストの小さい順に最大 `params.max_results` 件返す。`reading` 自身は含まない。
    ///
    /// 読みを打鍵列（ローマ字）に戻して打ち間違いを展開する。読みに英字が無ければ、かな直接入力の
    /// 誤り（濁点・小書き・同じ行内の隣）も展開する。展開した読みのうち辞書で成立しないものは
    /// 途中で捨てる（`index`）
    pub fn corrections(
        &self,
        reading: &str,
        index: &dyn ReadingIndex,
        params: &Params,
    ) -> Vec<Correction> {
        let count = reading.chars().count();
        if !(MIN_READING_CHARS..=MAX_READING_CHARS).contains(&count)
            || !params.kinds.any()
            || params.max_results == 0
        {
            return Vec::new();
        }
        let valid = |c: char| ('ぁ'..='ん').contains(&c) || c == 'ー' || c.is_ascii_lowercase();
        if !reading.chars().all(valid) {
            return Vec::new();
        }
        let mut all: Vec<Correction> = Vec::new();
        if let Some(typed) = self.reverse.to_romaji(reading) {
            let search = search::RomajiSearch {
                adjacency: &self.adjacency,
                table: &self.table,
            };
            all.extend(search.run(&typed, index, params));
        }
        if reading.chars().all(|c| !c.is_ascii()) {
            let search = search::KanaSearch { table: &self.kana };
            all.extend(search.run(reading, index, params));
        }
        all.retain(|c| c.reading != reading);
        search::merge(all, params.max_results)
    }
}

#[cfg(test)]
mod tests;
