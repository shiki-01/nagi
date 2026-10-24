import XCTest

/// 実機確認用のテスト素材（`Shared/TestAids.swift`、`config/keyboard/test-layouts/`、`DiagLog` の概要整形）
final class TestAidsTests: XCTestCase {
    // MARK: ユーザー辞書のサンプル TSV

    func testUTF16LESampleHasBOMAndDecodesBack() {
        let data = NagiTestSamples.dictSampleData(.utf16LE)
        let bytes = [UInt8](data)
        XCTAssertEqual(Array(bytes.prefix(2)), [0xFF, 0xFE])
        XCTAssertEqual(bytes.count % 2, 0)
        // BOM の次は "!" (0x21 0x00)
        XCTAssertEqual(Array(bytes[2..<4]), [0x21, 0x00])
        XCTAssertEqual(NagiMozcTSV.decodeText(data), NagiTestSamples.msimeText)
    }

    func testUTF16LEDataEncodesNonBMP() {
        let data = NagiTestSamples.utf16LEData("😀")
        // BOM + サロゲートペア D83D DE00 (LE)
        XCTAssertEqual([UInt8](data), [0xFF, 0xFE, 0x3D, 0xD8, 0x00, 0xDE])
    }

    func testUTF8SampleParsesWithoutIssues() {
        let text = NagiMozcTSV.decodeText(NagiTestSamples.dictSampleData(.utf8))
        let parsed = NagiMozcTSV.parse(text, file: "sample")
        XCTAssertEqual(parsed.entries.count, 5)
        XCTAssertTrue(parsed.issues.isEmpty)
        let snippet = parsed.entries.first { $0.isSnippet }
        XCTAssertEqual(snippet?.surface, "お世話になっております。\n株式会社〇〇")
    }

    func testMSIMESampleParsesThreeEntries() {
        let text = NagiMozcTSV.decodeText(NagiTestSamples.dictSampleData(.utf16LE))
        let parsed = NagiMozcTSV.parse(text, file: "sample")
        XCTAssertEqual(parsed.entries.map { $0.surface }, ["テスト", "山田", "大阪"])
        XCTAssertEqual(parsed.entries.map { $0.pos }, ["名詞", "人名", "地名"])
        XCTAssertTrue(parsed.issues.isEmpty)
    }

    func testInvalidSampleHasExpectedIssues() {
        let parsed = NagiMozcTSV.parse(NagiTestSamples.invalidText, file: "sample")
        // 列不足（3 行目）と単語が空（5 行目）が読み飛ばされる
        XCTAssertEqual(parsed.issues.map { $0.line }, [3, 5])
        XCTAssertEqual(parsed.entries.count, 4)
        // カタカナの読みはひらがなに直る
        XCTAssertTrue(parsed.entries.contains { $0.reading == "かたかな" })
        // 読みに漢字を含む行は、取り込み時の検証で弾かれる
        var failing = parsed.entries.first { $0.line == 6 }
        XCTAssertNotNil(failing)
        if failing != nil {
            XCTAssertNotNil(NagiMozcTSV.validate(&failing!))
        }
        // それ以外は検証を通る
        let valid = parsed.entries.filter { entry in
            var copy = entry
            return NagiMozcTSV.validate(&copy) == nil
        }
        XCTAssertEqual(valid.count, 3)
    }

    func testDuplicatesSampleCollapsesToTwoKeys() {
        let parsed = NagiMozcTSV.parse(NagiTestSamples.duplicatesText, file: "sample")
        XCTAssertEqual(parsed.entries.count, 5)
        XCTAssertTrue(parsed.issues.isEmpty)
        XCTAssertEqual(Set(parsed.entries.map { $0.duplicateKey }).count, 2)
    }

    func testBulkTSVHasUniqueValidEntries() {
        var reported: [Double] = []
        let text = NagiTestSamples.bulkTSV(count: 500) { reported.append($0) }
        let parsed = NagiMozcTSV.parse(text, file: "bulk")
        XCTAssertEqual(parsed.entries.count, 500)
        XCTAssertTrue(parsed.issues.isEmpty)
        XCTAssertEqual(Set(parsed.entries.map { $0.duplicateKey }).count, 500)
        for entry in parsed.entries {
            var copy = entry
            XCTAssertNil(NagiMozcTSV.validate(&copy), entry.reading)
        }
        XCTAssertEqual(reported.last, 1)
        XCTAssertTrue(reported.count >= 2)
    }

    func testBulkReadingsAreHiraganaAndUnique() {
        XCTAssertEqual(NagiTestSamples.bulkReading(0), "てすとあ")
        XCTAssertEqual(NagiTestSamples.bulkReading(10), "てすといあ")
        let readings = Set((0..<5000).map { NagiTestSamples.bulkReading($0) })
        XCTAssertEqual(readings.count, 5000)
    }

    func testReadonlySampleParsesTwoEntries() {
        let parsed = NagiMozcTSV.parse(NagiTestSamples.readonlySampleText, file: NagiTestSamples.readonlySampleFileName)
        XCTAssertEqual(parsed.entries.count, 2)
        XCTAssertTrue(parsed.issues.isEmpty)
        XCTAssertNotEqual(NagiTestSamples.readonlySampleFileName, "nagi-user.tsv")
    }

    // MARK: テスト用レイアウト

    private func loadCase(_ testCase: NagiTestLayoutCase) throws -> Data {
        let url = try XCTUnwrap(NagiTestLayouts.url(for: testCase, in: TestSupport.bundle), "\(testCase.id).json がバンドルにない")
        return try Data(contentsOf: url)
    }

    /// 取り込み時と同じ検証（layout.target は組み込み名と自身のみ有効）。問題があれば理由
    private func rejectionReason(_ data: Data) -> String? {
        do {
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data)
            return def.validationError(targets: [])
        } catch {
            return LayoutLoader.describe(error)
        }
    }

    func testTestLayoutsMatchExpectations() throws {
        for testCase in NagiTestLayouts.cases {
            let reason = rejectionReason(try loadCase(testCase))
            switch testCase.expectation {
            case .success:
                XCTAssertNil(reason, "\(testCase.id)")
            case .error(let needle):
                guard let reason = reason else {
                    XCTFail("\(testCase.id) は拒否されるはず")
                    continue
                }
                XCTAssertTrue(reason.contains(needle), "\(testCase.id): 「\(needle)」が理由に含まれない: \(reason)")
            }
        }
    }

    func testValidTestLayoutsDeclareExpectedIdAndMode() throws {
        for testCase in NagiTestLayouts.cases where testCase.isValid {
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: try loadCase(testCase))
            XCTAssertEqual(def.id, testCase.layoutId, testCase.id)
            XCTAssertEqual(def.mode, testCase.assignMode, testCase.id)
            XCTAssertTrue(LayoutLoader.isSafeId(def.id))
        }
    }

    func testShortcutLayoutInsertsParenthesesAndMovesCursor() throws {
        let testCase = try XCTUnwrap(NagiTestLayouts.cases.first { $0.id == "valid-shortcut" })
        let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: try loadCase(testCase))
        let key = try XCTUnwrap(def.rows.flatMap { $0.keys }.first { $0.id == "paren" })
        XCTAssertEqual(key.steps?.first?.text, "（）")
        XCTAssertEqual(key.steps?.last?.action, "moveLeft")
    }

    func testEveryTestLayoutFileIsCataloged() {
        let dir = TestSupport.bundle.resourceURL?.appendingPathComponent(NagiTestLayouts.folder, isDirectory: true)
        let names = (dir.flatMap { try? FileManager.default.contentsOfDirectory(atPath: $0.path) } ?? [])
            .filter { $0.hasSuffix(".json") }
            .map { String($0.dropLast(5)) }
        XCTAssertEqual(Set(names), Set(NagiTestLayouts.cases.map { $0.id }))
    }

    func testHugeLayoutExceedsBothLimits() {
        let data = NagiTestLayouts.hugeLayoutJSON()
        XCTAssertGreaterThan(data.count, 1536 * 1024)
        let url = NagiLayoutURL.url(forJSON: data)
        XCTAssertNotNil(url)
        // base64 にすると URL の data の上限（1MB）も超える
        XCTAssertGreaterThan(data.count * 4 / 3, NagiLayoutURL.maxDataChars)
        if let url = url {
            XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: url))
        }
        // JSON としては正しい
        XCTAssertNotNil(try? JSONSerialization.jsonObject(with: data))
    }

    // MARK: 診断ログの概要

    func testLineCount() {
        XCTAssertEqual(DiagLog.lineCount(in: Data()), 0)
        XCTAssertEqual(DiagLog.lineCount(in: Data("a\n".utf8)), 1)
        XCTAssertEqual(DiagLog.lineCount(in: Data("a\nb\n".utf8)), 2)
        XCTAssertEqual(DiagLog.lineCount(in: Data("a\nb".utf8)), 2)
        XCTAssertEqual(DiagLog.lineCount(in: Data("\n".utf8)), 1)
    }

    func testSummaryText() {
        let utc = TimeZone(identifier: "UTC")!
        XCTAssertEqual(DiagLog.summaryText(.empty, timeZone: utc), "ログは空です")
        // 2026-01-02 03:04:05 UTC
        let date = Date(timeIntervalSince1970: 1_767_323_045)
        let small = DiagLog.Summary(lineCount: 3, byteCount: 512, modified: date)
        XCTAssertEqual(DiagLog.summaryText(small, timeZone: utc), "3 行 / 512 B / 最終更新 2026-01-02 03:04:05")
        let large = DiagLog.Summary(lineCount: 1200, byteCount: 65_536, modified: nil)
        XCTAssertEqual(DiagLog.summaryText(large, timeZone: utc), "1200 行 / 64.0 KB")
    }

    func testTailLines() {
        XCTAssertEqual(DiagLog.tailLines(of: "", maxLines: 3), [])
        XCTAssertEqual(DiagLog.tailLines(of: "a\nb\nc\nd\n", maxLines: 2), ["c", "d"])
        XCTAssertEqual(DiagLog.tailLines(of: "a\nb", maxLines: 5), ["a", "b"])
        XCTAssertEqual(DiagLog.tailLines(of: "a\n\nb\n", maxLines: 3), ["a", "", "b"])
        XCTAssertEqual(DiagLog.tailLines(of: "a", maxLines: 0), [])
    }
}
