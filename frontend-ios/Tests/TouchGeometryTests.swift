import XCTest

final class TouchGeometryTests: XCTestCase {
    /// 幅 100 の領域に f と g を 1 行で並べる（各 50 x 50）
    private func twoKeyFrames() -> [KeyFrame] {
        let row = KeyRow(height: nil, keys: [
            KeyDef(id: "f", label: nil, width: nil, spacer: nil, style: nil,
                   tap: KeyAction(type: "char", text: "f", name: nil, layout: nil),
                   flick: nil, longPress: nil, repeats: nil),
            KeyDef(id: "g", label: nil, width: nil, spacer: nil, style: nil,
                   tap: KeyAction(type: "char", text: "g", name: nil, layout: nil),
                   flick: nil, longPress: nil, repeats: nil),
        ])
        let layout = KeyboardLayout(id: "t", name: "t", direct: nil, rows: [row])
        return LayoutGeometry.frames(for: layout, in: CGSize(width: 100, height: 50))
    }

    func testNormalizedDistanceUsesKeyWidth() {
        let target = TouchTarget(text: "g", center: CGPoint(x: 75, y: 25), width: 50)
        let d = TouchGeometry.normalizedDistance(from: CGPoint(x: 45, y: 25), to: target)
        XCTAssertEqual(d, 0.6, accuracy: 1e-9)
        let zero = TouchTarget(text: "g", center: .zero, width: 0)
        XCTAssertTrue(TouchGeometry.normalizedDistance(from: .zero, to: zero).isInfinite)
    }

    func testHitTestInsideAndOutside() {
        let frames = twoKeyFrames()
        XCTAssertEqual(TouchGeometry.hitTest(point: CGPoint(x: 10, y: 10), frames: frames)?.key.id, "f")
        XCTAssertEqual(TouchGeometry.hitTest(point: CGPoint(x: 60, y: 10), frames: frames)?.key.id, "g")
        // 領域の外は最も近いキー
        XCTAssertEqual(TouchGeometry.hitTest(point: CGPoint(x: 500, y: 10), frames: frames)?.key.id, "g")
        XCTAssertNil(TouchGeometry.hitTest(point: .zero, frames: []))
    }

    func testQwertyAlternativesNearbyKeyIsIncluded() {
        let frames = twoKeyFrames()
        let targets = TouchGeometry.charTargets(frames: frames, transform: { $0 })
        let alts = TouchGeometry.alternatives(touch: CGPoint(x: 45, y: 25), targets: targets, excluding: "f")
        XCTAssertEqual(alts.count, 1)
        XCTAssertEqual(alts[0].text, "g")
        XCTAssertEqual(alts[0].distance, 0.6, accuracy: 1e-9)
    }

    func testAlternativesBeyondMaxDistanceAreDropped() {
        let frames = twoKeyFrames()
        let targets = TouchGeometry.charTargets(frames: frames, transform: { $0 })
        // g の中心まで 65 / 50 = 1.3 > 1.0
        let alts = TouchGeometry.alternatives(touch: CGPoint(x: 10, y: 25), targets: targets, excluding: "f")
        XCTAssertTrue(alts.isEmpty)
    }

    func testAlternativesAreSortedLimitedAndDeduplicated() {
        let targets = [
            TouchTarget(text: "a", center: CGPoint(x: 10, y: 0), width: 10),
            TouchTarget(text: "b", center: CGPoint(x: 5, y: 0), width: 10),
            TouchTarget(text: "c", center: CGPoint(x: 8, y: 0), width: 10),
            TouchTarget(text: "d", center: CGPoint(x: 9, y: 0), width: 10),
            // a の重複（より遠い方は捨てる）
            TouchTarget(text: "a", center: CGPoint(x: 0, y: 9.5), width: 10),
            TouchTarget(text: "x", center: CGPoint(x: 0, y: 0), width: 10),
        ]
        let alts = TouchGeometry.alternatives(touch: .zero, targets: targets, excluding: "x")
        XCTAssertEqual(alts.map { $0.text }, ["b", "c", "d"])
        XCTAssertEqual(alts.map { $0.distance }, [0.5, 0.8, 0.9])
        let more = TouchGeometry.alternatives(touch: .zero, targets: targets, excluding: "x", maxCount: 10)
        XCTAssertEqual(more.map { $0.text }, ["b", "c", "d", "a"])
        XCTAssertEqual(more.last?.distance ?? 0, 0.95, accuracy: 1e-9)
    }

    // MARK: - フリック

    private func flickFrames() -> [KeyFrame] {
        func kanaKey(_ id: String, _ center: String, _ flick: [String: String]?) -> KeyDef {
            return KeyDef(id: id, label: nil, width: nil, spacer: nil, style: nil,
                          tap: KeyAction(type: "kana", text: center, name: nil, layout: nil),
                          flick: flick, longPress: nil, repeats: nil)
        }
        let row = KeyRow(height: nil, keys: [
            kanaKey("a", "あ", ["left": "い", "up": "う", "right": "え", "down": "お"]),
            kanaKey("ka", "か", ["left": "き", "up": "く", "right": "け", "down": "こ"]),
        ])
        let layout = KeyboardLayout(id: "f", name: "f", direct: nil, rows: [row])
        // あ: (0,0,60,60)、か: (60,0,60,60)
        return LayoutGeometry.frames(for: layout, in: CGSize(width: 120, height: 60))
    }

    func testFlickCenterAndDirections() {
        let frames = flickFrames()
        let a = frames[0]
        // 中心（30, 30）
        XCTAssertEqual(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 30, y: 30), end: CGPoint(x: 30, y: 30), allFrames: frames)?.text, "あ")
        // 左ゾーンの中心は (30 - 48, 30)
        let left = FlickGeometry.decide(start: a, startPoint: CGPoint(x: 5, y: 30), end: CGPoint(x: 5, y: 30), allFrames: frames)
        XCTAssertEqual(left?.text, "い")
        XCTAssertEqual(left?.direction, .left)
        XCTAssertEqual(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 30, y: 0), end: CGPoint(x: 30, y: 0), allFrames: frames)?.text, "う")
        XCTAssertEqual(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 55, y: 30), end: CGPoint(x: 55, y: 30), allFrames: frames)?.text, "え")
        XCTAssertEqual(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 30, y: 60), end: CGPoint(x: 30, y: 60), allFrames: frames)?.text, "お")
    }

    func testFlickAlternativesExcludeChosenAndIncludeNeighbors() throws {
        let frames = flickFrames()
        let a = frames[0]
        let decision = try XCTUnwrap(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 30, y: 30), end: CGPoint(x: 30, y: 30), allFrames: frames))
        XCTAssertEqual(decision.text, "あ")
        XCTAssertFalse(decision.alternatives.contains(where: { $0.text == "あ" }))
        XCTAssertLessThanOrEqual(decision.alternatives.count, 3)
        for alt in decision.alternatives {
            XCTAssertLessThanOrEqual(alt.distance, 1.0)
        }
        // 右端（か キーの近く）で離すと、か のゾーンが代替に入る
        let nearKa = try XCTUnwrap(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 58, y: 30), end: CGPoint(x: 58, y: 30), allFrames: frames))
        XCTAssertTrue(nearKa.alternatives.contains(where: { $0.text == "か" }), "\(nearKa)")
    }

    func testFlickWithoutZonesReturnsNil() {
        let key = KeyDef(id: "k", label: nil, width: nil, spacer: nil, style: nil,
                         tap: KeyAction(type: "key", text: nil, name: "space", layout: nil),
                         flick: nil, longPress: nil, repeats: nil)
        let frame = KeyFrame(row: 0, index: 0, key: key, rect: CGRect(x: 0, y: 0, width: 10, height: 10))
        XCTAssertNil(FlickGeometry.decide(start: frame, startPoint: .zero, end: .zero, allFrames: [frame]))
    }
}
