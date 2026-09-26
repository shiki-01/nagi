//! 文節区切りの規則（暫定ヒューリスティック）。
//!
//! Viterbi の最良経路を、品詞（`id.def`）だけを見て文節に区切る。Mozc の `segmenter.def` は
//! 取得許可の範囲外なので使わない。基本は「自立語で文節が始まり、付属語（助詞・助動詞・接尾・
//! 非自立）は直前の文節に付く」。複合名詞（名詞+名詞）は 1 文節にまとめる。

use crate::pos::{Pos, PosClass, PosSub};

/// `cur` が新しい文節の先頭になるか。`prev` は直前の語（文頭なら None）
pub fn starts_bunsetsu(prev: Option<Pos>, cur: Pos) -> bool {
    let Some(prev) = prev else {
        return true;
    };
    // 接頭詞は直後の語と 1 文節を作る
    if prev.class == PosClass::Prefix {
        return false;
    }
    match cur.class {
        PosClass::Particle | PosClass::AuxVerb => false,
        PosClass::Symbol => matches!(cur.sub, PosSub::Other | PosSub::BracketOpen),
        PosClass::Noun => match cur.sub {
            PosSub::Suffix | PosSub::Dependent | PosSub::Special => false,
            // 名詞が連続する場合（複合名詞・姓+名・数の連続）は同一文節。ただし非自立名詞（こと 等）の後では区切る
            _ => !(prev.class == PosClass::Noun && prev.sub != PosSub::Dependent),
        },
        PosClass::Verb | PosClass::Adjective => {
            !matches!(cur.sub, PosSub::Dependent | PosSub::Suffix)
        }
        PosClass::Prefix
        | PosClass::Adverb
        | PosClass::Adnominal
        | PosClass::Conjunction
        | PosClass::Interjection
        | PosClass::Filler
        | PosClass::Other
        | PosClass::BosEos => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(def: &str) -> Pos {
        Pos::parse(def)
    }

    #[test]
    fn first_word_starts() {
        assert!(starts_bunsetsu(None, p("助詞,格助詞,一般,*,*,*,*")));
    }

    #[test]
    fn particle_and_auxiliary_attach() {
        let noun = p("名詞,一般,*,*,*,*,*");
        assert!(!starts_bunsetsu(Some(noun), p("助詞,係助詞,*,*,*,*,*")));
        assert!(!starts_bunsetsu(
            Some(noun),
            p("助動詞,*,*,*,特殊・デス,基本形,です")
        ));
    }

    #[test]
    fn independent_words_start() {
        let particle = p("助詞,係助詞,*,*,*,*,*");
        assert!(starts_bunsetsu(
            Some(particle),
            p("形容詞,自立,*,*,形容詞・イ段,基本形,いい")
        ));
        assert!(starts_bunsetsu(Some(particle), p("名詞,一般,*,*,*,*,*")));
        assert!(starts_bunsetsu(
            Some(particle),
            p("動詞,自立,*,*,五段・カ行促音便,基本形,行く")
        ));
        assert!(starts_bunsetsu(Some(particle), p("副詞,一般,*,*,*,*,*")));
    }

    #[test]
    fn dependent_and_suffix_attach() {
        let verb = p("動詞,自立,*,*,一段,連用形,食べる");
        assert!(!starts_bunsetsu(
            Some(verb),
            p("動詞,非自立,*,*,一段,基本形,いる")
        ));
        let noun = p("名詞,固有名詞,人名,姓,*,*,*");
        assert!(!starts_bunsetsu(Some(noun), p("名詞,接尾,人名,*,*,*,さん")));
        assert!(!starts_bunsetsu(Some(verb), p("名詞,非自立,一般,*,*,*,*")));
    }

    #[test]
    fn compound_nouns_stay_together() {
        let noun = p("名詞,一般,*,*,*,*,*");
        assert!(!starts_bunsetsu(Some(noun), p("名詞,サ変接続,*,*,*,*,*")));
        let num = p("名詞,数,*,*,*,*,*");
        assert!(!starts_bunsetsu(Some(num), p("名詞,数,*,*,*,*,*")));
        // 非自立名詞（こと）の後の名詞は新しい文節
        assert!(starts_bunsetsu(Some(p("名詞,非自立,一般,*,*,*,*")), noun));
    }

    #[test]
    fn prefix_binds_to_next() {
        let prefix = p("接頭詞,名詞接続,*,*,*,*,お");
        assert!(starts_bunsetsu(Some(p("助詞,格助詞,一般,*,*,*,*")), prefix));
        assert!(!starts_bunsetsu(Some(prefix), p("名詞,一般,*,*,*,*,*")));
    }

    #[test]
    fn symbols() {
        let noun = p("名詞,一般,*,*,*,*,*");
        assert!(!starts_bunsetsu(Some(noun), p("記号,句点,*,*,*,*,。")));
        assert!(!starts_bunsetsu(Some(noun), p("記号,読点,*,*,*,*,、")));
        assert!(!starts_bunsetsu(Some(noun), p("記号,括弧閉,*,*,*,*,*")));
        assert!(starts_bunsetsu(Some(noun), p("記号,括弧開,*,*,*,*,*")));
        assert!(starts_bunsetsu(Some(noun), p("記号,一般,*,*,*,*,*")));
    }
}
