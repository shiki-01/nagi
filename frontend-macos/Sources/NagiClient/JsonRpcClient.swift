import Foundation

/// JSON-RPC 2.0 クライアント（改行区切り）。通信路は `EngineConnection` で抽象化する。
///
/// - 要求は連番 ID で管理し、応答を ID で対応付ける
/// - 通知（サーバー→フロント）は `onNotification` に生の params で渡す
/// - 通信路が閉じたら未応答の要求はすべて `connectionClosed` で失敗する
public final class JsonRpcClient: @unchecked Sendable {
    private typealias Completion = (Result<Data, NagiClientError>) -> Void

    private let connection: EngineConnection
    private let lock = NSLock()
    private var nextId: Int64 = 1
    private var pending: [Int64: Completion] = [:]
    private var closed = false
    private var lineBuffer = LineBuffer()
    private let timeoutQueue = DispatchQueue(label: "com.shiki.nagi.rpc-timeout")

    /// 要求ごとの既定タイムアウト（秒）。設計書 §3.3: 応答が 500ms 無ければ異常
    public var defaultTimeout: TimeInterval

    /// サーバー通知のハンドラ `(method, params の JSON)`。任意のスレッドから呼ばれる
    public var onNotification: ((String, Data?) -> Void)? {
        get { lock.lock(); defer { lock.unlock() }; return _onNotification }
        set { lock.lock(); _onNotification = newValue; lock.unlock() }
    }
    private var _onNotification: ((String, Data?) -> Void)?

    /// 通信路が閉じたとき（高々 1 回）
    public var onClose: (() -> Void)? {
        get { lock.lock(); defer { lock.unlock() }; return _onClose }
        set { lock.lock(); _onClose = newValue; lock.unlock() }
    }
    private var _onClose: (() -> Void)?

    public init(connection: EngineConnection, defaultTimeout: TimeInterval = 0.5) {
        self.connection = connection
        self.defaultTimeout = defaultTimeout
        connection.setHandlers(
            onLine: { [weak self] line in self?.handleLine(line) },
            onClose: { [weak self] in self?.handleClose() }
        )
    }

    /// 通信路が閉じ済みか
    public var isClosed: Bool {
        lock.lock()
        defer { lock.unlock() }
        return closed
    }

    /// 通信を開始する（プロセス起動）
    public func start() throws {
        try connection.start()
    }

    /// 通信路を閉じる。未応答の要求は `connectionClosed` で失敗する
    public func close() {
        connection.terminate()
        handleClose()
    }

    // MARK: 送信

    /// 要求を送り、応答の result を `R` にデコードして返す
    public func request<P: Encodable, R: Decodable>(
        _ method: String, params: P, as type: R.Type = R.self, timeout: TimeInterval? = nil
    ) async throws -> R {
        let data = try await requestRaw(method, params: params, timeout: timeout)
        do {
            return try JSONDecoder().decode(R.self, from: data)
        } catch {
            throw NagiClientError.invalidResponse("\(method): \(error)")
        }
    }

    /// 通知（応答なし）を送る
    public func notify<P: Encodable>(_ method: String, params: P) throws {
        let body = try encodeParams(params)
        try send(["jsonrpc": "2.0", "method": method, "params": body])
    }

    private func requestRaw<P: Encodable>(
        _ method: String, params: P, timeout: TimeInterval?
    ) async throws -> Data {
        let body = try encodeParams(params)
        let limit = timeout ?? defaultTimeout
        return try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Data, Error>) in
            lock.lock()
            if closed {
                lock.unlock()
                cont.resume(throwing: NagiClientError.connectionClosed)
                return
            }
            let id = nextId
            nextId += 1
            pending[id] = { result in
                switch result {
                case .success(let data): cont.resume(returning: data)
                case .failure(let error): cont.resume(throwing: error)
                }
            }
            lock.unlock()

            timeoutQueue.asyncAfter(deadline: .now() + limit) { [weak self] in
                self?.complete(id: id, with: .failure(.timeout))
            }
            do {
                try send(["jsonrpc": "2.0", "id": id, "method": method, "params": body])
            } catch {
                let e = (error as? NagiClientError) ?? .sendFailed("\(error)")
                complete(id: id, with: .failure(e))
            }
        }
    }

    /// pending から取り出して一度だけ完了させる（応答・タイムアウト・切断の競合対策）
    private func complete(id: Int64, with result: Result<Data, NagiClientError>) {
        lock.lock()
        let completion = pending.removeValue(forKey: id)
        lock.unlock()
        completion?(result)
    }

    private func encodeParams<P: Encodable>(_ params: P) throws -> Any {
        do {
            let data = try JSONEncoder().encode(params)
            return try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        } catch {
            throw NagiClientError.sendFailed("encode: \(error)")
        }
    }

    private func send(_ object: [String: Any]) throws {
        do {
            let data = try JSONSerialization.data(withJSONObject: object, options: [.withoutEscapingSlashes])
            try connection.send(data)
        } catch let e as NagiClientError {
            throw e
        } catch {
            throw NagiClientError.sendFailed("\(error)")
        }
    }

    // MARK: 受信

    private func handleLine(_ chunk: Data) {
        // 通信路実装が行単位で渡す前提だが、念のため改行を含む場合は分割する
        lock.lock()
        let lines = lineBuffer.append(chunk + Data([0x0A]))
        lock.unlock()
        for line in lines { dispatch(line) }
    }

    private func dispatch(_ line: Data) {
        guard let obj = (try? JSONSerialization.jsonObject(with: line, options: [.fragmentsAllowed])) as? [String: Any]
        else { return }  // 壊れた行は捨てる（後続の行は処理する）

        if let method = obj["method"] as? String {
            // サーバー通知（id なし）。サーバー→フロントの要求は現状存在しない
            var params: Data?
            if let p = obj["params"], !(p is NSNull) {
                params = try? JSONSerialization.data(withJSONObject: p, options: [.fragmentsAllowed])
            }
            let handler = onNotification
            handler?(method, params)
            return
        }

        guard let idNumber = obj["id"] as? NSNumber else { return }
        let id = idNumber.int64Value
        if let err = obj["error"] as? [String: Any] {
            let code = (err["code"] as? NSNumber)?.intValue ?? RpcErrorCode.internalError
            let message = err["message"] as? String ?? ""
            complete(id: id, with: .failure(.rpc(code: code, message: message)))
        } else if let result = obj["result"] {
            if let data = try? JSONSerialization.data(withJSONObject: result, options: [.fragmentsAllowed]) {
                complete(id: id, with: .success(data))
            } else {
                complete(id: id, with: .failure(.invalidResponse("result")))
            }
        } else {
            complete(id: id, with: .failure(.invalidResponse("result も error も無い応答")))
        }
    }

    private func handleClose() {
        lock.lock()
        if closed {
            lock.unlock()
            return
        }
        closed = true
        let waiting = pending
        pending = [:]
        let handler = _onClose
        lock.unlock()
        for (_, completion) in waiting { completion(.failure(.connectionClosed)) }
        handler?()
    }
}
