//! Nagi 開発用タスクランナー。`cargo xtask <task>` で実行する（`core/` で実行）。
//!
//! タスクは OS 非依存にするため、シェルスクリプトではなくこの crate に Rust で書く。

mod build_dict;
mod fetch_dict;
mod fetch_model;
mod types;

use std::process::ExitCode;

const USAGE: &str = "\
使い方: cargo xtask <task> [args...]

タスク:
  fetch-dict     Mozc OSS 辞書を取得し SHA-256 検証する（data/dict/mozc/ へ）
  build-dict     取得済みの Mozc 辞書から Nagi 辞書バイナリを生成する（data/dict/nagi-dict.bin）
  fetch-model    変換モデル（zenz GGUF）を取得し SHA-256 検証する（data/models/ へ）
  gen-types      protocol の型から Swift の型を生成する（typeshare-cli が必要）
  check-types    gen-types を実行し、生成物に差分がないことを検査する
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(task) = args.next() else {
        eprint!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let rest: Vec<String> = args.collect();
    let result = match task.as_str() {
        "fetch-dict" => fetch_dict::run(&rest),
        "build-dict" => build_dict::run(&rest),
        "fetch-model" => fetch_model::run(&rest),
        "gen-types" => types::generate(&rest),
        "check-types" => types::check(&rest),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("不明なタスク: {other}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}
