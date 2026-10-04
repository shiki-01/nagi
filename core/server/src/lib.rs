//! Nagi サーバー: stdin/stdout の改行区切り JSON-RPC 2.0 ループと、変換パイプラインの組み立て。
//!
//! フロントエンドの子プロセスとして動き、キーイベントを受けて描画差分を返す。入力内容
//! （キーの文字・preedit・確定文字列）はログに一切出さない。
//!
//! - [`setup`] — 起動オプション・設定・辞書・モデル・学習・ユーザー辞書の読み込み
//! - [`pipeline`] — 学習 → ユーザー辞書 → LLM → システム辞書 の変換パイプライン
//! - [`runtime`] — 環境プロファイル・学習・ユーザー辞書・ランキング設定・通知の共有状態
//! - [`noise_eval`] — 曖昧入力の合成ノイズ評価（`nagi-eval --noise`）
//! - [`learning`] — 確定を学習に記録する observer
//! - [`server`] — JSON-RPC ループ
//! - [`settings`] — GUI 設定画面向けの設定 RPC（`get_settings` / `update_settings`）
//! - [`calibration`] — キャリブレーション推論の実測結果のファイルへの永続化
//! - [`update_check`] — アップデート確認（`config.toml` の `[update] auto_check`）

pub mod calibration;
pub mod learning;
pub mod noise_eval;
pub mod pipeline;
pub mod runtime;
pub mod server;
pub mod settings;
pub mod setup;
pub mod update_check;

pub use pipeline::Pipeline;
pub use runtime::{Effective, Event, Runtime, Tunables};
pub use server::Server;
pub use setup::{
    Built, DEFAULT_MODEL_FILE, ModelLoader, ServerOptions, build, build_with_loader, parse_args,
};
