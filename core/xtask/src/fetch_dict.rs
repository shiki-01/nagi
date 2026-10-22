//! `cargo xtask fetch-dict` — Mozc OSS 辞書を取得して SHA-256 検証。

use nagi_datafetch::{MOZC_COMMIT, Progress};
use std::fs;
use std::path::PathBuf;

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

/// 出力先ディレクトリを取得・作成する。
fn output_dir() -> Result<PathBuf, String> {
    let repo_root = find_repo_root()?;
    let dir = repo_root.join("data").join("dict").join("mozc");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dir.display(), e))?;
    Ok(dir)
}

/// 進捗を stderr へ出す。
fn report(p: Progress) {
    match p {
        Progress::Skipped { file, .. } => eprintln!("スキップ: {file} (SHA-256 一致)"),
        Progress::Download { file, .. } => eprintln!("ダウンロード中: {file} ..."),
        Progress::Verified { bytes, sha256, .. } => {
            eprintln!("  取得: {bytes} bytes, SHA-256: {sha256}")
        }
        _ => {}
    }
}

/// タスク実行。
pub fn run() -> Result<(), String> {
    let out_dir = output_dir()?;
    eprintln!("出力先: {}", out_dir.display());
    eprintln!("Mozc コミット: {}", MOZC_COMMIT);
    eprintln!();

    nagi_datafetch::fetch_mozc(&out_dir, &mut report)?;

    eprintln!(
        "\n完了。辞書ファイルは {} に保存されました",
        out_dir.display()
    );
    Ok(())
}
