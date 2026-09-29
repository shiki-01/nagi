//! Nagi 開発用タスクランナー。`cargo xtask <task>` で実行する（`core/` で実行）。
//!
//! タスクは OS 非依存にするため、シェルスクリプトではなくこの crate に Rust で書く。

mod build_dict;
mod fetch_dict;
mod fetch_model;
mod types;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use build_dict::DictSource;
use fetch_model::ModelVariant;

#[derive(Parser)]
#[command(name = "xtask", about = "Nagi 開発用タスク（OS 非依存）")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Mozc OSS 辞書を取得し SHA-256 検証する（data/dict/mozc/ へ）
    FetchDict,
    /// 取得済みの辞書（Mozc / SudachiDict）から Nagi 辞書バイナリを生成する
    /// （data/dict/nagi-dict.bin または data/dict/nagi-dict-sudachi.bin）
    BuildDict {
        #[arg(long, value_enum, default_value_t = DictSource::Mozc)]
        source: DictSource,
    },
    /// 変換モデル（zenz GGUF）を取得し SHA-256 検証する（data/models/ へ）
    FetchModel {
        #[arg(long, value_enum, default_value_t = ModelVariant::Small)]
        variant: ModelVariant,
    },
    /// protocol の型から Swift の型を生成する（typeshare-cli が必要）
    GenTypes,
    /// gen-types を実行し、生成物に差分がないことを検査する
    CheckTypes,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.task {
        Task::FetchDict => fetch_dict::run(),
        Task::BuildDict { source } => build_dict::run(source),
        Task::FetchModel { variant } => fetch_model::run(variant),
        Task::GenTypes => types::generate(),
        Task::CheckTypes => types::check(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}
