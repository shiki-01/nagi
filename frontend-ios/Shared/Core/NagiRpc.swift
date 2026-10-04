import Foundation

/// JSON-RPC の 1 行を渡して、応答の行を受け取る入口（実体は C ABI の `NagiEngine`。テストでは差し替える）
protocol LineTransport: AnyObject {
    func send(line: String) -> [String]
}

enum NagiRpcError: Error, CustomStringConvertible {
    case encodeFailed(method: String)
    /// 応答が空（FFI 側でパニックを捕捉した場合・ハンドルが無効な場合）
    case emptyResponse(method: String)
    /// コアが JSON-RPC エラーを返した（エンジン自体は生きている）
    case rpc(method: String, code: Int, message: String)
    case decodeFailed(method: String)

    /// ログ用。入力内容は含めない
    var description: String {
        switch self {
        case .encodeFailed(let m): return "encode failed (\(m))"
        case .emptyResponse(let m): return "empty response (\(m))"
        case .rpc(let m, let code, let message): return "rpc error \(code) (\(m)): \(message)"
        case .decodeFailed(let m): return "decode failed (\(m))"
        }
    }

    /// エンジンの再初期化が必要な失敗か
    var isFatal: Bool {
        switch self {
        case .rpc: return false
        default: return true
        }
    }
}

private struct RpcRequest<P: Encodable>: Encodable {
    let jsonrpc: String
    let id: Int?
    let method: String
    let params: P
}

private struct RpcErrorBody: Decodable {
    let code: Int
    let message: String
}

private struct RpcIdOnly: Decodable {
    let id: Int?
}

private struct RpcResponse<R: Decodable>: Decodable {
    let id: Int?
    let result: R?
    let error: RpcErrorBody?
}

/// Protocol.swift（typeshare 生成物）の型で JSON-RPC を呼ぶ薄いクライアント
final class NagiRpcClient {
    private let transport: LineTransport
    private var nextId = 1
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(transport: LineTransport) {
        self.transport = transport
    }

    /// 要求を送り、同じ id の応答の `result` を返す。通知行（id なし）は読み捨てる
    func call<P: Encodable, R: Decodable>(_ method: String, params: P) throws -> R {
        let id = nextId
        nextId += 1
        guard let line = encodeLine(method: method, id: id, params: params) else {
            throw NagiRpcError.encodeFailed(method: method)
        }
        let lines = transport.send(line: line)
        for response in lines {
            guard let data = response.data(using: .utf8),
                  let head = try? decoder.decode(RpcIdOnly.self, from: data),
                  head.id == id
            else { continue }
            let decoded: RpcResponse<R>
            do {
                decoded = try decoder.decode(RpcResponse<R>.self, from: data)
            } catch {
                throw NagiRpcError.decodeFailed(method: method)
            }
            if let e = decoded.error {
                throw NagiRpcError.rpc(method: method, code: e.code, message: e.message)
            }
            if let result = decoded.result { return result }
            throw NagiRpcError.decodeFailed(method: method)
        }
        throw NagiRpcError.emptyResponse(method: method)
    }

    /// 応答を待たない通知（`set_context` 等）
    func notify<P: Encodable>(_ method: String, params: P) {
        guard let line = encodeLine(method: method, id: nil, params: params) else { return }
        _ = transport.send(line: line)
    }

    private func encodeLine<P: Encodable>(method: String, id: Int?, params: P) -> String? {
        let request = RpcRequest(jsonrpc: "2.0", id: id, method: method, params: params)
        guard let data = try? encoder.encode(request) else { return nil }
        return String(data: data, encoding: .utf8)
    }
}

extension NagiRpcClient {
    /// `process_key`（押下）を送る
    func processKey(
        sessionId: String,
        keysym: UInt32,
        text: String,
        shift: Bool,
        alternatives: [AltCandidate]
    ) throws -> ProcessKeyResult {
        let modifiers = Modifiers(shift: shift, ctrl: false, alt: false, meta: false, caps_lock: false)
        let params = ProcessKeyParams(
            session_id: sessionId,
            keysym: keysym,
            keycode: 0,
            text: text,
            modifiers: modifiers,
            kind: .down,
            alternatives: NagiRpcClient.wireAlternatives(alternatives)
        )
        return try call("process_key", params: params)
    }

    /// `insert_reading`（かなの直接挿入）を送る
    func insertReading(
        sessionId: String,
        reading: String,
        alternatives: [AltCandidate]
    ) throws -> ProcessKeyResult {
        let params = InsertReadingParams(
            session_id: sessionId,
            reading: reading,
            alternatives: NagiRpcClient.wireAlternatives(alternatives)
        )
        return try call("insert_reading", params: params)
    }

    /// `perform_action`（Action 名の直接実行）を送る
    func performAction(sessionId: String, action: String) throws -> ProcessKeyResult {
        return try call("perform_action", params: PerformActionParams(session_id: sessionId, action: action))
    }

    /// 空なら nil（省略）にして送る
    static func wireAlternatives(_ alternatives: [AltCandidate]) -> [InputAlternative]? {
        if alternatives.isEmpty { return nil }
        var wire: [InputAlternative] = []
        for alt in alternatives {
            wire.append(InputAlternative(text: alt.text, distance: Float(alt.distance)))
        }
        return wire
    }
}
