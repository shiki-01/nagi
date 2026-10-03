//! `Dictionary::convert_scored`（総コストと未知語の文字数）のテスト。

use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 記号,句点,*,*,*,*,。
4 記号,読点,*,*,*,*,、
5 記号,括弧開,*,*,*,*,*
6 記号,括弧閉,*,*,*,*,*
7 記号,一般,*,*,*,*,*
";

const WORDS: &str = "\
にほん\t1\t1\t100\t日本
ご\t1\t1\t200\t語
";

fn dict() -> Dictionary {
    const N: usize = 8;
    let mut lines = vec![N.to_string()];
    lines.extend(std::iter::repeat_n("0".to_string(), N * N));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text("test", WORDS).unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    b.build().unwrap()
}

#[test]
fn scored_matches_convert_with_and_reports_cost() {
    let d = dict();
    let s = d.convert_scored("にほんご", None);
    assert_eq!(s.segments, d.convert_with("にほんご", None));
    assert_eq!(s.total_chars, 4);
    assert_eq!(s.unknown_chars, 0);
    assert_eq!(s.cost, 300);
    let surfaces: String = s.segments.iter().map(|x| x.surface.as_str()).collect();
    assert_eq!(surfaces, "日本語");
}

#[test]
fn unknown_characters_are_counted_and_cost_more() {
    let d = dict();
    let good = d.convert_scored("にほん", None);
    let bad = d.convert_scored("にhん", None);
    assert_eq!(bad.total_chars, 3);
    assert_eq!(bad.unknown_chars, 3);
    assert!(bad.cost > good.cost);
    // 空
    let e = d.convert_scored("", None);
    assert!(e.segments.is_empty());
    assert_eq!((e.cost, e.unknown_chars, e.total_chars), (0, 0, 0));
}
