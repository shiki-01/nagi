//! Mozc OSS 辞書の取得（コミット固定・全ファイル SHA-256 検証）と Nagi バイナリ辞書の組み立て。

use crate::Progress;
use crate::fsutil::{download_verified, sha256_file};
use crate::http::{Http, UreqHttp};
use std::fs;
use std::path::Path;
use std::time::Instant;

/// 取得する Mozc のコミット。
pub const MOZC_COMMIT: &str = "b9c3fcbd6d76b19649ef572324fa9da2559bc18e";
const MOZC_BASE_URL: &str = "https://raw.githubusercontent.com/google/mozc";

/// ダウンロードするファイルの定義（URL サフィックスと期待 SHA-256）。
const FILES: &[(&str, &str)] = &[
    (
        "src/data/dictionary_oss/dictionary00.txt",
        "9e07ce292932c4fff327ed8cb06274c09acca556bd8bdf7c7ef1f6173eae3b61",
    ),
    (
        "src/data/dictionary_oss/dictionary01.txt",
        "836fd5fd8399510de0d8ef7b33be913cca6bbaee7b712cc8045fba03f4c75a89",
    ),
    (
        "src/data/dictionary_oss/dictionary02.txt",
        "93943858d0cfd092b849a78def6901eaf228177e1ac5663544e865a3a1e45a9e",
    ),
    (
        "src/data/dictionary_oss/dictionary03.txt",
        "cb9f1b0b4a7550d682d86244a021cd17e0b15366328819e69135b5f183a1a24b",
    ),
    (
        "src/data/dictionary_oss/dictionary04.txt",
        "9fdc4e3948d03463de13ab5782adffbc11dd06e934cb80138c5920029cd3d2f6",
    ),
    (
        "src/data/dictionary_oss/dictionary05.txt",
        "199e3b0f050b691823ae7ba943090d437f51af6436b27c5f3dc69cafa12000e4",
    ),
    (
        "src/data/dictionary_oss/dictionary06.txt",
        "b95b14f7770ae2a824bc8e9fad8b03c1bc45c504f93a36554b8b56daa0368d2a",
    ),
    (
        "src/data/dictionary_oss/dictionary07.txt",
        "7f53649b529d4c8c495d3a8dd9c4d489dfe7696080f7423107ff01331174be48",
    ),
    (
        "src/data/dictionary_oss/dictionary08.txt",
        "50a31bb5fc1646721b00be371a5695dffab1292bda143e1e59cc70cbbdd8643f",
    ),
    (
        "src/data/dictionary_oss/dictionary09.txt",
        "4e5fcd4cbfbf11395166a37b506f547fc54d20edc0884f5184ecd3fc705f0d9a",
    ),
    (
        "src/data/dictionary_oss/connection_single_column.txt",
        "68823a1f6617372ccb89afdbcb334d7a0bcfa82c9c5dda03fc1f80f1cab5c48e",
    ),
    (
        "src/data/dictionary_oss/id.def",
        "07a05a268c3783b7e02b36e4aa591555a2dd19a1f3c93891576740761c154f2d",
    ),
    (
        "src/data/dictionary_oss/evaluation.tsv",
        "445ed21e6c83d55774f15a83a5f6ea72c945ef0f30ec7ed5fe4bc6bc7bee9836",
    ),
    (
        "src/data/dictionary_oss/README.txt",
        "6b1de66bc6fa30e0b45dc45b8c8f6c57bd78aff923c261a3efffac8eb86f7bac",
    ),
];

/// 取得するファイル数。
pub fn mozc_file_count() -> usize {
    FILES.len()
}

/// 取得するファイル名（ディレクトリを除く）の一覧。
pub fn mozc_file_names() -> impl Iterator<Item = &'static str> {
    FILES
        .iter()
        .map(|(suffix, _)| suffix.rsplit('/').next().unwrap_or(suffix))
}

/// Mozc 辞書一式を `dest_dir` へ取得する。SHA-256 が一致する既存ファイルはスキップする。
pub fn fetch_mozc(dest_dir: &Path, progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    fetch_mozc_with(&UreqHttp, dest_dir, progress)
}

/// [`fetch_mozc`] の HTTP 差し替え版。
pub fn fetch_mozc_with(
    http: &dyn Http,
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dest_dir.display(), e))?;
    let count = FILES.len();
    for (i, (suffix, expected)) in FILES.iter().enumerate() {
        let index = i + 1;
        let file = suffix.rsplit('/').next().unwrap_or(suffix).to_string();
        let dest = dest_dir.join(&file);

        // 既に存在して SHA-256 が一致したらスキップ
        if sha256_file(&dest).as_deref() == Some(*expected) {
            progress(Progress::Skipped { file, index, count });
            continue;
        }

        let url = format!("{MOZC_BASE_URL}/{MOZC_COMMIT}/{suffix}");
        progress(Progress::Download {
            file: file.clone(),
            index,
            count,
            url: url.clone(),
            size: None,
        });
        let (bytes, sha256) =
            download_verified(http, &url, &dest, expected, &file, &mut |done, total| {
                progress(Progress::Bytes {
                    file: file.clone(),
                    done,
                    total,
                })
            })?;
        progress(Progress::Verified {
            file,
            bytes,
            sha256,
        });
    }
    Ok(())
}

/// 取得済みの Mozc 辞書（`mozc_dir`）から Nagi バイナリ辞書を組み立て、`out_path` へ書く。
pub fn build_mozc_dict(
    mozc_dir: &Path,
    out_path: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    progress(Progress::BuildStart);
    let start = Instant::now();
    // Mozc の連接コストは「前の語の右 ID × 次の語の左 ID」の順（RidMajor。実データの変換結果で検証済み）
    let dictionary =
        nagi_dict::build_from_mozc_dir(mozc_dir, nagi_dict::ConnectionLayout::RidMajor)
            .map_err(|e| format!("辞書ビルド失敗: {e}"))?;
    let binary_data = dictionary.to_bytes();
    let seconds = start.elapsed().as_secs_f64();
    let bytes = binary_data.len() as u64;

    fs::create_dir_all(out_path.parent().ok_or("出力ディレクトリが特定できない")?)
        .map_err(|e| format!("ディレクトリ作成失敗: {e}"))?;
    // 同じ inode を上書きしない: 起動中のサーバーが mmap している辞書が SIGBUS しないよう、
    // 一時ファイルに書いてから rename で置き換える
    let temp_file = out_path.with_extension("tmp");
    fs::write(&temp_file, binary_data)
        .map_err(|e| format!("一時ファイル書き込み失敗 {}: {}", temp_file.display(), e))?;
    fs::rename(&temp_file, out_path).map_err(|e| {
        let _ = fs::remove_file(&temp_file);
        format!(
            "ファイルリネーム失敗 {} -> {}: {}",
            temp_file.display(),
            out_path.display(),
            e
        )
    })?;
    progress(Progress::BuildDone {
        bytes,
        seconds,
        path: out_path.to_path_buf(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Body;
    use std::io::Cursor;

    /// 固定のバイト列を返す HTTP（URL を記録する）
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
        assert_eq!(mozc_file_count(), FILES.len());
        for (suffix, sha) in FILES {
            assert_eq!(sha.len(), 64, "{suffix}");
            assert!(sha.bytes().all(|b| b.is_ascii_hexdigit()), "{suffix}");
            let url = format!("{MOZC_BASE_URL}/{MOZC_COMMIT}/{suffix}");
            assert!(url.starts_with("https://"));
        }
        assert!(mozc_file_names().any(|n| n == "id.def"));
    }

    #[test]
    fn sha_mismatch_fails_and_leaves_no_files() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-mozc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut events = Vec::new();
        let err = fetch_mozc_with(&FakeHttp(b"dummy".to_vec()), &dir, &mut |p| events.push(p))
            .unwrap_err();
        assert!(err.contains("SHA-256 が不一致"), "{err}");
        // 開始イベント → バイト進捗の順で、検証済みは出ない
        assert!(matches!(events[0], Progress::Download { index: 1, .. }));
        assert!(matches!(events[1], Progress::Bytes { .. }));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Progress::Verified { .. }))
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
