//! 取得済みの JMdict（gzip の XML）から索引ファイルを組み立てる。ネットワークは使わない
//! （取得は `nagi-datafetch`、iOS はホストアプリ）。

use crate::index::IndexBuilder;
use crate::jmdict;
use flate2::read::GzDecoder;
use std::fs;
use std::io::Read;
use std::path::Path;

/// 取得するファイル名（`<data_dir>/loanword/` 下）
pub const JMDICT_FILE: &str = "JMdict_e.gz";
/// 組み立てた索引のファイル名
pub const LOANWORD_INDEX_FILE: &str = "loanword.fst";
/// 取得元の URL（EDRDG）
pub const JMDICT_URL: &str = "https://www.edrdg.org/pub/Nihongo/JMdict_e.gz";
/// 固定した `JMdict_e.gz` の SHA-256。未固定は `None`（`docs/decisions.md` H-52）。
///
/// EDRDG の `JMdict_e.gz` は更新のたびに同じ URL の中身が変わるので、固定した時点のファイルに対して
/// 検証する。固定していない間は取得を拒否する（`cargo xtask fetch-loanword` だけが下見として取得できる）
pub const JMDICT_SHA256: Option<&str> = None;

/// XML から索引のバイト列と語数を作る
pub fn build_from_xml(xml: &str) -> Result<(Vec<u8>, usize), String> {
    let mut builder = IndexBuilder::new();
    for entry in jmdict::extract(xml) {
        builder.add(&entry);
    }
    let count = builder.len();
    if count == 0 {
        return Err("JMdict から外来語の原語を取り出せませんでした".to_string());
    }
    let bytes = builder
        .build()
        .map_err(|e| format!("索引の組み立てに失敗: {e}"))?;
    Ok((bytes, count))
}

/// 取得済みの JMdict（gzip の XML、`archive`）から索引を組み立て、`out_path` へ書く。語数を返す。
/// 起動中のサーバーが mmap している索引を壊さないよう、一時ファイル → rename で置き換える
pub fn build_index_file(archive: &Path, out_path: &Path) -> Result<usize, String> {
    let file = fs::File::open(archive)
        .map_err(|e| format!("JMdict を開けません {}: {}", archive.display(), e))?;
    let mut xml = Vec::new();
    GzDecoder::new(file)
        .read_to_end(&mut xml)
        .map_err(|e| format!("JMdict の展開に失敗: {e}"))?;
    let (bytes, count) = build_from_xml(&String::from_utf8_lossy(&xml))?;
    fs::create_dir_all(out_path.parent().ok_or("出力ディレクトリが特定できない")?)
        .map_err(|e| format!("ディレクトリ作成失敗: {e}"))?;
    let temp = out_path.with_extension("tmp");
    fs::write(&temp, bytes)
        .map_err(|e| format!("一時ファイル書き込み失敗 {}: {}", temp.display(), e))?;
    fs::rename(&temp, out_path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("ファイルリネーム失敗 {}: {}", out_path.display(), e)
    })?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_is_an_error() {
        assert!(build_from_xml("<JMdict></JMdict>").is_err());
    }

    #[test]
    fn builds_from_xml() {
        let (bytes, n) = build_from_xml(
            "<JMdict><entry><r_ele><reb>アルバイト</reb></r_ele>\
             <sense><lsource xml:lang=\"ger\">Arbeit</lsource></sense></entry></JMdict>",
        )
        .unwrap();
        assert_eq!(n, 1);
        let idx = crate::LoanwordIndex::from_vec(bytes).unwrap();
        assert_eq!(idx.lookup("アルバイト", true)[0].spelling, "Arbeit");
    }
}
