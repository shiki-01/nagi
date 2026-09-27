//! `imserver`: Nagi コアエンジンのサーバープロセス（stdin/stdout の JSON-RPC）。
//!
//! 使い方: `imserver [--data-dir <dir>] [--user-dir <dir>] [--config <file>] [--no-llm]
//! [--sync-budget-ms <ms>]`（`NAGI_DATA_DIR` でも指定可）。ログは stderr のみ。入力内容は出力しない。

use nagi_server::{Server, build, parse_args};
use std::io::{BufReader, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    let options = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("imserver: {message}");
            return ExitCode::from(2);
        }
    };
    let built = build(&options);
    for note in &built.notes {
        eprintln!("imserver: {note}");
    }
    eprintln!(
        "imserver: プロファイル {}（{}）",
        built.runtime.profile().as_str(),
        built.runtime.profile_reason()
    );
    let mut server = Server::from_built(built);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    match server.run_threaded(BufReader::new(stdin), &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("imserver: 入出力エラー: {e}");
            ExitCode::FAILURE
        }
    }
}
