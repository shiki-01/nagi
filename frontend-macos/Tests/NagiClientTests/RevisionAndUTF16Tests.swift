import Foundation
import XCTest

@testable import NagiClient

final class RevisionAndUTF16Tests: XCTestCase {
    func testRevisionGateDiscardsOlder() {
        let gate = RevisionGate()
        gate.record(sessionId: "s", revision: 5)
        XCTAssertFalse(gate.accept(sessionId: "s", revision: 4))
        XCTAssertTrue(gate.accept(sessionId: "s", revision: 5))
        XCTAssertTrue(gate.accept(sessionId: "s", revision: 7))
        XCTAssertFalse(gate.accept(sessionId: "s", revision: 6))
        // セッションごとに独立
        XCTAssertTrue(gate.accept(sessionId: "other", revision: 1))
        gate.reset()
        XCTAssertTrue(gate.accept(sessionId: "s", revision: 1))
    }

    func testUTF16LengthCountsSurrogatePairs() {
        XCTAssertEqual(UTF16Position.length("こんにちは"), 5)
        XCTAssertEqual(UTF16Position.length("😀"), 2)  // Swift の count は 1
        XCTAssertEqual(UTF16Position.length("👨‍👩‍👧"), 8)  // 書記素クラスタ 1 個でも UTF-16 では 8
    }

    func testScalarConversion() {
        let s = "a😀い"  // UTF-16: a(1) 😀(2) い(1)
        XCTAssertEqual(UTF16Position.scalarCount(inPrefixOf: s, utf16Offset: 0), 0)
        XCTAssertEqual(UTF16Position.scalarCount(inPrefixOf: s, utf16Offset: 1), 1)
        XCTAssertEqual(UTF16Position.scalarCount(inPrefixOf: s, utf16Offset: 2), 1)  // サロゲート途中は切り下げ
        XCTAssertEqual(UTF16Position.scalarCount(inPrefixOf: s, utf16Offset: 3), 2)
        XCTAssertEqual(UTF16Position.scalarCount(inPrefixOf: s, utf16Offset: 4), 3)
        XCTAssertEqual(UTF16Position.utf16Offset(ofScalarCount: 2, in: s), 3)
        XCTAssertEqual(UTF16Position.utf16Offset(ofScalarCount: 99, in: s), 4)
    }

    func testSubstringByUTF16Range() {
        let s = "a😀い"
        XCTAssertEqual(UTF16Position.substring(of: s, range: RangeU16(start: 1, end: 3)), "😀")
        XCTAssertEqual(UTF16Position.substring(of: s, range: RangeU16(start: 3, end: 4)), "い")
        XCTAssertNil(UTF16Position.substring(of: s, range: RangeU16(start: 2, end: 3)))  // サロゲート途中
        XCTAssertNil(UTF16Position.substring(of: s, range: RangeU16(start: 0, end: 9)))
    }

    func testRestoreParamsCursorIsScalarUnitsOfReading() {
        // 「あ😀」(reading, UTF-16 3 単位) + 「い」。カーソルは全体末尾（UTF-16 4）
        let preedit = PreeditView(
            text: "あ😀い",
            segments: [
                SegmentView(
                    surface: "あ😀", reading: "あ😀", kind: .japanese, range: RangeU16(start: 0, end: 3)),
                SegmentView(surface: "い", reading: "い", kind: .japanese, range: RangeU16(start: 3, end: 4)),
            ],
            cursor: 4, pending: nil, focus: nil, selection: nil)
        let p = RestoreSessionParams.make(sessionId: "s", preedit: preedit, converting: false)
        XCTAssertEqual(p.cursor, 3)  // スカラー 3 個（UTF-16 なら 4 になってしまう）
        XCTAssertEqual(p.segments.map(\.reading), ["あ😀", "い"])
        XCTAssertFalse(p.converting)
    }

    func testRestoreParamsCursorInsideSegment() {
        let preedit = PreeditView(
            text: "あ😀い",
            segments: [
                SegmentView(
                    surface: "あ😀い", reading: "あ😀い", kind: .japanese, range: RangeU16(start: 0, end: 4))
            ],
            cursor: 3, pending: nil, focus: nil, selection: nil)
        let p = RestoreSessionParams.make(sessionId: "s", preedit: preedit, converting: false)
        XCTAssertEqual(p.cursor, 2)
    }
}
