//! Nagi サーバー: stdin/stdout の改行区切り JSON-RPC 2.0 ループと、変換パイプラインの組み立て。
//!
//! フロントエンドの子プロセスとして動き、キーイベントを受けて描画差分を返す。入力内容
//! （キーの文字・preedit・確定文字列）はログに一切出さない。
//!
//! - [`setup`] — 起動オプション・設定・辞書・モデル・学習・ユーザー辞書の読み込み
//! - [`pipeline`] — 学習 → ユーザー辞書 → LLM → システム辞書 の変換パイプライン
//! - [`runtime`] — 環境プロファイル・学習・ユーザー辞書・ランキング設定・通知の共有状態
//! - [`learning`] — 確定を学習に記録する observer
//! - [`server`] — JSON-RPC ループ
//! - [`calibration`] — キャリブレーション推論の実測結果のファイルへの永続化

pub mod calibration;
pub mod learning;
pub mod pipeline;
pub mod runtime;
pub mod server;
pub mod setup;

pub use pipeline::Pipeline;
pub use runtime::{Effective, Event, Runtime, Tunables};
pub use server::Server;
pub use setup::{Built, DEFAULT_MODEL_FILE, ServerOptions, build, parse_args};
