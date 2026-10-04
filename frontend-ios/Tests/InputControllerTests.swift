import XCTest

/// 応答を台本どおりに返す偽のトランスポート
final class FakeTransport: LineTransport {
    /// メソッド名 → 返す `result` の JSON（複数あれば順に返し、最後の 1 つは使い回す）
    var results: [String: [String]] = [:]
    /// 空の応答（FFI の失敗相当）を返すメソッド
    var failing: Set<String> = []
    private(set) var requests: [[String: Any]] = []

    static let initializeResult = """
    {"protocol_version":1,"server_name":"fake","server_version":"0.0.0","profile":"minimal","mode":"hiragana",
     "capabilities":{"reconvert":true,"live_conversion":false,"style_conversion":false,"touch_input":true}}
    """
    static let notConsumed = "{\"consumed\":false,\"revision\":0}"

    var methods: [String] { return requests.compactMap { $0["method"] as? String } }

    func params(of method: String) -> [String: Any]? {
        return requests.last(where: { ($0["method"] as? String) == method })?["params"] as? [String: Any]
    }

    func send(line: String) -> [String] {
        guard let data = line.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let method = object["method"] as? String
        else { return [] }
        requests.append(object)
        guard let id = object["id"] as? Int else { return [] }
        if failing.contains(method) { return [] }
        var result = method == "initialize" ? FakeTransport.initializeResult : FakeTransport.notConsumed
        if var queue = results[method], !queue.isEmpty {
            result = queue[0]
            if queue.count > 1 {
                queue.removeFirst()
                results[method] = queue
            }
        }
        return ["{\"jsonrpc\":\"2.0\",\"id\":\(id),\"result\":\(result)}"]
    }
}

final class MockOutput: HostTextOutput {
    private(set) var events: [String] = []
    func insertText(_ text: String) { events.append("insert:\(text)") }
    func deleteBackward() { events.append("delete") }
    func setMarkedText(_ text: String, cursorUTF16: Int) { events.append("mark:\(text)@\(cursorUTF16)") }
    func unmarkText() { events.append("unmark") }
    func moveCursor(by offset: Int) { events.append("move:\(offset)") }
}

final class InputControllerTests: XCTestCase {
    private func preeditJSON(text: String, reading: String, focus: Int? = nil) -> String {
        let count = text.utf16.count
        let focusPart = focus.map { ",\"focus\":\($0)" } ?? ""
        return """
        {"consumed":true,"preedit":{"text":"\(text)","segments":[{"surface":"\(text)","reading":"\(reading)",
        "kind":"japanese","range":{"start":0,"end":\(count)}}],"cursor":\(count)\(focusPart)},"revision":1}
        """
    }

    private func makeReady() -> (NagiInputController, FakeTransport, MockOutput) {
        let output = MockOutput()
        let controller = NagiInputController(output: output)
        let transport = FakeTransport()
        controller.engineCreated(transport)
        return (controller, transport, output)
    }

    func testInitializeMakesControllerReady() {
        let (controller, transport, _) = makeReady()
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.profile, "minimal")
        XCTAssertEqual(Array(transport.methods.prefix(2)), ["initialize", "set_context"])
    }

    func testCharShowsMarkedTextAndSendsAlternatives() throws {
        let (controller, transport, output) = makeReady()
        transport.results["process_key"] = [preeditJSON(text: "あ", reading: "あ")]
        controller.handle(.char(text: "a", alternatives: [AltCandidate(text: "s", distance: 0.5)]))
        XCTAssertEqual(output.events, ["mark:あ@1"])
        let params = try XCTUnwrap(transport.params(of: "process_key"))
        XCTAssertEqual(params["text"] as? String, "a")
        let alternatives = try XCTUnwrap(params["alternatives"] as? [[String: Any]])
        XCTAssertEqual(alternatives.count, 1)
        XCTAssertEqual(alternatives[0]["text"] as? String, "s")
    }

    func testBackspaceNotConsumedFallsBackToDeleteBackward() {
        let (controller, _, output) = makeReady()
        controller.handle(.special(.backspace))
        XCTAssertEqual(output.events, ["delete"])
    }

    func testSpaceAndEnterNotConsumedInsertText() {
        let (controller, _, output) = makeReady()
        controller.handle(.special(.space))
        controller.handle(.special(.enter))
        XCTAssertEqual(output.events, ["insert: ", "insert:\n"])
    }

    func testCommitReplacesMarkedText() {
        let (controller, transport, output) = makeReady()
        transport.results["process_key"] = [
            preeditJSON(text: "あ", reading: "あ"),
            "{\"consumed\":true,\"commit\":\"亜\",\"preedit\":{\"text\":\"\",\"segments\":[],\"cursor\":0},\"revision\":2}",
        ]
        controller.handle(.char(text: "a", alternatives: []))
        controller.handle(.special(.enter))
        XCTAssertEqual(output.events, ["mark:あ@1", "mark:亜@1", "unmark"])
    }

    func testKanaUsesInsertReading() throws {
        let (controller, transport, output) = makeReady()
        transport.results["insert_reading"] = [preeditJSON(text: "か", reading: "か")]
        controller.handle(.kana(reading: "か", alternatives: [AltCandidate(text: "き", distance: 0.4)]))
        XCTAssertEqual(output.events, ["mark:か@1"])
        let params = try XCTUnwrap(transport.params(of: "insert_reading"))
        XCTAssertEqual(params["reading"] as? String, "か")
    }

    func testDakutenReplacesLastKana() throws {
        let (controller, transport, _) = makeReady()
        transport.results["insert_reading"] = [preeditJSON(text: "か", reading: "か")]
        controller.handle(.kana(reading: "か", alternatives: []))
        controller.handle(.special(.dakuten))
        let params = try XCTUnwrap(transport.params(of: "insert_reading"))
        XCTAssertEqual(params["reading"] as? String, "が")
        XCTAssertTrue(transport.methods.contains("process_key"))
    }

    func testSelectCandidateCommitsAfterLastSegment() throws {
        let (controller, transport, _) = makeReady()
        transport.results["process_key"] = [preeditJSON(text: "日本", reading: "にほん", focus: 0)]
        controller.handle(.special(.space))
        controller.handle(.selectCandidate(1))
        let tail = Array(transport.methods.suffix(2))
        XCTAssertEqual(tail, ["select_candidate", "perform_action"])
        let params = try XCTUnwrap(transport.params(of: "perform_action"))
        XCTAssertEqual(params["action"] as? String, "Commit")
    }

    func testEmptyResponseStartsRecoveryAndReplaysQueuedInput() {
        let (controller, transport, output) = makeReady()
        transport.failing = ["process_key"]
        controller.handle(.char(text: "a", alternatives: []))
        XCTAssertEqual(controller.state, .recovering)
        XCTAssertTrue(output.events.isEmpty, "再初期化中は入力を溜める")

        let second = FakeTransport()
        second.results["process_key"] = [preeditJSON(text: "あ", reading: "あ")]
        controller.engineCreated(second, isRecovery: true)
        XCTAssertEqual(controller.state, .ready)
        XCTAssertTrue(second.methods.contains("process_key"), "溜めた入力が再送される")
        XCTAssertEqual(output.events.last, "mark:あ@1")
    }

    func testUnavailableEnginePassesInputThrough() {
        let output = MockOutput()
        let controller = NagiInputController(output: output)
        controller.engineCreated(nil)
        XCTAssertEqual(controller.state, .unavailable)
        controller.handle(.char(text: "a", alternatives: []))
        controller.handle(.kana(reading: "か", alternatives: []))
        controller.handle(.special(.backspace))
        XCTAssertEqual(output.events, ["insert:a", "insert:か", "delete"])
    }

    func testDirectInputBypassesEngine() {
        let output = MockOutput()
        let controller = NagiInputController(output: output)
        controller.directInput = true
        controller.handle(.char(text: "x", alternatives: []))
        controller.handle(.special(.space))
        XCTAssertEqual(output.events, ["insert:x", "insert: "])
    }

    func testInputsBeforeReadyAreQueuedThenProcessed() {
        let output = MockOutput()
        let controller = NagiInputController(output: output)
        controller.handle(.special(.space))
        XCTAssertTrue(output.events.isEmpty)
        controller.engineCreated(FakeTransport())
        XCTAssertEqual(output.events, ["insert: "])
    }
}
