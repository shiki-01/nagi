import XCTest

/// KanaKeyTable のフォールバック表（コアの `get_key_table` が取れないときの複製）。
/// `install` はグローバル状態を変えるので、このファイルでは「フォールバックと同じ内容」の表しか入れない
/// （他のテストの結果が実行順で変わらないように）
final class KanaKeyTableTests: XCTestCase {
    private let expected: [(KanaKey, String, [String])] = [
        (.a, "あ", ["あ", "い", "う", "え", "お"]),
        (.ka, "か", ["か", "き", "く", "け", "こ"]),
        (.sa, "さ", ["さ", "し", "す", "せ", "そ"]),
        (.ta, "た", ["た", "ち", "つ", "て", "と"]),
        (.na, "な", ["な", "に", "ぬ", "ね", "の"]),
        (.ha, "は", ["は", "ひ", "ふ", "へ", "ほ"]),
        (.ma, "ま", ["ま", "み", "む", "め", "も"]),
        (.ya, "や", ["や", "（", "ゆ", "）", "よ"]),
        (.ra, "ら", ["ら", "り", "る", "れ", "ろ"]),
        (.wa, "わ", ["わ", "を", "ん", "ー", "〜"]),
        (.punct, "、", ["、", "。", "？", "！", "…"]),
    ]

    // chars は 中央・左・上・右・下 の順
    func testFallbackTableForAllKeys() {
        for (key, _, chars) in expected {
            let entry = KanaKeyTable.entry(for: key)
            XCTAssertEqual(entry?.chars, chars, "\(key)")
            XCTAssertEqual(entry?.key, key)
        }
    }

    func testEntryForName() {
        for (key, name, chars) in expected {
            let entry = KanaKeyTable.entry(forName: name)
            XCTAssertEqual(entry?.key, key, name)
            XCTAssertEqual(entry?.chars, chars, name)
        }
    }

    func testEntryForNameUsesOnlyFirstCharacter() {
        XCTAssertEqual(KanaKeyTable.entry(forName: "、。？！")?.key, .punct)
        XCTAssertEqual(KanaKeyTable.entry(forName: "あいう")?.key, .a)
    }

    func testEntryForUnknownName() {
        XCTAssertNil(KanaKeyTable.entry(forName: ""))
        XCTAssertNil(KanaKeyTable.entry(forName: "ゃ"))
        XCTAssertNil(KanaKeyTable.entry(forName: "a"))
        XCTAssertNil(KanaKeyTable.entry(forName: "。"))
    }

    func testCharForGesture() {
        for (key, _, chars) in expected {
            XCTAssertEqual(KanaKeyTable.char(for: key, gesture: .tap), chars[0], "\(key)")
            XCTAssertEqual(KanaKeyTable.char(for: key, gesture: .toggle), chars[0], "\(key)")
            XCTAssertEqual(KanaKeyTable.char(for: key, gesture: .flickLeft), chars[1], "\(key)")
            XCTAssertEqual(KanaKeyTable.char(for: key, gesture: .flickUp), chars[2], "\(key)")
            XCTAssertEqual(KanaKeyTable.char(for: key, gesture: .flickRight), chars[3], "\(key)")
            XCTAssertEqual(KanaKeyTable.char(for: key, gesture: .flickDown), chars[4], "\(key)")
        }
    }

    /// 例: あ行は 上=う、わ行は 上=ん・右=ー、や行は 左=（・右=）
    func testSpotChecks() {
        XCTAssertEqual(KanaKeyTable.char(for: .a, gesture: .flickUp), "う")
        XCTAssertEqual(KanaKeyTable.char(for: .wa, gesture: .flickUp), "ん")
        XCTAssertEqual(KanaKeyTable.char(for: .wa, gesture: .flickRight), "ー")
        XCTAssertEqual(KanaKeyTable.char(for: .ya, gesture: .flickLeft), "（")
        XCTAssertEqual(KanaKeyTable.char(for: .ya, gesture: .flickRight), "）")
        XCTAssertEqual(KanaKeyTable.char(for: .punct, gesture: .flickDown), "…")
    }

    func testInstallNilOrEmptyKeepsFallback() {
        KanaKeyTable.install(nil)
        XCTAssertTrue(KanaKeyTable.fetchAttempted)
        KanaKeyTable.install(KeyTableResult(keys: [], variants: []))
        XCTAssertEqual(KanaKeyTable.entry(for: .a)?.chars, ["あ", "い", "う", "え", "お"])
    }

    /// コアの表は 中央・左・上・右・下 の順で `chars` に入る。内容はフォールバックと同じにしておく
    func testInstallMapsFieldsInOrder() {
        let keys = expected.map { key, _, chars in
            KeyTableEntry(key: key, center: chars[0], left: chars[1], up: chars[2], right: chars[3], down: chars[4], toggle: chars)
        }
        KanaKeyTable.install(KeyTableResult(keys: keys, variants: [["は", "ば", "ぱ"]]))
        XCTAssertTrue(KanaKeyTable.fetchAttempted)
        for (key, _, chars) in expected {
            XCTAssertEqual(KanaKeyTable.entry(for: key)?.chars, chars, "\(key)")
        }
    }

    // 注意: 濁点循環（゛゜小）の表は `KeyTableResult.variants` としてコアが持ち、KanaKeyTable はそれを保持しない
    // （循環の解決はコア）。そのため濁点循環のテストはここではできない。
}
