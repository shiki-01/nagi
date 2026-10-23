//! 英単語の語彙（SCOWL）の取得（SHA-256 検証）と、fst 索引の組み立て。
//!
//! SCOWL（Spell Checking Oriented Word Lists）は寛容なライセンス（著作権表示と許諾表示を残せば
//! 利用・複製・再配布が自由）。同梱せず、利用者の手元で SourceForge から取得する。
//! 頻度は SCOWL の「サイズ」（10 が最も一般的、70 までで約 10 万語規模）を段階として使う。

use crate::Progress;
use crate::fsutil::{commit, download_verified, sha256_file, temp_path};
use crate::http::{Http, UreqHttp};
use flate2::read::GzDecoder;
use nagi_english::LexiconBuilder;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Instant;

/// 取得するアーカイブのファイル名（`<data_dir>/english/` 下）。
pub const SCOWL_FILE: &str = "scowl-2020.12.07.tar.gz";
const SCOWL_URL: &str =
    "https://downloads.sourceforge.net/project/wordlist/SCOWL/2020.12.07/scowl-2020.12.07.tar.gz";
const SCOWL_SHA256: &str = "5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc";

/// 取り込むサイズ（段階）。95 などの大きいサイズは珍語・固有名詞が増えて日本語との衝突が増えるので使わない
const LEVELS: [u8; 8] = [10, 20, 35, 40, 50, 55, 60, 70];
/// 取り込む単語リストの種類（アーカイブ内の `final/<種類>.<サイズ>`）
/// （`english-upper` は月・曜日・言語名・国名など、常に大文字で始まる語。小文字にして取り込む。
/// 固有名詞のリスト `english-proper-names` は、日本語の人名と衝突しやすいので使わない）
const KINDS: [&str; 4] = [
    "english-words",
    "american-words",
    "british-words",
    "english-upper",
];

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
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("ディレクトリ作成失敗 {}: {}", dest_dir.display(), e))?;
    let dest = dest_dir.join(SCOWL_FILE);
    let file = SCOWL_FILE.to_string();
    if sha256_file(&dest).as_deref() == Some(SCOWL_SHA256) {
        progress(Progress::Skipped {
            file,
            index: 1,
            count: 1,
        });
        return Ok(());
    }
    progress(Progress::Download {
        file: file.clone(),
        index: 1,
        count: 1,
        url: SCOWL_URL.to_string(),
        size: None,
    });
    let (bytes, sha256) = download_verified(
        http,
        SCOWL_URL,
        &dest,
        SCOWL_SHA256,
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
    Ok(())
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
    let file = fs::File::open(archive)
        .map_err(|e| format!("アーカイブを開けません {}: {}", archive.display(), e))?;
    let builder = lexicon_from_scowl_archive(file)?;
    let words = builder.len();
    if words == 0 {
        return Err("SCOWL のアーカイブから単語を取り出せませんでした".to_string());
    }
    let bytes = builder
        .build()
        .map_err(|e| format!("語彙の組み立てに失敗: {e}"))?;
    let size = bytes.len() as u64;

    fs::create_dir_all(out_path.parent().ok_or("出力ディレクトリが特定できない")?)
        .map_err(|e| format!("ディレクトリ作成失敗: {e}"))?;
    // 起動中のサーバーが mmap している語彙を壊さないよう、一時ファイル → rename で置き換える
    let temp = temp_path(out_path);
    fs::write(&temp, bytes)
        .map_err(|e| format!("一時ファイル書き込み失敗 {}: {}", temp.display(), e))?;
    commit(&temp, out_path)?;
    progress(Progress::BuildDone {
        bytes: size,
        seconds: start.elapsed().as_secs_f64(),
        path: out_path.to_path_buf(),
    });
    Ok(words)
}

/// アーカイブ内のパスから段階（サイズ）を求める。取り込み対象でなければ `None`
fn level_of(path: &str) -> Option<u8> {
    let name = path.rsplit('/').next()?;
    if !path.contains("/final/") {
        return None;
    }
    let (kind, level) = name.rsplit_once('.')?;
    let level: u8 = level.parse().ok()?;
    (KINDS.contains(&kind) && LEVELS.contains(&level)).then_some(level)
}

/// `.tar.gz` を 1 回なめて、対象の単語リストから語彙を作る
fn lexicon_from_scowl_archive(reader: impl Read) -> Result<LexiconBuilder, String> {
    let mut tar = GzDecoder::new(reader);
    let mut builder = LexiconBuilder::new();
    let mut long_name: Option<String> = None;
    let mut header = [0u8; 512];
    loop {
        match tar.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("アーカイブの読み込みに失敗: {e}")),
        }
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let size = parse_octal(&header[124..136])?;
        let padded = size.div_ceil(512) * 512;
        let typeflag = header[156];
        let mut name = cstr(&header[0..100]);
        let prefix = cstr(&header[345..500]);
        if !prefix.is_empty() {
            name = format!("{prefix}/{name}");
        }
        if let Some(long) = long_name.take() {
            name = long;
        }
        let wanted = (typeflag == b'0' || typeflag == 0)
            .then(|| level_of(&name))
            .flatten();
        let is_long_name = typeflag == b'L';
        if wanted.is_some() || is_long_name {
            let mut data = vec![0u8; size as usize];
            tar.read_exact(&mut data)
                .map_err(|e| format!("アーカイブの読み込みに失敗: {e}"))?;
            skip(&mut tar, padded - size)?;
            if let Some(level) = wanted {
                add_words(&mut builder, &data, level);
            } else {
                long_name = Some(cstr(&data));
            }
        } else {
            skip(&mut tar, padded)?;
        }
    }
    Ok(builder)
}

fn add_words(builder: &mut LexiconBuilder, data: &[u8], level: u8) {
    // 行は Latin-1 / UTF-8 が混ざりうる。ASCII 小文字だけの語だけを受け付けるので、バイトのまま見る
    for line in data.split(|b| *b == b'\n') {
        if let Ok(word) = std::str::from_utf8(line) {
            builder.add(&word.trim().to_ascii_lowercase(), level);
        }
    }
}

fn skip(reader: &mut impl Read, n: u64) -> Result<(), String> {
    let copied = std::io::copy(&mut reader.take(n), &mut std::io::sink())
        .map_err(|e| format!("アーカイブの読み込みに失敗: {e}"))?;
    if copied != n {
        return Err("アーカイブが途中で終わっています".to_string());
    }
    Ok(())
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn parse_octal(bytes: &[u8]) -> Result<u64, String> {
    let s = cstr(bytes);
    let s = s.trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| "tar のサイズ欄が不正です".to_string())
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

    #[test]
    fn level_of_filters_kind_and_size() {
        assert_eq!(level_of("scowl/final/english-words.10"), Some(10));
        assert_eq!(level_of("scowl/final/american-words.70"), Some(70));
        assert_eq!(level_of("scowl/final/english-words.80"), None);
        assert_eq!(level_of("scowl/final/english-upper.10"), Some(10));
        assert_eq!(level_of("scowl/final/english-proper-names.40"), None);
        assert_eq!(level_of("scowl/src/english-words.10"), None);
    }

    #[test]
    fn builds_lexicon_from_archive() {
        let tar = tar_of(&[
            ("scowl/Copyright", b"x\n"),
            ("scowl/final/english-words.10", b"the\nit's\nCaf\xc3\xa9\n"),
            ("scowl/final/english-upper.35", b"Monday\nB\n"),
            ("scowl/final/english-words.70", b"the\nobscure\n"),
            ("scowl/final/english-words.95", b"ignored\n"),
        ]);
        let builder = lexicon_from_scowl_archive(Cursor::new(gz(&tar))).unwrap();
        let lex = nagi_english::Lexicon::from_vec(builder.build().unwrap()).unwrap();
        assert_eq!(lex.tier("the"), Some(10));
        assert_eq!(lex.tier("obscure"), Some(70));
        assert_eq!(lex.tier("ignored"), None);
        assert_eq!(lex.tier("it's"), None);
        assert_eq!(lex.tier("monday"), Some(35));
        assert_eq!(lex.len(), 3);
    }

    #[test]
    fn truncated_archive_without_words_is_an_error() {
        let dir = std::env::temp_dir().join(format!("nagi-datafetch-en-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("a.tar.gz");
        fs::write(&archive, gz(&tar_of(&[("scowl/Copyright", b"x\n")]))).unwrap();
        let err = build_english_lexicon(&archive, &dir.join("w.fst"), &mut |_| {}).unwrap_err();
        assert!(err.contains("単語を取り出せません"), "{err}");
        let _ = fs::remove_dir_all(&dir);
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

    #[test]
    fn constants_are_well_formed() {
        assert_eq!(SCOWL_SHA256.len(), 64);
        assert!(SCOWL_URL.starts_with("https://"));
    }
}
