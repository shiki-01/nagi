import XCTest

/// クリップボード履歴: 種類の判定、設定、刈り込み、保存（時刻は引数で渡す）
final class ClipboardHistoryTests: XCTestCase {
    typealias K = NagiIOSSettings.Key

    private let now = Date(timeIntervalSince1970: 1_800_000_000)

    private func config(
        enabled: Bool = true, retention: TimeInterval? = 24 * 3600, maxItems: Int = 50,
        kinds: Set<ClipboardKind> = Set(ClipboardKind.allCases), previewChars: Int = 40
    ) -> ClipboardHistoryConfig {
        ClipboardHistoryConfig(
            enabled: enabled, retention: retention, maxItems: maxItems, kinds: kinds, previewChars: previewChars)
    }

    private func item(_ text: String, ago: TimeInterval = 0, pinned: Bool = false, id: String? = nil) -> ClipboardItem {
        ClipboardItem(
            id: id ?? text, text: text, kind: ClipboardKind.classify(text),
            copiedAt: now.addingTimeInterval(-ago), pinned: pinned)
    }

    private func makeStore() throws -> (ClipboardHistoryStore, URL) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("clip-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("clipboard-history.json")
        return (ClipboardHistoryStore(fileURL: url), url)
    }

    // MARK: 種類の判定

    func testClassify() {
        XCTAssertEqual(ClipboardKind.classify("https://example.com/a?b=1"), .url)
        XCTAssertEqual(ClipboardKind.classify("  http://example.com \n"), .url)
        XCTAssertEqual(ClipboardKind.classify("HTTPS://EXAMPLE.COM"), .url)
        // 空白を含む・スキームだけ・別のスキームは URL ではない
        XCTAssertEqual(ClipboardKind.classify("https://example.com は便利"), .text)
        XCTAssertEqual(ClipboardKind.classify("https://"), .text)
        XCTAssertEqual(ClipboardKind.classify("ftp://example.com"), .text)
        XCTAssertEqual(ClipboardKind.classify("taro@example.co.jp"), .email)
        XCTAssertEqual(ClipboardKind.classify(" a.b+c@example.com "), .email)
        XCTAssertEqual(ClipboardKind.classify("taro@example"), .text)
        XCTAssertEqual(ClipboardKind.classify("連絡は taro@example.com へ"), .text)
        XCTAssertEqual(ClipboardKind.classify("03-1234-5678"), .number)
        XCTAssertEqual(ClipboardKind.classify("+81 (90) 1234.5678"), .number)
        XCTAssertEqual(ClipboardKind.classify("１２３４"), .number)
        XCTAssertEqual(ClipboardKind.classify("123456"), .number)
        // 数字を含まない記号だけ、数字以外の文字を含むものは text
        XCTAssertEqual(ClipboardKind.classify("---"), .text)
        XCTAssertEqual(ClipboardKind.classify("ABC123"), .text)
        XCTAssertEqual(ClipboardKind.classify("こんにちは"), .text)
        XCTAssertEqual(ClipboardKind.classify(""), .text)
    }

    func testKindDecodesUnknownAsText() throws {
        let kinds = try JSONDecoder().decode([ClipboardKind].self, from: Data(#"["url","future","number"]"#.utf8))
        XCTAssertEqual(kinds, [.url, .text, .number])
    }

    // MARK: 設定

    func testSettingsDefaults() {
        let c = NagiIOSSettings().clipboardHistory
        XCTAssertFalse(c.enabled, "既定はオフ")
        XCTAssertEqual(c.retention, 24 * 3600)
        XCTAssertEqual(c.maxItems, 50)
        XCTAssertEqual(c.kinds, Set(ClipboardKind.allCases))
        XCTAssertEqual(c.previewChars, 40)
        XCTAssertEqual(NagiIOSSettings().string(K.clipboardHistoryRetention), "24h")
    }

    func testSettingsConversionAndNormalization() {
        var s = NagiIOSSettings()
        s.set(K.clipboardHistoryEnabled, .bool(true))
        s.set(K.clipboardHistoryRetention, .string("forever"))
        s.set(K.clipboardHistoryMaxItems, .string("200"))
        s.set(K.clipboardHistoryPreviewChars, .string("120"))
        // 種類: 知らない値は捨て、選択肢の並びに揃う。空は「何も選ばない」
        s.set(K.clipboardHistoryTypes, .array([.string("url"), .string("bogus"), .string("text"), .string("url")]))
        let c = s.clipboardHistory
        XCTAssertTrue(c.enabled)
        XCTAssertNil(c.retention)
        XCTAssertEqual(c.maxItems, 200)
        XCTAssertEqual(c.previewChars, 120)
        XCTAssertEqual(c.kinds, [.text, .url])
        XCTAssertEqual(s.stringList(K.clipboardHistoryTypes), ["text", "url"])
        s.set(K.clipboardHistoryTypes, .array([]))
        XCTAssertEqual(s.clipboardHistory.kinds, [])
        // 範囲外・型違いは既定
        s.set(K.clipboardHistoryRetention, .string("2y"))
        s.set(K.clipboardHistoryMaxItems, .number(50))
        s.set(K.clipboardHistoryTypes, .string("url"))
        XCTAssertEqual(s.clipboardHistory.retention, 24 * 3600)
        XCTAssertEqual(s.clipboardHistory.maxItems, 50)
        XCTAssertEqual(s.clipboardHistory.kinds, Set(ClipboardKind.allCases))
    }

    func testRetentionSeconds() {
        XCTAssertEqual(ClipboardHistoryLogic.retentionSeconds(for: "1h"), 3600)
        XCTAssertEqual(ClipboardHistoryLogic.retentionSeconds(for: "7d"), 7 * 86400)
        XCTAssertEqual(ClipboardHistoryLogic.retentionSeconds(for: "30d"), 30 * 86400)
        XCTAssertNil(ClipboardHistoryLogic.retentionSeconds(for: "forever"))
    }

    // MARK: 刈り込み・記録

    func testPruneRemovesExpiredButKeepsPinned() {
        let items = [item("new", ago: 10), item("old", ago: 7200), item("oldpin", ago: 7200, pinned: true)]
        let pruned = ClipboardHistoryLogic.prune(items, now: now, config: config(retention: 3600))
        XCTAssertEqual(pruned.map { $0.text }, ["new", "oldpin"])
        // 無期限なら残る
        XCTAssertEqual(ClipboardHistoryLogic.prune(items, now: now, config: config(retention: nil)).count, 3)
        // ちょうど期限は残る
        XCTAssertEqual(ClipboardHistoryLogic.prune([item("edge", ago: 3600)], now: now, config: config(retention: 3600)).count, 1)
    }

    func testPruneLimitsUnpinnedByCountOldestFirst() {
        let items = [item("a", ago: 1), item("b", ago: 2), item("c", ago: 3), item("p", ago: 100, pinned: true)]
        let pruned = ClipboardHistoryLogic.prune(items, now: now, config: config(maxItems: 2))
        XCTAssertEqual(Set(pruned.map { $0.text }), ["a", "b", "p"], "ピン留めは数に含めず、消さない")
        XCTAssertEqual(ClipboardHistoryLogic.prune(items, now: now, config: config(maxItems: 0)).map { $0.text }, ["p"])
    }

    func testRecordRules() {
        let c = config()
        XCTAssertEqual(ClipboardHistoryLogic.recorded([], text: "  \n ", now: now, config: c), [])
        XCTAssertEqual(ClipboardHistoryLogic.recorded([], text: "x", now: now, config: config(enabled: false)), [])
        XCTAssertEqual(ClipboardHistoryLogic.recorded([], text: "https://a.example", now: now, config: config(kinds: [.text])), [])
        XCTAssertEqual(ClipboardHistoryLogic.recorded([], text: "hello", now: now, config: config(kinds: [.text])).count, 1)
        // 長さの上限
        let limit = ClipboardHistoryLogic.maxTextLength
        XCTAssertEqual(ClipboardHistoryLogic.recorded([], text: String(repeating: "あ", count: limit), now: now, config: c).count, 1)
        XCTAssertEqual(ClipboardHistoryLogic.recorded([], text: String(repeating: "あ", count: limit + 1), now: now, config: c), [])
    }

    func testRecordMovesDuplicateToFrontKeepingIdAndPin() {
        let items = [item("a", ago: 10), item("b", ago: 20, pinned: true, id: "bid")]
        let result = ClipboardHistoryLogic.recorded(items, text: "b", now: now, config: config())
        XCTAssertEqual(result.map { $0.text }, ["b", "a"])
        XCTAssertEqual(result[0].id, "bid")
        XCTAssertTrue(result[0].pinned)
        XCTAssertEqual(result[0].copiedAt, now)
        XCTAssertEqual(result.count, 2)
    }

    func testRecordPrunesToMaxItems() {
        var items: [ClipboardItem] = []
        for index in 0..<5 {
            items = ClipboardHistoryLogic.recorded(
                items, text: "t\(index)", now: now.addingTimeInterval(Double(index)), config: config(maxItems: 3))
        }
        XCTAssertEqual(items.map { $0.text }, ["t4", "t3", "t2"])
    }

    func testOrderedPinnedFirstThenNewest() {
        let items = [item("a", ago: 1), item("b", ago: 5, pinned: true), item("c", ago: 3), item("d", ago: 9, pinned: true)]
        XCTAssertEqual(ClipboardHistoryLogic.ordered(items).map { $0.text }, ["b", "d", "a", "c"])
    }

    // MARK: 表示用

    func testPreviewTruncatesWithEllipsis() {
        XCTAssertEqual(ClipboardHistoryLogic.preview("12345", limit: 5), "12345")
        XCTAssertEqual(ClipboardHistoryLogic.preview("123456", limit: 5), "12345…")
        XCTAssertEqual(ClipboardHistoryLogic.preview("あいうえおかき", limit: 3), "あいう…")
    }

    func testRelativeTime() {
        func text(_ seconds: TimeInterval) -> String {
            ClipboardHistoryLogic.relativeTime(from: now.addingTimeInterval(-seconds), now: now)
        }
        XCTAssertEqual(text(5), "たった今")
        XCTAssertEqual(text(-30), "たった今")
        XCTAssertEqual(text(180), "3 分前")
        XCTAssertEqual(text(3 * 3600 + 10), "3 時間前")
        XCTAssertEqual(text(2 * 86400 + 10), "2 日前")
    }

    func testPageState() {
        XCTAssertEqual(ClipboardPageState.state(enabled: false, secure: false, itemCount: 3), .disabled)
        XCTAssertEqual(ClipboardPageState.state(enabled: true, secure: true, itemCount: 3), .secure)
        XCTAssertEqual(ClipboardPageState.state(enabled: true, secure: false, itemCount: 0), .empty)
        XCTAssertEqual(ClipboardPageState.state(enabled: true, secure: false, itemCount: 1), .items)
        XCTAssertNil(ClipboardPageState.items.message)
        XCTAssertEqual(ClipboardPageState.empty.message, "履歴はありません")
        XCTAssertEqual(ClipboardPageState.secure.message, "パスワード欄では履歴は使えません")
        XCTAssertEqual(ClipboardPageState.disabled.message, "設定でクリップボード履歴をオンにしてください")
    }

    func testShouldReadOnlyWhenEnabledAllowedAndChanged() {
        func read(_ enabled: Bool = true, full: Bool = true, secure: Bool = false, now: Int = 5, last: Int? = 4) -> Bool {
            ClipboardCaptureLogic.shouldRead(
                enabled: enabled, hasFullAccess: full, isSecure: secure, currentChangeCount: now, lastChangeCount: last)
        }
        XCTAssertTrue(read())
        XCTAssertTrue(read(last: nil))
        XCTAssertFalse(read(last: 5), "変わっていなければ読まない")
        XCTAssertFalse(read(false), "オフなら読まない")
        XCTAssertFalse(read(full: false))
        XCTAssertFalse(read(secure: true))
    }

    // MARK: 保存

    func testStoreRecordAndReadRoundTrip() throws {
        let (store, url) = try makeStore()
        XCTAssertTrue(store.record(text: "one", now: now, settings: config()))
        XCTAssertTrue(store.record(text: "https://example.com", now: now.addingTimeInterval(5), settings: config()))
        XCTAssertFalse(store.record(text: "  ", now: now, settings: config()))
        // 別のインスタンス（別プロセス相当）から読める
        let other = ClipboardHistoryStore(fileURL: url)
        let items = other.items(now: now.addingTimeInterval(10))
        XCTAssertEqual(items.map { $0.text }, ["https://example.com", "one"])
        XCTAssertEqual(items.map { $0.kind }, [.url, .text])
        XCTAssertEqual(items[1].copiedAt.timeIntervalSince1970, now.timeIntervalSince1970, accuracy: 0.001)
        // スキーマバージョン付き
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        XCTAssertEqual(object["schema"] as? Int, 1)
    }

    func testStoreSecondWriterKeepsFirstWritersUpdate() throws {
        // 2 つのインスタンス（ホストと拡張）が交互に書いても、互いの更新を消さない
        let (a, url) = try makeStore()
        let b = ClipboardHistoryStore(fileURL: url)
        a.record(text: "from-a", now: now, settings: config())
        b.record(text: "from-b", now: now.addingTimeInterval(1), settings: config())
        a.record(text: "from-a-2", now: now.addingTimeInterval(2), settings: config())
        XCTAssertEqual(b.items(now: now.addingTimeInterval(3)).map { $0.text }, ["from-a-2", "from-b", "from-a"])
    }

    func testStoreCorruptFileIsTreatedAsEmpty() throws {
        let (store, url) = try makeStore()
        try Data("{ not json".utf8).write(to: url)
        XCTAssertEqual(store.items(now: now), [])
        XCTAssertTrue(store.record(text: "ok", now: now, settings: config()))
        XCTAssertEqual(store.items(now: now).map { $0.text }, ["ok"])
    }

    func testStoreMissingFileAndUnavailableStore() throws {
        let (store, _) = try makeStore()
        XCTAssertEqual(store.items(now: now), [])
        let none = ClipboardHistoryStore(fileURL: nil)
        XCTAssertFalse(none.isAvailable)
        XCTAssertFalse(none.record(text: "x", now: now, settings: config()))
        XCTAssertEqual(none.items(now: now), [])
        none.clear()
    }

    func testStorePinDeleteClear() throws {
        let (store, _) = try makeStore()
        store.record(text: "a", now: now, settings: config())
        store.record(text: "b", now: now.addingTimeInterval(1), settings: config())
        let a = try XCTUnwrap(store.items(now: now).first { $0.text == "a" })
        XCTAssertTrue(store.togglePin(id: a.id))
        XCTAssertEqual(store.items(now: now).map { $0.text }, ["a", "b"], "ピン留めが先頭")
        XCTAssertTrue(store.togglePin(id: a.id))
        XCTAssertEqual(store.items(now: now).map { $0.text }, ["b", "a"])
        XCTAssertFalse(store.togglePin(id: "missing"))
        store.delete(id: a.id)
        XCTAssertEqual(store.items(now: now).map { $0.text }, ["b"])
        store.clear()
        XCTAssertEqual(store.items(now: now), [])
    }

    func testStorePinLimit() throws {
        let (store, _) = try makeStore()
        let total = ClipboardHistoryLogic.maxPinned + 1
        for index in 0..<total {
            store.record(text: "t\(index)", now: now.addingTimeInterval(Double(index)), settings: config())
        }
        let all = store.items(now: now.addingTimeInterval(100))
        var results: [Bool] = []
        for item in all { results.append(store.togglePin(id: item.id)) }
        XCTAssertEqual(results.filter { $0 }.count, ClipboardHistoryLogic.maxPinned)
        XCTAssertEqual(store.items(now: now.addingTimeInterval(100)).filter { $0.pinned }.count, ClipboardHistoryLogic.maxPinned)
    }

    func testStorePruneAndItemsWithSettingsHideExpired() throws {
        let (store, _) = try makeStore()
        store.record(text: "old", now: now, settings: config())
        store.record(text: "pinned", now: now, settings: config())
        let pinned = try XCTUnwrap(store.items(now: now).first { $0.text == "pinned" })
        store.togglePin(id: pinned.id)
        let later = now.addingTimeInterval(2 * 3600)
        let short = config(retention: 3600)
        // ファイルは書き換えずに隠す
        XCTAssertEqual(store.items(now: later, settings: short).map { $0.text }, ["pinned"])
        XCTAssertEqual(store.items(now: later).count, 2)
        store.prune(now: later, settings: short)
        XCTAssertEqual(store.items(now: later).map { $0.text }, ["pinned"])
    }

    func testDisabledSettingsNeverRecord() throws {
        let (store, url) = try makeStore()
        XCTAssertFalse(store.record(text: "secret", now: now, settings: config(enabled: false)))
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
    }
}
