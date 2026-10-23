//! 取得済みの JMdict（jmdict-simplified の zip）から索引ファイルを組み立てる。ネットワークは使わない
//! （取得は `nagi-datafetch`、iOS はホストアプリ）。

use crate::index::IndexBuilder;
use crate::jmdict;
use std::fs;
use std::io::{BufReader, Read};
use std::path::Path;

/// 固定した版（jmdict-simplified のリリース tag。JMdict の日付は 2026-10-19）
pub const JMDICT_TAG: &str = "3.6.2+20261019180022";
/// 取得するファイル名（`<data_dir>/loanword/` 下）
pub const JMDICT_FILE: &str = "jmdict-eng-3.6.2+20261019180022.json.zip";
/// 組み立てた索引のファイル名
pub const LOANWORD_INDEX_FILE: &str = "loanword.fst";
/// 取得元の URL（GitHub のリリース資産。版ごとに不変）
pub const JMDICT_URL: &str = "https://github.com/scriptin/jmdict-simplified/releases/download/3.6.2%2B20261019180022/jmdict-eng-3.6.2%2B20261019180022.json.zip";
/// 固定した資産の SHA-256
pub const JMDICT_SHA256: &str = "ef3ea475127ce2a85c60baa3ca5a8f7e33b836e02c4fad6633b3dd72d2b362c9";

/// 索引を組み立てるときの数え上げ
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BuildStats {
    /// 登録した語（正規形）の数
    pub words: usize,
    /// 入力の件数（lsource 由来 + 推定）
    pub entries: usize,
    /// うち gloss から推定したもの
    pub inferred: usize,
}

/// JSON の読み取り元から索引のバイト列を作る（1 語ずつ読むので、JSON 全体はメモリに載せない）
pub fn build_from_reader<R: Read>(reader: R) -> Result<(Vec<u8>, BuildStats), String> {
    let mut builder = IndexBuilder::new();
    let mut stats = BuildStats::default();
    jmdict::extract_reader(reader, &mut |e| {
        stats.entries += 1;
        if e.rank >= jmdict::INFERRED_RANK_BASE {
            stats.inferred += 1;
        }
        builder.add(&e);
    })?;
    stats.words = builder.len();
    if stats.words == 0 {
        return Err("JMdict から外来語の原語を取り出せませんでした".to_string());
    }
    let bytes = builder
        .build()
        .map_err(|e| format!("索引の組み立てに失敗: {e}"))?;
    Ok((bytes, stats))
}

/// 取得済みの zip（`archive`。中の最初の `.json`）から索引を組み立て、`out_path` へ書く。
/// 起動中のサーバーが mmap している索引を壊さないよう、一時ファイル → rename で置き換える
pub fn build_index_file(archive: &Path, out_path: &Path) -> Result<BuildStats, String> {
    let file = fs::File::open(archive)
        .map_err(|e| format!("JMdict を開けません {}: {}", archive.display(), e))?;
    let mut zip =
        zip::ZipArchive::new(BufReader::new(file)).map_err(|e| format!("zip を開けません: {e}"))?;
    let index = (0..zip.len())
        .find(|i| {
            zip.by_index_raw(*i)
                .is_ok_and(|f| f.name().to_ascii_lowercase().ends_with(".json"))
        })
        .ok_or("zip に JSON が含まれていません")?;
    let entry = zip
        .by_index(index)
        .map_err(|e| format!("zip の展開に失敗: {e}"))?;
    let (bytes, stats) = build_from_reader(BufReader::new(entry))?;
    fs::create_dir_all(out_path.parent().ok_or("出力ディレクトリが特定できない")?)
        .map_err(|e| format!("ディレクトリ作成失敗: {e}"))?;
    let temp = out_path.with_extension("tmp");
    fs::write(&temp, bytes)
        .map_err(|e| format!("一時ファイル書き込み失敗 {}: {}", temp.display(), e))?;
    fs::rename(&temp, out_path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("ファイルリネーム失敗 {}: {}", out_path.display(), e)
    })?;
    Ok(stats)
}

/// テスト用: JSON を 1 ファイルだけ含む zip のバイト列
#[doc(hidden)]
pub fn zip_of_json(json: &str) -> Vec<u8> {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file(
        "jmdict-eng-test.json",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    w.write_all(json.as_bytes()).unwrap();
    w.finish().unwrap().into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"words":[{"kana":[{"text":"アルバイト","common":true}],
        "sense":[{"languageSource":[{"lang":"ger","text":"Arbeit"}]}]},
        {"kana":[{"text":"チーズ","common":true}],"sense":[{"languageSource":[],"gloss":[{"text":"cheese"}]}]}]}"#;

    #[test]
    fn pinned_values_are_well_formed() {
        assert_eq!(JMDICT_SHA256.len(), 64);
        assert!(JMDICT_SHA256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(JMDICT_URL.starts_with("https://github.com/scriptin/jmdict-simplified/"));
        assert!(JMDICT_URL.contains("3.6.2%2B20261019180022"));
        assert_eq!(JMDICT_FILE, "jmdict-eng-3.6.2+20261019180022.json.zip");
        assert!(JMDICT_URL.contains(&JMDICT_FILE.replace('+', "%2B")));
        assert!(JMDICT_TAG.contains("20261019180022"));
    }

    #[test]
    fn empty_source_is_an_error() {
        assert!(build_from_reader(&b"{\"words\":[]}"[..]).is_err());
    }

    #[test]
    fn builds_from_zip_file() {
        let dir = std::env::temp_dir().join(format!("nagi-loanword-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("a.zip");
        fs::write(&archive, zip_of_json(JSON)).unwrap();
        let out = dir.join("loanword.fst");
        let stats = build_index_file(&archive, &out).unwrap();
        assert_eq!(
            stats,
            BuildStats {
                words: 2,
                entries: 2,
                inferred: 1
            }
        );
        let idx = crate::LoanwordIndex::load(&out).unwrap();
        assert_eq!(idx.lookup("アルバイト", true)[0].spelling, "Arbeit");
        assert_eq!(idx.lookup("チーズ", true)[0].spelling, "cheese");
        // zip でないファイル・JSON を含まない zip は理由つきのエラー
        fs::write(&archive, b"not a zip").unwrap();
        assert!(build_index_file(&archive, &out).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
