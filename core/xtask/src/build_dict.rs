//! `cargo xtask build-dict` — Mozc 辞書から Nagi バイナリ辞書を生成。

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// リポジトリルートを求める。
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

/// Mozc 辞書のディレクトリを取得。
fn mozc_dict_dir() -> Result<PathBuf, String> {
    let repo_root = find_repo_root()?;
    let dir = repo_root.join("data").join("dict").join("mozc");
    if !dir.exists() {
        return Err(format!(
            "Mozc 辞書ディレクトリが見つかりません: {}。\n\
            先に `cargo xtask fetch-dict` を実行してください。",
            dir.display()
        ));
    }
    Ok(dir)
}

/// 出力ファイルのパスを取得。
fn output_path() -> Result<PathBuf, String> {
    let repo_root = find_repo_root()?;
    Ok(repo_root.join("data").join("dict").join("nagi-dict.bin"))
}

/// タスク実行。
pub fn run(_args: &[String]) -> Result<(), String> {
    let mozc_dir = mozc_dict_dir()?;
    let output_file = output_path()?;

    eprintln!("Mozc 辞書ディレクトリ: {}", mozc_dir.display());
    eprintln!("出力ファイル: {}", output_file.display());
    eprintln!();

    eprintln!("Nagi バイナリ辞書を構築中...");
    let start = Instant::now();

    // Mozc の連接コストは「前の語の右 ID × 次の語の左 ID」の順（RidMajor。実データの変換結果で検証済み）
    let dictionary =
        nagi_dict::build_from_mozc_dir(&mozc_dir, nagi_dict::ConnectionLayout::RidMajor)
            .map_err(|e| format!("辞書ビルド失敗: {e}"))?;
    let binary_data = dictionary.to_bytes();

    let build_time = start.elapsed();
    let binary_size = binary_data.len();

    // バイナリを書き込む
    fs::create_dir_all(
        output_file
            .parent()
            .ok_or("出力ディレクトリが特定できない")?,
    )
    .map_err(|e| format!("ディレクトリ作成失敗: {}", e))?;
    fs::write(&output_file, binary_data).map_err(|e| format!("ファイル書き込み失敗: {}", e))?;

    eprintln!();
    eprintln!("完了");
    eprintln!("  構築時間: {:.2}s", build_time.as_secs_f64());
    eprintln!(
        "  バイナリサイズ: {} bytes ({:.2} MB)",
        binary_size,
        binary_size as f64 / 1_000_000.0
    );
    eprintln!("  出力先: {}", output_file.display());

    Ok(())
}
