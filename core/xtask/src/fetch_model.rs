//! `cargo xtask fetch-model` — zenz GGUF モデルを取得し SHA-256 検証。

use nagi_datafetch::{ModelKind, Progress};
use std::fs;
use std::path::PathBuf;

/// `fetch-model` で選択できるモデルのバリアント。
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ModelVariant {
    Small,
    Xsmall,
}

impl ModelVariant {
    fn kind(self) -> ModelKind {
        match self {
            ModelVariant::Small => ModelKind::Small,
            ModelVariant::Xsmall => ModelKind::Xsmall,
        }
    }
}

/// リポジトリルートを求める。manifest dir の 3 階層上（core/ の上）。
fn find_repo_root() -> Result<PathBuf, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|e| format!("CARGO_MANIFEST_DIR が未設定: {e}"))?;
    let path = PathBuf::from(manifest_dir)
        .parent()
        .ok_or("Cargo.toml の親ディレクトリが取得できない")?
        .parent()
        .ok_or("core/ の親ディレクトリが取得できない")?
        .to_path_buf();
    Ok(path)
}

/// モデルの出力先ディレクトリを取得・作成する。
fn model_dir() -> Result<PathBuf, String> {
    let repo_root = find_repo_root()?;
    let dir = repo_root.join("data").join("models");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dir.display(), e))?;
    Ok(dir)
}

/// 進捗を stderr へ出す。
fn report(p: Progress) {
    match p {
        Progress::Skipped { file, .. } => eprintln!("スキップ: {file} (SHA-256 一致)"),
        Progress::Mismatched { file } => {
            eprintln!("警告: {file} は存在しますが SHA-256 が不一致です。再ダウンロードします")
        }
        Progress::Download {
            file, url, size, ..
        } => {
            eprintln!("ダウンロード中: {file} ...");
            eprintln!("  URL: {url}");
            if let Some(size) = size {
                eprintln!("  サイズ: {size} バイト");
            }
        }
        Progress::Verified { bytes, sha256, .. } => {
            eprintln!("  取得: {bytes} バイト, SHA-256: {sha256}")
        }
        Progress::Saved { path } => eprintln!("保存完了: {}", path.display()),
        _ => {}
    }
}

/// タスク実行。
pub fn run(variant: ModelVariant) -> Result<(), String> {
    let kind = variant.kind();
    let out_dir = model_dir()?;
    eprintln!("出力先: {}", out_dir.display());
    eprintln!("モデル: {}", kind.info().filename);
    eprintln!();

    nagi_datafetch::fetch_model(kind, &out_dir, &mut report)?;

    eprintln!("\n完了。モデルは {} に保存されました", out_dir.display());
    Ok(())
}
