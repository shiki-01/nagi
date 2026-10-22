//! Nagi Windows フロントエンド（Phase 5）の OS 非依存部分。
//!
//! TSF/COM 層は含まず、Linux でもテストできる純粋なロジックだけを置く。
//! macOS 版（`frontend-macos/Sources/NagiClient` / `NagiIME/KeyNormalizer.swift`）と同じ設計で、
//! ロジックはコア側に置き、ここは変換とプロセス管理のみを行う。
//!
//! - [`keynorm`]: Windows 仮想キー → `KeyEvent`（XKB keysym 正規化）
//! - [`client`]: 改行区切り JSON-RPC 2.0 クライアントと通信路 trait
//! - [`revision`]: revision による古い非同期通知の破棄
//! - [`supervisor`]: エンジン子プロセスの指数バックオフ再起動と `restore_session`
//! - [`process`]: 子プロセス（imserver）の stdin/stdout を通信路にする実装

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod client;
pub mod keynorm;
pub mod process;
pub mod revision;
pub mod supervisor;

/// TSF/COM 層（未実装）。
///
/// ITfTextInputProcessorEx / ITfKeyEventSink / ITfComposition / 候補ウィンドウ等は
/// `windows` クレートが必要で `unsafe` を伴うため、`unsafe_code = "forbid"` のこのクレートには置かず、
/// 別クレート（または C++ 薄層）に分ける想定。ここにはキー入力を [`keynorm`] へ渡し、
/// 応答を [`supervisor::EngineSupervisor`] から受ける接続点だけを将来置く。
#[cfg(windows)]
pub mod tsf {}

#[cfg(test)]
mod testing;
