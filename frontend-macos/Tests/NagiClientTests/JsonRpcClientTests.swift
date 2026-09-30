import Foundation
import XCTest

@testable import NagiClient

final class JsonRpcClientTests: XCTestCase {
    func testRequestAndResponseAreCorrelatedById() async throws {
        let conn = MockConnection()
        // 応答を要求と逆順に返し、ID で対応付けられることを確認する
        var held: [(Any, String)] = []
        conn.responder = { req in
            guard let id = req["id"], let method = req["method"] as? String else { return nil }
            held.append((id, method))
            if held.count < 2 { return nil }
            return held.reversed().map { response(id: $0.0, resultJSON: "{\"profile\":\"\($0.1)\"}") }
        }
        let client = JsonRpcClient(connection: conn)
        try client.start()

        struct R: Decodable { let profile: String }
        async let a: R = client.request("first", params: SessionParams(session_id: "s"))
        // 1 つ目が pending になってから 2 つ目を送る
        _ = await waitUntil { conn.sent.count == 1 }
        async let b: R = client.request("second", params: SessionParams(session_id: "s"))
        let (ra, rb) = try await (a, b)
        XCTAssertEqual(ra.profile, "first")
        XCTAssertEqual(rb.profile, "second")
    }

    func testRequestEncodesJsonRpcEnvelopeNewlineDelimitedFields() async throws {
        let conn = MockConnection()
        conn.responder = standardResponder(revision: 3)
        let client = JsonRpcClient(connection: conn)
        try client.start()
        let result: ProcessKeyResult = try await client.request(
            "process_key",
            params: ProcessKeyParams(
                session_id: "s1", keysym: 0x61, keycode: 0, text: "a",
                modifiers: Modifiers(shift: false, ctrl: false, alt: false, meta: false, caps_lock: false),
                kind: .down))
        XCTAssertEqual(result.revision, 3)
        let req = try XCTUnwrap(conn.sent.first)
        XCTAssertEqual(req["jsonrpc"] as? String, "2.0")
        XCTAssertEqual(req["method"] as? String, "process_key")
        let params = try XCTUnwrap(req["params"] as? [String: Any])
        XCTAssertEqual(params["session_id"] as? String, "s1")
        XCTAssertEqual(params["kind"] as? String, "down")
        XCTAssertEqual((params["modifiers"] as? [String: Any])?["shift"] as? Bool, false)
    }

    func testErrorResponseThrowsRpcError() async throws {
        let conn = MockConnection()
        conn.responder = { req in
            guard let id = req["id"] else { return nil }
            return ["{\"jsonrpc\":\"2.0\",\"id\":\(id),\"error\":{\"code\":-32002,\"message\":\"not init\"}}"]
        }
        let client = JsonRpcClient(connection: conn)
        try client.start()
        do {
            let _: EmptyResult = try await client.request("ping", params: EmptyParams())
            XCTFail("throws expected")
        } catch let e as NagiClientError {
            XCTAssertEqual(e, .rpc(code: RpcErrorCode.notInitialized, message: "not init"))
        }
    }

    func testTimeout() async throws {
        let conn = MockConnection()  // 無応答
        let client = JsonRpcClient(connection: conn, defaultTimeout: 0.05)
        try client.start()
        do {
            let _: EmptyResult = try await client.request("ping", params: EmptyParams())
            XCTFail("throws expected")
        } catch let e as NagiClientError {
            XCTAssertEqual(e, .timeout)
        }
    }

    func testCloseFailsPendingRequestsAndFiresOnClose() async throws {
        let conn = MockConnection()
        let client = JsonRpcClient(connection: conn, defaultTimeout: 5)
        var closeCount = 0
        client.onClose = { closeCount += 1 }
        try client.start()
        async let r: EmptyResult = client.request("ping", params: EmptyParams())
        _ = await waitUntil { conn.sent.count == 1 }
        conn.simulateClose()
        do {
            _ = try await r
            XCTFail("throws expected")
        } catch let e as NagiClientError {
            XCTAssertEqual(e, .connectionClosed)
        }
        XCTAssertEqual(closeCount, 1)
        // 閉じた後の要求も即失敗する
        do {
            let _: EmptyResult = try await client.request("ping", params: EmptyParams())
            XCTFail("throws expected")
        } catch let e as NagiClientError {
            XCTAssertEqual(e, .connectionClosed)
        }
    }

    func testNotificationHandlerReceivesMethodAndParams() throws {
        let conn = MockConnection()
        let client = JsonRpcClient(connection: conn)
        var got: (String, [String: Any])?
        client.onNotification = { method, params in
            got = (method, (try? JSONSerialization.jsonObject(with: params ?? Data())) as? [String: Any] ?? [:])
        }
        try client.start()
        // 壊れた行の後でも処理が続く
        conn.receive("not json")
        conn.receive("{\"jsonrpc\":\"2.0\",\"method\":\"notify\",\"params\":{\"level\":\"info\",\"message\":\"hi\"}}")
        XCTAssertEqual(got?.0, "notify")
        XCTAssertEqual(got?.1["message"] as? String, "hi")
    }

    func testNotifySendsMessageWithoutId() throws {
        let conn = MockConnection()
        let client = JsonRpcClient(connection: conn)
        try client.start()
        try client.notify(
            "set_context",
            params: SetContextParams(session_id: "s", surrounding_text: nil, app_id: "com.example", secure_input: true))
        let msg = try XCTUnwrap(conn.sent.first)
        XCTAssertNil(msg["id"])
        XCTAssertEqual(msg["method"] as? String, "set_context")
        let p = try XCTUnwrap(msg["params"] as? [String: Any])
        XCTAssertEqual(p["secure_input"] as? Bool, true)
        XCTAssertNil(p["surrounding_text"])  // nil は省略される
    }

    func testLineBufferSplitsAcrossChunks() {
        var buf = LineBuffer()
        XCTAssertEqual(buf.append(Data("{\"a\":".utf8)), [])
        let lines = buf.append(Data("1}\n{\"b\":2}\r\n\n{\"c\"".utf8))
        XCTAssertEqual(lines.map { String(decoding: $0, as: UTF8.self) }, ["{\"a\":1}", "{\"b\":2}"])
        XCTAssertEqual(buf.append(Data(":3}\n".utf8)).count, 1)
    }
}
