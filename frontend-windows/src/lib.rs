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
//! - [`session`]: セッションごとの最新 preedit の保持（`restore_session` の材料）
//! - [`process`]: 子プロセス（imserver）の stdin/stdout を通信路にする実装

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod client;
pub mod keynorm;
pub mod process;
pub mod revision;
pub mod session;
pub mod supervisor;

#[cfg(test)]
mod testing;
