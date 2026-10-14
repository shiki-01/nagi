import XCTest

/// Mozc TSV の解析・書き出し・検証（`Shared/MozcTSV.swift`。core/dict/src/user.rs と同じ規則）
final class MozcTSVTests: XCTestCase {
    private func entry(_ reading: String, _ surface: String, _ pos: String = "名詞", _ comment: String = "") -> NagiUserDictEntry {
        NagiUserDictEntry(reading: reading, surface: surface, pos: pos, comment: comment)
    }

    // MARK: 文字コード

    private func utf16(_ text: String, littleEndian: Bool, bom: Bool = true) -> Data {
        var bytes: [UInt8] = []
        func put(_ unit: UInt16) {
            let hi = UInt8(unit >> 8)
            let lo = UInt8(unit & 0xFF)
            bytes += littleEndian ? [lo, hi] : [hi, lo]
        }
        if bom { put(0xFEFF) }
        for unit in text.utf16 { put(unit) }
        return Data(bytes)
    }

    func testDecodePlainUTF8() {
        XCTAssertEqual(NagiMozcTSV.decodeText(Data("あ\tい".utf8)), "あ\tい")
    }

    func testDecodeUTF8BOMIsStripped() {
        XCTAssertEqual(NagiMozcTSV.decodeText(Data([0xEF, 0xBB, 0xBF] + Array("あ".utf8))), "あ")
    }

    func testDecodeUTF16LittleEndianWithBOM() {
        let text = "きょう\t今日\t名詞\n"
        XCTAssertEqual(NagiMozcTSV.decodeText(utf16(text, littleEndian: true)), text)
    }

    func testDecodeUTF16BigEndianWithBOM() {
        let text = "きょう\t今日\t名詞\n"
        XCTAssertEqual(NagiMozcTSV.decodeText(utf16(text, littleEndian: false)), text)
    }

    /// サロゲートペア（𠮷 など）も UTF-16 から戻る
    func testDecodeUTF16SurrogatePair() {
        let text = "よし\t𠮷\t人名\n"
        XCTAssertEqual(NagiMozcTSV.decodeText(utf16(text, littleEndian: true)), text)
    }

    /// BOM が無い UTF-16 は判別しない（UTF-8 として読む。落ちない）
    func testDecodeUTF16WithoutBOMDoesNotCrash() {
        _ = NagiMozcTSV.decodeText(utf16("あ", littleEndian: true, bom: false))
    }

    /// 末尾の半端な 1 バイトは捨てる
    func testDecodeUTF16OddTrailingByte() {
        var data = utf16("あ", littleEndian: true)
        data.append(0x42)
        XCTAssertEqual(NagiMozcTSV.decodeText(data), "あ")
    }

    func testDecodeEmpty() {
        XCTAssertEqual(NagiMozcTSV.decodeText(Data()), "")
    }

    // MARK: 解析

    func testParseBasicLine() {
        let result = NagiMozcTSV.parse("きょう\t今日\t名詞\tメモ\n", file: "a.tsv")
        XCTAssertTrue(result.issues.isEmpty)
        XCTAssertEqual(result.entries.count, 1)
        XCTAssertEqual(result.entries[0].reading, "きょう")
        XCTAssertEqual(result.entries[0].surface, "今日")
        XCTAssertEqual(result.entries[0].pos, "名詞")
        XCTAssertEqual(result.entries[0].comment, "メモ")
        XCTAssertEqual(result.entries[0].line, 1)
    }

    func testParseWithoutCommentColumn() {
        let result = NagiMozcTSV.parse("あ\t亜\t名詞", file: "a.tsv")
        XCTAssertEqual(result.entries.count, 1)
        XCTAssertEqual(result.entries[0].comment, "")
    }

    func testParseConvertsKatakanaReadingToHiragana() {
        let result = NagiMozcTSV.parse("キョウ\t今日\t名詞\nヴァイオリン\t楽器\t名詞\n", file: "a.tsv")
        XCTAssertEqual(result.entries.map { $0.reading }, ["きょう", "ゔぁいおりん"])
    }

    /// 長音「ー」はそのまま
    func testToHiraganaKeepsLongVowelMark() {
        XCTAssertEqual(NagiMozcTSV.toHiragana("ラーメン"), "らーめん")
    }

    func testParseSkipsCommentsAndBlankLines() {
        let text = "# コメント\n! これも\n\n   \nあ\t亜\t名詞\n"
        let result = NagiMozcTSV.parse(text, file: "a.tsv")
        XCTAssertEqual(result.entries.count, 1)
        XCTAssertTrue(result.issues.isEmpty)
        // 行番号は元のファイルでの位置（1 始まり）
        XCTAssertEqual(result.entries[0].line, 5)
    }

    func testParseHandlesCRLF() {
        let result = NagiMozcTSV.parse("あ\t亜\t名詞\r\nい\t異\t名詞\r\n", file: "a.tsv")
        XCTAssertEqual(result.entries.map { $0.surface }, ["亜", "異"])
        XCTAssertEqual(result.entries.map { $0.pos }, ["名詞", "名詞"])
    }

    func testParseTrimsColumns() {
        let result = NagiMozcTSV.parse("  あ  \t  亜  \t  名詞  \t  メモ  \n", file: "a.tsv")
        XCTAssertEqual(result.entries[0].reading, "あ")
        XCTAssertEqual(result.entries[0].surface, "亜")
        XCTAssertEqual(result.entries[0].pos, "名詞")
        XCTAssertEqual(result.entries[0].comment, "メモ")
    }

    func testParseInvalidLinesAreReportedWithLineNumbers() {
        let text = "あ\t亜\t名詞\n列が足りない\t単語\n\t空の読み\t名詞\nい\t\t名詞\nう\t宇\t名詞\n"
        let result = NagiMozcTSV.parse(text, file: "x.tsv")
        XCTAssertEqual(result.entries.map { $0.reading }, ["あ", "う"])
        XCTAssertEqual(result.issues.map { $0.line }, [2, 3, 4])
        XCTAssertTrue(result.issues.allSatisfy { $0.file == "x.tsv" })
        XCTAssertTrue(result.issues[0].text.contains("2 行目"))
    }

    /// スニペットの本文は、前後の空白を保ち、`\n` `\t` `\\` を解釈する
    func testParseSnippetUnescapes() {
        let result = NagiMozcTSV.parse("めーる\t お世話に\\nなります \tスニペット\n", file: "a.tsv")
        XCTAssertEqual(result.entries.count, 1)
        XCTAssertTrue(result.entries[0].isSnippet)
        XCTAssertEqual(result.entries[0].surface, " お世話に\nなります ")
    }

    func testUnescape() {
        XCTAssertEqual(NagiMozcTSV.unescape("a\\nb\\tc\\\\d"), "a\nb\tc\\d")
        // 未知のエスケープと末尾の単独の `\` はそのまま
        XCTAssertEqual(NagiMozcTSV.unescape("a\\xb"), "a\\xb")
        XCTAssertEqual(NagiMozcTSV.unescape("end\\"), "end\\")
    }

    func testEscape() {
        XCTAssertEqual(NagiMozcTSV.escape("a\nb\tc\\d"), "a\\nb\\tc\\\\d")
    }

    func testEscapeUnescapeRoundTrip() {
        for text in ["", "plain", "\n\n", "\\n", "a\\\nb", "\t\\\t", "日本語\n二行目\\"] {
            XCTAssertEqual(NagiMozcTSV.unescape(NagiMozcTSV.escape(text)), text, text)
        }
    }

    // MARK: 書き出し

    func testSerialize() {
        let out = NagiMozcTSV.serialize([entry("あ", "亜", "名詞", "メモ"), entry("い", "異")])
        XCTAssertEqual(out, "あ\t亜\t名詞\tメモ\nい\t異\t名詞\t\n")
    }

    func testSerializeSnippetEscapesNewlines() {
        let out = NagiMozcTSV.serialize([entry("めーる", "一行目\n二行目", NagiMozcTSV.snippetPos)])
        XCTAssertEqual(out, "めーる\t一行目\\n二行目\tスニペット\t\n")
    }

    func testSerializeEmpty() {
        XCTAssertEqual(NagiMozcTSV.serialize([]), "")
    }

    func testSerializeParseRoundTrip() {
        let entries = [
            entry("あ", "亜", "名詞", "メモ"),
            entry("めーる", "お世話になっております。\n\t署名\\", NagiMozcTSV.snippetPos, "挨拶"),
            entry("にほん", "日本", "地名"),
        ]
        let parsed = NagiMozcTSV.parse(NagiMozcTSV.serialize(entries), file: "x.tsv")
        XCTAssertTrue(parsed.issues.isEmpty)
        XCTAssertEqual(parsed.entries.map { $0.reading }, entries.map { $0.reading })
        XCTAssertEqual(parsed.entries.map { $0.surface }, entries.map { $0.surface })
        XCTAssertEqual(parsed.entries.map { $0.pos }, entries.map { $0.pos })
        XCTAssertEqual(parsed.entries.map { $0.comment }, entries.map { $0.comment })
    }

    // MARK: 重複

    func testDuplicateKey() {
        XCTAssertEqual(entry("あ", "亜").duplicateKey, entry("あ", "亜", "地名", "別のコメント").duplicateKey)
        XCTAssertNotEqual(entry("あ", "亜").duplicateKey, entry("あ", "阿").duplicateKey)
        XCTAssertNotEqual(entry("あ", "亜").duplicateKey, entry("い", "亜").duplicateKey)
        // スニペットは語と別枠
        XCTAssertNotEqual(entry("あ", "亜").duplicateKey, entry("あ", "亜", NagiMozcTSV.snippetPos).duplicateKey)
    }

    func testParsedDuplicatesShareKey() {
        let parsed = NagiMozcTSV.parse("あ\t亜\t名詞\nア\t亜\t地名\n", file: "a.tsv")
        XCTAssertEqual(parsed.entries.count, 2)
        XCTAssertEqual(parsed.entries[0].duplicateKey, parsed.entries[1].duplicateKey)
    }

    // MARK: 検証

    func testValidateNormalizes() {
        var e = entry("  キョウ ", "  今日  ", " 名詞 ", " メモ ")
        XCTAssertNil(NagiMozcTSV.validate(&e))
        XCTAssertEqual(e.reading, "きょう")
        XCTAssertEqual(e.surface, "今日")
        XCTAssertEqual(e.pos, "名詞")
        XCTAssertEqual(e.comment, "メモ")
    }

    func testValidateRejectsEmptyFields() {
        var noReading = entry("  ", "亜")
        XCTAssertNotNil(NagiMozcTSV.validate(&noReading))
        var noSurface = entry("あ", "  ")
        XCTAssertNotNil(NagiMozcTSV.validate(&noSurface))
        var noPos = entry("あ", "亜", "")
        XCTAssertNotNil(NagiMozcTSV.validate(&noPos))
    }

    func testValidateRejectsNonHiraganaReading() {
        for reading in ["今日", "abc", "あ い", "あ\u{3000}い", "あ1"] {
            var e = entry(reading, "亜")
            let error = NagiMozcTSV.validate(&e)
            XCTAssertNotNil(error, reading)
            XCTAssertTrue(error?.contains("ひらがな") ?? false, reading)
        }
    }

    func testValidateAllowsLongVowelMark() {
        var e = entry("らーめん", "ラーメン")
        XCTAssertNil(NagiMozcTSV.validate(&e))
    }

    func testValidateRejectsControlCharactersInWord() {
        for surface in ["a\tb", "a\nb", "a\rb"] {
            var e = entry("あ", surface)
            XCTAssertNotNil(NagiMozcTSV.validate(&e), surface)
        }
        var badPos = entry("あ", "亜", "名\t詞")
        XCTAssertNotNil(NagiMozcTSV.validate(&badPos))
        var badComment = entry("あ", "亜", "名詞", "a\tb")
        XCTAssertNotNil(NagiMozcTSV.validate(&badComment))
    }

    func testValidateWordLengthLimit() {
        var atLimit = entry("あ", String(repeating: "亜", count: NagiMozcTSV.maxWordChars))
        XCTAssertNil(NagiMozcTSV.validate(&atLimit))
        var over = entry("あ", String(repeating: "亜", count: NagiMozcTSV.maxWordChars + 1))
        XCTAssertNotNil(NagiMozcTSV.validate(&over))
        var longReading = entry(String(repeating: "あ", count: NagiMozcTSV.maxWordChars + 1), "亜")
        XCTAssertNotNil(NagiMozcTSV.validate(&longReading))
    }

    func testValidateSnippet() {
        var ok = entry("めーる", "一行目\r\n二行目\r三行目", NagiMozcTSV.snippetPos)
        XCTAssertNil(NagiMozcTSV.validate(&ok))
        // 改行は LF にそろう
        XCTAssertEqual(ok.surface, "一行目\n二行目\n三行目")

        var blank = entry("めーる", " \n ", NagiMozcTSV.snippetPos)
        XCTAssertNotNil(NagiMozcTSV.validate(&blank))

        var atLimit = entry("めーる", String(repeating: "あ", count: NagiMozcTSV.maxSnippetChars), NagiMozcTSV.snippetPos)
        XCTAssertNil(NagiMozcTSV.validate(&atLimit))
        var over = entry("めーる", String(repeating: "あ", count: NagiMozcTSV.maxSnippetChars + 1), NagiMozcTSV.snippetPos)
        XCTAssertNotNil(NagiMozcTSV.validate(&over))
    }

    /// 失敗したときは入力を書き換えない
    func testValidateFailureLeavesEntryUnchanged() {
        var e = entry("カナ今日", "  亜  ")
        XCTAssertNotNil(NagiMozcTSV.validate(&e))
        XCTAssertEqual(e.reading, "カナ今日")
        XCTAssertEqual(e.surface, "  亜  ")
    }
}
