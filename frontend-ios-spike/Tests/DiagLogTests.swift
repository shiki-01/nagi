import XCTest

final class DiagLogTests: XCTestCase {
    private var originalEnabled = false

    override func setUp() {
        super.setUp()
        originalEnabled = DiagLog.isEnabled
        DiagLog.isEnabled = true
        DiagLog.clear()
    }

    override func tearDown() {
        DiagLog.clear()
        DiagLog.isEnabled = originalEnabled
        super.tearDown()
    }

    // MARK: quote（純粋な整形）

    func testQuotePlainText() {
        XCTAssertEqual(DiagLog.quote("abc"), "\"abc\"(u16=3)")
        XCTAssertEqual(DiagLog.quote(""), "\"\"(u16=0)")
    }

    func testQuoteEscapesControlCharactersAndQuotes() {
        XCTAssertEqual(DiagLog.quote("a\nb"), "\"a\\nb\"(u16=3)")
        XCTAssertEqual(DiagLog.quote("a\r\tb"), "\"a\\r\\tb\"(u16=4)")
        XCTAssertEqual(DiagLog.quote("say \"hi\""), "\"say \\\"hi\\\"\"(u16=8)")
    }

    func testQuoteCountsUTF16Units() {
        XCTAssertEqual(DiagLog.quote("あい"), "\"あい\"(u16=2)")
        // 絵文字（サロゲートペア）は UTF-16 で 2
        XCTAssertEqual(DiagLog.quote("😀"), "\"😀\"(u16=2)")
    }

    func testQuoteRedactHidesContent() {
        let quoted = DiagLog.quote("secret\npassword", redact: true)
        XCTAssertEqual(quoted, "<secure>(u16=15)")
        XCTAssertFalse(quoted.contains("secret"))
    }

    // MARK: 書き込み・読み出し

    func testLogAppendsLinesInOrderWithTimestamp() throws {
        DiagLog.log("first")
        DiagLog.log("second")
        let lines = DiagLog.read().split(separator: "\n").map(String.init)
        try XCTSkipIf(lines.isEmpty, "ログファイルに書けない環境")
        XCTAssertEqual(lines.count, 2)
        XCTAssertTrue(lines[0].hasSuffix(" first"), lines[0])
        XCTAssertTrue(lines[1].hasSuffix(" second"), lines[1])
        XCTAssertNotNil(lines[0].range(of: "^\\d{2}:\\d{2}:\\d{2}\\.\\d{3} first$", options: .regularExpression), lines[0])
    }

    func testClearRemovesLog() {
        DiagLog.log("x")
        DiagLog.clear()
        XCTAssertEqual(DiagLog.read(), "")
    }

    func testDisabledLogDoesNotEvaluateMessageOrWrite() {
        DiagLog.isEnabled = false
        var evaluated = false
        func message() -> String {
            evaluated = true
            return "should not be written"
        }
        DiagLog.log(message())
        XCTAssertFalse(evaluated)
        XCTAssertEqual(DiagLog.read(), "")
    }

    // MARK: 切り詰め

    /// `maxBytes` を超えたら後半だけを残す。行の途中から始まらず、最新の行は残る
    func testLogIsTruncatedAtMaxBytesKeepingNewestLines() throws {
        let padding = String(repeating: "x", count: 100)
        let total = 800 // 1 行 約 120 バイト × 800 = 約 96KB > maxBytes（64KB）
        for index in 0..<total {
            DiagLog.log("msg-\(String(format: "%04d", index)) \(padding)")
        }
        let text = DiagLog.read()
        try XCTSkipIf(text.isEmpty, "ログファイルに書けない環境")

        XCTAssertLessThanOrEqual(text.utf8.count, DiagLog.maxBytes)
        XCTAssertGreaterThan(text.utf8.count, 0)
        XCTAssertFalse(text.contains("msg-0000 "), "古い行が残っている")
        XCTAssertTrue(text.contains("msg-0799 "), "最新の行が失われた")

        // すべての行が完全（先頭が時刻、末尾が padding）
        let lines = text.split(separator: "\n").map(String.init)
        XCTAssertGreaterThan(lines.count, 100)
        for line in lines {
            XCTAssertNotNil(line.range(of: "^\\d{2}:\\d{2}:\\d{2}\\.\\d{3} msg-\\d{4} x{100}$", options: .regularExpression), line)
        }
        // 連番が連続している（途中が欠けていない）
        let numbers = lines.compactMap { line -> Int? in
            guard let range = line.range(of: "msg-\\d{4}", options: .regularExpression) else { return nil }
            return Int(line[range].dropFirst(4))
        }
        XCTAssertEqual(numbers.count, lines.count)
        XCTAssertEqual(numbers.last, total - 1)
        for (a, b) in zip(numbers, numbers.dropFirst()) {
            XCTAssertEqual(b, a + 1)
        }
    }
}
