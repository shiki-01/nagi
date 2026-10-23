//! `imserver`: Nagi コアエンジンのサーバープロセス（stdin/stdout の JSON-RPC）。
//!
//! 使い方: `imserver [--data-dir <dir>] [--user-dir <dir>] [--config <file>] [--no-llm]
//! [--sync-budget-ms <ms>]`（`NAGI_DATA_DIR` でも指定可）。ログは stderr のみ。入力内容は出力しない。
//!
//! データ取得: `imserver --install-data --data-dir <dir> [--model small|xsmall|none]`。辞書とモデルを
//! 取得して終了する（JSON-RPC は起動しない）。進捗は標準出力に 1 行 1 JSON。

use nagi_server::install_data::{self, parse_install_args};
use nagi_server::{Server, build, parse_args};
use std::io::{BufReader, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    // データ取得モード: 処理して終了する（JSON-RPC サーバーとしては起動しない）
    match parse_install_args(std::env::args().skip(1)) {
        Ok(Some(args)) => {
            return match install_data::run(&args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    nagi_server::elog!("imserver: {message}");
                    ExitCode::FAILURE
                }
            };
        }
        Ok(None) => {}
        Err(message) => {
            nagi_server::elog!("imserver: {message}");
            return ExitCode::from(2);
        }
    }
    let mut options = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(message) => {
            nagi_server::elog!("imserver: {message}");
            return ExitCode::from(2);
        }
    };
    // 起動（initialize への応答）をモデル読み込みで待たせない。読み込みが済むまでは辞書のみで応答する
    options.async_model_load = true;
    let built = build(&options);
    for note in &built.notes {
        nagi_server::elog!("imserver: {note}");
    }
    nagi_server::elog!(
        "imserver: プロファイル {}（{}）",
        built.runtime.profile().as_str(),
        built.runtime.profile_reason()
    );
    let mut server = Server::from_built(built);
    // `Server::run` は標準入力を読む専用スレッドに `input` を move するため、`Send + 'static` が要る。
    // `StdinLock` は（`MutexGuard` を内包するため）`Send` ではないので、ロックせずに `Stdin` を渡す
    // （`Stdin::read` は呼び出しごとに内部で lock/unlock するので、単一スレッドから使う限り挙動は変わらない）。
    let stdout = std::io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    match server.run(BufReader::new(std::io::stdin()), &mut output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            nagi_server::elog!("imserver: 入出力エラー: {e}");
            ExitCode::FAILURE
        }
    }
}
