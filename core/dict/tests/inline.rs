//! 小さなインライン辞書での単体テスト（実データ不要）

use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 動詞,自立,*,*,五段・カ行促音便,基本形,行く
4 助動詞,*,*,*,特殊・デス,基本形,です
5 記号,句点,*,*,*,*,。
6 記号,読点,*,*,*,*,、
7 記号,括弧開,*,*,*,*,*
8 記号,括弧閉,*,*,*,*,*
9 記号,一般,*,*,*,*,*
10 接頭詞,名詞接続,*,*,*,*,お
11 名詞,接尾,一般,*,*,*,さん
12 形容詞,自立,*,*,形容詞・イ段,基本形,いい
";

const N: usize = 13;

/// 連接コスト: 既定 100。自然な並び（名詞→助詞、形容詞→名詞 など）は 0 にして優遇する
fn connection_text(layout: ConnectionLayout) -> String {
    let mut cost = vec![100u32; N * N];
    let mut set = |rid: usize, lid: usize, c: u32| cost[rid * N + lid] = c;
    set(1, 2, 0); // 名詞 → 助詞
    set(1, 4, 0); // 名詞 → 助動詞
    set(12, 1, 0); // 形容詞 → 名詞
    set(2, 3, 0); // 助詞 → 動詞
    set(2, 1, 0); // 助詞 → 名詞
    set(0, 1, 0); // BOS → 名詞
    set(0, 12, 0); // BOS → 形容詞
    set(0, 3, 0); // BOS → 動詞
    set(0, 10, 0); // BOS → 接頭詞
    set(10, 1, 0); // 接頭詞 → 名詞
    set(1, 11, 0); // 名詞 → 接尾
    set(11, 2, 0); // 接尾 → 助詞
    set(1, 5, 0); // 名詞 → 句点
    set(2, 5, 0);
    set(2, 6, 0); // 助詞 → 読点
    set(6, 3, 0); // 読点 → 動詞
    set(4, 5, 0); // 助動詞 → 句点
    set(3, 5, 0);
    set(3, 1, 0); // 動詞 → 名詞
    set(5, 3, 0); // 句点 → 動詞
    set(1, 0, 0); // 名詞 → EOS
    set(4, 0, 0);
    set(5, 0, 0);
    set(3, 0, 0);
    set(2, 0, 0);
    let mut lines = vec![N.to_string()];
    for i in 0..N * N {
        let (a, b) = (i / N, i % N);
        // RidMajor: 行 = rid * N + lid、LidMajor: 行 = lid * N + rid
        let v = match layout {
            ConnectionLayout::RidMajor => cost[a * N + b],
            ConnectionLayout::LidMajor => cost[b * N + a],
        };
        lines.push(v.to_string());
    }
    lines.join("\n")
}

const WORDS: &str = "\
きょう\t1\t1\t3000\t今日
きょう\t1\t1\t5000\t京
きょう\t1\t1\t5000\t強
きょう\t1\t1\t9000\tきょう
は\t2\t2\t500\tは
は\t1\t1\t6000\t葉
いい\t12\t12\t2000\tいい
いい\t12\t12\t2500\t良い
てんき\t1\t1\t3000\t天気
です\t4\t4\t300\tです
いく\t3\t3\t3000\t行く
いく\t3\t3\t4000\t逝く
に\t2\t2\t400\tに
とうきょう\t1\t1\t3000\t東京
とう\t1\t1\t7000\t塔
きょうは\t1\t1\t20000\t今日は
お\t10\t10\t1000\tお
ちゃ\t1\t1\t3000\t茶
さん\t11\t11\t500\tさん
たなか\t1\t1\t3500\t田中
。\t5\t5\t100\t。
、\t6\t6\t100\t、
にほん\t1\t1\t3000\t日本
ご\t11\t11\t2500\t語
が\t2\t2\t400\tが
";

fn build(layout: ConnectionLayout) -> Dictionary {
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text("inline", WORDS).unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&connection_text(layout), layout)
        .unwrap();
    b.build().unwrap()
}

fn surfaces(d: &Dictionary, reading: &str) -> Vec<String> {
    d.convert(reading).into_iter().map(|s| s.surface).collect()
}

#[test]
fn lookup_is_sorted_by_cost() {
    let d = build(ConnectionLayout::RidMajor);
    let entries = d.lookup("きょう");
    let surfaces: Vec<_> = entries.iter().map(|e| e.surface.as_str()).collect();
    assert_eq!(surfaces, ["今日", "京", "強", "きょう"]);
    assert!(entries.windows(2).all(|w| w[0].cost <= w[1].cost));
    assert!(d.lookup("ぬ").is_empty());
}

#[test]
fn common_prefix_lookup_lists_short_to_long() {
    let d = build(ConnectionLayout::RidMajor);
    let r = d.common_prefix_lookup("とうきょうに");
    let lens: Vec<usize> = r.iter().map(|(len, _)| *len).collect();
    assert_eq!(lens, [6, 15]); // とう(6 バイト)、とうきょう(15 バイト)
    assert_eq!(r[0].1[0].surface, "塔");
    assert_eq!(r[1].1[0].surface, "東京");
}

#[test]
fn viterbi_prefers_lowest_total_cost() {
    let d = build(ConnectionLayout::RidMajor);
    // 「きょうは」は 1 語（20000）より 今日 + は（3000+500+接続）の方が安い
    assert_eq!(surfaces(&d, "きょうは"), ["今日は"]);
    let segs = d.convert("きょうはいいてんきです");
    let readings: Vec<_> = segs.iter().map(|s| s.reading.as_str()).collect();
    let surf: Vec<_> = segs.iter().map(|s| s.surface.as_str()).collect();
    // 形容詞 + 名詞は別文節（実データでは「いい」が連体詞扱いで 1 文節になる。文節規則は暫定）
    assert_eq!(readings, ["きょうは", "いい", "てんきです"]);
    assert_eq!(surf, ["今日は", "いい", "天気です"]);
}

#[test]
fn connection_layout_is_normalized() {
    // 同じ意味の連接コストを別の並びで与えても、同じ変換結果になる
    let a = build(ConnectionLayout::RidMajor);
    let b = build(ConnectionLayout::LidMajor);
    for s in ["きょうはいいてんきです", "とうきょうにいく", "おちゃ"] {
        assert_eq!(a.convert(s), b.convert(s), "{s}");
    }
}

#[test]
fn unknown_characters_pass_through() {
    let d = build(ConnectionLayout::RidMajor);
    assert_eq!(surfaces(&d, "ぬ"), ["ぬ"]);
    let segs = d.convert("きょうぬに");
    assert_eq!(
        segs.iter().map(|s| s.reading.as_str()).collect::<String>(),
        "きょうぬに"
    );
    assert!(
        segs.iter()
            .map(|s| s.surface.as_str())
            .collect::<String>()
            .contains('ぬ')
    );
    assert!(d.convert("").is_empty());
}

#[test]
fn punctuation_attaches_to_previous_segment() {
    let d = build(ConnectionLayout::RidMajor);
    let segs = d.convert("いくとうきょうに、いく。");
    let joined: String = segs.iter().map(|s| s.surface.as_str()).collect();
    assert_eq!(joined, "行く東京に、行く。");
    // 句読点は単独の文節にならない
    assert!(
        segs.iter().all(|s| s.surface != "、" && s.surface != "。"),
        "{segs:?}"
    );
}

#[test]
fn prefix_and_suffix_bind_within_segment() {
    let d = build(ConnectionLayout::RidMajor);
    // 接頭詞 お + 名詞 ちゃ、名詞 + 接尾 さん、名詞 + 接尾 ご は 1 文節
    assert_eq!(surfaces(&d, "おちゃ"), ["お茶"]);
    assert_eq!(surfaces(&d, "たなかさん"), ["田中さん"]);
    assert_eq!(surfaces(&d, "にほんご"), ["日本語"]);
}

#[test]
fn segment_candidates_are_ordered_and_unique() {
    let d = build(ConnectionLayout::RidMajor);
    let c = d.segment_candidates("きょうは", 10);
    let surf: Vec<_> = c.iter().map(|x| x.surface.as_str()).collect();
    assert_eq!(surf[0], "今日は");
    // 先頭語の別候補 + 据え置きの助詞
    assert!(surf.contains(&"京は"), "{surf:?}");
    let unique: std::collections::HashSet<_> = surf.iter().collect();
    assert_eq!(unique.len(), surf.len(), "重複がある: {surf:?}");
    assert!(d.segment_candidates("きょうは", 2).len() <= 2);
    assert!(d.segment_candidates("", 5).is_empty());
    assert!(d.segment_candidates("きょう", 0).is_empty());
    // 読み全体の完全一致も含まれる
    let whole: Vec<_> = d
        .segment_candidates("きょう", 10)
        .into_iter()
        .map(|x| x.surface)
        .collect();
    assert_eq!(whole, ["今日", "京", "強", "きょう"]);
}

#[test]
fn binary_round_trip_preserves_behavior() {
    let d = build(ConnectionLayout::RidMajor);
    let bytes = d.to_bytes();
    let loaded = Dictionary::from_bytes(&bytes).unwrap();
    assert_eq!(loaded.entry_count(), d.entry_count());
    assert_eq!(loaded.reading_count(), d.reading_count());
    for s in ["きょうはいいてんきです", "おちゃ", "ぬ"] {
        assert_eq!(loaded.convert(s), d.convert(s));
        assert_eq!(loaded.segment_candidates(s, 5), d.segment_candidates(s, 5));
    }
    assert_eq!(loaded.to_bytes(), bytes);
}

#[test]
fn corrupt_binaries_are_rejected() {
    let bytes = build(ConnectionLayout::RidMajor).to_bytes();
    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert!(Dictionary::from_bytes(&bad_magic).is_err());
    let mut bad_version = bytes.clone();
    bad_version[8] = 99;
    assert!(Dictionary::from_bytes(&bad_version).is_err());
    for cut in [0, 5, 12, 100, bytes.len() / 2, bytes.len() - 1] {
        assert!(Dictionary::from_bytes(&bytes[..cut]).is_err(), "cut={cut}");
    }
    assert!(Dictionary::from_bytes(&[]).is_err());
}

#[test]
fn duplicate_entries_keep_lowest_cost() {
    let mut b = DictionaryBuilder::new();
    b.add_entry("あ", "亜", 1, 1, 5000);
    b.add_entry("あ", "亜", 1, 1, 3000);
    b.add_entry("あ", "亜", 1, 1, 4000);
    b.add_entry("あ", "亜", 2, 2, 4000);
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(
        &connection_text(ConnectionLayout::RidMajor),
        ConnectionLayout::RidMajor,
    )
    .unwrap();
    let d = b.build().unwrap();
    let e = d.lookup("あ");
    assert_eq!(e.len(), 2);
    assert_eq!((e[0].lid, e[0].cost), (1, 3000));
    assert_eq!((e[1].lid, e[1].cost), (2, 4000));
}

#[test]
fn mozc_text_parse_errors_report_line_numbers() {
    let mut b = DictionaryBuilder::new();
    let err = b
        .add_dictionary_text("d.txt", "あ\t1\t1\t100\t亜\nい\t1\t1\n")
        .unwrap_err();
    assert!(err.to_string().contains("d.txt:2"), "{err}");
    let err = b
        .add_dictionary_text("d.txt", "あ\tx\t1\t100\t亜\n")
        .unwrap_err();
    assert!(err.to_string().contains("d.txt:1"), "{err}");
    let err = b
        .set_pos_defs_text("0 BOS/EOS,*\n2 名詞,一般\n")
        .unwrap_err();
    assert!(err.to_string().contains("id.def:2"), "{err}");
    let err = b
        .set_connection_text("2\n1\n2\n3\n", ConnectionLayout::RidMajor)
        .unwrap_err();
    assert!(err.to_string().contains("N×N"), "{err}");

    // 空行・CRLF は許容し、SPELLING_CORRECTION（6 列）は無視する
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text(
        "d.txt",
        "あ\t1\t1\t100\t亜\r\n\r\nい\t1\t1\t100\t井\tSPELLING_CORRECTION\n",
    )
    .unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(
        &connection_text(ConnectionLayout::RidMajor),
        ConnectionLayout::RidMajor,
    )
    .unwrap();
    let d = b.build().unwrap();
    assert_eq!(d.entry_count(), 1);
    assert_eq!(d.lookup("あ")[0].surface, "亜");
}

#[test]
fn build_requires_connection_and_pos() {
    let b = DictionaryBuilder::new();
    assert!(b.build().is_err());
}
