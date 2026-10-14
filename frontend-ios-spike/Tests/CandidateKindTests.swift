import XCTest

/// 候補の振り分け（`classifyCandidates`。確定直後の次語予測かどうか）
final class CandidateKindTests: XCTestCase {
    private func view(
        _ items: [(String, [CandidateSource], UInt32?)], visible: Bool = true
    ) -> CandidatesView {
        CandidatesView(
            items: items.map { CandidateView(surface: $0.0, sources: $0.1, covers: $0.2) },
            selected: 0, page_size: 9, visible: visible)
    }

    func testNextWordInDirectState() {
        let v = view([("です", [.learned], nil), ("ます", [.learned], nil)])
        XCTAssertEqual(classifyCandidates(v, hasPreedit: false), .nextWord)
    }

    func testEmptyOrInvisibleIsNone() {
        XCTAssertEqual(classifyCandidates(view([]), hasPreedit: false), .hidden)
        // 入力の開始などで消す差分（visible = false）。items が残っていても出さない
        let hidden = view([("です", [.learned], nil)], visible: false)
        XCTAssertEqual(classifyCandidates(hidden, hasPreedit: false), .hidden)
        XCTAssertEqual(classifyCandidates(hidden, hasPreedit: true), .hidden)
    }

    func testCandidatesWhileComposingAreConversion() {
        // 入力中は出所が learned のみでも通常の候補（古い次語候補の通知が遅れて届いても次語として出さない）
        let v = view([("です", [.learned], nil)])
        XCTAssertEqual(classifyCandidates(v, hasPreedit: true), .conversion)
    }

    func testOtherSourcesOrCoversAreConversion() {
        XCTAssertEqual(classifyCandidates(view([("今日", [.systemDict], nil)]), hasPreedit: false), .conversion)
        XCTAssertEqual(classifyCandidates(view([("今日", [.learned, .llm], nil)]), hasPreedit: false), .conversion)
        XCTAssertEqual(classifyCandidates(view([("今", [.learned], 1)]), hasPreedit: false), .conversion)
        // 1 つでも次語の形でなければ全体を通常の候補として扱う
        let mixed = view([("です", [.learned], nil), ("今日", [.systemDict], nil)])
        XCTAssertEqual(classifyCandidates(mixed, hasPreedit: false), .conversion)
    }
}
