import XCTest

/// トグルの置き換え判定（flick キーの cycle を含む）、長押しの取り消し判定、触覚の互換モードの対応表
final class FeedbackAndPopupLogicTests: XCTestCase {
    // MARK: ToggleCycle

    func testNextIndexStartsAtZeroWithoutPrevious() {
        XCTAssertEqual(ToggleCycle.nextIndex(previous: nil, cycleCount: 3), 0)
        XCTAssertEqual(ToggleCycle.nextIndex(previous: nil, cycleCount: 0), 0)
    }

    func testNextIndexAdvancesAndWraps() {
        XCTAssertEqual(ToggleCycle.nextIndex(previous: 0, cycleCount: 3), 1)
        XCTAssertEqual(ToggleCycle.nextIndex(previous: 1, cycleCount: 3), 2)
        XCTAssertEqual(ToggleCycle.nextIndex(previous: 2, cycleCount: 3), 0)
        XCTAssertEqual(ToggleCycle.nextIndex(previous: 3, cycleCount: 4), 0)
    }

    func testContinuationWithinTimeout() {
        XCTAssertTrue(ToggleCycle.isContinuation(cycleCount: 3, elapsedMs: 0, timeoutMs: 1000))
        XCTAssertTrue(ToggleCycle.isContinuation(cycleCount: 3, elapsedMs: 999, timeoutMs: 1000))
        XCTAssertFalse(ToggleCycle.isContinuation(cycleCount: 3, elapsedMs: 1000, timeoutMs: 1000))
        XCTAssertFalse(ToggleCycle.isContinuation(cycleCount: 3, elapsedMs: 2500, timeoutMs: 1000))
    }

    func testContinuationRejectsSingleElementAndInvalidTime() {
        XCTAssertFalse(ToggleCycle.isContinuation(cycleCount: 1, elapsedMs: 10, timeoutMs: 1000))
        XCTAssertFalse(ToggleCycle.isContinuation(cycleCount: 0, elapsedMs: 10, timeoutMs: 1000))
        XCTAssertFalse(ToggleCycle.isContinuation(cycleCount: 3, elapsedMs: -1, timeoutMs: 1000))
        XCTAssertFalse(ToggleCycle.isContinuation(cycleCount: 3, elapsedMs: .nan, timeoutMs: 1000))
    }

    func testFlickCycleOrderIsCenterLeftUpRightDown() {
        let cycle = ToggleCycle.flickCycle(
            center: FlickChar(text: "a"), left: FlickChar(text: "b"), up: FlickChar(text: "c"),
            right: FlickChar(text: "d"), down: FlickChar(text: "e")
        )
        XCTAssertEqual(cycle, ["a", "b", "c", "d", "e"])
    }

    func testFlickCycleSkipsMissingAndEmptyDirections() {
        let cycle = ToggleCycle.flickCycle(
            center: FlickChar(text: "a"), left: nil, up: FlickChar(text: ""), right: FlickChar(text: "d"), down: nil
        )
        XCTAssertEqual(cycle, ["a", "d"])
        // center だけなら 1 要素（呼び出し側はトグルにせず center を入れる）
        XCTAssertEqual(ToggleCycle.flickCycle(center: FlickChar(text: "a"), left: nil, up: nil, right: nil, down: nil), ["a"])
        XCTAssertEqual(ToggleCycle.flickCycle(center: nil, left: nil, up: nil, right: nil, down: nil), [])
    }

    // MARK: LongPressCancel

    func testCancelOnlyBelowHeightFactorPastBottom() {
        let h = 46.0
        let margin = h * LongPressCancel.heightFactor // 0.3 倍 = 13.8
        XCTAssertFalse(LongPressCancel.isCancelled(y: -30, keyHeight: h))   // 上
        XCTAssertFalse(LongPressCancel.isCancelled(y: 20, keyHeight: h))    // キー内
        XCTAssertFalse(LongPressCancel.isCancelled(y: h, keyHeight: h))     // 下端
        XCTAssertFalse(LongPressCancel.isCancelled(y: h + margin, keyHeight: h)) // ちょうどは境界（超えたら取り消し）
        XCTAssertTrue(LongPressCancel.isCancelled(y: h + margin + 0.1, keyHeight: h))
        XCTAssertTrue(LongPressCancel.isCancelled(y: 200, keyHeight: h))
    }

    func testCancelScalesWithKeyHeightAndIgnoresInvalid() {
        XCTAssertFalse(LongPressCancel.isCancelled(y: 60 + 17, keyHeight: 60))
        XCTAssertTrue(LongPressCancel.isCancelled(y: 60 + 19, keyHeight: 60))
        XCTAssertFalse(LongPressCancel.isCancelled(y: 500, keyHeight: 0))
        XCTAssertFalse(LongPressCancel.isCancelled(y: .nan, keyHeight: 46))
    }

    // MARK: HapticMapping

    func testSystemSoundIDForHaptic() {
        XCTAssertEqual(HapticMapping.systemSoundID(for: "light"), 1519)
        XCTAssertEqual(HapticMapping.systemSoundID(for: "medium"), 1520)
        XCTAssertEqual(HapticMapping.systemSoundID(for: "heavy"), 1521)
        XCTAssertNil(HapticMapping.systemSoundID(for: "off"))
        XCTAssertNil(HapticMapping.systemSoundID(for: "x"))
    }

    // MARK: PopupClamp（フリックの案内を view の枠内に収める）

    private let keySize = CGSize(width: 60, height: 46)

    func testShiftIsZeroWhenInside() {
        let bounds = CGRect(x: 0, y: 0, width: 390, height: 260)
        XCTAssertEqual(PopupClamp.shift(of: CGRect(x: 100, y: 100, width: 60, height: 46), in: bounds), .zero)
    }

    func testShiftMovesBackFromEachEdge() {
        let bounds = CGRect(x: 0, y: 0, width: 390, height: 260)
        XCTAssertEqual(PopupClamp.shift(of: CGRect(x: 350, y: 240, width: 60, height: 46), in: bounds), CGSize(width: -20, height: -26))
        XCTAssertEqual(PopupClamp.shift(of: CGRect(x: -10, y: -5, width: 60, height: 46), in: bounds), CGSize(width: 10, height: 5))
    }

    func testClampedRectEndsInsideBounds() {
        let bounds = CGRect(x: 0, y: 0, width: 390, height: 260)
        let clamped = PopupClamp.clamped(CGRect(x: 380, y: 300, width: 60, height: 46), in: bounds)
        XCTAssertEqual(clamped, CGRect(x: 330, y: 214, width: 60, height: 46))
        XCTAssertTrue(bounds.contains(clamped))
    }

    func testShiftAlignsToTopLeftWhenLargerThanBounds() {
        let bounds = CGRect(x: 0, y: 0, width: 100, height: 50)
        XCTAssertEqual(PopupClamp.shift(of: CGRect(x: 20, y: 30, width: 200, height: 80), in: bounds), CGSize(width: -20, height: -30))
    }

    func testShiftIgnoresUnmeasuredBounds() {
        XCTAssertEqual(PopupClamp.shift(of: CGRect(x: 0, y: 500, width: 60, height: 46), in: .zero), .zero)
    }

    func testFlickClusterRectCountsOnlyPresentDirections() {
        let all = PopupClamp.flickClusterRect(keySize: keySize, spacing: 2, left: true, up: true, right: true, down: true)
        XCTAssertEqual(all, CGRect(x: -62, y: -48, width: 184, height: 142))
        let onlyCenter = PopupClamp.flickClusterRect(keySize: keySize, spacing: 2, left: false, up: false, right: false, down: false)
        XCTAssertEqual(onlyCenter, CGRect(origin: .zero, size: keySize))
    }

    func testBottomRowDownCellIsPulledInsideView() {
        // 最下段のキー（キー下端から view の下端まで 6pt）。下のセルは view の外へ出るので、全体を上へ寄せる
        let keyFrame = CGRect(x: 100, y: 200, width: 60, height: 46)
        let root = CGSize(width: 390, height: 252)
        let bounds = CGRect(x: -keyFrame.minX, y: -keyFrame.minY, width: root.width, height: root.height)
        let moved = PopupClamp.flickShift(keySize: keySize, spacing: 2, left: true, up: true, right: true, down: true, bounds: bounds)
        XCTAssertEqual(moved, CGSize(width: 0, height: -42))
        let cluster = PopupClamp.flickClusterRect(keySize: keySize, spacing: 2, left: true, up: true, right: true, down: true)
            .offsetBy(dx: moved.width, dy: moved.height)
        XCTAssertTrue(bounds.contains(cluster))
    }

    func testLeftmostKeyLeftCellIsPulledInsideView() {
        let keyFrame = CGRect(x: 6, y: 100, width: 60, height: 46)
        let bounds = CGRect(x: -keyFrame.minX, y: -keyFrame.minY, width: 390, height: 252)
        let moved = PopupClamp.flickShift(keySize: keySize, spacing: 2, left: true, up: true, right: true, down: true, bounds: bounds)
        XCTAssertEqual(moved, CGSize(width: 56, height: 0))
    }

    func testMiddleKeyIsNotMoved() {
        let keyFrame = CGRect(x: 160, y: 100, width: 60, height: 46)
        let bounds = CGRect(x: -keyFrame.minX, y: -keyFrame.minY, width: 390, height: 252)
        XCTAssertEqual(
            PopupClamp.flickShift(keySize: keySize, spacing: 2, left: true, up: true, right: true, down: true, bounds: bounds),
            .zero
        )
    }

    // MARK: 設定

    func testHapticEngineSetting() {
        var s = NagiIOSSettings()
        XCTAssertEqual(s.hapticEngine, "standard")
        s.set(NagiIOSSettings.Key.keyboardHapticEngine, .string("compat"))
        XCTAssertEqual(s.hapticEngine, "compat")
        s.set(NagiIOSSettings.Key.keyboardHapticEngine, .string("bogus"))
        XCTAssertEqual(s.hapticEngine, "standard")
    }
}
