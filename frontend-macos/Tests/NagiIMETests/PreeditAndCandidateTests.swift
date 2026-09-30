import Foundation
import NagiClient
import XCTest

@testable import NagiIME

final class PreeditAndCandidateTests: XCTestCase {
    private func seg(_ surface: String, _ kind: SegmentKind, _ start: UInt32) -> SegmentView {
        SegmentView(
            surface: surface, reading: surface, kind: kind,
            range: RangeU16(start: start, end: start + UInt32(surface.utf16.count)))
    }

    func testFocusedSegmentAndPending() {
        let p = PreeditView(
            text: "きょうはk",
            segments: [seg("きょう", .japanese, 0), seg("は", .japanese, 3)],
            cursor: 5, pending: RangeU16(start: 4, end: 5), focus: 1, selection: nil)
        let runs = PreeditStyler.runs(for: p)
        XCTAssertEqual(
            runs,
            [
                StyleRun(range: NSRange(location: 0, length: 3), style: .normal),
                StyleRun(range: NSRange(location: 3, length: 1), style: .focused),
                StyleRun(range: NSRange(location: 4, length: 1), style: .pending),
            ])
    }

    func testSelectionAndKinds() {
        let p = PreeditView(
            text: "ABC",
            segments: [seg("A", .fixed, 0), seg("B", .japanese, 1), seg("C", .styled, 2)],
            cursor: 3, pending: nil, focus: nil, selection: RangeU16(start: 1, end: 2))
        XCTAssertEqual(PreeditStyler.runs(for: p).map(\.style), [.fixed, .selected, .styled])
    }

    func testRangesAreClampedToText() {
        let p = PreeditView(
            text: "あ", segments: [SegmentView(surface: "あ", reading: "あ", kind: .japanese,
                range: RangeU16(start: 0, end: 9))],
            cursor: 1, pending: nil, focus: nil, selection: nil)
        XCTAssertEqual(PreeditStyler.runs(for: p).first?.range, NSRange(location: 0, length: 1))
    }

    private func candidates(count: Int, selected: UInt32, pageSize: UInt32, visible: Bool = true)
        -> CandidatesView
    {
        CandidatesView(
            items: (0..<count).map { CandidateView(surface: "c\($0)", sources: [.llm, .systemDict, .llm]) },
            selected: selected, page_size: pageSize, visible: visible)
    }

    func testPagingSecondPage() throws {
        let page = try XCTUnwrap(CandidatePaging.page(for: candidates(count: 12, selected: 10, pageSize: 9)))
        XCTAssertEqual(page.pageIndex, 1)
        XCTAssertEqual(page.pageCount, 2)
        XCTAssertEqual(page.rows.map(\.surface), ["c9", "c10", "c11"])
        XCTAssertEqual(page.rows.map(\.number), [1, 2, 3])
        XCTAssertEqual(page.rows.map(\.isSelected), [false, true, false])
        XCTAssertEqual(page.rows[0].label, "AI・辞書")
    }

    func testHiddenOrEmptyIsNil() {
        XCTAssertNil(CandidatePaging.page(for: candidates(count: 3, selected: 0, pageSize: 9, visible: false)))
        XCTAssertNil(CandidatePaging.page(for: candidates(count: 0, selected: 0, pageSize: 9)))
    }

    func testSelectedOutOfRangeIsClamped() throws {
        let page = try XCTUnwrap(CandidatePaging.page(for: candidates(count: 2, selected: 50, pageSize: 0)))
        XCTAssertEqual(page.rows.count, 1)
        XCTAssertTrue(page.rows[0].isSelected)
    }
}
