//! カタカナ語の原語データ（JMdict の `lsource`）の取得と、fst 索引の組み立て。
//!
//! JMdict は EDRDG の辞書（CC BY-SA 4.0）。**同梱せず**、利用者の手元で配布元から取得して
//! 索引（`loanword.fst`）だけを組み立てる。出典の表示は `NOTICE`。
//!
//! EDRDG の `JMdict_e.gz` は更新のたびに同じ URL の中身が変わるので、SHA-256 は「固定した
//! 時点のファイル」に対して検証する。固定していない間（[`JMDICT_SHA256`] が `None`）は
//! 取得を拒否する（`cargo xtask fetch-loanword` だけが、下見として SHA-256 を表示して取得できる）。

use crate::Progress;
use crate::fsutil::{Source, download_verified, fetch_pinned};
use crate::http::{Http, UreqHttp};
use nagi_loanword::archive::{JMDICT_SHA256, JMDICT_URL};
use std::fs;
use std::path::Path;
use std::time::Instant;

pub use nagi_loanword::archive::{JMDICT_FILE, LOANWORD_INDEX_FILE};

/// 取得（スキップ可）と索引の組み立てまでを行う（`imserver --install-data` 用）。
/// 取得済みで SHA-256 が一致し、索引もあるときは何もしない。何かを行ったとき `true`
pub fn install_loanword(
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<bool, String> {
    let sha = JMDICT_SHA256.ok_or_else(|| {
        "JMdict の SHA-256 が未固定のため、原語データは取得しません（docs/decisions.md H-52）"
            .to_string()
    })?;
    install_loanword_from(
        &UreqHttp,
        &Source {
            file: JMDICT_FILE,
            url: JMDICT_URL,
            sha256: sha,
        },
        dest_dir,
        progress,
    )
}

fn install_loanword_from(
    http: &dyn Http,
    src: &Source<'_>,
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<bool, String> {
    let downloaded = fetch_pinned(http, dest_dir, src, progress)?;
    let index = dest_dir.join(LOANWORD_INDEX_FILE);
    if downloaded || !index.is_file() {
        build_loanword_index(&dest_dir.join(src.file), &index, progress)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// SHA-256 を検証せずに取得して、そのハッシュを返す（固定する値を決めるための下見。xtask 専用）
pub fn fetch_loanword_unpinned(
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<String, String> {
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dest_dir.display(), e))?;
    let file = JMDICT_FILE.to_string();
    progress(Progress::Download {
        file: file.clone(),
        index: 1,
        count: 1,
        url: JMDICT_URL.to_string(),
        size: None,
    });
    let (_, sha) = download_verified(
        &UreqHttp,
        JMDICT_URL,
        &dest_dir.join(JMDICT_FILE),
        "",
        &file,
        &mut |done, total| {
            progress(Progress::Bytes {
                file: file.clone(),
                done,
                total,
            })
        },
    )?;
    Ok(sha)
}

/// 取得済みの JMdict（gzip の XML）から索引を組み立て、`out_path` へ書く。登録した語の数を返す
pub fn build_loanword_index(
    archive: &Path,
    out_path: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<usize, String> {
    progress(Progress::BuildStart);
    let start = Instant::now();
    let count = nagi_loanword::archive::build_index_file(archive, out_path)?;
    progress(Progress::BuildDone {
        bytes: fs::metadata(out_path).map(|m| m.len()).unwrap_or(0),
        seconds: start.elapsed().as_secs_f64(),
        path: out_path.to_path_buf(),
    });
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Body;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};

    const XML: &str = "<JMdict><entry><r_ele><reb>アルバイト</reb></r_ele>\
        <sense><lsource xml:lang=\"ger\">Arbeit</lsource></sense></entry></JMdict>";

    fn gz(data: &[u8]) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

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
    fn install_builds_then_skips() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-lw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let archive = gz(XML.as_bytes());
        let sha = format!("{:x}", Sha256::digest(&archive));
        let src = Source {
            file: "fake.gz",
            url: "https://example.invalid/fake.gz",
            sha256: &sha,
        };
        let http = FakeHttp(archive);
        let mut events = Vec::new();
        assert!(install_loanword_from(&http, &src, &dir, &mut |p| events.push(p)).unwrap());
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Progress::Download { .. }))
        );
        let index = nagi_loanword::LoanwordIndex::load(&dir.join(LOANWORD_INDEX_FILE)).unwrap();
        assert_eq!(index.lookup("アルバイト", true)[0].spelling, "Arbeit");

        let mut events = Vec::new();
        assert!(!install_loanword_from(&http, &src, &dir, &mut |p| events.push(p)).unwrap());
        assert!(matches!(events.as_slice(), [Progress::Skipped { .. }]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha_mismatch_fails_and_leaves_no_files() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-lw2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = Source {
            file: "fake.gz",
            url: "https://example.invalid/fake.gz",
            sha256: &"0".repeat(64),
        };
        let err =
            install_loanword_from(&FakeHttp(b"x".to_vec()), &src, &dir, &mut |_| {}).unwrap_err();
        assert!(err.contains("SHA-256 が不一致"), "{err}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpinned_install_is_refused() {
        if JMDICT_SHA256.is_none() {
            let dir = std::env::temp_dir().join("nagi-datafetch-lw3-never-created");
            let err = install_loanword(&dir, &mut |_| {}).unwrap_err();
            assert!(err.contains("未固定"), "{err}");
            assert!(!dir.exists());
        }
    }
}
