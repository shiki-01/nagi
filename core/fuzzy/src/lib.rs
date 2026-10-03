//! Nagi 曖昧入力: ローマ字の構造に基づくタイポ補正と、タッチ距離コストによる読み仮説の生成。
//!
//! 特定のキーボード配列・QWERTY 隣接表・フリック角度モデルには依存しない。
//! タッチ・フリックの曖昧さはフロントが `(文字, 距離)` の代替として渡し、
//! コアはそれを [`distance_cost`] でコストに変換するだけである。
//! タイポ補正はローマ字の構造（子音・母音の欠落、余剰、入れ替え、音の紛れ）のみに基づく。
//!
//! 辞書への依存は [`PrefixOracle`] で逆転している。結線（`Dictionary` への実装）はエンジン側で行う。

mod config;
mod keys;
mod search;

pub use config::{CONSONANT_CONFUSIONS, FuzzyConfig, distance_cost};
pub use keys::{KeyAlternative, KeyAlternatives};
pub use search::{
    EditSummary, ExpandStats, PrefixOracle, ReadingHypothesis, expand, expand_typed,
    expand_with_stats,
};
