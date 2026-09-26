//! `imserver`: Nagi コアエンジンのサーバープロセス（stdin/stdout の JSON-RPC）。
//!
//! 使い方: `imserver [--data-dir <dir>]`（環境変数 `NAGI_DATA_DIR` でも指定可）。
//! ログは stderr のみ。入力内容は出力しない。

use nagi_engine::EngineConfig;
use nagi_server::server::parse_args;
use nagi_server::{Server, ServerOptions, load_backend};
use std::io::{BufReader, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    let options: ServerOptions = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("imserver: {message}");
            return ExitCode::from(2);
        }
    };
    let (backend, profile, note) = load_backend(&options);
    eprintln!("imserver: {note}（バックエンド: {}）", backend.name());
    let mut server = Server::new(backend, profile, EngineConfig::default());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    match server.run(BufReader::new(stdin.lock()), &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("imserver: 入出力エラー: {e}");
            ExitCode::FAILURE
        }
    }
}
