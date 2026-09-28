//! `cargo xtask gen-types` / `check-types`（typeshare で Swift の型を生成）。

use std::path::{Path, PathBuf};
use std::process::Command;

/// リポジトリルートを取得する。
fn repo_root() -> PathBuf {
    // xtask は core/ の下にあるので、2 階層上に上がるとリポジトリルート
    // CARGO_MANIFEST_DIR は xtask の Cargo.toml のあるディレクトリ
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Swift の型を生成する。
///
/// typeshare を実行して protocol crate から Swift の型を生成し、
/// `frontend-macos/Sources/Generated/Protocol.swift` に出力します。
pub fn generate() -> Result<(), String> {
    let repo = repo_root();
    let protocol_src = repo.join("core/protocol");
    let output_file = repo.join("frontend-macos/Sources/Generated/Protocol.swift");

    // 出力ディレクトリを確認・作成
    let output_dir = output_file
        .parent()
        .ok_or("出力ディレクトリの決定に失敗しました")?;
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("出力ディレクトリの作成に失敗しました: {}", e))?;

    // typeshare コマンドを実行
    let mut cmd = Command::new("typeshare");
    cmd.arg(protocol_src.to_string_lossy().to_string())
        .arg("--lang=swift")
        .arg(format!("--output-file={}", output_file.display()));

    let status = cmd.status().map_err(|e| {
        format!(
            "typeshare の実行に失敗しました（PATH に typeshare がありますか？）: {}",
            e
        )
    })?;

    if !status.success() {
        return Err(format!(
            "typeshare の実行に失敗しました（終了コード: {}）",
            status.code().unwrap_or(-1)
        ));
    }

    println!("✓ Swift の型を生成しました: {}", output_file.display());
    Ok(())
}

/// 生成物に差分がないことを検査する。
///
/// 1. generate を実行して Swift の型を生成
/// 2. git diff で差分がないことを確認（未追跡ファイルも含む）
pub fn check() -> Result<(), String> {
    // まず generate を実行
    generate()?;

    let repo = repo_root();
    let output_file = repo.join("frontend-macos/Sources/Generated/Protocol.swift");
    let output_dir = output_file
        .parent()
        .ok_or("出力ディレクトリの決定に失敗しました")?;

    // git diff を実行して、生成物に差分がないか確認
    let mut diff_cmd = Command::new("git");
    diff_cmd
        .arg("diff")
        .arg("--exit-code")
        .arg("--")
        .arg(output_dir);

    let diff_status = diff_cmd
        .status()
        .map_err(|e| format!("git diff の実行に失敗しました: {}", e))?;

    if !diff_status.success() {
        return Err(
            "生成物に差分があります。`cargo xtask gen-types` を実行して、\
             差分を確認してください。"
                .to_string(),
        );
    }

    // git status で未追跡ファイルを確認
    let mut status_cmd = Command::new("git");
    status_cmd
        .arg("status")
        .arg("--porcelain")
        .arg("--")
        .arg(output_dir);

    let status_output = status_cmd
        .output()
        .map_err(|e| format!("git status の実行に失敗しました: {}", e))?;

    if !status_output.stdout.is_empty() {
        return Err("生成物のディレクトリに未追跡ファイルがあります。".to_string());
    }

    println!("✓ 生成物に差分はありません");
    Ok(())
}
