//! `sudachi_pos_to_mozc_like` が `Pos::parse` に正しく解釈されることの確認（実データ不要）

use nagi_dict::pos::{Pos, PosClass, PosSub, sudachi_pos_to_mozc_like};

#[test]
fn symbol_period() {
    let def = sudachi_pos_to_mozc_like("補助記号", "句点");
    let pos = Pos::parse(&def);
    assert_eq!(pos.class, PosClass::Symbol);
    assert_eq!(pos.sub, PosSub::Period);
}

#[test]
fn symbol_comma() {
    let def = sudachi_pos_to_mozc_like("補助記号", "読点");
    let pos = Pos::parse(&def);
    assert_eq!(pos.class, PosClass::Symbol);
    assert_eq!(pos.sub, PosSub::Comma);
}

#[test]
fn verb_general() {
    let def = sudachi_pos_to_mozc_like("動詞", "一般");
    let pos = Pos::parse(&def);
    assert_eq!(pos.class, PosClass::Verb);
    assert_eq!(pos.sub, PosSub::Other);
}

#[test]
fn verb_dependent() {
    let def = sudachi_pos_to_mozc_like("動詞", "非自立可能");
    let pos = Pos::parse(&def);
    assert_eq!(pos.class, PosClass::Verb);
    assert_eq!(pos.sub, PosSub::Dependent);
}

#[test]
fn noun_suffix() {
    let def = sudachi_pos_to_mozc_like("接尾辞", "名詞的");
    let pos = Pos::parse(&def);
    assert_eq!(pos.class, PosClass::Noun);
    assert_eq!(pos.sub, PosSub::Suffix);
}

#[test]
fn pronoun_is_noun() {
    let def = sudachi_pos_to_mozc_like("代名詞", "*");
    let pos = Pos::parse(&def);
    assert_eq!(pos.class, PosClass::Noun);
}
