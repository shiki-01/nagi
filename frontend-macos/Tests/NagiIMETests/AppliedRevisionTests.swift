import XCTest

@testable import NagiIME

final class AppliedRevisionTests: XCTestCase {
    /// 確定（同期応答）のあとに、それより前に計算された非同期更新が届いても反映しない
    func testStaleAsyncUpdateAfterCommitIsDropped() {
        var r = AppliedRevision()
        XCTAssertTrue(r.accept(async: 3))  // ライブ変換の結果（rev 3）
        r.applied(sync: 5)  // Enter で確定した応答（rev 5）
        XCTAssertFalse(r.accept(async: 4))  // 確定より前に計算された古い結果
        XCTAssertFalse(r.accept(async: 5))
        XCTAssertTrue(r.accept(async: 6))  // 確定後の新しい更新は反映する
    }

    /// プリエディットと候補が同じ revision で続けて届いても、2 つ目を捨てない
    func testPreeditAndCandidatesWithTheSameRevisionAreBothApplied() {
        var r = AppliedRevision()
        r.applied(sync: 2)
        XCTAssertTrue(r.accept(async: 3))
        XCTAssertTrue(r.accept(async: 3))
        XCTAssertFalse(r.accept(async: 2))
    }

    func testSyncNeverRewinds() {
        var r = AppliedRevision()
        r.applied(sync: 7)
        r.applied(sync: 2)
        XCTAssertEqual(r.sync, 7)
    }

    func testResetOnEngineRestart() {
        var r = AppliedRevision()
        r.applied(sync: 9)
        XCTAssertTrue(r.accept(async: 10))
        r.reset()
        XCTAssertTrue(r.accept(async: 1))
    }
}
