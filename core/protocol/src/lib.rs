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
//! - [`settings`]: 設定 RPC（`get_settings` / `update_settings`）の型
//! - [`utf16`]: UTF-16 ユーティリティ（offset 変換）
//! - [`jsonrpc`]: JSON-RPC エンベロープ・エラーコード（typeshare 対象外）

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod jsonrpc;
pub mod key;
pub mod methods;
pub mod settings;
pub mod utf16;
pub mod view;

// 主要型の再エクスポート
pub use key::{KeyEvent, KeyKind, Modifiers};
pub use methods::{
    ConfigError, EmptyResult, InitializeParams, InitializeResult, InsertReadingParams, KanaKey,
    KeyGesture, MemoryPressureState, NotifyLevel, NotifyParams, PerformActionParams,
    ProcessKeyParams, ProcessKeyResult, ReadingInput, ReconvertParams, ReloadConfigResult,
    RestoreSegment, RestoreSessionParams, SelectCandidateParams, ServerCapabilities, SessionParams,
    SetContextParams, SetEnvironmentParams, StatusResult, SurroundingText, ThermalState,
    TouchAction, TouchDirection, UpdateCandidatesParams, UpdatePreeditParams,
};
pub use settings::{
    ChoiceOptions, FloatRange, IntRange, SettingChange, SettingChoice, SettingError, SettingItem,
    SettingKind, SettingLevel, SettingPlatforms, SettingValue, SettingsSnapshot,
    UpdateSettingsParams, UpdateSettingsResult,
};
pub use view::{
    CandidateSource, CandidateView, CandidatesView, InputMode, PreeditView, RangeU16, SegmentKind,
    SegmentView,
};

/// プロトコルバージョン（現在 1）
pub const PROTOCOL_VERSION: u32 = 1;
