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
//! - [`candidates`] — 候補ウィンドウのページング・行モデル・寸法・表示位置
//! - [`environment`] — 電源・メモリ・負荷 → `set_environment` の params
//! - [`context`] — 周辺テキスト・secure input → `set_context` の params
//! - [`preedit_style`] — preedit の下線などの表示属性の描画計画
//! - [`status`] — 通知・メニュー・入力モードの文言
//! - [`mode`] — 入力モードの OS（コンパートメント）との同期（同じモードなら送らない）
//! - [`reconvert`] — 再変換の対象テキストの判定
//! - [`toast`] — 通知パネルの配置
//! - [`icon`] — 入力方式アイコン（.ico）の生成
//! - [`diag`] — 診断ログ（tsf.log）の整形・ローテーション判断・イベントの文言（入力内容は含めない）
//! - [`process`]: 子プロセス（imserver）の stdin/stdout を通信路にする実装

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod candidates;
pub mod client;
pub mod context;
pub mod diag;
pub mod environment;
pub mod icon;
pub mod keynorm;
pub mod mode;
pub mod preedit_style;
pub mod process;
pub mod reconvert;
pub mod revision;
pub mod session;
pub mod status;
pub mod supervisor;
pub mod toast;

#[cfg(test)]
mod testing;
