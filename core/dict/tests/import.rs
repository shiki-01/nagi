//! 他 IME のユーザー辞書の取り込み（形式判定・文字コード・品詞の写像・スキップ）

use nagi_dict::{
    ImportError, ImportFormat, SkipReason, UserDictionary, UserEntry, decode_import,
    import_user_dict,
};

fn utf16le(text: &str) -> Vec<u8> {
    let mut out = vec![0xff, 0xfe];
    for u in text.encode_utf16() {
        out.extend(u.to_le_bytes());
    }
    out
}

fn utf16be(text: &str, bom: bool) -> Vec<u8> {
    let mut out = if bom { vec![0xfe, 0xff] } else { vec![] };
    for u in text.encode_utf16() {
        out.extend(u.to_be_bytes());
    }
    out
}

fn run(bytes: &[u8]) -> nagi_dict::ImportOutcome {
    import_user_dict(bytes, None, &UserDictionary::default()).unwrap()
}

fn triples(o: &nagi_dict::ImportOutcome) -> Vec<(&str, &str, &str)> {
    o.entries
        .iter()
        .map(|e| (e.reading.as_str(), e.surface.as_str(), e.pos.as_str()))
        .collect()
}

#[test]
fn mozc_tsv_utf8_with_bom_keeps_pos_comment_and_snippet() {
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend(
        "# コメント\nこうえん\t公苑\t地名\t備考\nサンプル\t見本\t固有名詞\nあいさつ\tこんにちは\\nよろしく\tスニペット\n"
            .as_bytes(),
    );
    let o = run(&bytes);
    assert_eq!(o.format, ImportFormat::MozcTsv);
    assert_eq!(
        triples(&o),
        vec![
            ("こうえん", "公苑", "地名"),
            ("さんぷる", "見本", "固有名詞"),
            ("あいさつ", "こんにちは\nよろしく", "スニペット"),
        ]
    );
    assert_eq!(o.entries[0].comment, "備考");
    assert_eq!(o.total, 3);
    assert!(o.skipped.is_empty());
}

#[test]
fn ms_ime_utf16le_with_header_maps_pos() {
    let text = "!Microsoft IME Dictionary Tool\r\n!Version: 98\r\n!Format:WORDLIST\r\n\
        やまだ\t山田\t人名\r\nしぶや\t渋谷\t地名\r\nはしる\t走る\tラ行五段\r\n\
        かわいい\t可愛い\t形容詞\r\nあいう\t愛雨\t短縮よみ\r\nとうきょう\t東京\t固有名詞\r\n";
    let o = run(&utf16le(text));
    assert_eq!(o.format, ImportFormat::MsIme);
    assert_eq!(
        triples(&o),
        vec![
            ("やまだ", "山田", "人名"),
            ("しぶや", "渋谷", "地名"),
            ("はしる", "走る", "動詞ラ行五段"),
            ("かわいい", "可愛い", "形容詞"),
            ("あいう", "愛雨", "名詞"),
            ("とうきょう", "東京", "固有名詞"),
        ]
    );
}

#[test]
fn verbs_keep_conjugation_when_known() {
    let text = "!Microsoft IME Dictionary Tool\nはしる\t走る\t動詞ラ行五段\nたべる\t食べる\t一段動詞\nする\t為る\tサ行変格動詞\nよむ\t読む\t動詞\n";
    let o = run(&utf16be(text, true));
    assert_eq!(o.format, ImportFormat::MsIme);
    assert_eq!(
        triples(&o),
        vec![
            ("はしる", "走る", "動詞ラ行五段"),
            ("たべる", "食べる", "動詞一段"),
            ("する", "為る", "動詞サ行変格"),
            ("よむ", "読む", "動詞"),
        ]
    );
}

#[test]
fn utf16_without_bom_is_detected() {
    let text = "ゆうびん\t郵便\t名詞\nにほん\t日本\t名詞\n";
    assert_eq!(decode_import(&utf16be(text, false)), text);
    let mut le = Vec::new();
    for u in text.encode_utf16() {
        le.extend(u.to_le_bytes());
    }
    assert_eq!(decode_import(&le), text);
}

#[test]
fn atok_in_shift_jis_utf8_and_utf16() {
    let text = "!!ATOK_TANGO_TEXT_HEADER_1\nさとう\t佐藤\t人名\nおおさか\t大阪\t地名\n";
    let sjis = encoding_rs::SHIFT_JIS.encode(text).0.into_owned();
    for bytes in [sjis, text.as_bytes().to_vec(), utf16le(text)] {
        let o = run(&bytes);
        assert_eq!(o.format, ImportFormat::Atok);
        assert_eq!(
            triples(&o),
            vec![("さとう", "佐藤", "人名"), ("おおさか", "大阪", "地名")]
        );
    }
}

#[test]
fn mac_plist_uses_shortcut_as_reading() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<array>
	<dict>
		<key>phrase</key>
		<string>Tom &amp; Jerry</string>
		<key>shortcut</key>
		<string>とむ</string>
	</dict>
	<dict>
		<key>phrase</key>
		<string>見本</string>
		<key>shortcut</key>
		<string>ミホン</string>
	</dict>
	<dict>
		<key>phrase</key>
		<string>読みなし</string>
		<key>shortcut</key>
		<string/>
	</dict>
	<dict>
		<key>phrase</key>
		<string/>
		<key>shortcut</key>
		<string>から</string>
	</dict>
</array>
</plist>
"#;
    let o = run(xml.as_bytes());
    assert_eq!(o.format, ImportFormat::MacPlist);
    assert_eq!(
        triples(&o),
        vec![("とむ", "Tom & Jerry", "名詞"), ("みほん", "見本", "名詞")]
    );
    let reasons: Vec<_> = o.skipped.iter().map(|s| (s.line, s.reason)).collect();
    assert_eq!(
        reasons,
        vec![
            (3, SkipReason::InvalidReading),
            (4, SkipReason::EmptySurface)
        ]
    );
    assert_eq!(o.total, 4);
}

#[test]
fn binary_plist_is_rejected() {
    let err = import_user_dict(b"bplist00\x01\x02", None, &UserDictionary::default());
    assert_eq!(err.unwrap_err(), ImportError::BinaryPlist);
}

#[test]
fn two_column_text() {
    let o = run("てすと\tテスト\nふつう\t普通\n".as_bytes());
    assert_eq!(o.format, ImportFormat::TwoColumn);
    assert_eq!(
        triples(&o),
        vec![("てすと", "テスト", "名詞"), ("ふつう", "普通", "名詞")]
    );
}

#[test]
fn skips_invalid_duplicate_and_existing_with_reasons() {
    let mut existing = UserDictionary::default();
    existing.entries.push(UserEntry {
        reading: "かんじ".into(),
        surface: "漢字".into(),
        pos: "名詞".into(),
        comment: String::new(),
    });
    let text = "ひらがな\t平仮名\t名詞\n\
        漢字\t読みが漢字\t名詞\n\
        こわれた行\n\
        からたん\t\t名詞\n\
        ひらがな\t平仮名\t名詞\n\
        カンジ\t漢字\t名詞\n\
        あたらしい\t新しい\t形容詞\n";
    let o = import_user_dict(text.as_bytes(), None, &existing).unwrap();
    assert_eq!(o.format, ImportFormat::MozcTsv);
    assert_eq!(
        triples(&o),
        vec![
            ("ひらがな", "平仮名", "名詞"),
            ("あたらしい", "新しい", "形容詞")
        ]
    );
    let skipped: Vec<_> = o.skipped.iter().map(|s| (s.line, s.reason)).collect();
    assert_eq!(
        skipped,
        vec![
            (2, SkipReason::InvalidReading),
            (3, SkipReason::MissingColumns),
            (4, SkipReason::EmptySurface),
            (5, SkipReason::DuplicateInFile),
            (6, SkipReason::DuplicateExisting),
        ]
    );
    assert_eq!(o.total, 7);
}

#[test]
fn format_hint_overrides_detection() {
    // 3 列に見えるが、2 列テキストとして読めば品詞は読まれない
    let o = import_user_dict(
        "あ\t亜\t人名\n".as_bytes(),
        Some(ImportFormat::TwoColumn),
        &UserDictionary::default(),
    )
    .unwrap();
    assert_eq!(triples(&o), vec![("あ", "亜", "名詞")]);
}

#[test]
fn empty_and_unrecognized_inputs_are_errors() {
    let d = UserDictionary::default();
    assert_eq!(
        import_user_dict(b"", None, &d).unwrap_err(),
        ImportError::Empty
    );
    assert_eq!(
        import_user_dict(b"!Microsoft IME Dictionary Tool\n", None, &d).unwrap_err(),
        ImportError::Empty
    );
    assert_eq!(
        import_user_dict("ただの文章です\n".as_bytes(), None, &d).unwrap_err(),
        ImportError::Unrecognized
    );
}
