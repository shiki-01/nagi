//! 外来語の原語を候補にする Rewriter。

use crate::index::LoanwordIndex;
use crate::normalize::{is_katakana_word, normalize};
use nagi_rewriter::Rewriter;
use std::sync::Arc;

/// [`LoanwordRewriter`] の識別子
pub const LOANWORD_REWRITER_ID: &str = "loanword";

/// カタカナ語の原語を候補にする。和製英語・複合語（索引にない連続）は出さない
#[derive(Debug, Clone)]
pub struct LoanwordRewriter {
    index: Arc<LoanwordIndex>,
}

impl LoanwordRewriter {
    /// 索引から作る
    pub fn new(index: Arc<LoanwordIndex>) -> Self {
        LoanwordRewriter { index }
    }

    /// 候補のカタカナ表記（`surfaces`）と読み（`reading`）から原語の候補を作る。
    ///
    /// 表記にカタカナだけの語があれば、それを引く（ゆるい一致あり。順位は正確な一致の後ろ）。
    /// 読み（ひらがな）をカタカナにしたものは、正確な一致だけを引く（辞書に無いカタカナ語の拾い上げ）。
    /// 重複は除き、最大 3 件
    pub fn candidates<'a>(
        &self,
        surfaces: impl IntoIterator<Item = &'a str>,
        reading: &str,
    ) -> Vec<String> {
        let mut exact: Vec<String> = Vec::new();
        let mut fuzzy: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut visit = |word: &str, allow_fuzzy: bool| {
            let norm = normalize(word);
            if seen.contains(&norm) {
                return;
            }
            seen.push(norm);
            for loan in self.index.lookup(word, allow_fuzzy) {
                let bucket = if loan.fuzzy { &mut fuzzy } else { &mut exact };
                if !bucket.contains(&loan.spelling) {
                    bucket.push(loan.spelling);
                }
            }
        };
        for s in surfaces {
            if is_katakana_word(s) {
                visit(s, true);
            }
        }
        let as_kana = to_kana_word(reading);
        if let Some(k) = as_kana {
            visit(&k, false);
        }
        for f in fuzzy {
            if !exact.contains(&f) {
                exact.push(f);
            }
        }
        exact.truncate(crate::index::MAX_RESULTS);
        exact
    }
}

/// かな（ひらがな・カタカナ・長音符）だけの読みなら、その正規形でない素のカタカナ表記を返す
fn to_kana_word(reading: &str) -> Option<String> {
    let k: String = reading
        .chars()
        .map(|c| match c {
            '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            other => other,
        })
        .collect();
    (k.chars().count() >= 3 && is_katakana_word(&k)).then_some(k)
}

impl Rewriter for LoanwordRewriter {
    fn id(&self) -> &str {
        LOANWORD_REWRITER_ID
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        self.candidates(std::iter::once(reading), reading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Entry, IndexBuilder};

    fn rewriter() -> LoanwordRewriter {
        let mut b = IndexBuilder::new();
        for (h, lang, sp) in [
            ("コンピューター", "eng", "computer"),
            ("シミュレーション", "eng", "simulation"),
        ] {
            b.add(&Entry {
                headword: h.into(),
                lang: lang.into(),
                spelling: sp.into(),
                wasei: false,
                rank: 0,
            });
        }
        LoanwordRewriter::new(Arc::new(
            LoanwordIndex::from_vec(b.build().unwrap()).unwrap(),
        ))
    }

    #[test]
    fn from_reading_and_surfaces() {
        let r = rewriter();
        assert_eq!(r.rewrite("こんぴゅーたー"), ["computer"]);
        assert_eq!(
            r.candidates(["計算機", "コンピュータ"], "けいさんき"),
            ["computer"]
        );
        assert!(r.candidates(["計算機"], "けいさんき").is_empty());
    }

    #[test]
    fn fuzzy_applies_to_surfaces_but_not_to_readings() {
        let r = rewriter();
        assert_eq!(
            r.candidates(["シュミレーション"], "しゅみれーしょん"),
            ["simulation"]
        );
        assert!(r.rewrite("しゅみれーしょん").is_empty());
    }
}
