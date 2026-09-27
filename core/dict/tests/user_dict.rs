//! ユーザー辞書（Mozc TSV 互換）・スニペット・オーバーレイのテスト

use nagi_dict::{
    ConnectionLayout, Dictionary, DictionaryBuilder, SNIPPET_POS, UserDictIssue, UserDictionary,
    decode_text,
};

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 名詞,固有名詞,人名,姓,*,*,*
4 動詞,自立,*,*,一段,基本形,*
";

fn dict() -> Dictionary {
    let n = 5;
    let mut cost = vec![100u32; n * n];
    for (rid, lid) in [
        (0, 1),
        (0, 3),
        (1, 2),
        (3, 2),
        (2, 4),
        (2, 1),
        (1, 0),
        (2, 0),
        (4, 0),
        (3, 0),
    ] {
        cost[rid * n + lid] = 0;
    }
    let mut lines = vec![n.to_string()];
    lines.extend(cost.iter().map(u32::to_string));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text(
        "inline",
        "\
ふじさわ\t3\t3\t6000\t藤沢
ふじさわ\t1\t1\t9000\t不二沢
は\t2\t2\t500\tは
いく\t4\t4\t3000\t行く
",
    )
    .unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    b.build().unwrap()
}

fn parse(text: &str) -> (UserDictionary, Vec<UserDictIssue>) {
    let mut d = UserDictionary::default();
    let mut issues = Vec::new();
    d.add_tsv("user.txt", text, &mut issues);
    (d, issues)
}

#[test]
fn parses_mozc_tsv_with_comments_and_normalization() {
    let (d, issues) = parse(
        "# コメント\n!Microsoft IME Dictionary Tool\n\nフジサワ\t藤澤\t人名\t同僚\r\nしのはら\t篠原\t姓\n",
    );
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(d.entries.len(), 2);
    assert_eq!(
        d.entries[0].reading, "ふじさわ",
        "カタカナの読みはひらがなにする"
    );
    assert_eq!(d.entries[0].surface, "藤澤");
    assert_eq!(d.entries[0].pos, "人名");
    assert_eq!(d.entries[0].comment, "同僚");
    assert_eq!(d.entries[1].comment, "");
    assert_eq!((d.word_count(), d.snippet_count()), (2, 0));
}

#[test]
fn bad_lines_are_reported_with_line_numbers_and_skipped() {
    let (d, issues) = parse("ok\tOK\t名詞\nbad line\n\t空の読み\t名詞\nfine\t表記\t名詞\n");
    assert_eq!(d.entries.len(), 2);
    let lines: Vec<usize> = issues.iter().map(|i| i.line).collect();
    assert_eq!(lines, [2, 3]);
    assert!(issues.iter().all(|i| i.file == "user.txt"));
}

#[test]
fn snippets_unescape_and_export_round_trips() {
    let text = format!(
        "ひなた\t拝啓\\n時下ますます\\tご清祥\\\\のこと\t{SNIPPET_POS}\tあいさつ\nかんじ\t漢字\t名詞\t\n"
    );
    let (d, issues) = parse(&text);
    assert!(issues.is_empty());
    assert!(d.entries[0].is_snippet());
    assert_eq!(d.entries[0].surface, "拝啓\n時下ますます\tご清祥\\のこと");
    assert_eq!((d.word_count(), d.snippet_count()), (1, 1));
    // エクスポート → 再読込で同じ
    let (again, issues) = parse(&d.to_mozc_tsv());
    assert!(issues.is_empty());
    assert_eq!(again, d);
}

#[test]
fn decodes_utf8_bom_and_utf16_exports() {
    let text = "よみ\t単語\t名詞\n";
    let mut utf8 = vec![0xef, 0xbb, 0xbf];
    utf8.extend_from_slice(text.as_bytes());
    assert_eq!(decode_text(&utf8), text);
    assert_eq!(decode_text(text.as_bytes()), text);
    let mut le = vec![0xff, 0xfe];
    le.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(decode_text(&le), text);
    let mut be = vec![0xfe, 0xff];
    be.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    assert_eq!(decode_text(&be), text);
    // MS-IME のエクスポート（UTF-16 LE + ヘッダ）をそのまま読める
    let ms = "!Microsoft IME Dictionary Tool\r\nふじさわ\t藤沢\t人名\t\r\n";
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(ms.encode_utf16().flat_map(u16::to_le_bytes));
    let (d, issues) = parse(&decode_text(&bytes));
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(d.entries[0].surface, "藤沢");
}

#[test]
fn load_dir_reads_txt_and_tsv_sorted_and_tolerates_missing_dir() {
    let dir = std::env::temp_dir().join(format!("nagi-user-dict-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("b.tsv"), "べー\tB\t名詞\n").unwrap();
    std::fs::write(dir.join("a.txt"), "えー\tA\t名詞\nbroken\n").unwrap();
    std::fs::write(dir.join("ignored.md"), "ちがう\t無視\t名詞\n").unwrap();
    let (d, issues) = UserDictionary::load_dir(&dir);
    let surfaces: Vec<_> = d.entries.iter().map(|e| e.surface.as_str()).collect();
    assert_eq!(
        surfaces,
        ["A", "B"],
        "ファイル名順、対象外の拡張子は読まない"
    );
    assert_eq!(issues.len(), 1);
    assert_eq!((issues[0].file.as_str(), issues[0].line), ("a.txt", 2));
    let _ = std::fs::remove_dir_all(&dir);
    let (empty, issues) = UserDictionary::load_dir(&dir);
    assert!(empty.entries.is_empty() && issues.is_empty());
}

#[test]
fn overlay_words_win_in_conversion_and_do_not_touch_the_base_dictionary() {
    let d = dict();
    let plain = d.convert("ふじさわはいく");
    assert_eq!(plain[0].surface, "藤沢は");

    let (user, _) = parse("ふじさわ\t藤澤\t人名\n");
    let overlay = d.make_overlay(&user);
    assert_eq!((overlay.word_count(), overlay.snippet_count()), (1, 0));
    assert_eq!(overlay.lookup("ふじさわ")[0].surface, "藤澤");
    let with = d.convert_with("ふじさわはいく", Some(&overlay));
    assert_eq!(with[0].surface, "藤澤は", "ユーザー語が優先される");
    // オーバーレイ無しの変換は変わらない
    assert_eq!(d.convert("ふじさわはいく")[0].surface, "藤沢は");
    // 文節候補にも反映
    let c = d.segment_candidates_with("ふじさわ", 5, Some(&overlay));
    assert_eq!(c[0].surface, "藤澤");
}

#[test]
fn overlay_pos_mapping_uses_system_dictionary_ids() {
    let d = dict();
    let (user, _) = parse("しのはら\t篠原\t姓\nみち\t道\tなぞの品詞\n");
    let overlay = d.make_overlay(&user);
    // 姓 → 名詞,固有名詞,人名,姓（id 3）。未知の品詞 → 名詞,一般（id 1）
    assert_eq!(overlay.lookup("しのはら")[0].lid, 3);
    assert_eq!(overlay.lookup("みち")[0].lid, 1);
    assert_eq!(overlay.max_reading_chars(), 4);
}

#[test]
fn snippets_are_indexed_separately_and_not_added_to_the_lattice() {
    let d = dict();
    let (user, _) = parse(&format!("ひなた\t拝啓\\n敬具\t{SNIPPET_POS}\n"));
    let overlay = d.make_overlay(&user);
    assert_eq!(overlay.snippets("ひなた"), ["拝啓\n敬具"]);
    assert!(overlay.lookup("ひなた").is_empty());
    assert_eq!(overlay.max_reading_chars(), 0);
    assert_eq!(overlay.snippet_count(), 1);
    let segs = d.convert_with("ひなた", Some(&overlay));
    assert_eq!(
        segs.iter().map(|s| s.surface.as_str()).collect::<String>(),
        "ひなた"
    );
}

#[test]
fn overlay_lookup_prefix_matches_forward_only() {
    let d = dict();
    let (user, _) = parse("ふじさわ\t藤澤\t人名\nふじさわ\t不二沢\t名詞\nふじ\t富士\t名詞\n");
    let overlay = d.make_overlay(&user);
    // "ふじ" は "ふじさわ" の前方一致
    let found = overlay.lookup_prefix("ふじ", 10);
    let readings: Vec<&str> = found.iter().map(|(r, _)| *r).collect();
    assert!(readings.contains(&"ふじさわ"));
    // 完全一致（"ふじ" 自身）は除外される
    assert!(!readings.contains(&"ふじ"));
    // limit が効く（"ふじさわ" には2件登録されているが1件だけ返す）
    assert_eq!(overlay.lookup_prefix("ふじ", 1).len(), 1);
    // 前方一致しない読みは含まれない
    assert!(overlay.lookup_prefix("さわ", 10).is_empty());
}

#[test]
fn align_with_overlay_uses_user_words() {
    let d = dict();
    let (user, _) = parse("ふじさわ\t藤澤\t人名\n");
    let overlay = d.make_overlay(&user);
    let segs = d
        .align_with("ふじさわはいく", "藤澤は行く", Some(&overlay))
        .unwrap();
    assert_eq!(
        segs.iter()
            .map(|s| (s.reading.as_str(), s.surface.as_str()))
            .collect::<Vec<_>>(),
        [("ふじさわは", "藤澤は"), ("いく", "行く")]
    );
    // オーバーレイ無しでも未知の連続部分として整列できる（読みの割り当ては同じになるとは限らない）
    assert!(d.align("ふじさわはいく", "藤澤は行く").is_some());
}
