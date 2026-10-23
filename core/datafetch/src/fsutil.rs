//! SHA-256 計算・一時ファイル経由の書き込み。

use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

/// ファイルの SHA-256（16 進）。読めなければ `None`。
pub(crate) fn sha256_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// 一時ファイルの `path`（拡張子を `tmp` にしたもの）。
pub(crate) fn temp_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("tmp")
}

/// 一時ファイルを `path` へ rename する。失敗したら一時ファイルを消す。
pub(crate) fn commit(temp: &Path, path: &Path) -> Result<(), String> {
    fs::rename(temp, path).map_err(|e| {
        let _ = fs::remove_file(temp);
        format!(
            "ファイルリネーム失敗 {} -> {}: {}",
            temp.display(),
            path.display(),
            e
        )
    })
}

/// `url` をストリームで一時ファイルへ書き、SHA-256 を検証してから `dest` へ rename する。
/// 受信のたびに `on_bytes(受信済み, 全体)` を呼ぶ。成功時は (受信バイト数, SHA-256)。
pub(crate) fn download_verified(
    http: &dyn crate::http::Http,
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    name: &str,
    on_bytes: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(u64, String), String> {
    use std::io::Write;

    let mut body = http.get(url)?;
    let temp = temp_path(dest);
    let result = (|| {
        let mut file = fs::File::create(&temp)
            .map_err(|e| format!("一時ファイル作成失敗 {}: {}", temp.display(), e))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = body
                .reader
                .read(&mut buf)
                .map_err(|e| format!("レスポンス読み込み失敗: {e}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| format!("ファイル書き込み失敗: {e}"))?;
            done += n as u64;
            on_bytes(done, body.content_length);
        }
        file.flush()
            .map_err(|e| format!("ファイル書き込み失敗: {e}"))?;
        Ok((done, format!("{:x}", hasher.finalize())))
    })();
    let (bytes, actual) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
    };
    // 空の期待値は検証しない（SHA-256 を固定する前の下見用。`fetch_loanword_unpinned`）
    if !expected_sha256.is_empty() && actual != expected_sha256 {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "{name} の SHA-256 が不一致\n  期待: {expected_sha256}\n  実際: {actual}"
        ));
    }
    commit(&temp, dest)?;
    Ok((bytes, actual))
}

/// 固定（ピン留め）した取得元。`file` は保存先ディレクトリ内のファイル名
pub(crate) struct Source<'a> {
    pub file: &'a str,
    pub url: &'a str,
    pub sha256: &'a str,
}

/// `src` を `dest_dir` へ取得する。SHA-256 が一致する既存ファイルはスキップする。
/// 実際にダウンロードしたとき `true`、スキップしたとき `false`
pub(crate) fn fetch_pinned(
    http: &dyn crate::http::Http,
    dest_dir: &Path,
    src: &Source<'_>,
    progress: &mut dyn FnMut(crate::Progress),
) -> Result<bool, String> {
    use crate::Progress;

    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dest_dir.display(), e))?;
    let dest = dest_dir.join(src.file);
    let file = src.file.to_string();
    if sha256_file(&dest).as_deref() == Some(src.sha256) {
        progress(Progress::Skipped {
            file,
            index: 1,
            count: 1,
        });
        return Ok(false);
    }
    progress(Progress::Download {
        file: file.clone(),
        index: 1,
        count: 1,
        url: src.url.to_string(),
        size: None,
    });
    let (bytes, sha256) = download_verified(
        http,
        src.url,
        &dest,
        src.sha256,
        &file,
        &mut |done, total| {
            progress(Progress::Bytes {
                file: file.clone(),
                done,
                total,
            })
        },
    )?;
    progress(Progress::Verified {
        file,
        bytes,
        sha256,
    });
    Ok(true)
}
