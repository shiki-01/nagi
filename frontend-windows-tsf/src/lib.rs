//! Nagi Windows フロントエンドの TSF/COM 層。
//!
//! `nagi-windows`（OS 非依存・`unsafe` 禁止）の上に載せる薄い層で、役割は変換と描画だけ:
//! キー入力を `nagi_windows::keynorm` で正規化してエンジンへ送り、応答の確定・preedit を
//! `ITfComposition` / `ITfEditSession` で反映する。何を確定・表示するかはエンジンが決める
//! （ロジックをフロントに持ち込まない。CLAUDE.md の分離の原則）。
//!
//! - [`render`]: 応答 → 編集計画（OS 非依存。Linux でもテストする）
//! - [`engine`]: エンジン接続のまとめ（OS 非依存。UI スレッドを塞がない起動・短いタイムアウト）
//! - [`tsf_log`]: 診断ログ `%LOCALAPPDATA%\Nagi\logs\tsf.log` への追記（入力内容は書かない。H-54）
//! - Windows のみ: COM/TSF の実装（`dll` / `register` / `text_service` / `edit` / `keys`）。
//!   `unsafe` はこれらのモジュールに隔離し、`#![allow(unsafe_code)]` で明示している
//!
//! # Win32 / TSF 層（H-41）
//! - `candidate_window` / `toast_window`: 自前のポップアップ（GDI。フォーカスを奪わない）。寸法・配置・文言は `nagi_windows`
//! - `message_window`: 隠しメッセージウィンドウ。別スレッドのエンジン通知を `PostMessage` で UI スレッドへ渡して
//!   即時に反映し、定期タイマー（環境の報告・スリープ復帰の検出）と電源通知も受ける
//! - `probe`: `EnvironmentProbe` の Win32 実装 / `context_info`: 周辺テキスト・`InputScope`・アプリ ID
//! - `display_attr`: `ITfDisplayAttributeProvider` と `GUID_PROP_ATTRIBUTE` への書き込み
//! - `mode_comp`: 入力モードのコンパートメント（タスクトレイの「あ / A」）/ `langbar`: 言語バーのボタン
//!   （モード表示・クリックで切り替え・メニュー）/ `reconversion`: `ITfFnReconversion`
//!   （`Reconvert` と `GetReconversion`）
//!
//! # 未実装（TODO）
//! - 複数のホストプロセスでエンジンを共有する仕組み（現状はホストプロセスごとに imserver を起動する）
//! - 候補ウィンドウのマウス操作（v1 の非目標）
//! - アプリ内の自動更新（置き換え。通知のみ）、ARM64 / 32bit ホスト用の DLL
//!
//! # 検証状況
//! Linux ホストでの `cargo check/clippy --target x86_64-pc-windows-gnu`（型・シグネチャの確認）と、
//! OS 非依存部のテストのみ。**Windows 実機での動作（登録・キー入力・composition の描画）は未確認**。

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod engine;
pub mod render;
pub mod tsf_log;

#[cfg(windows)]
mod candidate_window;
#[cfg(windows)]
mod context_info;
#[cfg(windows)]
mod display_attr;
#[cfg(windows)]
mod dll;
#[cfg(windows)]
mod edit;
#[cfg(windows)]
mod guids;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod langbar;
#[cfg(windows)]
mod message_window;
#[cfg(windows)]
mod mode_comp;
#[cfg(windows)]
mod probe;
#[cfg(windows)]
mod reconversion;
#[cfg(windows)]
mod register;
#[cfg(windows)]
mod text_service;
#[cfg(windows)]
mod toast_window;
#[cfg(windows)]
mod win;
