//! `cargo xtask fetch-english` — 英単語の語彙（SCOWL）を取得して SHA-256 検証し、
//! fst 索引（`data/english/words.fst`）を組み立てる。

use nagi_datafetch::{Progress, SCOWL_FILE};
use std::fs;
use std::path::PathBuf;

/// データディレクトリ（リポジトリルートの `data/english`）。
fn output_dir() -> Result<PathBuf, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|e| format!("CARGO_MANIFEST_DIR が未設定: {e}"))?;
    let dir = PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .ok_or("リポジトリルートが特定できない")?
        .join("data")
        .join("english");
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
            eprintln!("  語彙を書き出しました: {} ({bytes} bytes)", path.display())
        }
        _ => {}
    }
}

/// タスク実行。
pub fn run() -> Result<(), String> {
    let dir = output_dir()?;
    eprintln!("出力先: {}", dir.display());
    nagi_datafetch::fetch_english(&dir, &mut report)?;
    let words = nagi_datafetch::build_english_lexicon(
        &dir.join(SCOWL_FILE),
        &dir.join("words.fst"),
        &mut report,
    )?;
    eprintln!("\n完了。{words} 語");
    Ok(())
}
