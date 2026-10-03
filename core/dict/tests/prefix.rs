//! `Dictionary::has_prefix` / `contains_reading` のテスト。

use nagi_dict::DictionaryBuilder;

#[test]
fn prefix_and_exact() {
    let mut b = DictionaryBuilder::new();
    b.set_pos_defs(vec!["名詞".to_string()]);
    b.set_connection(1, vec![0]);
    for r in ["にほん", "にほんご", "かんじ"] {
        b.add_entry(r, r, 0, 0, 0, 100);
    }
    let d = b.build().unwrap();
    assert!(d.has_prefix(""));
    assert!(d.has_prefix("に"));
    assert!(d.has_prefix("にほん"));
    assert!(d.has_prefix("にほんご"));
    assert!(!d.has_prefix("にほんごお"));
    assert!(!d.has_prefix("ほ"));
    assert!(d.contains_reading("にほん"));
    assert!(!d.contains_reading("にほ"));
    assert!(!d.contains_reading(""));
}
