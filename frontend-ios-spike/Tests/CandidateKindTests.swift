import XCTest

/// 候補の振り分け（`classifyCandidates`。確定直後の次語予測かどうか）
final class CandidateKindTests: XCTestCase {
    private func view(
        _ items: [(String, [CandidateSource], UInt32?)], visible: Bool = true
    ) -> CandidatesView {
        CandidatesView(
            items: items.map { CandidateView(surface: $0.0, sources: $0.1, covers: $0.2, reading: nil) },
            selected: 0, page_size: 9, visible: visible, unselected: nil)
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

    // MARK: - 曖昧入力（補正候補）

    func testFuzzyCandidatesAreConversionWhileComposing() {
        let v = view([("感じ", [.fuzzy], nil), ("幹事", [.systemDict], nil)])
        XCTAssertEqual(classifyCandidates(v, hasPreedit: true), .conversion)
    }

    func testFuzzyCandidatesAreNeverNextWord() {
        // 出所が learned のみでなければ次語候補ではない
        let v = view([("感じ", [.fuzzy], nil)])
        XCTAssertEqual(classifyCandidates(v, hasPreedit: false), .conversion)
    }

    func testFuzzyHint() {
        XCTAssertEqual(fuzzyHint(sources: [.fuzzy], reading: "かんじ"), "もしかして: かんじ")
        XCTAssertEqual(fuzzyHint(sources: [.fuzzy], reading: nil), "もしかして")
        XCTAssertEqual(fuzzyHint(sources: [.fuzzy], reading: ""), "もしかして")
        XCTAssertNil(fuzzyHint(sources: [.systemDict], reading: "かんじ"))
        XCTAssertEqual(fuzzyHint(sources: [.llm, .fuzzy], reading: "かんじ"), "もしかして: かんじ")
    }

    // MARK: - 候補バーの作り直しの鍵・initialize の capabilities（H-58）

    func testCandidateBarIdentityChangesWhenTheListIsReplaced() {
        let a = candidateBarIdentity(["今日は", "今日", "京"])
        XCTAssertEqual(a, candidateBarIdentity(["今日は", "今日", "京"]))
        XCTAssertNotEqual(a, candidateBarIdentity(["今日", "京"]))
        XCTAssertNotEqual(a, candidateBarIdentity(["今日は", "今日", "巨"]))
        XCTAssertEqual(candidateBarIdentity([]), "0||")
    }

    func testClientCapabilitiesDeclarePartialSuggestionsAndTouch() {
        let caps = makeClientCapabilities(environment: nil)
        XCTAssertEqual(caps.input_method, .touch)
        XCTAssertEqual(caps.partial_suggestions, true)
        XCTAssertNil(caps.environment)
    }

    func testPrefixCandidatesKeepTheirCovers() {
        // 接頭辞の候補（covers が読み全体より短い）も、入力中は通常の変換候補として扱う
        let v = view([("今日は良い天気", [.llm], nil), ("今日", [.systemDict], 3), ("木", [.systemDict], 1)])
        XCTAssertEqual(classifyCandidates(v, hasPreedit: true), .conversion)
        XCTAssertEqual(v.items.map { $0.covers }, [nil, 3, 1])
    }

    func testPinnedFuzzyIsTheFirstCorrection() {
        // 補正候補のうち最初（コアの並びで最有力）を固定する
        XCTAssertEqual(pinnedFuzzyIndex(hints: [nil, "もしかして: あおぶた", "もしかして"], nextWord: false), 1)
        // 補正候補が無い・次語予測なら固定しない
        XCTAssertNil(pinnedFuzzyIndex(hints: [nil, nil], nextWord: false))
        XCTAssertNil(pinnedFuzzyIndex(hints: ["もしかして"], nextWord: true))
        XCTAssertNil(pinnedFuzzyIndex(hints: [], nextWord: false))
    }
}
