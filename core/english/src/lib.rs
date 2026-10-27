//! Nagi 英語の自動判定（`docs/plan-auto-english.md` 段階 1）。
//!
//! ローマ字のまま打たれた英単語を、日本語のローマ字入力と見分ける。判定の手がかりは 2 つ:
//!
//! - [`features`]: キー列をローマ字変換したとき、かなにならない ASCII がどれだけ残るか
//! - [`lexicon`]: 英単語の語彙（頻度の段階つき。fst 索引を mmap で読む）。無くても動く（劣化動作）
//!
//! [`spell`] は語彙に無い打鍵列に近い語（綴りの「もしかして」）を探す。
//!
//! [`Judge`] がこの 2 つと「日本語として引ける読みか」を合わせて [`Verdict`] を返す。
//! 状態機械・辞書には依存しない（呼び出し側が判断材料を渡す）。

pub mod features;
pub mod judge;
pub mod lexicon;
pub mod scowl;
pub mod spell;

pub use features::{Features, analyze};
pub use judge::{HIGH_THRESHOLD, Judge, Level, MID_THRESHOLD, VALID_ROMAJI_SPELL_COST, Verdict};
pub use lexicon::{Lexicon, LexiconBuilder, LexiconError};
pub use spell::{SpellCosts, Suggestion, suggest};
