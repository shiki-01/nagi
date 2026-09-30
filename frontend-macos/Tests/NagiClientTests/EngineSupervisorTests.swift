import Foundation
import XCTest

@testable import NagiClient

final class EngineSupervisorTests: XCTestCase {
    private func makeSupervisor(
        launcher: MockLauncher, scheduler: ImmediateScheduler = ImmediateScheduler(),
        backoff: BackoffPolicy = BackoffPolicy()
    ) -> (EngineSupervisor, EventLog) {
        let sup = EngineSupervisor(
            launcher: launcher,
            configuration: .init(clientVersion: "0.0.0", backoff: backoff),
            scheduler: scheduler)
        let log = EventLog()
        sup.onEvent = { log.append($0) }
        return (sup, log)
    }

    private func keyParams(_ session: String = "s1") -> ProcessKeyParams {
        ProcessKeyParams(
            session_id: session, keysym: 0x61, keycode: 0, text: "a",
            modifiers: Modifiers(shift: false, ctrl: false, alt: false, meta: false, caps_lock: false),
            kind: .down)
    }

    func testStartSendsInitializeAndProcessKeyRoundTrip() async throws {
        let launcher = MockLauncher()
        launcher.configure = { _, c in c.responder = standardResponder(revision: 1) }
        let (sup, _) = makeSupervisor(launcher: launcher)
        let result = try await sup.start()
        XCTAssertEqual(result.server_name, "nagi-engine")
        let key = try await sup.processKey(keyParams())
        XCTAssertTrue(key.consumed)
        XCTAssertEqual(launcher.connections[0].sentMethods(), ["initialize", "process_key"])
        let initParams = try XCTUnwrap(launcher.connections[0].sent[0]["params"] as? [String: Any])
        XCTAssertEqual(initParams["protocol_version"] as? Int, 1)
        XCTAssertEqual(initParams["os"] as? String, "macos")
    }

    func testCrashRestartsWithBackoffAndRestoresSession() async throws {
        let launcher = MockLauncher()
        let scheduler = ImmediateScheduler()
        let (sup, log) = makeSupervisor(launcher: launcher, scheduler: scheduler)
        let restoreParams = RestoreSessionParams(
            session_id: "s1", segments: [RestoreSegment(reading: "にほん", surface: "にほん", kind: .japanese)],
            cursor: 3, focus: nil, converting: false)
        sup.restoreProvider = { return [restoreParams] }
        try await sup.start()
        // 初回起動では restore_session を呼ばない
        XCTAssertFalse(launcher.connections[0].sentMethods().contains("restore_session"))

        launcher.connections[0].simulateClose()  // クラッシュ

        let restarted = await waitUntil { launcher.connections.count == 2 && sup.isConnected }
        XCTAssertTrue(restarted)
        let restored = await waitUntil { launcher.connections[1].sentMethods().contains("restore_session") }
        XCTAssertTrue(restored)
        XCTAssertEqual(Array(launcher.connections[1].sentMethods().prefix(2)), ["initialize", "restore_session"])
        let sent = try XCTUnwrap(launcher.connections[1].sent.last)
        let p = try XCTUnwrap(sent["params"] as? [String: Any])
        XCTAssertEqual(p["session_id"] as? String, "s1")
        XCTAssertEqual(p["cursor"] as? Int, 3)
        XCTAssertEqual(scheduler.delays, [BackoffPolicy().delay(forAttempt: 1)])

        _ = await waitUntil {
            log.events.contains { if case .restored = $0 { return true } else { return false } }
        }
        XCTAssertTrue(
            log.events.contains {
                if case .crashed(let attempt, _) = $0 { return attempt == 1 } else { return false }
            })
        // 復帰後は通常のキー入力ができる
        let key = try await sup.processKey(keyParams())
        XCTAssertTrue(key.consumed)
    }

    func testRestoreFailureIsReported() async throws {
        let launcher = MockLauncher()
        launcher.configure = { n, c in
            let base = standardResponder(revision: 0)
            c.responder = { req in
                if n > 1, req["method"] as? String == "restore_session", let id = req["id"] {
                    return ["{\"jsonrpc\":\"2.0\",\"id\":\(id),\"error\":{\"code\":-32004,\"message\":\"x\"}}"]
                }
                return base(req)
            }
        }
        let (sup, log) = makeSupervisor(launcher: launcher)
        sup.restoreProvider = {
            return [RestoreSessionParams(session_id: "s1", segments: [], cursor: 0, focus: nil, converting: false)]
        }
        try await sup.start()
        launcher.connections[0].simulateClose()
        let failed = await waitUntil {
            log.events.contains { if case .restoreFailed = $0 { return true } else { return false } }
        }
        XCTAssertTrue(failed)
    }

    func testBackoffGrowsAndEntersMinimalModeAfterThreshold() async throws {
        let launcher = MockLauncher()
        let scheduler = ImmediateScheduler()
        let backoff = BackoffPolicy(
            initialDelay: 0.1, multiplier: 2, maxDelay: 0.5, minimalAfterRestarts: 3, stableUptime: 1000)
        let (sup, log) = makeSupervisor(launcher: launcher, scheduler: scheduler, backoff: backoff)
        // 最初の起動と minimal 起動だけ成功させ、それ以外は即クラッシュさせる（起動失敗ループ）
        launcher.configure = { n, c in
            if n == 1 || launcher.minimalFlags.last == true {
                c.responder = standardResponder(revision: 0)
            } else {
                c.responder = { [weak c] _ in
                    c?.simulateClose()
                    return nil
                }
            }
        }
        try await sup.start()
        launcher.connections[0].simulateClose()

        let recovered = await waitUntil { launcher.minimalFlags.last == true && sup.isConnected }
        XCTAssertTrue(recovered)
        XCTAssertEqual(scheduler.delays, [0.1, 0.2, 0.4])
        XCTAssertEqual(launcher.minimalFlags, [false, false, false, true])
        XCTAssertTrue(
            log.events.contains { if case .enteredMinimalMode = $0 { return true } else { return false } })
    }

    func testStaleAsyncNotificationsAreDiscarded() async throws {
        let launcher = MockLauncher()
        launcher.configure = { _, c in c.responder = standardResponder(revision: 5) }
        let (sup, log) = makeSupervisor(launcher: launcher)
        try await sup.start()
        _ = try await sup.processKey(keyParams())  // session s1 の revision = 5

        let conn = launcher.connections[0]
        func update(_ rev: Int, _ text: String) -> String {
            "{\"jsonrpc\":\"2.0\",\"method\":\"update_preedit\",\"params\":{\"session_id\":\"s1\",\"revision\":\(rev),"
                + "\"preedit\":{\"text\":\"\(text)\",\"segments\":[],\"cursor\":0}}}"
        }
        conn.receive(update(4, "古い"))  // 破棄される
        conn.receive(update(6, "新しい"))  // 反映される
        conn.receive(update(5, "さらに古い"))  // 6 を受けた後なので破棄される

        let texts: [String] = log.events.compactMap {
            if case .updatePreedit(let p) = $0 { return p.preedit.text } else { return nil }
        }
        XCTAssertEqual(texts, ["新しい"])
    }

    func testStaleCandidatesDiscardedAndNotifyDelivered() async throws {
        let launcher = MockLauncher()
        launcher.configure = { _, c in c.responder = standardResponder(revision: 2) }
        let (sup, log) = makeSupervisor(launcher: launcher)
        try await sup.start()
        _ = try await sup.processKey(keyParams())
        let conn = launcher.connections[0]
        func cands(_ rev: Int) -> String {
            "{\"jsonrpc\":\"2.0\",\"method\":\"update_candidates\",\"params\":{\"session_id\":\"s1\",\"revision\":\(rev),"
                + "\"candidates\":{\"items\":[],\"selected\":0,\"page_size\":9,\"visible\":true}}}"
        }
        conn.receive(cands(1))
        conn.receive(cands(3))
        conn.receive("{\"jsonrpc\":\"2.0\",\"method\":\"notify\",\"params\":{\"level\":\"warning\",\"message\":\"w\"}}")
        let revs: [UInt32] = log.events.compactMap {
            if case .updateCandidates(let p) = $0 { return p.revision } else { return nil }
        }
        XCTAssertEqual(revs, [3])
        XCTAssertTrue(log.events.contains { if case .notify = $0 { return true } else { return false } })
    }

    func testRequestWhileDisconnectedThrowsNotConnected() async throws {
        let launcher = MockLauncher()
        // 再起動を保留する（スケジューラが実行しない）
        final class HoldScheduler: EngineScheduler {
            func schedule(after delay: TimeInterval, _ work: @escaping () -> Void) {}
        }
        let sup = EngineSupervisor(
            launcher: launcher, configuration: .init(clientVersion: "0"), scheduler: HoldScheduler())
        try await sup.start()
        launcher.connections[0].simulateClose()
        do {
            _ = try await sup.processKey(keyParams())
            XCTFail("throws expected")
        } catch let e as NagiClientError {
            XCTAssertEqual(e, .notConnected)
        }
    }

    func testRestartNowRestoresWithoutBackoff() async throws {
        let launcher = MockLauncher()
        let scheduler = ImmediateScheduler()
        let (sup, _) = makeSupervisor(launcher: launcher, scheduler: scheduler)
        sup.restoreProvider = {
            return [RestoreSessionParams(session_id: "s1", segments: [], cursor: 0, focus: nil, converting: false)]
        }
        try await sup.start()
        await sup.restartNow()
        XCTAssertEqual(launcher.connections.count, 2)
        XCTAssertTrue(launcher.connections[1].sentMethods().contains("restore_session"))
        XCTAssertTrue(scheduler.delays.isEmpty)
        XCTAssertTrue(sup.isConnected)
    }

    func testStopDoesNotRestart() async throws {
        let launcher = MockLauncher()
        let (sup, _) = makeSupervisor(launcher: launcher)
        try await sup.start()
        await sup.stop()
        try? await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(launcher.connections.count, 1)
        XCTAssertTrue(launcher.connections[0].sentMethods().contains("shutdown"))
        XCTAssertFalse(sup.isConnected)
    }
}
