//! 文全体 N-best（`Dictionary::convert_n_best`）のテスト

use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};

// 品詞 ID: 0 = BOS/EOS, 1 = 名詞,一般（"今日"・"橋"・"箸"）, 2 = 助詞,係助詞（"は"）
const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
";

/// 読み "はし" に表記の異なる2つの語（"橋": 低コスト、"箸": 高コスト）を登録する。
/// 読み "きょう" → "今日"、読み "は" → "は"（助詞）も登録し、複数文節の入力でも
/// N-best が機能することを検査できるようにする。
fn dict() -> Dictionary {
    let n = 3;
    let mut cost = vec![100u32; n * n];
    // BOS→名詞、名詞→名詞、名詞→助詞、名詞→EOS、助詞→名詞、助詞→EOS を無コストにする
    for (rid, lid) in [(0, 1), (1, 1), (1, 2), (1, 0), (2, 1), (2, 0)] {
        cost[rid * n + lid] = 0;
    }
    let mut lines = vec![n.to_string()];
    lines.extend(cost.iter().map(u32::to_string));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text(
        "inline",
        "はし\t1\t1\t100\t橋
はし\t1\t1\t200\t箸
きょう\t1\t1\t100\t今日
は\t2\t2\t100\tは
",
    )
    .unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    b.build().unwrap()
}

fn surfaces(candidates: &[nagi_dict::SentenceCandidate]) -> Vec<String> {
    candidates
        .iter()
        .map(|c| c.segments.iter().map(|s| s.surface.as_str()).collect())
        .collect()
}

#[test]
fn convert_n_best_returns_candidates_in_ascending_cost_order() {
    let dict = dict();
    let candidates = dict.convert_n_best("はし", 5, None);
    assert!(candidates.len() >= 2, "{candidates:?}");
    let surfaces = surfaces(&candidates);
    // 1番目がコストの低い方（意図した表記）であること
    assert_eq!(surfaces[0], "橋");
}

#[test]
fn convert_n_best_respects_k_limit() {
    let dict = dict();
    let candidates = dict.convert_n_best("はし", 1, None);
    assert_eq!(candidates.len(), 1);
}

#[test]
fn convert_n_best_deduplicates_identical_surfaces() {
    let dict = dict();
    let candidates = dict.convert_n_best("はし", 10, None);
    let surfaces = surfaces(&candidates);
    let unique: std::collections::HashSet<&String> = surfaces.iter().collect();
    assert_eq!(surfaces.len(), unique.len(), "{surfaces:?}");
}

#[test]
fn convert_n_best_zero_k_or_empty_reading_returns_empty() {
    let dict = dict();
    assert!(dict.convert_n_best("はし", 0, None).is_empty());
    assert!(dict.convert_n_best("", 5, None).is_empty());
}

#[test]
fn convert_n_best_splits_into_bunsetsu_like_single_best() {
    let dict = dict();
    // convert() (1-best) と同じ文節区切りルールが N-best の1位にも適用されること
    let best = dict.convert("きょうははし"); // "今日は" + "橋"（"は"は助詞なので前の文節に付く）
    let n_best = dict.convert_n_best("きょうははし", 5, None);
    let best_surface: String = best.iter().map(|s| s.surface.as_str()).collect();
    let top_surface: String = n_best[0]
        .segments
        .iter()
        .map(|s| s.surface.as_str())
        .collect();
    assert_eq!(best_surface, top_surface);
}
