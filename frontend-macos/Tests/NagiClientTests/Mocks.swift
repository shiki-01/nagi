import Foundation
import XCTest

@testable import NagiClient

/// テスト用の通信路。送られた要求を記録し、`responder` があれば同期的に応答する
final class MockConnection: EngineConnection, @unchecked Sendable {
    private let lock = NSLock()
    private var onLine: ((Data) -> Void)?
    private var onClose: (() -> Void)?
    private var _sent: [[String: Any]] = []
    private var closed = false

    /// 受信した JSON オブジェクトに対する応答行を返す（nil なら無応答）
    var responder: (([String: Any]) -> [String]?)?
    var failSend = false

    var sent: [[String: Any]] {
        lock.lock()
        defer { lock.unlock() }
        return _sent
    }

    func sentMethods() -> [String] { sent.compactMap { $0["method"] as? String } }

    func setHandlers(onLine: @escaping (Data) -> Void, onClose: @escaping () -> Void) {
        lock.lock()
        self.onLine = onLine
        self.onClose = onClose
        lock.unlock()
    }

    func start() throws {}

    func send(_ message: Data) throws {
        if failSend { throw NagiClientError.sendFailed("mock") }
        let obj = try JSONSerialization.jsonObject(with: message) as! [String: Any]
        lock.lock()
        _sent.append(obj)
        let responder = self.responder
        lock.unlock()
        if let lines = responder?(obj) {
            for l in lines { receive(l) }
        }
    }

    func terminate() { simulateClose() }

    /// サーバーからの 1 行を注入する
    func receive(_ line: String) {
        lock.lock()
        let h = onLine
        lock.unlock()
        h?(Data(line.utf8))
    }

    /// プロセス終了（EOF）を模擬する
    func simulateClose() {
        lock.lock()
        if closed {
            lock.unlock()
            return
        }
        closed = true
        let h = onClose
        lock.unlock()
        h?()
    }
}

final class MockLauncher: EngineLauncher, @unchecked Sendable {
    private let lock = NSLock()
    private var _connections: [MockConnection] = []
    private var _minimalFlags: [Bool] = []
    /// 接続ごとの初期設定（起動回数を引数に取る）
    var configure: (Int, MockConnection) -> Void = { _, c in c.responder = standardResponder(revision: 0) }

    var connections: [MockConnection] {
        lock.lock()
        defer { lock.unlock() }
        return _connections
    }
    var minimalFlags: [Bool] {
        lock.lock()
        defer { lock.unlock() }
        return _minimalFlags
    }

    func makeConnection(minimal: Bool) throws -> EngineConnection {
        let c = MockConnection()
        lock.lock()
        _connections.append(c)
        _minimalFlags.append(minimal)
        let n = _connections.count
        lock.unlock()
        configure(n, c)
        return c
    }
}

final class ImmediateScheduler: EngineScheduler, @unchecked Sendable {
    private let lock = NSLock()
    private var _delays: [TimeInterval] = []
    var delays: [TimeInterval] {
        lock.lock()
        defer { lock.unlock() }
        return _delays
    }
    func schedule(after delay: TimeInterval, _ work: @escaping () -> Void) {
        lock.lock()
        _delays.append(delay)
        lock.unlock()
        work()
    }
}

// MARK: JSON 応答の組み立て

let initializeResultJSON = """
    {"protocol_version":1,"server_name":"nagi-engine","server_version":"0.0.0","profile":"balanced",\
    "mode":"hiragana","capabilities":{"reconvert":true,"live_conversion":false,"style_conversion":false}}
    """

func processKeyResultJSON(revision: Int, commit: String? = nil, preeditText: String? = nil) -> String {
    var parts = ["\"consumed\":true", "\"revision\":\(revision)"]
    if let commit { parts.append("\"commit\":\"\(commit)\"") }
    if let t = preeditText {
        let n = t.utf16.count
        parts.append(
            "\"preedit\":{\"text\":\"\(t)\",\"segments\":[{\"surface\":\"\(t)\",\"reading\":\"\(t)\","
                + "\"kind\":\"japanese\",\"range\":{\"start\":0,\"end\":\(n)}}],\"cursor\":\(n)}")
    }
    return "{" + parts.joined(separator: ",") + "}"
}

func response(id: Any, resultJSON: String) -> String {
    "{\"jsonrpc\":\"2.0\",\"id\":\(id),\"result\":\(resultJSON)}"
}

/// initialize / restore_session / process_key / ping 等に標準的に応答する
func standardResponder(revision: Int) -> ([String: Any]) -> [String]? {
    return { req in
        guard let id = req["id"], let method = req["method"] as? String else { return nil }
        switch method {
        case "initialize": return [response(id: id, resultJSON: initializeResultJSON)]
        case "ping", "shutdown", "clear_learning": return [response(id: id, resultJSON: "{}")]
        default: return [response(id: id, resultJSON: processKeyResultJSON(revision: revision))]
        }
    }
}

/// 条件が満たされるまで待つ（非同期の再起動待ち用）
func waitUntil(timeout: TimeInterval = 5, _ condition: () -> Bool) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if condition() { return true }
        try? await Task.sleep(nanoseconds: 5_000_000)
    }
    return condition()
}

/// イベントを貯めるスレッドセーフな箱
final class EventLog: @unchecked Sendable {
    private let lock = NSLock()
    private var _events: [EngineEvent] = []
    func append(_ e: EngineEvent) {
        lock.lock()
        _events.append(e)
        lock.unlock()
    }
    var events: [EngineEvent] {
        lock.lock()
        defer { lock.unlock() }
        return _events
    }
}
