import XCTest

/// 曖昧入力のためのレイアウト幾何（`LayoutGeometry`）と、タッチ座標・フリック角度（`TouchGeometry`）
final class LayoutGeometryTests: XCTestCase {
    private func textKey(_ text: String, width: Double = 1, kind: LayoutKeyKind = .text) -> LayoutKeyDef {
        var key = LayoutKeyDef(id: text, kind: kind, label: nil, action: nil, width: width)
        key.text = text
        return key
    }

    private func spacer(_ id: String, width: Double) -> LayoutKeyDef {
        LayoutKeyDef(id: id, kind: .spacer, label: nil, action: nil, width: width)
    }

    /// 同梱の qwerty-alpha と同じ形（10 / 9+スペーサ / シフト+7+⌫ / 機能行）
    private func qwerty() -> KeyboardLayoutDef {
        let top = "qwertyuiop".map { textKey(String($0)) }
        let mid = [spacer("sl", width: 0.5)] + "asdfghjkl".map { textKey(String($0)) } + [spacer("sr", width: 0.5)]
        let bottom = [LayoutKeyDef(id: "sh", kind: .action, label: "⇧", action: "shift", width: 1.4)]
            + "zxcvbnm".map { textKey(String($0)) }
            + [LayoutKeyDef(id: "bs", kind: .action, label: "⌫", action: "backspace", width: 1.4)]
        let last = [
            LayoutKeyDef(id: "sp", kind: .action, label: "空白", action: "space", width: 4.4),
            LayoutKeyDef(id: "en", kind: .action, label: "改行", action: "enter", width: 1.6),
        ]
        return KeyboardLayoutDef(id: "t-qwerty", name: "t", mode: .alpha, rows: [
            LayoutRow(height: 1, keys: top), LayoutRow(height: 1, keys: mid),
            LayoutRow(height: 1, keys: bottom), LayoutRow(height: 1, keys: last),
        ])
    }

    func testOnlyLetterKeysAreIncluded() {
        let rects = LayoutGeometry.rects(layout: qwerty(), size: CGSize(width: 400, height: 200), keySpacing: 0, rowSpacing: 0)
        XCTAssertEqual(rects.count, 26)
        XCTAssertEqual(Set(rects.map(\.label)).count, 26)
        XCTAssertFalse(rects.contains { $0.id == "sh" || $0.id == "bs" || $0.id == "sp" })
    }

    func testRectsWithoutSpacingFollowRatios() {
        let rects = LayoutGeometry.rects(layout: qwerty(), size: CGSize(width: 400, height: 200), keySpacing: 0, rowSpacing: 0)
        let byLabel = Dictionary(uniqueKeysWithValues: rects.map { ($0.label, $0) })
        // 1 行目: 幅 400 を 10 等分、高さは 200 / 4
        XCTAssertEqual(byLabel["q"]?.x ?? -1, 0, accuracy: 0.001)
        XCTAssertEqual(byLabel["q"]?.w ?? -1, 40, accuracy: 0.001)
        XCTAssertEqual(byLabel["p"]?.x ?? -1, 360, accuracy: 0.001)
        XCTAssertEqual(byLabel["q"]?.h ?? -1, 50, accuracy: 0.001)
        // 2 行目: 先頭のスペーサ 0.5 の分だけ右へずれる（合計 10 → 1 比 = 40）
        XCTAssertEqual(byLabel["a"]?.x ?? -1, 20, accuracy: 0.001)
        XCTAssertEqual(byLabel["a"]?.y ?? -1, 50, accuracy: 0.001)
        // 3 行目: シフト 1.4 の後に z（合計 1.4 + 7 + 1.4 = 9.8）
        XCTAssertEqual(byLabel["z"]?.x ?? -1, 400 * 1.4 / 9.8, accuracy: 0.001)
        XCTAssertEqual(byLabel["z"]?.y ?? -1, 100, accuracy: 0.001)
    }

    func testSpacingIsSubtractedLikeTheRenderer() {
        // 描画（RowView）は、間隔を除いた幅・高さを比で按分し、キーの間に間隔を入れる
        let rects = LayoutGeometry.rects(layout: qwerty(), size: CGSize(width: 409, height: 206), keySpacing: 4, rowSpacing: 4)
        let byLabel = Dictionary(uniqueKeysWithValues: rects.map { ($0.label, $0) })
        // 1 行目: (409 - 4*9) / 10 = 37.3
        XCTAssertEqual(byLabel["q"]?.w ?? -1, 37.3, accuracy: 0.001)
        XCTAssertEqual(byLabel["w"]?.x ?? -1, 37.3 + 4, accuracy: 0.001)
        // 行の高さ: (206 - 4*3) / 4 = 48.5
        XCTAssertEqual(byLabel["q"]?.h ?? -1, 48.5, accuracy: 0.001)
        XCTAssertEqual(byLabel["a"]?.y ?? -1, 48.5 + 4, accuracy: 0.001)
        XCTAssertEqual(byLabel["z"]?.y ?? -1, (48.5 + 4) * 2, accuracy: 0.001)
    }

    func testKeySpacingAndRowSpacingAreIndependent() {
        let rects = LayoutGeometry.rects(layout: qwerty(), size: CGSize(width: 409, height: 206), keySpacing: 4, rowSpacing: 10)
        let byLabel = Dictionary(uniqueKeysWithValues: rects.map { ($0.label, $0) })
        // 幅はキー間隔 4 だけで按分（行間隔は影響しない）: (409 - 4*9) / 10 = 37.3
        XCTAssertEqual(byLabel["q"]?.w ?? -1, 37.3, accuracy: 0.001)
        XCTAssertEqual(byLabel["w"]?.x ?? -1, 37.3 + 4, accuracy: 0.001)
        // 高さは行間隔 10 だけで按分: (206 - 10*3) / 4 = 44
        XCTAssertEqual(byLabel["q"]?.h ?? -1, 44, accuracy: 0.001)
        XCTAssertEqual(byLabel["a"]?.y ?? -1, 44 + 10, accuracy: 0.001)
        XCTAssertEqual(byLabel["z"]?.y ?? -1, (44 + 10) * 2, accuracy: 0.001)
    }

    func testSameSpacingGivesTheSameResultAsBefore() {
        let size = CGSize(width: 400, height: 200)
        XCTAssertEqual(
            LayoutGeometry.rects(layout: qwerty(), size: size, keySpacing: 6, rowSpacing: 6),
            LayoutGeometry.rects(layout: qwerty(), size: size, keySpacing: 6, rowSpacing: 6)
        )
        let a = LayoutGeometry.snapshot(layout: qwerty(), size: size, keySpacing: 4, rowSpacing: 4)
        let b = LayoutGeometry.snapshot(layout: qwerty(), size: size, keySpacing: 4, rowSpacing: 5)
        XCTAssertNotNil(a)
        XCTAssertNotEqual(a, b)
    }

    func testRowHeightRatio() {
        var def = qwerty()
        def.rows[0].height = 2
        let rects = LayoutGeometry.rects(layout: def, size: CGSize(width: 100, height: 500), keySpacing: 0, rowSpacing: 0)
        let byLabel = Dictionary(uniqueKeysWithValues: rects.map { ($0.label, $0) })
        // 高さの比の合計は 5。1 行目は 200、2 行目はその下から 100
        XCTAssertEqual(byLabel["q"]?.h ?? -1, 200, accuracy: 0.001)
        XCTAssertEqual(byLabel["a"]?.y ?? -1, 200, accuracy: 0.001)
        XCTAssertEqual(byLabel["a"]?.h ?? -1, 100, accuracy: 0.001)
    }

    func testUppercaseLabelIsLowercased() {
        var key = LayoutKeyDef(id: "k", kind: .text, label: "K", action: nil)
        key.text = nil
        XCTAssertEqual(LayoutGeometry.letterLabel(key), "k")
    }

    func testNonLetterTextKeysAreExcluded() {
        XCTAssertNil(LayoutGeometry.letterLabel(textKey("1")))
        XCTAssertNil(LayoutGeometry.letterLabel(textKey("あ")))
        XCTAssertNil(LayoutGeometry.letterLabel(textKey("ab")))
        XCTAssertNil(LayoutGeometry.letterLabel(textKey("q", kind: .flick)))
        XCTAssertEqual(LayoutGeometry.letterLabel(textKey("q")), "q")
    }

    func testSnapshotSkipsLayoutsWithFewLetterKeys() {
        let kana = KeyboardLayoutDef(id: "k", name: "k", mode: .kana, rows: [
            LayoutRow(height: 1, keys: [textKey("a"), textKey("b"), textKey("c")]),
        ])
        XCTAssertNil(LayoutGeometry.snapshot(layout: kana, size: CGSize(width: 300, height: 100), keySpacing: 4, rowSpacing: 4))
        let snapshot = LayoutGeometry.snapshot(layout: qwerty(), size: CGSize(width: 400, height: 200), keySpacing: 4, rowSpacing: 4)
        XCTAssertEqual(snapshot?.layoutId, "t-qwerty")
        XCTAssertEqual(snapshot?.mode, "qwerty")
        XCTAssertEqual(snapshot?.keys.count, 26)
        XCTAssertEqual(snapshot?.width, 400)
        XCTAssertEqual(snapshot?.height, 200)
    }

    func testSnapshotIsEquatableForDeduplication() {
        let a = LayoutGeometry.snapshot(layout: qwerty(), size: CGSize(width: 400, height: 200), keySpacing: 4, rowSpacing: 4)
        let b = LayoutGeometry.snapshot(layout: qwerty(), size: CGSize(width: 400, height: 200), keySpacing: 4, rowSpacing: 4)
        let c = LayoutGeometry.snapshot(layout: qwerty(), size: CGSize(width: 400, height: 210), keySpacing: 4, rowSpacing: 4)
        XCTAssertNotNil(a)
        XCTAssertEqual(a, b)
        XCTAssertNotEqual(a, c)
    }

    func testEmptyOrZeroSizeGivesNothing() {
        XCTAssertTrue(LayoutGeometry.rects(layout: qwerty(), size: .zero, keySpacing: 4, rowSpacing: 4).isEmpty)
        XCTAssertNil(LayoutGeometry.snapshot(layout: qwerty(), size: CGSize(width: 0, height: 100), keySpacing: 4, rowSpacing: 4))
    }

    func testEmojiLayoutIsSkipped() {
        var def = qwerty()
        def.mode = .emoji
        XCTAssertNil(LayoutGeometry.snapshot(layout: def, size: CGSize(width: 400, height: 200), keySpacing: 4, rowSpacing: 4))
    }

    // MARK: - タッチ座標

    func testRelativeTouchPoint() {
        let center = TouchGeometry.relative(x: 20, y: 25, width: 40, height: 50)
        XCTAssertEqual(center?.x ?? -1, 0.5, accuracy: 0.0001)
        XCTAssertEqual(center?.y ?? -1, 0.5, accuracy: 0.0001)
        let corner = TouchGeometry.relative(x: 0, y: 50, width: 40, height: 50)
        XCTAssertEqual(corner?.x ?? -1, 0, accuracy: 0.0001)
        XCTAssertEqual(corner?.y ?? -1, 1, accuracy: 0.0001)
    }

    func testRelativeTouchPointClampsAndRejectsEmptyKey() {
        let out = TouchGeometry.relative(x: -5, y: 80, width: 40, height: 50)
        XCTAssertEqual(out?.x ?? -1, 0, accuracy: 0.0001)
        XCTAssertEqual(out?.y ?? -1, 1, accuracy: 0.0001)
        XCTAssertNil(TouchGeometry.relative(x: 1, y: 1, width: 0, height: 50))
    }

    // MARK: - フリック角度

    func testFlickAngleFollowsCounterClockwiseConvention() {
        // 画面座標は下向きが正。右 0、上 90、左 180、下 270
        XCTAssertEqual(TouchGeometry.flick(dx: 30, dy: 0, keyWidth: 60)?.angleDeg ?? -1, 0, accuracy: 0.001)
        XCTAssertEqual(TouchGeometry.flick(dx: 0, dy: -30, keyWidth: 60)?.angleDeg ?? -1, 90, accuracy: 0.001)
        XCTAssertEqual(TouchGeometry.flick(dx: -30, dy: 0, keyWidth: 60)?.angleDeg ?? -1, 180, accuracy: 0.001)
        XCTAssertEqual(TouchGeometry.flick(dx: 0, dy: 30, keyWidth: 60)?.angleDeg ?? -1, 270, accuracy: 0.001)
        XCTAssertEqual(TouchGeometry.flick(dx: 30, dy: -30, keyWidth: 60)?.angleDeg ?? -1, 45, accuracy: 0.001)
    }

    func testFlickDistanceIsInKeyWidths() {
        XCTAssertEqual(TouchGeometry.flick(dx: 30, dy: 40, keyWidth: 100)?.distance ?? -1, 0.5, accuracy: 0.0001)
        XCTAssertNil(TouchGeometry.flick(dx: 0, dy: 0, keyWidth: 100))
        XCTAssertNil(TouchGeometry.flick(dx: 10, dy: 0, keyWidth: 0))
    }
}
