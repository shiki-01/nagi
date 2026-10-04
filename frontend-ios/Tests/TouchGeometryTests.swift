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

    func testQwertyAlternativesCanIncludeTheChosenKeyDistance() {
        let frames = twoKeyFrames()
        let targets = TouchGeometry.charTargets(frames: frames, transform: { $0 })
        // f の中心 (25, 25) まで 20 / 50 = 0.4、g の中心 (75, 25) まで 30 / 50 = 0.6
        let alts = TouchGeometry.alternatives(
            touch: CGPoint(x: 45, y: 25), targets: targets, excluding: "f", includeChosen: true
        )
        XCTAssertEqual(alts.map { $0.text }, ["g", "f"])
        XCTAssertEqual(alts[0].distance, 0.6, accuracy: 1e-9)
        XCTAssertEqual(alts[1].distance, 0.4, accuracy: 1e-9)
        // 遠くても（上限 1.0 を超えても）確定したキー自身は加える: (10, 25) → g は 1.3 で落ち、f は 0.3
        let far = TouchGeometry.alternatives(
            touch: CGPoint(x: 10, y: 25), targets: targets, excluding: "f", includeChosen: true
        )
        XCTAssertEqual(far.map { $0.text }, ["f"])
        XCTAssertEqual(far[0].distance, 0.3, accuracy: 1e-9)
        // ターゲットに無い文字は加えない
        let none = TouchGeometry.alternatives(
            touch: CGPoint(x: 45, y: 25), targets: targets, excluding: "z", includeChosen: true
        )
        XCTAssertEqual(none.map { $0.text }, ["f", "g"])
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

    func testFlickAlternativesIncludeNeighborsAndTheChosenZoneItself() throws {
        let frames = flickFrames()
        let a = frames[0]
        // あ の中心（30, 30）で離す。spread = 60 * 0.8 = 48
        // 他のゾーン い・う・え・お は距離 48 / 60 = 0.8、か の中央ゾーンは (90, 30) まで 60 / 60 = 1.0。
        // 上位 3 件は 0.8 の同率を文字の昇順で い・う・え。そのあとに確定したゾーン あ（距離 0）
        let decision = try XCTUnwrap(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 30, y: 30), end: CGPoint(x: 30, y: 30), allFrames: frames))
        XCTAssertEqual(decision.text, "あ")
        XCTAssertEqual(decision.alternatives.map { $0.text }, ["い", "う", "え", "あ"])
        XCTAssertEqual(decision.alternatives[0].distance, 0.8, accuracy: 1e-9)
        XCTAssertEqual(decision.alternatives[1].distance, 0.8, accuracy: 1e-9)
        XCTAssertEqual(decision.alternatives[2].distance, 0.8, accuracy: 1e-9)
        XCTAssertEqual(decision.alternatives[3].distance, 0.0, accuracy: 1e-9)
        // 確定したゾーン以外は最大 3 件・上限 1.0 以内
        XCTAssertEqual(decision.alternatives.filter { $0.text != decision.text }.count, 3)
        for alt in decision.alternatives {
            XCTAssertLessThanOrEqual(alt.distance, 1.0)
        }
        // 右端（か キーの近く）で離すと、か のゾーンが代替に入る。
        // 確定は え（右ゾーン (78, 30) まで 20 / 60）。あ は 28 / 60、か は (90, 30) まで 32 / 60、
        // う は (30, -18) まで sqrt(28^2 + 48^2) / 60 = 0.926、お も同じ、い は 76 / 60 で圏外。
        // 上位 3 件は あ・か・う（う と お は同率で文字の昇順）。そのあとに え（20 / 60）
        let nearKa = try XCTUnwrap(FlickGeometry.decide(start: a, startPoint: CGPoint(x: 58, y: 30), end: CGPoint(x: 58, y: 30), allFrames: frames))
        XCTAssertEqual(nearKa.text, "え")
        XCTAssertEqual(nearKa.alternatives.map { $0.text }, ["あ", "か", "う", "え"], "\(nearKa)")
        XCTAssertEqual(nearKa.alternatives[1].distance, 32.0 / 60.0, accuracy: 1e-9)
        XCTAssertEqual(nearKa.alternatives[3].distance, 20.0 / 60.0, accuracy: 1e-9)
    }

    private func singleKanaFrame(center: String, flick: [String: String]) -> [KeyFrame] {
        let row = KeyRow(height: nil, keys: [
            KeyDef(id: "k", label: nil, width: nil, spacer: nil, style: nil,
                   tap: KeyAction(type: "kana", text: center, name: nil, layout: nil),
                   flick: flick, longPress: nil, repeats: nil),
        ])
        let layout = KeyboardLayout(id: "s", name: "s", direct: nil, rows: [row])
        // キーは (0, 0, 60, 60)。中心 (30, 30)、spread 48
        return LayoutGeometry.frames(for: layout, in: CGSize(width: 60, height: 60))
    }

    func testFlickAlternativesExcludeNonKanaBeforeTakingTheTopCount() throws {
        // や の左 ( ・右 ) は、かな以外なので代替に入れない（枠 3 を消費しない）。
        // 並びの昇順では ( ) ゆ ... なので、除かずに上位 3 件を取ると ゆ の前に 2 件を使ってしまう
        let frames = singleKanaFrame(center: "や", flick: ["left": "(", "up": "ゆ", "right": ")", "down": "よ"])
        let center = CGPoint(x: 30, y: 30)
        let decision = try XCTUnwrap(FlickGeometry.decide(start: frames[0], startPoint: center, end: center, allFrames: frames))
        XCTAssertEqual(decision.text, "や")
        // ゆ・よ（各 0.8）と、確定した や（0）
        XCTAssertEqual(decision.alternatives.map { $0.text }, ["ゆ", "よ", "や"])
        XCTAssertEqual(decision.alternatives[2].distance, 0.0, accuracy: 1e-9)
    }

    func testFlickProlongedSoundMarkIsAKanaAlternative() throws {
        // わ（中央）を ー（右ゾーン (78, 30)）で離す。を は (-18, 30) まで 96 / 60、ん は (30, -18) まで 1.13 で圏外。
        // 代替は わ（48 / 60 = 0.8）と、確定した ー（0）
        let frames = singleKanaFrame(center: "わ", flick: ["left": "を", "up": "ん", "right": "ー"])
        let decision = try XCTUnwrap(FlickGeometry.decide(start: frames[0], startPoint: CGPoint(x: 78, y: 30), end: CGPoint(x: 78, y: 30), allFrames: frames))
        XCTAssertEqual(decision.text, "ー")
        XCTAssertEqual(decision.alternatives.map { $0.text }, ["わ", "ー"])
        XCTAssertEqual(decision.alternatives[0].distance, 0.8, accuracy: 1e-9)
        XCTAssertEqual(decision.alternatives[1].distance, 0.0, accuracy: 1e-9)
    }

    func testFlickSendsNoAlternativesWhenTheChosenReadingIsNotKana() throws {
        // 確定した 、 がかな以外なら、代替は送らない（他のゾーン 。 もかな以外）
        let frames = singleKanaFrame(center: "、", flick: ["right": "。"])
        let center = CGPoint(x: 30, y: 30)
        let decision = try XCTUnwrap(FlickGeometry.decide(start: frames[0], startPoint: center, end: center, allFrames: frames))
        XCTAssertEqual(decision.text, "、")
        XCTAssertTrue(decision.alternatives.isEmpty)
        // 隣のかなキー（あ）が近くても、記号キーが確定したなら代替は空
        let row = KeyRow(height: nil, keys: [
            KeyDef(id: "a", label: nil, width: nil, spacer: nil, style: nil,
                   tap: KeyAction(type: "kana", text: "あ", name: nil, layout: nil),
                   flick: nil, longPress: nil, repeats: nil),
            KeyDef(id: "t", label: nil, width: nil, spacer: nil, style: nil,
                   tap: KeyAction(type: "kana", text: "、", name: nil, layout: nil),
                   flick: ["right": "。"], longPress: nil, repeats: nil),
        ])
        let layout = KeyboardLayout(id: "n", name: "n", direct: nil, rows: [row])
        // あ: (0,0,60,60)、、: (60,0,60,60)。、 の中心 (90, 30) のすぐ左 (70, 30) を押す
        let two = LayoutGeometry.frames(for: layout, in: CGSize(width: 120, height: 60))
        let near = CGPoint(x: 70, y: 30)
        let next = try XCTUnwrap(FlickGeometry.decide(start: two[1], startPoint: near, end: near, allFrames: two))
        XCTAssertEqual(next.text, "、")
        XCTAssertTrue(next.alternatives.isEmpty, "\(next)")
    }

    func testFlickWithoutZonesReturnsNil() {
        let key = KeyDef(id: "k", label: nil, width: nil, spacer: nil, style: nil,
                         tap: KeyAction(type: "key", text: nil, name: "space", layout: nil),
                         flick: nil, longPress: nil, repeats: nil)
        let frame = KeyFrame(row: 0, index: 0, key: key, rect: CGRect(x: 0, y: 0, width: 10, height: 10))
        XCTAssertNil(FlickGeometry.decide(start: frame, startPoint: .zero, end: .zero, allFrames: [frame]))
    }
}
