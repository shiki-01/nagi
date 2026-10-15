import XCTest

/// `KeyboardLayoutDef.validationError` / `LayoutLoader.decode` の規則。
/// 規則の対応表（web/src/lib/keyboard/validate.ts との対応）は各テスト名のあとの「web:」を参照
final class LayoutValidationTests: XCTestCase {
    private func s(_ ch: String, _ n: Int) -> String { String(repeating: ch, count: n) }

    // MARK: 全体

    func testMinimalLayoutIsValid() {
        assertValid(LB.single(LB.text()))
    }

    // web: schema は 1 のみ
    func testSchemaMustBe1() {
        assertInvalid(LB.layout([LB.row([LB.text()])], overrides: ["schema": 2]), contains: "スキーマ")
        assertInvalid(LB.layout([LB.row([LB.text()])], overrides: ["schema": 0]), contains: "スキーマ")
    }

    // web: LAYOUT_ID_PATTERN
    func testLayoutIdPattern() {
        for ok in ["a", "0", "flick-kana", "a_b-c", s("a", 64)] {
            assertValid(LB.layout([LB.row([LB.text()])], overrides: ["id": ok]))
        }
        for bad in ["", "Bad", "-a", "_a", "a b", "a.b", "あ", s("a", 65)] {
            assertInvalid(LB.layout([LB.row([LB.text()])], overrides: ["id": bad]), contains: "id は", file: #filePath, line: #line)
        }
    }

    // web: name は 1〜64 文字（コードポイント）
    func testNameLength() {
        assertValid(LB.layout([LB.row([LB.text()])], overrides: ["name": s("名", 64)]))
        assertInvalid(LB.layout([LB.row([LB.text()])], overrides: ["name": ""]), contains: "name")
        assertInvalid(LB.layout([LB.row([LB.text()])], overrides: ["name": s("名", 65)]), contains: "name")
    }

    func testNameIsRequired() {
        assertInvalid(LB.layout([LB.row([LB.text()])], remove: ["name"]), contains: "name")
    }

    // web: mode は列挙のいずれか
    func testModeMustBeKnown() {
        for mode in ["kana", "alpha", "number", "symbol", "emoji", "custom"] {
            assertValid(LB.layout([LB.row([LB.text()])], overrides: ["mode": mode]))
        }
        assertInvalid(LB.layout([LB.row([LB.text()])], overrides: ["mode": "weird"]), contains: "mode")
    }

    // MARK: 行

    // web: rows は 1〜8
    func testRowCount() {
        assertInvalid(LB.layout([]), contains: "rows")
        let rows = (0..<9).map { LB.row([LB.text("k\($0)")], height: 0.4) }
        assertInvalid(LB.layout(rows), contains: "rows")
        let eight = (0..<8).map { LB.row([LB.text("k\($0)")], height: 0.5) }
        assertValid(LB.layout(eight))
    }

    // web: height は 0 より大きく 4 以下
    func testRowHeightRange() {
        assertInvalid(LB.layout([LB.row([LB.text()], height: 0)]), contains: "高さ")
        assertInvalid(LB.layout([LB.row([LB.text()], height: -1)]), contains: "高さ")
        assertInvalid(LB.layout([LB.row([LB.text()], height: 4.1)]), contains: "高さ")
        assertValid(LB.layout([LB.row([LB.text()], height: 4)]))
    }

    // web: 行の高さ比の合計は 4.5 以下
    func testRowHeightSumLimit() {
        let five = (0..<5).map { LB.row([LB.text("k\($0)")]) }
        assertInvalid(LB.layout(five), contains: "高さの比の合計")
        let exactly = [LB.row([LB.text("a")]), LB.row([LB.text("b")]), LB.row([LB.text("c")]), LB.row([LB.text("d")], height: 1.5)]
        assertValid(LB.layout(exactly))
        let over = [LB.row([LB.text("a")], height: 4), LB.row([LB.text("b")], height: 0.6)]
        assertInvalid(LB.layout(over), contains: "高さの比の合計")
    }

    func testRowHeightDefaultsToOne() throws {
        let data = try JSONSerialization.data(withJSONObject: LB.single(LB.text()))
        let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data)
        XCTAssertEqual(def.rows[0].height, 1)
    }

    // web: 行には 1 つ以上のキー
    func testRowNeedsKeys() {
        assertInvalid(LB.layout([LB.row([])]), contains: "1 つ以上")
    }

    // web: 1 行のキーは 24 個以内
    func testKeysPerRowLimit() {
        let ok = (0..<24).map { LB.text("k\($0)", "a", ["width": 0.8]) }
        assertValid(LB.layout([LB.row(ok)]))
        let over = (0..<25).map { LB.text("k\($0)", "a", ["width": 0.8]) }
        assertInvalid(LB.layout([LB.row(over)]), contains: "24")
    }

    // web: 行内の幅の合計は 1〜20
    func testRowWidthSumRange() {
        assertInvalid(LB.single(LB.text("a", "a", ["width": 0.5])), contains: "幅の合計")
        assertValid(LB.single(LB.text("a", "a", ["width": 1])))
        assertValid(LB.layout([LB.row([LB.text("a", "a", ["width": 10]), LB.text("b", "b", ["width": 10])])]))
        assertInvalid(LB.layout([LB.row([LB.text("a", "a", ["width": 11]), LB.text("b", "b", ["width": 11])])]), contains: "幅の合計")
    }

    // web: width は 0 より大きく 20 以下
    func testKeyWidthRange() {
        assertInvalid(LB.single(LB.text("a", "a", ["width": 0])), contains: "幅")
        assertInvalid(LB.single(LB.text("a", "a", ["width": -1])), contains: "幅")
        assertInvalid(LB.single(LB.text("a", "a", ["width": 21])), contains: "幅")
        assertValid(LB.single(LB.text("a", "a", ["width": 20])))
    }

    func testKeyDefaults() throws {
        let data = try JSONSerialization.data(withJSONObject: LB.single(LB.text()))
        let key = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data).rows[0].keys[0]
        XCTAssertEqual(key.width, 1)
        XCTAssertEqual(key.style, .normal)
        XCTAssertTrue(key.idGiven)
    }

    // MARK: キー id

    // web: id は全レイアウトで一意（行をまたいでも）
    func testDuplicateKeyIdIsRejected() {
        assertInvalid(LB.layout([LB.row([LB.text("a"), LB.text("a", "b")])]), contains: "重複")
        assertInvalid(LB.layout([LB.row([LB.text("a")]), LB.row([LB.text("a", "b")])]), contains: "重複")
    }

    // web: KEY_ID_PATTERN
    func testKeyIdPattern() {
        for ok in ["a", "A", "0", "a.b-c_1", s("a", 32)] {
            assertValid(LB.single(LB.text(ok, "a")))
        }
        for bad in ["", " a", "a b", ".a", "-a", "_a", "あ", s("a", 33)] {
            assertInvalid(LB.single(LB.text(bad, "a")), contains: "id", file: #filePath, line: #line)
        }
    }

    // web: id は必須（spacer も同じ）。LayoutKeyDef のコメントは「省略可」とあるが、検証は web と同じく必須にしている
    func testKeyIdIsRequired() {
        var key = LB.text()
        key.removeValue(forKey: "id")
        assertInvalid(LB.single(key), contains: "id")
        assertInvalid(LB.single(["kind": "spacer"]), contains: "id")
    }

    // web: label は 16 文字以内、icon は 64 文字以内
    func testLabelAndIconLength() {
        assertValid(LB.single(LB.text("a", "a", ["label": s("あ", 16), "icon": s("i", 64)])))
        assertInvalid(LB.single(LB.text("a", "a", ["label": s("あ", 17)])), contains: "label")
        assertInvalid(LB.single(LB.text("a", "a", ["icon": s("i", 65)])), contains: "icon")
    }

    // web: style は normal / function / accent
    func testStyleMustBeKnown() {
        for style in ["normal", "function", "accent"] {
            assertValid(LB.single(LB.text("a", "a", ["style": style])))
        }
        assertInvalid(LB.single(LB.text("a", "a", ["style": "fancy"])), contains: "style")
    }

    func testUnknownKindFailsToDecode() {
        assertInvalid(LB.single(LB.key("a", "weird")), contains: "decode")
    }

    // MARK: text

    // web: text は 1〜16 文字（コードポイント）、shift も 1〜16
    func testTextKey() {
        assertInvalid(LB.single(LB.key("a", "text")), contains: "text")
        assertInvalid(LB.single(LB.text("a", "")), contains: "text")
        assertInvalid(LB.single(LB.text("a", s("a", 17))), contains: "text")
        assertValid(LB.single(LB.text("a", s("a", 16))))
        assertInvalid(LB.single(LB.text("a", "a", ["shift": ""])), contains: "shift")
        assertInvalid(LB.single(LB.text("a", "a", ["shift": s("A", 17)])), contains: "shift")
        assertValid(LB.single(LB.text("a", "a", ["shift": "A"])))
    }

    // 文字数はコードポイント数（絵文字 1 つは 1 文字。UTF-16 では 2）
    func testTextLengthCountsCodePoints() {
        assertValid(LB.single(LB.text("a", s("😀", 16))))
        assertInvalid(LB.single(LB.text("a", s("😀", 17))), contains: "text")
    }

    // MARK: flick

    // web: flick は center か kana が必要
    func testFlickNeedsCenterOrKana() {
        assertInvalid(LB.single(LB.key("f", "flick")), contains: "center")
        assertInvalid(LB.single(LB.key("f", "flick", ["up": ["text": "い"]])), contains: "center")
        assertValid(LB.single(LB.key("f", "flick", ["center": ["text": "あ"]])))
        assertValid(LB.single(LB.key("f", "flick", ["kana": "あ"])))
    }

    // web: 各方向の text は 1〜16 文字
    func testFlickDirectionTexts() {
        let full: JSON = ["center": ["text": "a"], "up": ["text": "b"], "down": ["text": "c"], "left": ["text": "d"], "right": ["text": "e"]]
        assertValid(LB.single(LB.key("f", "flick", full)))
        for dir in ["center", "up", "down", "left", "right"] {
            var cells = full
            cells[dir] = ["text": ""]
            assertInvalid(LB.single(LB.key("f", "flick", cells)), contains: dir, file: #filePath, line: #line)
            cells[dir] = ["text": s("x", 17)]
            assertInvalid(LB.single(LB.key("f", "flick", cells)), contains: dir, file: #filePath, line: #line)
        }
    }

    // web: kana は 1〜4 文字。アプリ側はさらに 12 キー名（先頭の 1 文字）に限る（web は未検査）
    func testFlickKanaName() {
        for name in ["あ", "か", "さ", "た", "な", "は", "ま", "や", "ら", "わ", "、", "、。？！"] {
            assertValid(LB.single(LB.key("f", "flick", ["kana": name])))
        }
        assertInvalid(LB.single(LB.key("f", "flick", ["kana": "ゃ"])), contains: "未知")
        assertInvalid(LB.single(LB.key("f", "flick", ["kana": "a"])), contains: "未知")
        assertInvalid(LB.single(LB.key("f", "flick", ["kana": ""])), contains: "kana")
        assertInvalid(LB.single(LB.key("f", "flick", ["kana": "あああああ"])), contains: "kana")
    }

    // MARK: toggle

    // web: cycle は 1〜16 要素、各要素 1〜16 文字
    func testToggleCycle() {
        assertValid(LB.single(LB.key("t", "toggle", ["cycle": ["a", "b", "c"]])))
        assertInvalid(LB.single(LB.key("t", "toggle")), contains: "cycle")
        assertInvalid(LB.single(LB.key("t", "toggle", ["cycle": [String]()])), contains: "cycle")
        assertValid(LB.single(LB.key("t", "toggle", ["cycle": (0..<16).map { "c\($0)" }])))
        assertInvalid(LB.single(LB.key("t", "toggle", ["cycle": (0..<17).map { "c\($0)" }])), contains: "cycle")
        assertInvalid(LB.single(LB.key("t", "toggle", ["cycle": ["a", ""]])), contains: "cycle")
        assertInvalid(LB.single(LB.key("t", "toggle", ["cycle": ["a", s("x", 17)]])), contains: "cycle")
    }

    // MARK: action

    // web: ACTION_PATTERN（英字で始まる英数字 32 文字以内）
    func testActionPattern() {
        for ok in ["space", "moveLeft", "a", "A1", s("a", 32)] {
            assertValid(LB.single(LB.key("x", "action", ["action": ok])))
        }
        for bad in ["", "1abc", "a-b", "a_b", "a b", "あ", s("a", 33)] {
            assertInvalid(LB.single(LB.key("x", "action", ["action": bad])), contains: "action", file: #filePath, line: #line)
        }
        assertInvalid(LB.single(LB.key("x", "action")), contains: "action")
    }

    // MARK: shortcut

    // web: steps は 1〜32 個
    func testShortcutStepCount() {
        assertValid(LB.single(LB.key("s", "shortcut", ["steps": [["action": "space"]]])))
        assertInvalid(LB.single(LB.key("s", "shortcut")), contains: "steps")
        assertInvalid(LB.single(LB.key("s", "shortcut", ["steps": [JSON]()])), contains: "steps")
        let thirtyTwo = (0..<32).map { _ in ["text": "a"] }
        let thirtyThree = (0..<33).map { _ in ["text": "a"] }
        assertValid(LB.single(LB.key("s", "shortcut", ["steps": thirtyTwo])))
        assertInvalid(LB.single(LB.key("s", "shortcut", ["steps": thirtyThree])), contains: "steps")
    }

    // web: 手順は action か text のどちらか一方だけ
    func testShortcutStepActionXorText() {
        assertInvalid(LB.single(LB.key("s", "shortcut", ["steps": [["action": "space", "text": "a"]]])), contains: "どちらか一方")
        assertInvalid(LB.single(LB.key("s", "shortcut", ["steps": [JSON()]])), contains: "どちらか一方")
        assertValid(LB.single(LB.key("s", "shortcut", ["steps": [["action": "space"], ["text": "。"]]])))
    }

    // web: 手順の action は ACTION_PATTERN、text は 1〜256 文字
    func testShortcutStepContents() {
        assertInvalid(LB.single(LB.key("s", "shortcut", ["steps": [["action": "1x"]]])), contains: "action")
        assertInvalid(LB.single(LB.key("s", "shortcut", ["steps": [["text": ""]]])), contains: "text")
        assertValid(LB.single(LB.key("s", "shortcut", ["steps": [["text": s("あ", 256)]]])))
        assertInvalid(LB.single(LB.key("s", "shortcut", ["steps": [["text": s("あ", 257)]]])), contains: "text")
    }

    // MARK: layout

    // web: target は LAYOUT_ID_PATTERN
    func testLayoutTargetPattern() {
        assertInvalid(LB.single(LB.key("l", "layout")), contains: "target")
        assertInvalid(LB.single(LB.key("l", "layout", ["target": "Bad"])), contains: "target")
        assertInvalid(LB.single(LB.key("l", "layout", ["target": ""])), contains: "target")
    }

    // web: target は組み込み名・自身・既知の id のいずれか。targets が nil なら存在を確かめない
    func testLayoutTargetExistence() {
        let nowhere = LB.single(LB.key("l", "layout", ["target": "nowhere"]))
        assertInvalid(nowhere, contains: "切り替え先", targets: [])
        assertValid(nowhere, targets: nil)
        for builtin in LayoutValidation.builtinLayoutIds {
            assertValid(LB.single(LB.key("l", "layout", ["target": builtin])), targets: [])
        }
        // 自身の id
        assertValid(LB.single(LB.key("l", "layout", ["target": "test"])), targets: [])
        // 他のレイアウト
        let other = LB.single(LB.key("l", "layout", ["target": "other"]))
        assertInvalid(other, targets: [])
        assertValid(other, targets: ["other"])
    }

    func testBuiltinLayoutIdsMatchWeb() {
        // web/src/lib/keyboard/types.ts の BUILTIN_LAYOUT_IDS と同じ集合
        XCTAssertEqual(
            Set(LayoutValidation.builtinLayoutIds),
            ["qwerty-kana", "qwerty-alpha", "flick-kana", "flick-alpha", "symbols", "numbers", "emoji", "kana", "alpha"])
    }

    func testLimitsMatchWeb() {
        // web/src/lib/keyboard/types.ts の LIMITS と同じ値
        XCTAssertEqual(LayoutValidation.rowsMax, 8)
        XCTAssertEqual(LayoutValidation.keysPerRowMax, 24)
        XCTAssertEqual(LayoutValidation.rowWidthMin, 1)
        XCTAssertEqual(LayoutValidation.rowWidthMax, 20)
        XCTAssertEqual(LayoutValidation.rowHeightMax, 4)
        XCTAssertEqual(LayoutValidation.rowHeightSumMax, 4.5)
        XCTAssertEqual(LayoutValidation.widthMax, 20)
        XCTAssertEqual(LayoutValidation.textMax, 16)
        XCTAssertEqual(LayoutValidation.shortcutTextMax, 256)
        XCTAssertEqual(LayoutValidation.cycleMax, 16)
        XCTAssertEqual(LayoutValidation.stepsMax, 32)
        XCTAssertEqual(LayoutValidation.longPressMax, 12)
        XCTAssertEqual(LayoutValidation.nameMax, 64)
        XCTAssertEqual(LayoutValidation.labelMax, 16)
        XCTAssertEqual(LayoutValidation.iconMax, 64)
        XCTAssertEqual(LayoutValidation.kanaMax, 4)
    }

    // MARK: longPress

    private func popupText(_ label: String = "x", _ text: String = "y") -> JSON {
        ["kind": "text", "label": label, "text": text]
    }

    // web: longPress は 1〜12 個
    func testLongPressCount() {
        assertValid(LB.single(LB.text("a", "a", ["longPress": [popupText()]])))
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [JSON]()])), contains: "longPress")
        assertValid(LB.single(LB.text("a", "a", ["longPress": (0..<12).map { _ in popupText() }])))
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": (0..<13).map { _ in popupText() }])), contains: "longPress")
    }

    // web: 長押しの項目は label 必須（1〜16 文字）。kind は text / action / shortcut のみ
    func testLongPressItems() {
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [["kind": "text", "text": "y"]]])), contains: "label")
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [popupText(s("あ", 17))]])), contains: "label")
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [["kind": "text", "label": "x"]]])), contains: "text")
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [["kind": "action", "label": "x", "action": "1"]]])), contains: "action")
        assertValid(LB.single(LB.text("a", "a", ["longPress": [["kind": "action", "label": "x", "action": "space"]]])))
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [["kind": "shortcut", "label": "x", "steps": [JSON()]]]])), contains: "どちらか一方")
        assertValid(LB.single(LB.text("a", "a", ["longPress": [["kind": "shortcut", "label": "x", "steps": [["text": "z"]]]]])))
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [["kind": "flick", "label": "x", "kana": "あ"]]])), contains: "kind")
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [["kind": "spacer", "label": "x"]]])), contains: "kind")
    }

    // web: 長押しの中に長押しは置けない
    func testLongPressCannotNest() {
        var inner = popupText()
        inner["longPress"] = [popupText()]
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [inner]])), contains: "longPress の中に")
    }

    // web: 長押しの id は省略可、指定するなら KEY_ID_PATTERN
    func testLongPressItemId() {
        var withId = popupText()
        withId["id"] = "ok.1"
        assertValid(LB.single(LB.text("a", "a", ["longPress": [withId]])))
        withId["id"] = "bad id"
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [withId]])), contains: "id")
    }

    // MARK: visible

    func testVisibleConditionIsDecoded() throws {
        let obj = LB.single(LB.text("a", "a", ["visible": ["field": "returnKey", "in": ["go", "search"]]]))
        let data = try JSONSerialization.data(withJSONObject: obj)
        let key = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data).rows[0].keys[0]
        XCTAssertEqual(key.visible?.field, "returnKey")
        XCTAssertEqual(key.visible?.values, ["go", "search"])
    }

    // 条件の形が想定と違っても、レイアウト全体は無効にしない（web は検証でエラーにする。アプリ側は実行時に寛容）
    func testMalformedVisibleDoesNotInvalidateLayout() throws {
        let obj = LB.single(LB.text("a", "a", ["visible": "bad"]))
        assertValid(obj)
        let key = try JSONDecoder().decode(KeyboardLayoutDef.self, from: JSONSerialization.data(withJSONObject: obj)).rows[0].keys[0]
        XCTAssertNil(key.visible)
    }

    // MARK: LayoutLoader.decode / describe

    private func loadError(_ json: String) -> String? {
        do {
            _ = try LayoutLoader.decode(Data(json.utf8))
            return nil
        } catch let error as LayoutLoader.LoadError {
            return error.message
        } catch {
            return "other: \(error)"
        }
    }

    func testDecodeAcceptsValidJSON() throws {
        let data = try JSONSerialization.data(withJSONObject: LB.single(LB.text()))
        let def = try LayoutLoader.decode(data)
        XCTAssertEqual(def.id, "test")
    }

    func testDecodeReportsBrokenJSON() {
        let message = loadError("{")
        XCTAssertNotNil(message)
        XCTAssertFalse(message?.isEmpty ?? true)
    }

    func testDecodeReportsMissingRequiredKey() {
        let message = loadError(#"{"schema":1,"id":"x","name":"n","mode":"kana","rows":[{"keys":[{"id":"a","text":"a"}]}]}"#)
        XCTAssertTrue(message?.contains("必須") ?? false, message ?? "nil")
        XCTAssertTrue(message?.contains("kind") ?? false, message ?? "nil")
    }

    func testDecodeReportsTypeMismatch() {
        let message = loadError(#"{"schema":1,"id":"x","name":"n","mode":"kana","rows":"oops"}"#)
        XCTAssertTrue(message?.contains("型") ?? false, message ?? "nil")
        XCTAssertTrue(message?.contains("rows") ?? false, message ?? "nil")
    }

    func testDecodeAppliesValidationAfterDecoding() {
        let message = loadError(#"{"schema":1,"id":"x","name":"n","mode":"kana","rows":[{"keys":[{"id":"a","kind":"text","text":""}]}]}"#)
        XCTAssertTrue(message?.contains("text") ?? false, message ?? "nil")
    }

    func testDescribeNonDecodingError() {
        let message = LayoutLoader.describe(NSError(domain: "x", code: 1))
        XCTAssertTrue(message.hasPrefix("読み取れません"))
    }

    // MARK: isSafeId

    func testIsSafeId() {
        for ok in ["flick-kana", "a_b", "A1", s("a", 64)] {
            XCTAssertTrue(LayoutLoader.isSafeId(ok), ok)
        }
        for bad in ["", "../x", "a/b", "a b", "a.json", "あ", s("a", 65)] {
            XCTAssertFalse(LayoutLoader.isSafeId(bad), bad)
        }
    }
}
