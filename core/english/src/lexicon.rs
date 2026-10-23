//! 英単語の語彙: 単語（小文字の ASCII）→ 頻度の段階（小さいほど一般的）の fst 索引。
//!
//! ファイルは fst の `Map` のバイト列そのもの。非 Windows では mmap で読む（`nagi-dict` と同じ方針）ので、
//! 更新は別 inode への rename で行うこと（`nagi-datafetch` がそうする）。

use fst::{Map, MapBuilder};
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

/// 語彙の読み書きのエラー
#[derive(Debug, Error)]
pub enum LexiconError {
    /// ファイル入出力
    #[error("語彙ファイルの入出力に失敗しました: {0}")]
    Io(#[from] std::io::Error),
    /// fst として不正
    #[error("語彙ファイルが不正です: {0}")]
    Fst(#[from] fst::Error),
}

/// 語彙ファイル全体のバイト列の持ち主（メモリ上の `Vec` か、mmap）
enum Backing {
    Owned(Vec<u8>),
    #[cfg(not(windows))]
    Mapped(memmap2::Mmap),
}

impl AsRef<[u8]> for Backing {
    fn as_ref(&self) -> &[u8] {
        match self {
            Backing::Owned(v) => v,
            #[cfg(not(windows))]
            Backing::Mapped(m) => m,
        }
    }
}

/// 読み込み済みの英単語語彙（不変。`Send + Sync`）
pub struct Lexicon {
    map: Map<Backing>,
}

impl std::fmt::Debug for Lexicon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lexicon")
            .field("words", &self.map.len())
            .finish()
    }
}

impl Lexicon {
    /// 語彙ファイルを読み込む。非 Windows では mmap し、失敗したら全量を読む
    pub fn load(path: &Path) -> Result<Lexicon, LexiconError> {
        #[cfg(not(windows))]
        if let Some(mapped) = map_file(path)
            && let Ok(map) = Map::new(Backing::Mapped(mapped))
        {
            return Ok(Lexicon { map });
        }
        Lexicon::from_vec(std::fs::read(path)?)
    }

    /// バイト列から読み込む
    pub fn from_vec(bytes: Vec<u8>) -> Result<Lexicon, LexiconError> {
        Ok(Lexicon {
            map: Map::new(Backing::Owned(bytes))?,
        })
    }

    /// 単語の頻度の段階（小さいほど一般的）。語彙に無ければ `None`。小文字の ASCII で引く
    pub fn tier(&self, word: &str) -> Option<u8> {
        self.map.get(word).map(|v| v as u8)
    }

    /// 単語数
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// 語彙ファイルを mmap する。失敗したら `None`（呼び出し側が全量読み込みに戻す）
#[cfg(not(windows))]
#[allow(unsafe_code)]
fn map_file(path: &Path) -> Option<memmap2::Mmap> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: 読み取り専用で開く。マップ中に同じ inode が書き換えられると SIGBUS になりうるため、
    // 更新は別 inode への rename で行う運用とする（モジュール先頭の注意）
    unsafe { memmap2::Mmap::map(&file) }.ok()
}

/// 語彙の組み立て
#[derive(Debug, Default)]
pub struct LexiconBuilder {
    words: BTreeMap<String, u8>,
}

impl LexiconBuilder {
    /// 空の組み立て器
    pub fn new() -> Self {
        Self::default()
    }

    /// 単語を足す。小文字の ASCII 英字だけの語（2 文字以上）のみ受け付け、受け付けたら `true`。
    /// 同じ語が複数あれば、小さい（一般的な）段階を採る
    pub fn add(&mut self, word: &str, tier: u8) -> bool {
        if word.len() < 2 || !word.bytes().all(|b| b.is_ascii_lowercase()) {
            return false;
        }
        let slot = self.words.entry(word.to_string()).or_insert(tier);
        *slot = (*slot).min(tier);
        true
    }

    /// 単語数
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// fst のバイト列にする
    pub fn build(&self) -> Result<Vec<u8>, LexiconError> {
        let mut builder = MapBuilder::memory();
        for (word, tier) in &self.words {
            builder.insert(word, u64::from(*tier))?;
        }
        Ok(builder.into_inner()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_lookup() {
        let mut b = LexiconBuilder::new();
        assert!(b.add("typescript", 70));
        assert!(b.add("the", 10));
        assert!(b.add("the", 35)); // 小さい段階を採る
        assert!(!b.add("Don't", 10));
        assert!(!b.add("a", 10));
        let lex = Lexicon::from_vec(b.build().unwrap()).unwrap();
        assert_eq!(lex.len(), 2);
        assert_eq!(lex.tier("the"), Some(10));
        assert_eq!(lex.tier("typescript"), Some(70));
        assert_eq!(lex.tier("kyou"), None);
    }

    #[test]
    fn load_maps_the_file() {
        let mut b = LexiconBuilder::new();
        b.add("hello", 10);
        let dir = std::env::temp_dir().join(format!("nagi-english-lex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("words.fst");
        std::fs::write(&path, b.build().unwrap()).unwrap();
        let lex = Lexicon::load(&path).unwrap();
        assert_eq!(lex.tier("hello"), Some(10));
        assert!(Lexicon::load(&dir.join("none.fst")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
