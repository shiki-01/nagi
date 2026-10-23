//! 外来語の索引（fst）と問い合わせ。
//!
//! ファイルは fst の `Set` のバイト列そのもの。キーは 2 種類:
//!
//! - 語: `正規形 \0 順位(1B) 和製英語('0'/'1') 言語 \0 原語の綴り`
//! - ゆるい一致用の逆引き: `\x01 骨格 \0 正規形`
//!
//! 語のキーは正規形の接頭辞で範囲検索でき、順位の昇順（よく使われる語が先）に並ぶ。

use crate::normalize::{normalize, skeleton, within_one_edit};
use fst::{IntoStreamer, Set, SetBuilder, Streamer};
use std::collections::BTreeSet;
use std::path::Path;
use thiserror::Error;

/// 1 つの正規形に付ける原語の最大数（索引の大きさの上限）
const MAX_PER_KEY: usize = 8;
/// ゆるい一致を試す骨格の最小の長さ（短い語は別の語と取り違えやすい。実データで 4〜5 文字は
/// フォルダー → フォルム のような誤りが出たので 6 文字以上にした）
const FUZZY_MIN_SKELETON_CHARS: usize = 6;
/// 返す原語の最大数
pub const MAX_RESULTS: usize = 3;

/// 索引の読み書きのエラー
#[derive(Debug, Error)]
pub enum LoanwordError {
    /// ファイル入出力
    #[error("外来語の索引の入出力に失敗しました: {0}")]
    Io(#[from] std::io::Error),
    /// fst として不正
    #[error("外来語の索引が不正です: {0}")]
    Fst(#[from] fst::Error),
}

/// 索引に入れる 1 件（組み立て用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 見出し（カタカナ表記。正規化は組み立て側で行う）
    pub headword: String,
    /// 原語の言語（ISO 639-2 の 3 文字。`eng` `ger` `fre` など）
    pub lang: String,
    /// 原語の綴り
    pub spelling: String,
    /// 和製英語（`ls_wasei`）か
    pub wasei: bool,
    /// 順位（0 が最も一般的。1 バイトに収める）
    pub rank: u8,
}

/// 組み立て
#[derive(Default)]
pub struct IndexBuilder {
    keys: BTreeSet<Vec<u8>>,
    per_key: std::collections::HashMap<String, usize>,
    skeletons: BTreeSet<(String, String)>,
}

impl IndexBuilder {
    /// 空の組み立て
    pub fn new() -> Self {
        Self::default()
    }

    /// 1 件足す
    pub fn add(&mut self, e: &Entry) {
        let norm = normalize(&e.headword);
        if norm.is_empty() || e.spelling.is_empty() || e.spelling.contains('\0') {
            return;
        }
        let n = self.per_key.entry(norm.clone()).or_insert(0);
        if *n >= MAX_PER_KEY {
            return;
        }
        let mut key = norm.as_bytes().to_vec();
        key.push(0);
        key.push(e.rank);
        key.push(if e.wasei { b'1' } else { b'0' });
        key.extend_from_slice(e.lang.as_bytes());
        key.push(0);
        key.extend_from_slice(e.spelling.as_bytes());
        if self.keys.insert(key) {
            *n += 1;
        }
        self.skeletons.insert((skeleton(&norm), norm));
    }

    /// 登録した語（正規形）の数
    pub fn len(&self) -> usize {
        self.per_key.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.per_key.is_empty()
    }

    /// fst のバイト列にする
    pub fn build(self) -> Result<Vec<u8>, LoanwordError> {
        let mut all: BTreeSet<Vec<u8>> = self.keys;
        for (skel, norm) in self.skeletons {
            let mut key = vec![1u8];
            key.extend_from_slice(skel.as_bytes());
            key.push(0);
            key.extend_from_slice(norm.as_bytes());
            all.insert(key);
        }
        let mut builder = SetBuilder::memory();
        for key in all {
            builder.insert(key)?;
        }
        Ok(builder.into_inner()?)
    }
}

/// 問い合わせの結果の 1 件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loan {
    /// 原語の綴り
    pub spelling: String,
    /// 原語の言語（3 文字コード）
    pub lang: String,
    /// 骨格の編集距離でのゆるい一致か（順位は下）
    pub fuzzy: bool,
}

/// 索引ファイル全体のバイト列の持ち主（メモリ上の `Vec` か、mmap）
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

/// 索引ファイルを mmap する。失敗したら `None`（呼び出し側が全量読み込みに戻す）
#[cfg(not(windows))]
#[allow(unsafe_code)]
fn map_file(path: &Path) -> Option<memmap2::Mmap> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: 読み取り専用で開く。マップ中に同じ inode が書き換えられると SIGBUS になりうるため、
    // 更新は別 inode への rename で行う運用とする（`nagi-datafetch` がそうする）
    unsafe { memmap2::Mmap::map(&file) }.ok()
}

/// 読み込み済みの索引（不変）
pub struct LoanwordIndex {
    set: Set<Backing>,
}

impl std::fmt::Debug for LoanwordIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoanwordIndex")
            .field("keys", &self.set.len())
            .finish()
    }
}

/// 語のキーから取り出した 1 件
struct Raw {
    rank: u8,
    wasei: bool,
    lang: String,
    spelling: String,
}

impl LoanwordIndex {
    /// ファイルから読み込む。非 Windows では mmap し、失敗したら全量を読む
    pub fn load(path: &Path) -> Result<LoanwordIndex, LoanwordError> {
        #[cfg(not(windows))]
        if let Some(mapped) = map_file(path)
            && let Ok(set) = Set::new(Backing::Mapped(mapped))
        {
            return Ok(LoanwordIndex { set });
        }
        Self::from_vec(std::fs::read(path)?)
    }

    /// バイト列から読み込む
    pub fn from_vec(bytes: Vec<u8>) -> Result<LoanwordIndex, LoanwordError> {
        Ok(LoanwordIndex {
            set: Set::new(Backing::Owned(bytes))?,
        })
    }

    /// キー数（語のキーと逆引きの合計）
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.set.len() == 0
    }

    /// 正規形に付く原語（順位順）
    fn entries(&self, norm: &str) -> Vec<Raw> {
        let mut lo = norm.as_bytes().to_vec();
        lo.push(0);
        let mut hi = norm.as_bytes().to_vec();
        hi.push(1);
        let mut stream = self.set.range().ge(&lo).lt(&hi).into_stream();
        let mut out = Vec::new();
        while let Some(key) = stream.next() {
            let rest = &key[lo.len()..];
            if rest.len() < 3 {
                continue;
            }
            let (rank, wasei, tail) = (rest[0], rest[1] == b'1', &rest[2..]);
            let Some(split) = tail.iter().position(|b| *b == 0) else {
                continue;
            };
            let (Ok(lang), Ok(spelling)) = (
                std::str::from_utf8(&tail[..split]),
                std::str::from_utf8(&tail[split + 1..]),
            ) else {
                continue;
            };
            out.push(Raw {
                rank,
                wasei,
                lang: lang.to_string(),
                spelling: spelling.to_string(),
            });
        }
        out
    }

    /// 骨格が `skel` と編集距離 1 以内の正規形（長さの近いものだけ走査する）
    fn fuzzy_norms(&self, skel: &str) -> Vec<String> {
        let mut stream = self.set.range().ge([1u8]).lt([2u8]).into_stream();
        let mut out = Vec::new();
        let want = skel.len();
        while let Some(key) = stream.next() {
            let body = &key[1..];
            let Some(split) = body.iter().position(|b| *b == 0) else {
                continue;
            };
            // 骨格は UTF-8 で、1 文字の挿入・削除は 3 バイト前後の差
            if split + 3 < want || split > want + 3 {
                continue;
            }
            let (Ok(s), Ok(norm)) = (
                std::str::from_utf8(&body[..split]),
                std::str::from_utf8(&body[split + 1..]),
            ) else {
                continue;
            };
            // 先頭の 1 文字が違う語は、別の語である可能性が高い（バイオリン / アイオリ）ので除く
            if s.chars().next() == skel.chars().next() && (s == skel || within_one_edit(skel, s)) {
                out.push(norm.to_string());
            }
        }
        out
    }

    /// カタカナ表記 `word` の原語を引く。和製英語は含めない。最大 [`MAX_RESULTS`] 件。
    ///
    /// 正規形で一致する語があればその原語（順位順）。索引に語そのものが無いときだけ
    /// （和製英語として載っている語は「ある」と数える）、`allow_fuzzy` なら骨格の編集距離 1 以内の
    /// 語の原語を、ゆるい一致として返す
    pub fn lookup(&self, word: &str, allow_fuzzy: bool) -> Vec<Loan> {
        let norm = normalize(word);
        if norm.chars().count() < 2 {
            return Vec::new();
        }
        let exact = self.entries(&norm);
        let mut out: Vec<Loan> = Vec::new();
        let push = |out: &mut Vec<Loan>, raw: Raw, fuzzy: bool| {
            if raw.wasei
                || out.len() >= MAX_RESULTS
                || out.iter().any(|l| l.spelling == raw.spelling)
            {
                return;
            }
            out.push(Loan {
                spelling: raw.spelling,
                lang: raw.lang,
                fuzzy,
            });
        };
        if !exact.is_empty() {
            for raw in exact {
                push(&mut out, raw, false);
            }
            return out;
        }
        let skel = skeleton(&norm);
        if !allow_fuzzy || skel.chars().count() < FUZZY_MIN_SKELETON_CHARS {
            return out;
        }
        let mut found: Vec<Raw> = self
            .fuzzy_norms(&skel)
            .iter()
            .flat_map(|n| self.entries(n))
            .collect();
        found.sort_by_key(|r| r.rank);
        for raw in found {
            push(&mut out, raw, true);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(h: &str, lang: &str, sp: &str, wasei: bool, rank: u8) -> Entry {
        Entry {
            headword: h.into(),
            lang: lang.into(),
            spelling: sp.into(),
            wasei,
            rank,
        }
    }

    fn index(entries: &[Entry]) -> LoanwordIndex {
        let mut b = IndexBuilder::new();
        for e in entries {
            b.add(e);
        }
        LoanwordIndex::from_vec(b.build().unwrap()).unwrap()
    }

    fn spellings(v: &[Loan]) -> Vec<&str> {
        v.iter().map(|l| l.spelling.as_str()).collect()
    }

    #[test]
    fn exact_lookup_with_variants_and_rank_order() {
        let idx = index(&[
            entry("コンピューター", "eng", "computer", false, 0),
            entry("アルバイト", "ger", "Arbeit", false, 0),
            entry("ガード", "eng", "girder", false, 3),
            entry("ガード", "eng", "guard", false, 0),
            entry("ヴァイオリン", "por", "violino", false, 2),
        ]);
        assert_eq!(spellings(&idx.lookup("コンピュータ", true)), ["computer"]);
        assert_eq!(spellings(&idx.lookup("こんぴゅーたー", true)), ["computer"]);
        assert_eq!(spellings(&idx.lookup("アルバイト", true)), ["Arbeit"]);
        assert_eq!(idx.lookup("アルバイト", true)[0].lang, "ger");
        assert_eq!(spellings(&idx.lookup("ガード", true)), ["guard", "girder"]);
        assert_eq!(spellings(&idx.lookup("バイオリン", true)), ["violino"]);
    }

    #[test]
    fn wasei_is_never_returned_and_blocks_fuzzy() {
        let idx = index(&[
            entry("サラリーマン", "eng", "salaried man", true, 0),
            entry("サラダ", "eng", "salad", false, 0),
        ]);
        assert!(idx.lookup("サラリーマン", true).is_empty());
        // 和製英語として載っている語は「ある」扱いなので、ゆるい一致で別の語を出さない
        assert!(idx.lookup("サラリマン", true).is_empty());
    }

    #[test]
    fn fuzzy_only_when_absent_and_is_marked() {
        let idx = index(&[entry("シミュレーション", "eng", "simulation", false, 0)]);
        let r = idx.lookup("シュミレーション", true);
        assert_eq!(spellings(&r), ["simulation"]);
        assert!(r[0].fuzzy);
        assert!(idx.lookup("シュミレーション", false).is_empty());
        // 短い語はゆるい一致をしない
        let idx = index(&[entry("コート", "eng", "coat", false, 0)]);
        assert!(idx.lookup("コード", true).is_empty());
    }

    #[test]
    fn caps_results_and_ignores_garbage() {
        let entries: Vec<Entry> = (0..6)
            .map(|i| entry("テスト", "eng", &format!("test{i}"), false, i))
            .collect();
        let idx = index(&entries);
        assert_eq!(idx.lookup("テスト", true).len(), MAX_RESULTS);
        assert!(idx.lookup("あ", true).is_empty());
        assert!(idx.lookup("", true).is_empty());
        assert!(LoanwordIndex::from_vec(b"garbage".to_vec()).is_err());
    }
}
