//! `cargo xtask fetch-loanword` — カタカナ語の原語データ（JMdict）を取得し、
//! fst 索引（`data/loanword/loanword.fst`）を組み立てる。
//!
//! 固定した SHA-256 があれば `imserver --install-data` と同じ検証つきの取得。未固定の間は
//! 検証せずに取得して SHA-256 を表示する（固定する値を決めるための下見）。

use nagi_datafetch::{JMDICT_FILE, LOANWORD_INDEX_FILE, Progress};
use std::fs;
use std::path::PathBuf;

fn output_dir() -> Result<PathBuf, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|e| format!("CARGO_MANIFEST_DIR が未設定: {e}"))?;
    let dir = PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .ok_or("リポジトリルートが特定できない")?
        .join("data")
        .join("loanword");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dir.display(), e))?;
    Ok(dir)
}

fn report(p: Progress) {
    match p {
        Progress::Skipped { file, .. } => eprintln!("スキップ: {file} (SHA-256 一致)"),
        Progress::Download { file, url, .. } => eprintln!("ダウンロード中: {file} ({url}) ..."),
        Progress::Verified { bytes, sha256, .. } => {
            eprintln!("  取得: {bytes} bytes, SHA-256: {sha256}")
        }
        Progress::BuildDone { bytes, path, .. } => {
            eprintln!("  索引を書き出しました: {} ({bytes} bytes)", path.display())
        }
        _ => {}
    }
}

/// タスク実行。
pub fn run(from: Option<PathBuf>) -> Result<(), String> {
    let dir = output_dir()?;
    eprintln!("出力先: {}", dir.display());
    if let Some(archive) = from {
        let words = nagi_datafetch::build_loanword_index(
            &archive,
            &dir.join(LOANWORD_INDEX_FILE),
            &mut report,
        )?;
        eprintln!("\n完了。{words} 語（{} から）", archive.display());
        return Ok(());
    }
    match nagi_datafetch::install_loanword(&dir, &mut report) {
        Ok(_) => {}
        Err(e) if e.contains("未固定") => {
            eprintln!("警告: SHA-256 が未固定のため、検証せずに取得します");
            let sha = nagi_datafetch::fetch_loanword_unpinned(&dir, &mut report)?;
            eprintln!("  この取得の SHA-256: {sha}");
            eprintln!(
                "  → core/datafetch/src/loanword.rs の JMDICT_SHA256 を Some(\"{sha}\") にして固定してください"
            );
            nagi_datafetch::build_loanword_index(
                &dir.join(JMDICT_FILE),
                &dir.join(LOANWORD_INDEX_FILE),
                &mut report,
            )?;
        }
        Err(e) => return Err(e),
    }
    eprintln!("\n完了");
    Ok(())
}
