//! Nagi サーバー: stdin/stdout の改行区切り JSON-RPC 2.0 ループ。
//!
//! フロントエンドの子プロセスとして動き、キーイベントを受けて描画差分を返す。入力内容
//! （キーの文字・preedit・確定文字列）はログに一切出さない。

pub mod backend;
pub mod server;

pub use backend::DictBackend;
pub use server::{Server, ServerOptions, load_backend};
