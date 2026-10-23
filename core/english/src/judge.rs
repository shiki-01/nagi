//! 判定スコア。ローマ字の特徴量・英単語の語彙・「日本語として引ける読みか」を合わせて、
//! キー列が英語である確からしさ（0.0〜1.0）を求める。
//!
//! 方針（`docs/decisions.md` H-51）:
//!
//! - かなにならない ASCII が残らない（有効なローマ字）なら、語彙に載っていて日本語として引けない
//!   4 文字以上の語だけが中程度。日本語として引ける（`sense` → せんせ のような）なら日本語を優先
//! - かなにならない ASCII が残る語が語彙に載っていれば高い。載っていなければ（技術用語・固有名詞）
//!   残る割合から見積もり、語彙があるときは「高い」までは上げない
//! - 語彙の 2 語をつないだ複合語（`frontend`）も、語彙の語に近い扱い（高い）
//! - 英字以外を含む・3 文字未満は判定しない（0）

use crate::features::{Features, analyze};
use crate::lexicon::Lexicon;
use nagi_romaji::RomajiTable;
use std::sync::Arc;

/// 「高い」とみなす最小のスコア（候補の 2 番目に置く）
pub const HIGH_THRESHOLD: f32 = 0.8;
/// 「中程度」とみなす最小のスコア（候補の少し後ろに置く）
pub const MID_THRESHOLD: f32 = 0.5;

/// 判定の確からしさの段階
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// 英語とは言えない（候補の並びは変えない）
    None,
    /// 英語の可能性はあるが弱い
    Low,
    /// 英語の可能性が高い
    Mid,
    /// 英語である可能性が非常に高い
    High,
}

/// 判定の結果
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    /// 英語である確からしさ（0.0〜1.0）
    pub score: f32,
    /// 段階
    pub level: Level,
    /// 求めた特徴量
    pub features: Features,
}

impl Verdict {
    fn none(features: Features) -> Self {
        Verdict {
            score: 0.0,
            level: Level::None,
            features,
        }
    }
}

/// 英語の判定器
#[derive(Clone)]
pub struct Judge {
    table: Arc<RomajiTable>,
    lexicon: Option<Arc<Lexicon>>,
}

impl std::fmt::Debug for Judge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Judge")
            .field("lexicon", &self.lexicon)
            .finish()
    }
}

impl Judge {
    /// 判定器を作る。`lexicon` が無ければ特徴量だけで判定する（劣化動作）
    pub fn new(table: Arc<RomajiTable>, lexicon: Option<Arc<Lexicon>>) -> Self {
        Judge { table, lexicon }
    }

    /// 標準のローマ字テーブルで判定器を作る
    pub fn standard(lexicon: Option<Arc<Lexicon>>) -> Self {
        Judge::new(RomajiTable::standard(), lexicon)
    }

    /// 語彙の 2 語（各 3 文字以上）をつないだ形か
    fn is_compound(&self, raw: &str) -> bool {
        let Some(lex) = self.lexicon.as_ref() else {
            return false;
        };
        (3..=raw.len().saturating_sub(3)).any(|i| {
            raw.is_char_boundary(i)
                && lex.tier(&raw[..i]).is_some()
                && lex.tier(&raw[i..]).is_some()
        })
    }

    /// 語彙を持つか
    pub fn has_lexicon(&self) -> bool {
        self.lexicon.is_some()
    }

    /// `raw`（打鍵列）が英語である確からしさ。`japanese_known` は、同じ打鍵列を日本語として読んだ
    /// かなが辞書・学習・ユーザー辞書で単語として引けるか
    pub fn judge(&self, raw: &str, japanese_known: bool) -> Verdict {
        let features = analyze(&self.table, raw);
        if features.len < 3 || !raw.bytes().all(|b| b.is_ascii_lowercase()) {
            return Verdict::none(features);
        }
        let tier = self.lexicon.as_ref().and_then(|l| l.tier(raw));
        let score = if features.raw_count == 0 {
            // 有効なローマ字。日本語として読めるので、語彙にあっても強くは勧めない
            match tier {
                // 3 文字は最も一般的な語（`the` など）だけ
                Some(t) if (features.len >= 4 || t <= 20) && !japanese_known => {
                    if t <= 40 {
                        0.6
                    } else {
                        0.45
                    }
                }
                _ => 0.0,
            }
        } else if tier.is_some() {
            0.9
        } else if self.is_compound(raw) {
            // 語彙の 2 語をつないだ複合語（`frontend` = front + end）。単語そのものよりやや低い
            0.85
        } else if features.raw_count < 2 {
            // 1 文字だけの取りこぼしは打ち間違いの可能性が高い
            (0.2 + 0.9 * features.raw_ratio()).min(0.3)
        } else {
            let s = 0.2 + 0.9 * features.raw_ratio();
            if self.lexicon.is_some() {
                // 語彙にない語（技術用語・固有名詞・活用形）は確証がない
                s.min(HIGH_THRESHOLD - 0.01)
            } else {
                s
            }
        };
        let score = score.clamp(0.0, 1.0);
        let level = if score >= HIGH_THRESHOLD {
            Level::High
        } else if score >= MID_THRESHOLD {
            Level::Mid
        } else if score >= 0.3 {
            Level::Low
        } else {
            Level::None
        };
        Verdict {
            score,
            level,
            features,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::LexiconBuilder;

    fn lexicon(words: &[(&str, u8)]) -> Arc<Lexicon> {
        let mut b = LexiconBuilder::new();
        for (w, t) in words {
            b.add(w, *t);
        }
        Arc::new(Lexicon::from_vec(b.build().unwrap()).unwrap())
    }

    #[test]
    fn lexicon_word_with_leftover_is_high() {
        let j = Judge::standard(Some(lexicon(&[("string", 10), ("typescript", 70)])));
        assert_eq!(j.judge("string", false).level, Level::High);
        assert_eq!(j.judge("typescript", false).level, Level::High);
    }

    #[test]
    fn valid_romaji_is_japanese_unless_only_lexicon_knows() {
        let j = Judge::standard(Some(lexicon(&[("sense", 10), ("lemon", 20), ("data", 10)])));
        // 辞書で引ける → 日本語優先
        assert_eq!(j.judge("lemon", true).level, Level::None);
        // 語彙にあるが引けない → 中程度
        assert_eq!(j.judge("data", false).level, Level::Mid);
        // 語彙にない有効なローマ字は常に日本語
        assert_eq!(j.judge("kyouha", false).level, Level::None);
        assert_eq!(j.judge("arigatou", false).score, 0.0);
    }

    #[test]
    fn compound_of_two_lexicon_words_is_high() {
        let j = Judge::standard(Some(lexicon(&[("front", 20), ("end", 10)])));
        assert_eq!(j.judge("frontend", false).level, Level::High);
        assert!(j.judge("frontx", false).level < Level::High);
    }

    #[test]
    fn unknown_word_is_estimated_from_features() {
        let with = Judge::standard(Some(lexicon(&[("hello", 10)])));
        let without = Judge::standard(None);
        // 語彙にない技術用語は、語彙があるときは Mid まで、無いときは特徴量のまま
        assert_eq!(with.judge("typescript", false).level, Level::Mid);
        assert!(without.judge("typescript", false).score >= MID_THRESHOLD);
        // 劣化動作では語彙に依存する判定は出ない
        assert_eq!(without.judge("lemon", false).level, Level::None);
    }

    #[test]
    fn short_or_non_letter_input_is_not_judged() {
        let j = Judge::standard(None);
        assert_eq!(j.judge("zx", false).level, Level::None);
        assert_eq!(j.judge("ab1xyz", false).level, Level::None);
        assert_eq!(j.judge("Typescript", false).level, Level::None);
        assert_eq!(j.judge("", false).level, Level::None);
    }

    #[test]
    fn a_single_stray_letter_is_a_typo_not_english() {
        let j = Judge::standard(None);
        assert_eq!(j.judge("kyouhaxtenki", false).level, Level::None);
    }
}
