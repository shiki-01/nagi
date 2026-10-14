//! `cargo xtask fetch-model` — zenz GGUF モデルを取得し SHA-256 検証。

use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// `fetch-model` で選択できるモデルのバリアント。
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ModelVariant {
    Small,
    Xsmall,
}

/// Hugging Face から取得するモデルの情報。
struct ModelInfo {
    url: &'static str,
    filename: &'static str,
    size: u64,
    /// SHA-256 期待値。`None` は「配布元から未取得のため未検証」を意味し、
    /// `fetch_model` はこの場合エラーを返して処理を止める。
    sha256: Option<&'static str>,
}

impl ModelVariant {
    fn info(self) -> ModelInfo {
        match self {
            ModelVariant::Small => ModelInfo {
                url: "https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf/resolve/ddf6e44b2e05ab7ea9a3e31559c5e7948761365c/ggml-model-Q5_K_M.gguf",
                filename: "zenz-v3.1-small-Q5_K_M.gguf",
                size: 73_871_968,
                sha256: Some("4de930c06bef8c263aa1aa40684af206db4ce1b96375b3b8ed0ea508e0b14f6c"),
            },
            ModelVariant::Xsmall => ModelInfo {
                url: "https://huggingface.co/Miwa-Keita/zenz-v3.1-xsmall-gguf/resolve/5c0c4db8c8cc66a6ef01520475bebd56b41f6236/ggml-model-Q5_K_M.gguf",
                filename: "zenz-v3.1-xsmall-Q5_K_M.gguf",
                size: 20_970_304,
                sha256: Some("189638370c43292fd54ba5e83854b24887ddd57e8914e19095514e663f60c7f5"),
            },
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
fn fetch_model(info: &ModelInfo, output_dir: &Path) -> Result<(), String> {
    let Some(expected_sha256) = info.sha256 else {
        return Err(format!(
            "{} の SHA-256 が未確定のため取得できません。ネットワーク到達性確認後に SHA-256 を計算し fetch_model.rs に反映してください",
            info.filename
        ));
    };

    let dest_path = output_dir.join(info.filename);

    // 既に存在して SHA-256 が一致したらスキップ
    if let Ok(data) = fs::read(&dest_path) {
        let hash = sha256_hex(&data);
        if hash == expected_sha256 {
            eprintln!("スキップ: {} (SHA-256 一致)", info.filename);
            return Ok(());
        } else {
            eprintln!(
                "警告: {} は存在しますが SHA-256 が不一致です。再ダウンロードします",
                info.filename
            );
        }
    }

    eprintln!("ダウンロード中: {} ...", info.filename);
    eprintln!("  URL: {}", info.url);
    eprintln!("  サイズ: {} バイト", info.size);

    let response = crate::http::get(info.url)?;

    if response.status() != 200 {
        return Err(format!(
            "{} ダウンロード失敗 (HTTP {})",
            info.filename,
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

    if actual_hash != expected_sha256 {
        return Err(format!(
            "{} の SHA-256 が不一致\n  期待: {}\n  実際: {}",
            info.filename, expected_sha256, actual_hash
        ));
    }

    atomic_write(&dest_path, &bytes)?;
    eprintln!("保存完了: {}", dest_path.display());
    Ok(())
}

/// タスク実行。
pub fn run(variant: ModelVariant) -> Result<(), String> {
    let info = variant.info();
    let out_dir = model_dir()?;
    eprintln!("出力先: {}", out_dir.display());
    eprintln!("モデル: {}", info.filename);
    eprintln!();

    fetch_model(&info, &out_dir)?;

    eprintln!("\n完了。モデルは {} に保存されました", out_dir.display());
    Ok(())
}
