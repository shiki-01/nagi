//! Nagi Windows フロントエンドの TSF/COM 層。
//!
//! `nagi-windows`（OS 非依存・`unsafe` 禁止）の上に載せる薄い層で、役割は変換と描画だけ:
//! キー入力を `nagi_windows::keynorm` で正規化してエンジンへ送り、応答の確定・preedit を
//! `ITfComposition` / `ITfEditSession` で反映する。何を確定・表示するかはエンジンが決める
//! （ロジックをフロントに持ち込まない。CLAUDE.md の分離の原則）。
//!
//! - [`render`]: 応答 → 編集計画（OS 非依存。Linux でもテストする）
//! - [`engine`]: エンジン接続のまとめ（OS 非依存。UI スレッドを塞がない起動・短いタイムアウト）
//! - Windows のみ: COM/TSF の実装（`dll` / `register` / `text_service` / `edit` / `keys`）。
//!   `unsafe` はこれらのモジュールに隔離し、`#![allow(unsafe_code)]` で明示している
//!
//! # 未実装（TODO）
//! - 候補ウィンドウの描画（`ITfUIElement` / 自前ウィンドウ）。`stubs::CandidateWindow` のスタブのみ
//! - 再変換（`ITfFnReconversion`）と応答の `reconvert_selection` の処理。`stubs` のスタブのみ
//! - 表示属性（`ITfDisplayAttributeProvider`。注目文節の太い下線など）。いまは装飾なしで描く
//! - 入力モード（`mode`）の言語バー・`GUID_COMPARTMENT_KEYBOARD_OPENCLOSE` への反映
//! - 非同期通知（`update_preedit` 等）の即時反映。いまは次のキー入力のときに反映する
//!   （隠しウィンドウへの `PostMessage` で UI スレッドを起こす実装が必要）
//! - 複数のホストプロセスでエンジンを共有する仕組み（現状はホストプロセスごとに imserver を起動する）
//! - `set_environment`（電源・メモリ）、`set_context`（周辺テキスト）、通知の表示
//!
//! # 検証状況
//! Linux ホストでの `cargo check/clippy --target x86_64-pc-windows-gnu`（型・シグネチャの確認）と、
//! OS 非依存部のテストのみ。**Windows 実機での動作（登録・キー入力・composition の描画）は未確認**。

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod engine;
pub mod render;

#[cfg(windows)]
mod dll;
#[cfg(windows)]
mod edit;
#[cfg(windows)]
mod guids;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod register;
#[cfg(windows)]
mod stubs;
#[cfg(windows)]
mod text_service;
