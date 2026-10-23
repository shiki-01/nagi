//! カタカナ語の原語データ（JMdict の languageSource）の取得と、fst 索引の組み立ての進捗通知。
//!
//! JMdict は EDRDG の辞書（CC BY-SA 4.0）。**同梱せず**、利用者の手元で配布元から取得して
//! 索引（`loanword.fst`）だけを組み立てる。出典の表示は `NOTICE`。取得するのは
//! scriptin/jmdict-simplified のリリース資産（版ごとに不変）で、`nagi_loanword::archive` の
//! 固定値（tag・URL・SHA-256）に対して検証する。組み立て本体は `nagi_loanword::archive`。

use crate::Progress;
use crate::fsutil::{Source, fetch_pinned};
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
    install_loanword_from(
        &UreqHttp,
        &Source {
            file: JMDICT_FILE,
            url: JMDICT_URL,
            sha256: JMDICT_SHA256,
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

/// 取得済みの JMdict（zip）から索引を組み立て、`out_path` へ書く。登録した語の数を返す
pub fn build_loanword_index(
    archive: &Path,
    out_path: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<usize, String> {
    progress(Progress::BuildStart);
    let start = Instant::now();
    let count = nagi_loanword::archive::build_index_file(archive, out_path)?.words;
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
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    const JSON: &str = r#"{"words":[{"kana":[{"text":"アルバイト","common":true}],
        "sense":[{"languageSource":[{"lang":"ger","text":"Arbeit"}]}]}]}"#;

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
        let archive = nagi_loanword::archive::zip_of_json(JSON);
        let sha = format!("{:x}", Sha256::digest(&archive));
        let src = Source {
            file: "fake.json.zip",
            url: "https://example.invalid/fake.json.zip",
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
            file: "fake.json.zip",
            url: "https://example.invalid/fake.json.zip",
            sha256: &"0".repeat(64),
        };
        let err =
            install_loanword_from(&FakeHttp(b"x".to_vec()), &src, &dir, &mut |_| {}).unwrap_err();
        assert!(err.contains("SHA-256 が不一致"), "{err}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_source_is_the_jmdict_simplified_release() {
        assert!(JMDICT_URL.contains("scriptin/jmdict-simplified/releases/download/"));
        assert_eq!(JMDICT_SHA256.len(), 64);
    }
}
