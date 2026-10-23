//! 英単語の語彙（SCOWL）の取得（SHA-256 検証）と、fst 索引の組み立ての進捗通知。
//!
//! アーカイブの読み取りと索引の組み立て本体は `nagi_english::scowl`（ネットワーク非依存。iOS の
//! ホストアプリも FFI 経由で使う）。ここは取得と、進捗の通知を足す。

use crate::Progress;
use crate::fsutil::{Source, fetch_pinned};
use crate::http::{Http, UreqHttp};
use nagi_english::scowl::{SCOWL_FILE as FILE, SCOWL_SHA256, SCOWL_URL};
use std::fs;
use std::path::Path;
use std::time::Instant;

pub use nagi_english::scowl::{LEXICON_FILE, SCOWL_FILE};

const SCOWL: Source<'static> = Source {
    file: FILE,
    url: SCOWL_URL,
    sha256: SCOWL_SHA256,
};

/// SCOWL のアーカイブを `dest_dir` へ取得する。SHA-256 が一致する既存ファイルはスキップする。
pub fn fetch_english(dest_dir: &Path, progress: &mut dyn FnMut(Progress)) -> Result<(), String> {
    fetch_english_with(&UreqHttp, dest_dir, progress)
}

/// [`fetch_english`] の HTTP 差し替え版。
pub fn fetch_english_with(
    http: &dyn Http,
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    fetch_pinned(http, dest_dir, &SCOWL, progress).map(|_| ())
}

/// 取得（スキップ可）と索引の組み立てまでを行う（`imserver --install-data` 用）。
/// 取得済みで SHA-256 が一致し、索引もあるときは何もしない。何かを行ったとき `true`
pub fn install_english(
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<bool, String> {
    install_english_from(&UreqHttp, &SCOWL, dest_dir, progress)
}

fn install_english_from(
    http: &dyn Http,
    src: &Source<'_>,
    dest_dir: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<bool, String> {
    let downloaded = fetch_pinned(http, dest_dir, src, progress)?;
    let lexicon = dest_dir.join(LEXICON_FILE);
    if downloaded || !lexicon.is_file() {
        build_english_lexicon(&dest_dir.join(src.file), &lexicon, progress)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 取得済みのアーカイブ（`archive`）から語彙の fst 索引を組み立て、`out_path` へ書く。
/// 作った単語数を返す
pub fn build_english_lexicon(
    archive: &Path,
    out_path: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<usize, String> {
    progress(Progress::BuildStart);
    let start = Instant::now();
    let words = nagi_english::scowl::build_lexicon_file(archive, out_path)?;
    progress(Progress::BuildDone {
        bytes: fs::metadata(out_path).map(|m| m.len()).unwrap_or(0),
        seconds: start.elapsed().as_secs_f64(),
        path: out_path.to_path_buf(),
    });
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Body;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::{Cursor, Write};

    /// 最小の tar（ustar）を作る
    fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in files {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}", data.len());
            h[124..135].copy_from_slice(size.as_bytes());
            h[156] = b'0';
            out.extend_from_slice(&h);
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

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
    fn sha_mismatch_fails_and_leaves_no_files() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-en2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let err = fetch_english_with(&FakeHttp(b"dummy".to_vec()), &dir, &mut |_| {}).unwrap_err();
        assert!(err.contains("SHA-256 が不一致"), "{err}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    fn sha_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(data))
    }

    #[test]
    fn install_builds_then_skips_when_already_installed() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-en3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let archive = gz(&tar_of(&[(
            "scowl/final/english-words.10",
            b"the\nhello\n",
        )]));
        let sha = sha_hex(&archive);
        let src = Source {
            file: "fake.tar.gz",
            url: "https://example.invalid/fake.tar.gz",
            sha256: &sha,
        };
        let http = FakeHttp(archive);

        let mut events = Vec::new();
        let changed = install_english_from(&http, &src, &dir, &mut |p| events.push(p)).unwrap();
        assert!(changed);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Progress::Download { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Progress::BuildDone { .. }))
        );
        let lex = nagi_english::Lexicon::load(&dir.join(LEXICON_FILE)).unwrap();
        assert_eq!(lex.tier("hello"), Some(10));

        // 2 回目: SHA-256 一致・索引ありなので取得も組み立てもしない
        let mut events = Vec::new();
        let changed = install_english_from(&http, &src, &dir, &mut |p| events.push(p)).unwrap();
        assert!(!changed);
        assert!(matches!(events.as_slice(), [Progress::Skipped { .. }]));

        // 索引だけ消えていれば、取得はせず索引だけ組み立て直す
        fs::remove_file(dir.join(LEXICON_FILE)).unwrap();
        let mut events = Vec::new();
        let changed = install_english_from(&http, &src, &dir, &mut |p| events.push(p)).unwrap();
        assert!(changed);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Progress::Download { .. }))
        );
        assert!(dir.join(LEXICON_FILE).is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn constants_are_well_formed() {
        assert_eq!(SCOWL_SHA256.len(), 64);
        assert!(SCOWL_URL.starts_with("https://"));
    }
}
