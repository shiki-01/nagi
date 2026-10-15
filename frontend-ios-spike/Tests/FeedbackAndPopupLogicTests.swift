import XCTest

/// トグルの置き換え判定、長押しの取り消し判定、触覚の互換モードの対応表
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

    // MARK: LongPressCancel

    func testCancelOnlyBelowHalfKeyHeightPastBottom() {
        let h = 46.0
        XCTAssertFalse(LongPressCancel.isCancelled(y: -30, keyHeight: h))   // 上
        XCTAssertFalse(LongPressCancel.isCancelled(y: 20, keyHeight: h))    // キー内
        XCTAssertFalse(LongPressCancel.isCancelled(y: h, keyHeight: h))     // 下端
        XCTAssertFalse(LongPressCancel.isCancelled(y: h + 22.9, keyHeight: h))
        XCTAssertFalse(LongPressCancel.isCancelled(y: h + 23, keyHeight: h)) // ちょうど 0.5 倍は境界（超えたら取り消し）
        XCTAssertTrue(LongPressCancel.isCancelled(y: h + 23.1, keyHeight: h))
        XCTAssertTrue(LongPressCancel.isCancelled(y: 200, keyHeight: h))
    }

    func testCancelScalesWithKeyHeightAndIgnoresInvalid() {
        XCTAssertFalse(LongPressCancel.isCancelled(y: 60 + 29, keyHeight: 60))
        XCTAssertTrue(LongPressCancel.isCancelled(y: 60 + 31, keyHeight: 60))
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
