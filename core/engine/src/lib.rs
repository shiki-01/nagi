//! Nagi コアエンジン: IR・状態機械・アクション・変換パイプライン。
//!
//! OS 非依存の純粋な状態機械。キー入力 → Action → 状態遷移 → 描画差分。変換そのものは
//! [`ConversionBackend`] の裏に置き、辞書（`minimal` 経路）や LLM を差し替えられる。

pub mod action;
pub mod apps;
pub mod backend;
pub mod engine;
pub mod ir;
pub mod kana;
pub mod keymap;
pub mod keys;
pub mod observer;
pub mod touch;

pub use action::{Action, Direction};
pub use apps::{AppClass, AppsConfig};
pub use backend::{
    BackendCandidate, BackendSegment, BackendSentence, Completion, ConversionBackend, KanaBackend,
    MockBackend, TypingHint,
};
pub use engine::{Engine, EngineConfig, InitialView, SessionSnapshot, StateKind};
pub use ir::{Candidate, CandidateSet, CursorPos, Preedit, Segment};
pub use keymap::{KeyContext, KeyMap};
pub use keys::{KeyParseError, key_from_text, parse_key};
pub use observer::{CommitContext, CommitObserver, CommittedSegment};
