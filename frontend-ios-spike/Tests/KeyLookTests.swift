import XCTest

// 見た目（look）の型の読み込み・色・範囲・既定値（docs/keyboard-style-spec.md §2、§5.5、§7.5）。
// 解決（重ね合わせ）は KeyLookVectorTests.swift が web と共有のテストベクタで確かめる。

private func decodeLook(_ json: String, file: StaticString = #filePath, line: UInt = #line) -> KeyLook {
    guard let look = try? JSONDecoder().decode(KeyLook.self, from: Data(json.utf8)) else {
        XCTFail("decode できない: \(json)", file: file, line: line)
        return KeyLook()
    }
    return look
}

private func decodeLayout(_ json: String) throws -> KeyboardLayoutDef {
    try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(json.utf8))
}

private let minimalRows = #""rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]}]"#

// MARK: - 寛容な decode

final class KeyLookDecodeTests: XCTestCase {
    func testWrongTypesNullAndUnknownNamesAreDropped() {
        let look = decodeLook(#"{"fontSize":"big","opacity":null,"fill":5,"fontWeight":3,"mystery":1,"cornerRadius":6,"label":"x"}"#)
        let expected: [String: LookValue] = ["cornerRadius": .number(6), "label": .string("x")]
        XCTAssertEqual(look.values, expected)
    }

    func testNonObjectLookFailsToDecode() {
        XCTAssertNil(try? JSONDecoder().decode(KeyLook.self, from: Data("5".utf8)))
        XCTAssertNil(try? JSONDecoder().decode(KeyLook.self, from: Data("[1]".utf8)))
    }

    func testLayoutIgnoresLookItemsThatAreNotObjects() throws {
        let json = #"{"schema":2,"id":"x","name":"n","mode":"kana","look":5,"roles":[],"rules":{},"metrics":3,"background":"x",\#(minimalRows)}"#
        let def = try decodeLayout(json)
        XCTAssertNil(def.look)
        XCTAssertNil(def.roles)
        XCTAssertTrue(def.rules.isEmpty)
        XCTAssertNil(def.metrics)
        XCTAssertNil(def.background)
        XCTAssertNil(def.validationError())
    }

    func testRowAndKeyLookAreReadAndBrokenOnesIgnored() throws {
        let json = #"""
        {"schema":2,"id":"x","name":"n","mode":"kana","rows":[
        {"look":{"opacity":0.5},"keys":[
        {"id":"a","kind":"text","text":"a","look":{"scale":1.1},"states":{"pressed":{"scale":0.9,"holdMs":300},"nope":{"scale":2}}},
        {"id":"b","kind":"text","text":"b","look":7,"states":"x"}]}]}
        """#
        let def = try decodeLayout(json)
        XCTAssertEqual(def.rows[0].look?.number("opacity"), 0.5)
        let a = def.rows[0].keys[0]
        XCTAssertEqual(a.look?.number("scale"), 1.1)
        XCTAssertEqual(a.states?.byState[.pressed]?.look.number("scale"), 0.9)
        XCTAssertEqual(a.states?.byState[.pressed]?.holdMs, 300)
        XCTAssertEqual(a.states?.byState.count, 1)
        let b = def.rows[0].keys[1]
        XCTAssertNil(b.look)
        XCTAssertNil(b.states)
        XCTAssertNil(def.validationError())
    }

    func testRolesIgnoreUnknownRoles() throws {
        let def = try decodeLayout(#"{"schema":2,"id":"x","name":"n","mode":"kana","roles":{"function":{"scale":1.1},"bogus":{"scale":2},"accent":9},\#(minimalRows)}"#)
        XCTAssertEqual(def.roles?.byRole.count, 1)
        XCTAssertEqual(def.roles?.byRole[.function]?.number("scale"), 1.1)
    }

    func testRulesKeepOnlyReadableOnesButCountAll() throws {
        let json = #"""
        {"schema":2,"id":"x","name":"n","mode":"kana","rules":[
        {"when":"pressed","look":{"scale":0.9}},
        {"when":"zzz","look":{}},
        5,
        {"when":"shift","target":"nope","look":{}},
        {"when":"released","holdMs":150,"target":{"keys":["a"]},"look":{"animationMs":100}}],
        \#(minimalRows)}
        """#
        let def = try decodeLayout(json)
        XCTAssertEqual(def.rulesRawCount, 5)
        XCTAssertEqual(def.rules.count, 2)
        XCTAssertEqual(def.rules[0].when, .pressed)
        XCTAssertEqual(def.rules[0].target, .all)
        XCTAssertEqual(def.rules[1].when, .released)
        XCTAssertEqual(def.rules[1].target, .keys(["a"]))
        XCTAssertEqual(def.rules[1].holdMs, 150)
    }

    func testTargetForms() throws {
        func target(_ json: String) -> LookTarget? {
            let text = #"{"schema":2,"id":"x","name":"n","mode":"kana","rules":[{"when":"pressed","target":\#(json)}],\#(minimalRows)}"#
            return (try? decodeLayout(text))?.rules.first?.target
        }
        XCTAssertEqual(target(#""all""#), .all)
        XCTAssertEqual(target(#""others""#), .others)
        XCTAssertEqual(target(#"{"keys":["a","b"]}"#), .keys(["a", "b"]))
        XCTAssertEqual(target(#"{"roles":["function","bogus"]}"#), .roles([.function]))
        XCTAssertEqual(target(#"{"actions":["space"]}"#), .actions(["space"]))
        XCTAssertNil(target(#""everyone""#))
        XCTAssertNil(target(#"{"keys":["a"],"roles":["function"]}"#))
        XCTAssertNil(target(#"{"nope":[]}"#))
        XCTAssertNil(target("5"))
    }

    func testMetricsOutOfRangeAreDropped() throws {
        let def = try decodeLayout(#"{"schema":2,"id":"x","name":"n","mode":"kana","metrics":{"keySpacing":8,"rowSpacing":9,"padding":"x"},\#(minimalRows)}"#)
        XCTAssertEqual(def.metrics?.keySpacing, 8)
        XCTAssertNil(def.metrics?.rowSpacing)
        XCTAssertNil(def.metrics?.padding)
        let big = try decodeLayout(#"{"schema":2,"id":"x","name":"n","mode":"kana","metrics":{"padding":16},\#(minimalRows)}"#)
        XCTAssertEqual(big.metrics?.padding, 16)
    }

    func testBackgroundKeepsOnlyBackgroundProperties() throws {
        let def = try decodeLayout(#"{"schema":2,"id":"x","name":"n","mode":"kana","background":{"fill":"@clear","fillImage":"wood","fillImageFit":"fit","opacity":0.5,"scale":2},\#(minimalRows)}"#)
        XCTAssertEqual(Set((def.background?.look.values ?? [:]).keys), ["fill", "fillImage", "fillImageFit"])
    }

    func testAssetsOfWrongTypeFailToDecode() {
        XCTAssertThrowsError(try decodeLayout(#"{"schema":2,"id":"x","name":"n","mode":"kana","assets":["a"],\#(minimalRows)}"#))
        XCTAssertThrowsError(try decodeLayout(#"{"schema":2,"id":"x","name":"n","mode":"kana","assets":{"a":5},\#(minimalRows)}"#))
    }

    func testSchema1IsRejected() {
        let json = #"{"schema":1,"id":"x","name":"n","mode":"kana",\#(minimalRows)}"#
        XCTAssertThrowsError(try LayoutLoader.decode(Data(json.utf8))) { error in
            let message = (error as? LayoutLoader.LoadError)?.message ?? ""
            XCTAssertTrue(message.contains("未対応のスキーマ 1"), message)
            XCTAssertTrue(message.contains("schema 2"), message)
        }
    }

    func testStyleIsNoLongerReadAsRole() throws {
        // 旧名 `style` は読まない（読み込みは role だけ）
        let def = try decodeLayout(#"{"schema":2,"id":"x","name":"n","mode":"kana","rows":[{"keys":[{"id":"a","kind":"text","text":"a","style":"accent"}]}]}"#)
        XCTAssertEqual(def.rows[0].keys[0].role, .normal)
    }
}

// MARK: - 色

final class LookColorTests: XCTestCase {
    func testHexForms() {
        XCTAssertEqual(LookColor.parseSolid("#abc"), .rgba(r: 170, g: 187, b: 204, a: 255))
        XCTAssertEqual(LookColor.parseSolid("#1a2B3c"), .rgba(r: 26, g: 43, b: 60, a: 255))
        XCTAssertEqual(LookColor.parseSolid("#11223344"), .rgba(r: 17, g: 34, b: 51, a: 68))
        XCTAssertEqual(LookColor.parseSolid("#000"), .rgba(r: 0, g: 0, b: 0, a: 255))
        XCTAssertEqual(LookColor.parseSolid("#FFFFFFFF"), .rgba(r: 255, g: 255, b: 255, a: 255))
    }

    func testTokens() {
        for name in ["label", "secondaryLabel", "key", "function", "accent", "separator", "clear"] {
            XCTAssertEqual(LookColor.parseSolid("@" + name), .token(name), name)
        }
    }

    func testInvalidSolidsAreNil() {
        for bad in ["", "#", "#12", "#1234", "#12345", "#1234567", "#123456789", "#GG0", "#gggggg", "abc", "@unknown", "@", "@Label", "red", " #fff", "#fff "] {
            XCTAssertNil(LookColor.parseSolid(bad), bad)
        }
    }

    private func fill(_ json: String) -> LookColor? {
        let look = decodeLook(#"{"fill":\#(json)}"#)
        if case .color(let color)? = look.values["fill"] { return color }
        return nil
    }

    func testPairs() {
        XCTAssertEqual(fill(##"{"light":"#fff","dark":"@key"}"##),
                       .pair(light: .rgba(r: 255, g: 255, b: 255, a: 255), dark: .token("key")))
        XCTAssertEqual(fill(##"{"light":"#fff"}"##), .pair(light: .rgba(r: 255, g: 255, b: 255, a: 255), dark: nil))
        XCTAssertEqual(fill(##"{"dark":"#000"}"##), .pair(light: nil, dark: .rgba(r: 0, g: 0, b: 0, a: 255)))
    }

    func testInvalidPairsAndTypesAreNil() {
        XCTAssertNil(fill(#"{}"#))
        XCTAssertNil(fill(#"{"light":"nope"}"#))
        XCTAssertNil(fill(##"{"light":"#fff","dark":"nope"}"##))
        XCTAssertNil(fill(##"{"light":"#fff","extra":"#000"}"##))
        XCTAssertNil(fill(#"{"light":5}"#))
        XCTAssertNil(fill(##"{"light":{"light":"#fff"}}"##))
        XCTAssertNil(fill("5"))
        XCTAssertNil(fill("null"))
        XCTAssertNil(fill("[]"))
        XCTAssertNil(fill("\"\""))
    }

    func testResolvedForScheme() {
        let pair = LookColor.pair(light: .token("key"), dark: nil)
        XCTAssertEqual(pair.resolved(for: .light), .token("key"))
        XCTAssertNil(pair.resolved(for: .dark))
        XCTAssertEqual(LookColor.solid(.token("accent")).resolved(for: .dark), .token("accent"))
    }
}

// MARK: - 範囲

final class LookSanitizeTests: XCTestCase {
    func testEveryNumericPropertyAcceptsItsBoundsAndRejectsJustOutside() {
        var checked = 0
        for (name, spec) in KeyLookLimits.specs {
            guard case .number(let range) = spec.kind else { continue }
            checked += 1
            let inside = KeyLook(values: [name: .number(range.lowerBound)])
            XCTAssertEqual(LookSanitizer.sanitize(inside, allowsContent: false), inside, "\(name) 下限")
            let upper = KeyLook(values: [name: .number(range.upperBound)])
            XCTAssertEqual(LookSanitizer.sanitize(upper, allowsContent: false), upper, "\(name) 上限")
            for outside in [range.lowerBound - 0.01, range.upperBound + 0.01, .infinity, -.infinity, .nan] {
                let look = KeyLook(values: [name: .number(outside)])
                XCTAssertTrue(LookSanitizer.sanitize(look, allowsContent: false).isEmpty, "\(name) \(outside)")
            }
        }
        XCTAssertEqual(checked, 16)
    }

    func testRangesMatchTheSpec() {
        func range(_ name: String) -> ClosedRange<Double>? {
            if case .number(let r)? = KeyLookLimits.specs[name]?.kind { return r }
            return nil
        }
        XCTAssertEqual(range("fontSize"), 8.0...40.0)
        XCTAssertEqual(range("fillOpacity"), 0.0...1.0)
        XCTAssertEqual(range("cornerRadius"), 0.0...24.0)
        XCTAssertEqual(range("borderWidth"), 0.0...4.0)
        XCTAssertEqual(range("shadowRadius"), 0.0...16.0)
        XCTAssertEqual(range("shadowX"), -8.0...8.0)
        XCTAssertEqual(range("shadowY"), -8.0...8.0)
        XCTAssertEqual(range("scale"), 0.5...1.5)
        XCTAssertEqual(range("offsetX"), -20.0...20.0)
        XCTAssertEqual(range("offsetY"), -20.0...20.0)
        XCTAssertEqual(range("inset"), 0.0...12.0)
        XCTAssertEqual(range("padding"), 0.0...16.0)
        XCTAssertEqual(range("animationMs"), 0.0...1000.0)
    }

    func testChoicesAndImageRefs() {
        func kept(_ name: String, _ value: String) -> Bool {
            !LookSanitizer.sanitize(KeyLook(values: [name: .string(value)]), allowsContent: false).isEmpty
        }
        XCTAssertTrue(kept("fontWeight", "ultraLight"))
        XCTAssertTrue(kept("fontWeight", "black"))
        XCTAssertFalse(kept("fontWeight", "extra"))
        XCTAssertFalse(kept("fontWeight", "Bold"))
        XCTAssertTrue(kept("fontDesign", "rounded"))
        XCTAssertFalse(kept("fontDesign", "fancy"))
        XCTAssertTrue(kept("fillImageFit", "tile"))
        XCTAssertFalse(kept("fillImageFit", "stretch"))
        XCTAssertTrue(kept("fillImage", "wood"))
        XCTAssertTrue(kept("fillImage", "none"))
        XCTAssertTrue(kept("fillImage", "a_b-1"))
        XCTAssertFalse(kept("fillImage", "Bad Id"))
        XCTAssertFalse(kept("fillImage", ""))
        XCTAssertFalse(kept("fillImage", String(repeating: "a", count: 33)))
    }

    func testWrongValueKindIsRejected() {
        let wrong = KeyLook(values: ["fontSize": .string("12"), "fill": .number(1), "fontWeight": .number(1)])
        XCTAssertTrue(LookSanitizer.sanitize(wrong, allowsContent: true).isEmpty)
    }

    func testLabelAndIconOnlyInStatesAndRules() {
        let look = KeyLook(values: ["label": .string("移動"), "icon": .string("sf:globe"), "scale": .number(1)])
        XCTAssertEqual(LookSanitizer.sanitize(look, allowsContent: false).values, ["scale": .number(1)])
        XCTAssertEqual(LookSanitizer.sanitize(look, allowsContent: true), look)
    }

    func testLabelAndIconFormats() {
        func kept(_ name: String, _ value: String) -> Bool {
            !LookSanitizer.sanitize(KeyLook(values: [name: .string(value)]), allowsContent: true).isEmpty
        }
        XCTAssertTrue(kept("label", "あ"))
        XCTAssertTrue(kept("label", String(repeating: "あ", count: 16)))
        XCTAssertFalse(kept("label", String(repeating: "あ", count: 17)))
        XCTAssertFalse(kept("label", ""))
        XCTAssertTrue(kept("icon", "sf:globe"))
        XCTAssertTrue(kept("icon", "shift"))
        XCTAssertTrue(kept("icon", "iconify:mdi:home"))
        XCTAssertFalse(kept("icon", "iconify:mdi"))
        XCTAssertFalse(kept("icon", "sf:"))
        XCTAssertFalse(kept("icon", ""))
        XCTAssertFalse(kept("icon", String(repeating: "a", count: 65)))
    }
}

// MARK: - 既定値（現行の見た目）

final class LookDefaultsTests: XCTestCase {
    private func key(_ kind: LayoutKeyKind, label: String?) -> LayoutKeyDef {
        LayoutKeyDef(id: "k", kind: kind, label: label, action: nil)
    }

    func testFontSizeMatchesTheCurrentRule() {
        XCTAssertEqual(LookDefaults.fontSize(for: key(.text, label: "a")), 20)
        XCTAssertEqual(LookDefaults.fontSize(for: key(.text, label: "ab")), 20)
        XCTAssertEqual(LookDefaults.fontSize(for: key(.text, label: "abc")), 16)
        XCTAssertEqual(LookDefaults.fontSize(for: key(.action, label: "空白")), 17)
        XCTAssertEqual(LookDefaults.fontSize(for: key(.action, label: "123")), 16)
        XCTAssertEqual(LookDefaults.fontSize(for: key(.toggle, label: nil)), 17)
        var flick = key(.flick, label: "あ")
        XCTAssertEqual(LookDefaults.fontSize(for: flick), 22)
        flick.label = "、。？！"
        XCTAssertEqual(LookDefaults.fontSize(for: flick), 16)
        flick.label = nil
        flick.kana = "か"
        XCTAssertEqual(LookDefaults.fontSize(for: flick), 22)
    }

    func testRoleColors() {
        XCTAssertEqual(LookDefaults.fill(role: .normal), .token("key"))
        XCTAssertEqual(LookDefaults.fill(role: .function), .token("function"))
        XCTAssertEqual(LookDefaults.fill(role: .accent), .token("accent"))
        XCTAssertEqual(LookDefaults.color(role: .normal), .token("label"))
        XCTAssertEqual(LookDefaults.color(role: .function), .token("label"))
        XCTAssertEqual(LookDefaults.color(role: .accent), .rgba(r: 255, g: 255, b: 255, a: 255))
    }

    func testPressedAndCornerDefaults() {
        XCTAssertEqual(LookDefaults.pressedFillOpacity, 0.6)
        XCTAssertEqual(LookDefaults.pressedLook.number("fillOpacity"), 0.6)
        XCTAssertEqual(LookDefaults.cornerRadius, NagiIOSSettings().keyCornerPt)
        XCTAssertEqual(LookDefaults.iconSize, 17)
    }
}
