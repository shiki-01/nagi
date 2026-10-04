import XCTest

final class LayoutFileTests: XCTestCase {
    private func loadBundled() -> LayoutStore.LoadResult {
        return LayoutStore.load(bundle: Bundle(for: LayoutFileTests.self), userDir: nil)
    }

    func testBundledLayoutsAreValid() {
        let result = loadBundled()
        XCTAssertTrue(result.errors.isEmpty, "読み込みエラー: \(result.errors)")
        let ids = Set(result.layouts.map { $0.id })
        for required in ["qwerty-romaji", "flick-12", "qwerty-direct", "symbols", "symbols-direct"] {
            XCTAssertTrue(ids.contains(required), "\(required) がありません")
        }
    }

    func testLayoutSwitchTargetsExist() {
        let result = loadBundled()
        let ids = Set(result.layouts.map { $0.id })
        for layout in result.layouts {
            for target in layout.referencedLayoutIDs {
                XCTAssertTrue(ids.contains(target), "\(layout.id) の切替先 \(target) がありません")
            }
        }
    }

    func testFramesStayInsideTheArea() {
        let size = CGSize(width: 390, height: 208)
        for layout in loadBundled().layouts {
            let frames = LayoutGeometry.frames(for: layout, in: size)
            XCTAssertFalse(frames.isEmpty, layout.id)
            for frame in frames {
                XCTAssertGreaterThanOrEqual(frame.rect.minX, -0.001, layout.id)
                XCTAssertGreaterThanOrEqual(frame.rect.minY, -0.001, layout.id)
                XCTAssertLessThanOrEqual(frame.rect.maxX, size.width + 0.001, layout.id)
                XCTAssertLessThanOrEqual(frame.rect.maxY, size.height + 0.001, layout.id)
            }
        }
    }

    func testFrameOrderDoesNotDependOnSize() {
        for layout in loadBundled().layouts {
            let a = LayoutGeometry.frames(for: layout, in: CGSize(width: 100, height: 100))
            let b = LayoutGeometry.frames(for: layout, in: CGSize(width: 390, height: 208))
            XCTAssertEqual(a.map { $0.key.id }, b.map { $0.key.id }, layout.id)
        }
    }

    func testFlickKeysHaveFiveZones() {
        guard let flick = loadBundled().layouts.first(where: { $0.id == "flick-12" }) else {
            return XCTFail("flick-12 がありません")
        }
        let frames = LayoutGeometry.frames(for: flick, in: CGSize(width: 390, height: 208))
        guard let a = frames.first(where: { $0.key.id == "kana-あ" }) else { return XCTFail("あ キーがありません") }
        let zones = FlickGeometry.zones(of: a.key)
        XCTAssertEqual(zones.map { $0.text }, ["あ", "い", "う", "え", "お"])
    }

    func testValidationRejectsBrokenLayouts() {
        let broken = KeyboardLayout(
            id: "x",
            name: "x",
            direct: nil,
            rows: [KeyRow(height: nil, keys: [
                KeyDef(id: "a", label: nil, width: nil, spacer: nil, style: nil,
                       tap: KeyAction(type: "char", text: "a", name: nil, layout: nil),
                       flick: nil, longPress: nil, repeats: nil),
                KeyDef(id: "a", label: nil, width: nil, spacer: nil, style: nil,
                       tap: KeyAction(type: "bogus", text: nil, name: nil, layout: nil),
                       flick: nil, longPress: nil, repeats: nil),
            ])]
        )
        XCTAssertFalse(broken.validationErrors().isEmpty)
    }

    func testDecodeMinimalLayoutJSON() throws {
        let json = """
        {"id":"mini","name":"mini","rows":[{"keys":[
          {"id":"a","tap":{"type":"char","text":"a"}},
          {"spacer":true,"width":0.5},
          {"id":"ka","tap":{"type":"kana","text":"か"},"flick":{"left":"き"}}
        ]}]}
        """
        let layout = try LayoutStore.decode(Data(json.utf8))
        XCTAssertTrue(layout.validationErrors().isEmpty)
        XCTAssertEqual(layout.rows[0].keys.count, 3)
        XCTAssertEqual(layout.rows[0].keys[0].tapAction, .char("a"))
        XCTAssertTrue(layout.rows[0].keys[1].isSpacer)
    }
}
