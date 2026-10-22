//! zenz GGUF モデルの取得（Hugging Face、リビジョン固定・SHA-256 検証）。

use crate::Progress;
use crate::fsutil::{download_verified, sha256_file};
use crate::http::{Http, UreqHttp};
use std::fs;
use std::path::Path;

/// 取得できるモデルのバリアント。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    /// zenz-v3.1-small
    Small,
    /// zenz-v3.1-xsmall
    Xsmall,
}

/// Hugging Face から取得するモデルの情報。
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    /// 取得元 URL
    pub url: &'static str,
    /// 保存するファイル名（`<data_dir>/models/` 下）
    pub filename: &'static str,
    /// ファイルサイズ（バイト）
    pub size: u64,
    /// SHA-256 期待値（16 進）
    pub sha256: &'static str,
}

impl ModelKind {
    /// 全バリアント。
    pub const ALL: [ModelKind; 2] = [ModelKind::Small, ModelKind::Xsmall];

    /// モデルの情報。
    pub fn info(self) -> ModelInfo {
        match self {
            ModelKind::Small => ModelInfo {
                url: "https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf/resolve/ddf6e44b2e05ab7ea9a3e31559c5e7948761365c/ggml-model-Q5_K_M.gguf",
                filename: "zenz-v3.1-small-Q5_K_M.gguf",
                size: 73_871_968,
                sha256: "4de930c06bef8c263aa1aa40684af206db4ce1b96375b3b8ed0ea508e0b14f6c",
            },
            ModelKind::Xsmall => ModelInfo {
                url: "https://huggingface.co/Miwa-Keita/zenz-v3.1-xsmall-gguf/resolve/5c0c4db8c8cc66a6ef01520475bebd56b41f6236/ggml-model-Q5_K_M.gguf",
                filename: "zenz-v3.1-xsmall-Q5_K_M.gguf",
                size: 20_970_304,
                sha256: "189638370c43292fd54ba5e83854b24887ddd57e8914e19095514e663f60c7f5",
            },
        }
    }
}

/// モデルを `dest_dir` へ取得する。既に存在して SHA-256 が一致したらスキップ。
pub fn fetch_model(
    kind: ModelKind,
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    fetch_model_with(&UreqHttp, kind, dest_dir, progress)
}

/// [`fetch_model`] の HTTP 差し替え版。
pub fn fetch_model_with(
    http: &dyn Http,
    kind: ModelKind,
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    let info = kind.info();
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dest_dir.display(), e))?;
    let dest = dest_dir.join(info.filename);
    let file = info.filename.to_string();

    // 既に存在して SHA-256 が一致したらスキップ
    match sha256_file(&dest) {
        Some(hash) if hash == info.sha256 => {
            progress(Progress::Skipped {
                file,
                index: 1,
                count: 1,
            });
            return Ok(());
        }
        Some(_) => progress(Progress::Mismatched { file: file.clone() }),
        None => {}
    }

    progress(Progress::Download {
        file: file.clone(),
        index: 1,
        count: 1,
        url: info.url.to_string(),
        size: Some(info.size),
    });
    let (bytes, sha256) = download_verified(
        http,
        info.url,
        &dest,
        info.sha256,
        info.filename,
        &mut |done, total| {
            progress(Progress::Bytes {
                file: file.clone(),
                // Content-Length が無い場合は表のサイズを全体とみなす
                done,
                total: total.or(Some(info.size)),
            })
        },
    )?;
    progress(Progress::Verified {
        file,
        bytes,
        sha256,
    });
    progress(Progress::Saved { path: dest });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Body;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    struct FakeHttp(Vec<u8>);

    impl Http for FakeHttp {
        fn get(&self, _url: &str) -> Result<Body, String> {
            Ok(Body {
                reader: Box::new(Cursor::new(self.0.clone())),
                content_length: Some(self.0.len() as u64),
            })
        }
    }

    #[test]
    fn table_is_consistent() {
        for kind in ModelKind::ALL {
            let info = kind.info();
            assert_eq!(info.sha256.len(), 64);
            assert!(info.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(info.url.starts_with("https://"));
            assert!(info.filename.ends_with(".gguf"));
            assert!(info.size > 0);
        }
    }

    #[test]
    fn sha_mismatch_fails_without_saving() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-model-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut events = Vec::new();
        let err = fetch_model_with(
            &FakeHttp(b"not a model".to_vec()),
            ModelKind::Xsmall,
            &dir,
            &mut |p| events.push(p),
        )
        .unwrap_err();
        assert!(err.contains("SHA-256 が不一致"), "{err}");
        assert!(matches!(events[0], Progress::Download { .. }));
        assert!(matches!(events[1], Progress::Bytes { .. }));
        assert_eq!(events.len(), 2);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 表のハッシュを持つ本文を返せれば、検証・保存まで進み、再実行はスキップされる。
    /// 本物のモデルは使えないので、一時的にハッシュを差し替える代わりに download_verified を直接試す。
    #[test]
    fn download_verified_succeeds_and_orders_events() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-dl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let body = vec![7u8; 200_000];
        let expected = format!("{:x}", Sha256::digest(&body));
        let dest = dir.join("x.bin");
        let mut seen = Vec::new();
        let (bytes, sha) = crate::fsutil::download_verified(
            &FakeHttp(body.clone()),
            "https://example.invalid/x",
            &dest,
            &expected,
            "x.bin",
            &mut |done, total| seen.push((done, total)),
        )
        .unwrap();
        assert_eq!(bytes, 200_000);
        assert_eq!(sha, expected);
        assert_eq!(fs::read(&dest).unwrap(), body);
        // バイト進捗は単調増加で、最後が全体
        assert!(seen.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(seen.last().unwrap(), &(200_000, Some(200_000)));
        assert!(!dest.with_extension("tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
