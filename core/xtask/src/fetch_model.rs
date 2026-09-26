//! `cargo xtask fetch-model` — zenz-v3.1-small GGUF モデルを取得し SHA-256 検証。

use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// Hugging Face から取得するモデルの情報
const MODEL_URL: &str = "https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf/resolve/ddf6e44b2e05ab7ea9a3e31559c5e7948761365c/ggml-model-Q5_K_M.gguf";
const MODEL_FILENAME: &str = "zenz-v3.1-small-Q5_K_M.gguf";
const MODEL_SIZE: u64 = 73_871_968; // バイト
const MODEL_SHA256: &str = "4de930c06bef8c263aa1aa40684af206db4ce1b96375b3b8ed0ea508e0b14f6c";

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

/// ファイルの SHA-256 ハッシュ値を計算して 16 進数文字列で返す。
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// `bytes` を `path` へ一時ファイル経由で安全に書き込む。
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temp = path.to_path_buf();
    temp.set_extension("tmp");

    let mut file = fs::File::create(&temp)
        .map_err(|e| format!("一時ファイル作成失敗 {}: {}", temp.display(), e))?;

    file.write_all(bytes)
        .map_err(|e| format!("ファイル書き込み失敗: {e}"))?;

    drop(file);

    fs::rename(&temp, path).map_err(|e| {
        format!(
            "ファイルリネーム失敗 {} -> {}: {}",
            temp.display(),
            path.display(),
            e
        )
    })?;

    Ok(())
}

/// モデルをダウンロード・検証する。既に存在して SHA-256 が一致したらスキップ。
fn fetch_model(output_dir: &Path) -> Result<(), String> {
    let dest_path = output_dir.join(MODEL_FILENAME);

    // 既に存在して SHA-256 が一致したらスキップ
    if let Ok(data) = fs::read(&dest_path) {
        let hash = sha256_hex(&data);
        if hash == MODEL_SHA256 {
            eprintln!("スキップ: {} (SHA-256 一致)", MODEL_FILENAME);
            return Ok(());
        } else {
            eprintln!(
                "警告: {} は存在しますが SHA-256 が不一致です。再ダウンロードします",
                MODEL_FILENAME
            );
        }
    }

    eprintln!("ダウンロード中: {} ...", MODEL_FILENAME);
    eprintln!("  URL: {}", MODEL_URL);
    eprintln!("  サイズ: {} バイト", MODEL_SIZE);

    let response = ureq::get(MODEL_URL)
        .call()
        .map_err(|e| format!("HTTP リクエスト失敗: {}", e))?;

    if response.status() != 200 {
        return Err(format!(
            "{} ダウンロード失敗 (HTTP {})",
            MODEL_FILENAME,
            response.status()
        ));
    }

    let mut buf_reader = BufReader::new(response.into_reader());
    let mut bytes = Vec::new();
    buf_reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("レスポンス読み込み失敗: {e}"))?;

    let actual_hash = sha256_hex(&bytes);
    eprintln!("  取得: {} バイト, SHA-256: {}", bytes.len(), actual_hash);

    if actual_hash != MODEL_SHA256 {
        return Err(format!(
            "{} の SHA-256 が不一致\n  期待: {}\n  実際: {}",
            MODEL_FILENAME, MODEL_SHA256, actual_hash
        ));
    }

    atomic_write(&dest_path, &bytes)?;
    eprintln!("保存完了: {}", dest_path.display());
    Ok(())
}

/// タスク実行。
pub fn run(_args: &[String]) -> Result<(), String> {
    let out_dir = model_dir()?;
    eprintln!("出力先: {}", out_dir.display());
    eprintln!("モデル: {}", MODEL_FILENAME);
    eprintln!();

    fetch_model(&out_dir)?;

    eprintln!("\n完了。モデルは {} に保存されました", out_dir.display());
    Ok(())
}
