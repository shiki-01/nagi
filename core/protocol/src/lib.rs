//! Nagi プロトコル: JSON-RPC 2.0 のメッセージ型（正のスキーマ）。
//!
//! コアエンジンとフロントエンド（macOS IMKit / Windows TSF）は、
//! JSON-RPC 2.0（改行区切り、stdin/stdout）でこのプロトコルで通信します。
//!
//! # モジュール構成
//!
//! - [`key`]: キーイベント（KeyEvent、Modifiers、KeyKind）と XKB keysym 定数
//! - [`view`]: 表示型（PreeditView、CandidatesView、InputMode など）
//! - [`methods`]: メソッドの params/result 型と method 名定数
//! - [`utf16`]: UTF-16 ユーティリティ（offset 変換）
//! - [`jsonrpc`]: JSON-RPC エンベロープ・エラーコード（typeshare 対象外）

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod jsonrpc;
pub mod key;
pub mod methods;
pub mod utf16;
pub mod view;

// 主要型の再エクスポート
pub use key::{KeyEvent, KeyKind, Modifiers};
pub use methods::{
    ConfigError, EmptyResult, InitializeParams, InitializeResult, MemoryPressureState, NotifyLevel,
    NotifyParams, ProcessKeyParams, ProcessKeyResult, ReconvertParams, ReloadConfigResult,
    RestoreSegment, RestoreSessionParams, SelectCandidateParams, ServerCapabilities, SessionParams,
    SetContextParams, SetEnvironmentParams, StatusResult, SurroundingText, ThermalState,
    UpdateCandidatesParams, UpdatePreeditParams,
};
pub use view::{
    CandidateSource, CandidateView, CandidatesView, InputMode, PreeditView, RangeU16, SegmentKind,
    SegmentView,
};

/// プロトコルバージョン（現在 1）
pub const PROTOCOL_VERSION: u32 = 1;
