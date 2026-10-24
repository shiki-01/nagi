import XCTest

// 解決済みの見た目 → 描画パラメータの変換、表示内容の上書き、背景、余白（docs/keyboard-style-spec.md §2、§3.5、§6.4、§7.2）。

private func layout(_ extra: String = "", rows: String? = nil) throws -> KeyboardLayoutDef {
    let defaultRows = #""rows":[{"keys":[{"id":"a","kind":"text","text":"a"},{"id":"f","kind":"action","action":"enter","label":"改行","role":"function"}]}]"#
    let json = #"{"schema":2,"id":"x","name":"n","mode":"kana",\#(extra)\#(rows ?? defaultRows)}"#
    return try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(json.utf8))
}

private func params(
    _ def: KeyboardLayoutDef, key id: String, states: Set<LookStateName> = [], scheme: LookScheme = .light, active: Bool = false
) -> KeyRenderParams {
    let key = def.rows[0].keys.first { $0.id == id }!
    let resolved = LookResolver.resolve(
        layout: def, rowIndex: 0, key: key, context: LookContext(states: states, origin: nil, scheme: scheme)
    )
    return KeyRenderParams.make(look: resolved, key: key, active: active, cornerRadius: 6)
}

final class KeyRenderParamsDefaultsTests: XCTestCase {
    func testNothingSpecifiedGivesTheCurrentLook() throws {
        let p = params(try layout(), key: "a")
        XCTAssertEqual(p.fill, .token("key"))
        XCTAssertEqual(p.color, .token("label"))
        XCTAssertEqual(p.fontSize, 20)
        XCTAssertEqual(p.iconSize, 17)
        XCTAssertEqual(p.fontWeight, "regular")
        XCTAssertEqual(p.fontDesign, "system")
        XCTAssertEqual(p.cornerRadius, 6)
        XCTAssertEqual(p.fillOpacity, 1)
        XCTAssertNil(p.fillImage)
        XCTAssertFalse(p.hasBorder)
        XCTAssertFalse(p.hasShadow)
        XCTAssertEqual(p.opacity, 1)
        XCTAssertEqual(p.scale, 1)
        XCTAssertEqual(p.offsetX, 0)
        XCTAssertEqual(p.offsetY, 0)
        XCTAssertEqual(p.inset, 0)
        XCTAssertEqual(p.padding, 0)
        XCTAssertEqual(p.animationMs, 0)
    }

    func testRoleColorsAreTheCurrentOnes() throws {
        let def = try layout()
        XCTAssertEqual(params(def, key: "f").fill, .token("function"))
        XCTAssertEqual(params(def, key: "f").color, .token("label"))
        XCTAssertEqual(params(def, key: "f").fontSize, 17)
    }

    func testActiveShiftKeyUsesTheAccentColors() throws {
        let p = params(try layout(), key: "f", active: true)
        XCTAssertEqual(p.fill, .token("accent"))
        XCTAssertEqual(p.color, .rgba(r: 255, g: 255, b: 255, a: 255))
    }

    func testPressedKeepsTheCurrentBackgroundOpacity() throws {
        let p = params(try layout(), key: "a", states: [.pressed])
        XCTAssertEqual(p.fillOpacity, 0.6)
        // ほかは変わらない
        XCTAssertEqual(p.fill, .token("key"))
    }

    func testLayoutLookDoesNotRemoveThePressedDefault() throws {
        let p = params(try layout(#""look":{"fillOpacity":0.8},"#), key: "a", states: [.pressed])
        XCTAssertEqual(p.fillOpacity, 0.6)
        XCTAssertEqual(params(try layout(#""look":{"fillOpacity":0.8},"#), key: "a").fillOpacity, 0.8)
    }
}

final class KeyRenderParamsStyleTests: XCTestCase {
    func testLayoutRoleRowAndKeyLooksAreApplied() throws {
        let def = try layout(
            #""look":{"cornerRadius":12,"fontDesign":"rounded","shadowRadius":2,"shadowY":1},"roles":{"function":{"fontWeight":"medium"}},"#,
            rows: #""rows":[{"look":{"padding":3},"keys":[{"id":"a","kind":"text","text":"a","look":{"scale":0.9,"inset":2}},{"id":"f","kind":"action","action":"enter","label":"改行","role":"function"}]}]"#
        )
        let a = params(def, key: "a")
        XCTAssertEqual(a.cornerRadius, 12)
        XCTAssertEqual(a.fontDesign, "rounded")
        XCTAssertEqual(a.fontWeight, "regular")
        XCTAssertTrue(a.hasShadow)
        XCTAssertEqual(a.shadowRadius, 2)
        XCTAssertEqual(a.shadowY, 1)
        XCTAssertEqual(a.shadowOpacity, 0.3)
        XCTAssertEqual(a.padding, 3)
        XCTAssertEqual(a.scale, 0.9)
        XCTAssertEqual(a.inset, 2)
        XCTAssertEqual(params(def, key: "f").fontWeight, "medium")
    }

    func testFontSizeAlsoSizesTheIcon() throws {
        let p = params(try layout(#""look":{"fontSize":30},"#), key: "a")
        XCTAssertEqual(p.fontSize, 30)
        XCTAssertEqual(p.iconSize, 30)
    }

    func testColorsFollowTheScheme() throws {
        let def = try layout(##""roles":{"normal":{"fill":{"light":"#FFFFFF","dark":"#3A3A3C"}}},"##)
        XCTAssertEqual(params(def, key: "a", scheme: .light).fill, .rgba(r: 255, g: 255, b: 255, a: 255))
        XCTAssertEqual(params(def, key: "a", scheme: .dark).fill, .rgba(r: 0x3A, g: 0x3A, b: 0x3C, a: 255))
    }

    func testOneSidedColorFallsBackToTheDefaultOnTheOtherScheme() throws {
        let def = try layout(##""look":{"fill":{"light":"#FF0000"}},"##)
        XCTAssertEqual(params(def, key: "a", scheme: .light).fill, .rgba(r: 255, g: 0, b: 0, a: 255))
        XCTAssertEqual(params(def, key: "a", scheme: .dark).fill, .token("key"))
    }

    func testBorderAndImageParams() throws {
        let def = try layout(#""look":{"borderWidth":1.5,"borderColor":"@accent","fillImage":"wood","fillImageFit":"tile","fillImageOpacity":0.5},"#)
        let p = params(def, key: "a")
        XCTAssertTrue(p.hasBorder)
        XCTAssertEqual(p.borderColor, .token("accent"))
        XCTAssertEqual(p.fillImage, "wood")
        XCTAssertEqual(p.fillImageFit, "tile")
        XCTAssertEqual(p.fillImageOpacity, 0.5)
    }

    func testInvalidValuesFallBackToTheDefault() throws {
        let p = params(try layout(#""look":{"cornerRadius":99,"scale":3,"fontWeight":"fat"},"#), key: "a")
        XCTAssertEqual(p.cornerRadius, 6)
        XCTAssertEqual(p.scale, 1)
        XCTAssertEqual(p.fontWeight, "regular")
    }

    func testZeroShadowRadiusMeansNoShadow() throws {
        let p = params(try layout(#""look":{"shadowRadius":0,"shadowOpacity":1},"#), key: "a")
        XCTAssertFalse(p.hasShadow)
    }
}

final class KeyFaceContentMergeTests: XCTestCase {
    private let base = KeyFaceContent(text: "空白", icon: "space")
    private func content(_ label: String?, _ icon: String?) -> LookContent {
        LookContent(label: label, icon: icon, hidesKeyIcon: label != nil && icon == nil)
    }

    func testNoOverrideKeepsTheBase() {
        XCTAssertEqual(KeyFaceContent.merged(base: base, override: content(nil, nil), iconAvailable: { _ in true }), base)
    }

    func testLabelOnlyReplacesTextAndHidesTheKeyIcon() {
        let merged = KeyFaceContent.merged(base: base, override: content("移動", nil), iconAvailable: { _ in true })
        XCTAssertEqual(merged, KeyFaceContent(text: "移動", icon: nil))
    }

    func testAvailableIconWins() {
        let merged = KeyFaceContent.merged(base: base, override: content("移動", "arrow.left.and.right"), iconAvailable: { _ in true })
        XCTAssertEqual(merged.icon, "arrow.left.and.right")
    }

    func testUnavailableIconFallsBackToTheLabel() {
        let merged = KeyFaceContent.merged(base: base, override: content("移動", "iconify:mdi:home"), iconAvailable: { _ in false })
        XCTAssertEqual(merged, KeyFaceContent(text: "移動", icon: nil))
    }

    func testUnavailableIconWithoutLabelKeepsTheBase() {
        let merged = KeyFaceContent.merged(base: base, override: content(nil, "iconify:mdi:home"), iconAvailable: { _ in false })
        XCTAssertEqual(merged, base)
    }
}

final class BackgroundAndMetricsTests: XCTestCase {
    func testNoBackgroundMeansNothingIsPainted() throws {
        XCTAssertNil(BackgroundParams.make(try layout().background, scheme: .light))
        // 指定があっても色も画像も無ければ塗らない
        XCTAssertNil(BackgroundParams.make(try layout(#""background":{"fillOpacity":0.5},"#).background, scheme: .light))
    }

    func testBackgroundFillFollowsTheScheme() throws {
        let def = try layout(##""background":{"fill":{"light":"#EEEEEE","dark":"#111111"},"fillOpacity":0.8},"##)
        let light = BackgroundParams.make(def.background, scheme: .light)
        XCTAssertEqual(light?.fill, .rgba(r: 0xEE, g: 0xEE, b: 0xEE, a: 255))
        XCTAssertEqual(light?.fillOpacity, 0.8)
        XCTAssertEqual(BackgroundParams.make(def.background, scheme: .dark)?.fill, .rgba(r: 0x11, g: 0x11, b: 0x11, a: 255))
    }

    func testBackgroundImageDefaults() throws {
        let def = try layout(#""background":{"fillImage":"sky"},"#)
        let p = BackgroundParams.make(def.background, scheme: .light)
        XCTAssertNil(p?.fill)
        XCTAssertEqual(p?.fillImage, "sky")
        XCTAssertEqual(p?.fillImageFit, "fill")
        XCTAssertEqual(p?.fillImageOpacity, 1)
    }

    func testBackgroundNoneImageIsNoImage() throws {
        let def = try layout(##""background":{"fill":"#000000","fillImage":"none"},"##)
        XCTAssertNil(BackgroundParams.make(def.background, scheme: .light)?.fillImage)
    }

    func testMetricsDefaultsToTheSettingAndPaddingSix() throws {
        let m = ResolvedMetrics.make(nil, settingSpacing: 4)
        XCTAssertEqual(m, ResolvedMetrics(keySpacing: 4, rowSpacing: 4, padding: 6))
    }

    func testMetricsOverrideTheSettingPerItem() throws {
        let def = try layout(#""metrics":{"keySpacing":5,"padding":10},"#)
        XCTAssertEqual(
            ResolvedMetrics.make(def.metrics, settingSpacing: 4),
            ResolvedMetrics(keySpacing: 5, rowSpacing: 4, padding: 10)
        )
    }

    func testOutOfRangeMetricsAreIgnored() throws {
        let def = try layout(#""metrics":{"keySpacing":20,"rowSpacing":-1},"#)
        XCTAssertEqual(ResolvedMetrics.make(def.metrics, settingSpacing: 4), ResolvedMetrics(keySpacing: 4, rowSpacing: 4, padding: 6))
    }

    func testPadHeightUsesTheRowSpacing() throws {
        let rows = #""rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]},{"height":2,"keys":[{"id":"b","kind":"text","text":"b"}]}]"#
        let def = try layout(rows: rows)
        XCTAssertEqual(LayoutGeometry.rowsHeight(layout: def, keyHeight: 46, rowSpacing: 4), 46 * 3 + 4)
        XCTAssertEqual(LayoutGeometry.padHeight(layout: def, keyHeight: 46, rowSpacing: 6), 46 * 3 + 6)
    }

    func testEmojiPadHeightIsFourRows() throws {
        let json = #"{"schema":2,"id":"e","name":"n","mode":"emoji","rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]}]}"#
        let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(json.utf8))
        XCTAssertEqual(LayoutGeometry.padHeight(layout: def, keyHeight: 46, rowSpacing: 4), 46 * 4 + 12)
        XCTAssertEqual(LayoutGeometry.rowsHeight(layout: def, keyHeight: 46, rowSpacing: 4), 46)
    }
}
