//! `cargo xtask fetch-dict` — Mozc OSS 辞書を取得して SHA-256 検証。

use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

const MOZC_COMMIT: &str = "b9c3fcbd6d76b19649ef572324fa9da2559bc18e";
const MOZC_BASE_URL: &str = "https://raw.githubusercontent.com/google/mozc";

/// ダウンロードするファイルの定義（URL サフィックスと期待 SHA-256）。
const FILES: &[(&str, Option<&str>)] = &[
    (
        "src/data/dictionary_oss/dictionary00.txt",
        Some("9e07ce292932c4fff327ed8cb06274c09acca556bd8bdf7c7ef1f6173eae3b61"),
    ),
    (
        "src/data/dictionary_oss/dictionary01.txt",
        Some("836fd5fd8399510de0d8ef7b33be913cca6bbaee7b712cc8045fba03f4c75a89"),
    ),
    (
        "src/data/dictionary_oss/dictionary02.txt",
        Some("93943858d0cfd092b849a78def6901eaf228177e1ac5663544e865a3a1e45a9e"),
    ),
    (
        "src/data/dictionary_oss/dictionary03.txt",
        Some("cb9f1b0b4a7550d682d86244a021cd17e0b15366328819e69135b5f183a1a24b"),
    ),
    (
        "src/data/dictionary_oss/dictionary04.txt",
        Some("9fdc4e3948d03463de13ab5782adffbc11dd06e934cb80138c5920029cd3d2f6"),
    ),
    (
        "src/data/dictionary_oss/dictionary05.txt",
        Some("199e3b0f050b691823ae7ba943090d437f51af6436b27c5f3dc69cafa12000e4"),
    ),
    (
        "src/data/dictionary_oss/dictionary06.txt",
        Some("b95b14f7770ae2a824bc8e9fad8b03c1bc45c504f93a36554b8b56daa0368d2a"),
    ),
    (
        "src/data/dictionary_oss/dictionary07.txt",
        Some("7f53649b529d4c8c495d3a8dd9c4d489dfe7696080f7423107ff01331174be48"),
    ),
    (
        "src/data/dictionary_oss/dictionary08.txt",
        Some("50a31bb5fc1646721b00be371a5695dffab1292bda143e1e59cc70cbbdd8643f"),
    ),
    (
        "src/data/dictionary_oss/dictionary09.txt",
        Some("4e5fcd4cbfbf11395166a37b506f547fc54d20edc0884f5184ecd3fc705f0d9a"),
    ),
    (
        "src/data/dictionary_oss/connection_single_column.txt",
        Some("68823a1f6617372ccb89afdbcb334d7a0bcfa82c9c5dda03fc1f80f1cab5c48e"),
    ),
    (
        "src/data/dictionary_oss/id.def",
        Some("07a05a268c3783b7e02b36e4aa591555a2dd19a1f3c93891576740761c154f2d"),
    ),
    (
        "src/data/dictionary_oss/evaluation.tsv",
        Some("445ed21e6c83d55774f15a83a5f6ea72c945ef0f30ec7ed5fe4bc6bc7bee9836"),
    ),
    (
        "src/data/dictionary_oss/README.txt",
        Some("6b1de66bc6fa30e0b45dc45b8c8f6c57bd78aff923c261a3efffac8eb86f7bac"),
    ),
];

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

/// 単一ファイルをダウンロード・検証する。存在して SHA-256 が一致したらスキップ。
fn fetch_file(suffix: &str, expected_hash: Option<&str>, output_dir: &Path) -> Result<(), String> {
    let filename = suffix.rsplit('/').next().unwrap_or(suffix);
    let dest_path = output_dir.join(filename);

    // 既に存在して SHA-256 が一致したらスキップ
    if let Ok(data) = fs::read(&dest_path) {
        let hash = sha256_hex(&data);
        if let Some(expected) = expected_hash
            && hash == expected
        {
            eprintln!("スキップ: {} (SHA-256 一致)", filename);
            return Ok(());
        }
    }

    let url = format!("{}/{}/{}", MOZC_BASE_URL, MOZC_COMMIT, suffix);
    eprintln!("ダウンロード中: {} ...", filename);

    let response = crate::http::get(&url)?;

    if response.status() != 200 {
        return Err(format!(
            "{} ダウンロード失敗 (HTTP {})",
            filename,
            response.status()
        ));
    }

    let mut buf_reader = BufReader::new(response.into_reader());
    let mut bytes = Vec::new();
    buf_reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("レスポンス読み込み失敗: {e}"))?;

    let actual_hash = sha256_hex(&bytes);
    eprintln!("  取得: {} bytes, SHA-256: {}", bytes.len(), actual_hash);

    if let Some(expected) = expected_hash {
        if actual_hash != expected {
            return Err(format!(
                "{} の SHA-256 が不一致\n  期待: {}\n  実際: {}",
                filename, expected, actual_hash
            ));
        }
    } else {
        eprintln!(
            "警告: {} の SHA-256 がコードに未記載。上記の値をコードに追加してください",
            filename
        );
    }

    atomic_write(&dest_path, &bytes)?;
    Ok(())
}

/// タスク実行。
pub fn run() -> Result<(), String> {
    let out_dir = output_dir()?;
    eprintln!("出力先: {}", out_dir.display());
    eprintln!("Mozc コミット: {}", MOZC_COMMIT);
    eprintln!();

    for (suffix, expected_hash) in FILES {
        fetch_file(suffix, *expected_hash, &out_dir)?;
    }

    eprintln!(
        "\n完了。辞書ファイルは {} に保存されました",
        out_dir.display()
    );
    Ok(())
}
