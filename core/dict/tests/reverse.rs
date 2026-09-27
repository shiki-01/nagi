//! 確定済み表記からの読み推定（逆変換、`reconvert` 用）のテスト

use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 形容詞,自立,*,*,形容詞・イ段,基本形,いい
";

/// 以下を登録した合成辞書:
///   - 読み "きょう" → 表記 "今日"（名詞。低コスト）
///   - 読み "こんにち" → 表記 "今日"（名詞。高コスト。"きょう" とのコスト比較用）
///   - 読み "は" → 表記 "は"（助詞。名詞に付いて 1 文節になる）
///   - 読み "てんき" → 表記 "天気"（名詞）
///   - 読み "いい" → 表記 "いい"（形容詞・自立語。新しい文節を開始する）
fn dict() -> Dictionary {
    let n = 4;
    let mut cost = vec![100u32; n * n];
    for (rid, lid) in [
        (0, 1), // BOS → 名詞
        (0, 3), // BOS → 形容詞
        (1, 2), // 名詞 → 助詞
        (1, 0), // 名詞 → EOS
        (2, 3), // 助詞 → 形容詞
        (2, 0), // 助詞 → EOS
        (3, 1), // 形容詞 → 名詞
        (3, 0), // 形容詞 → EOS
    ] {
        cost[rid * n + lid] = 0;
    }
    let mut lines = vec![n.to_string()];
    lines.extend(cost.iter().map(u32::to_string));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text(
        "inline",
        "\
きょう\t1\t1\t3000\t今日
こんにち\t1\t1\t8000\t今日
は\t2\t2\t500\tは
てんき\t1\t1\t3000\t天気
いい\t3\t3\t2000\tいい
",
    )
    .unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    b.build().unwrap()
}

#[test]
fn reverse_convert_estimates_reading_from_known_surface() {
    let dict = dict();
    let segments = dict.reverse_convert("今日は");
    let readings: Vec<&str> = segments.iter().map(|s| s.reading.as_str()).collect();
    let surfaces: Vec<&str> = segments.iter().map(|s| s.surface.as_str()).collect();
    // 文節区切り（名詞+助詞は1文節）と、読みが正しく推定されることを確認
    assert_eq!(surfaces, ["今日は"]);
    assert_eq!(readings, ["きょうは"]);
}

#[test]
fn reverse_convert_prefers_lower_cost_reading_for_ambiguous_surface() {
    let dict = dict();
    let segments = dict.reverse_convert("今日");
    assert_eq!(segments[0].reading, "きょう"); // コストの低い方が選ばれる
}

#[test]
fn reverse_convert_splits_into_bunsetsu() {
    let dict = dict();
    let segments = dict.reverse_convert("今日はいい天気");
    let surfaces: Vec<&str> = segments.iter().map(|s| s.surface.as_str()).collect();
    assert_eq!(surfaces, ["今日は", "いい", "天気"]);
}

#[test]
fn reverse_convert_falls_back_to_verbatim_reading_for_unknown_kanji() {
    let dict = dict();
    // 辞書に無い漢字（例えば "犬"）は読みを推定できないので、その文字自体が reading になる
    let segments = dict.reverse_convert("犬");
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].surface, "犬");
    assert_eq!(segments[0].reading, "犬"); // 正しい読みではないが、クラッシュせず通ることを確認
}

#[test]
fn reverse_convert_empty_surface_returns_empty() {
    let dict = dict();
    assert!(dict.reverse_convert("").is_empty());
}

#[test]
fn reverse_convert_is_cached_and_repeatable() {
    let dict = dict();
    // 2回呼んでも同じ結果になる（キャッシュされた索引が壊れていないことの確認）
    let a = dict.reverse_convert("今日は");
    let b = dict.reverse_convert("今日は");
    assert_eq!(a, b);
}
