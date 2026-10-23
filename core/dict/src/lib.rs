//! Nagi 辞書: Mozc OSS 辞書の読み込み・double-array trie・連接コスト・Viterbi。
//!
//! モデルが無くても日本語入力できる `minimal` 経路の中核。使い方:
//!
//! 1. `cargo xtask fetch-dict` で Mozc OSS 辞書を取得し、`cargo xtask build-dict` で
//!    `data/dict/nagi-dict.bin` を生成する
//! 2. [`Dictionary::load`] で読み込み、[`Dictionary::convert`]（文の最良変換 + 文節区切り）と
//!    [`Dictionary::segment_candidates`]（文節 1 つ分の候補）を使う
//!
//! テストでは [`DictionaryBuilder`] で小さな辞書をメモリ上に組み立てられる。

mod align;
pub mod builder;
pub mod bunsetsu;
pub mod convert;
pub mod dictionary;
pub mod error;
pub mod import;
mod nbest;
pub mod overlay;
pub mod pos;
mod reverse;
pub mod user;

pub use builder::{
    ConnectionLayout, DictionaryBuilder, build_from_mozc_dir, build_from_sudachi_files,
};
pub use convert::{ConvertedSegment, SegmentCandidate};
pub use dictionary::{Dictionary, Entry};
pub use error::DictError;
pub use import::{
    ImportError, ImportFormat, ImportOutcome, ImportSkip, SkipReason, decode_import,
    import_user_dict,
};
pub use nbest::SentenceCandidate;
pub use overlay::{Overlay, OverlayEntry};
pub use user::{SNIPPET_POS, UserDictIssue, UserDictionary, UserEntry, decode_text};
