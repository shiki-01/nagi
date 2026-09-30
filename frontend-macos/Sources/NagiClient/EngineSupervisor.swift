import Foundation
@_exported import NagiProtocol

/// 再起動のバックオフ設定
public struct BackoffPolicy {
    public var initialDelay: TimeInterval
    public var multiplier: Double
    public var maxDelay: TimeInterval
    /// 連続再起動がこの回数に達したら通知し、minimal プロファイルで起動する（設計書 §3.3）
    public var minimalAfterRestarts: Int
    /// この時間以上生き延びてからのクラッシュは「連続」に数えない（カウンタを 0 に戻す）
    public var stableUptime: TimeInterval

    public init(
        initialDelay: TimeInterval = 0.2, multiplier: Double = 2, maxDelay: TimeInterval = 10,
        minimalAfterRestarts: Int = 5, stableUptime: TimeInterval = 30
    ) {
        self.initialDelay = initialDelay
        self.multiplier = multiplier
        self.maxDelay = maxDelay
        self.minimalAfterRestarts = minimalAfterRestarts
        self.stableUptime = stableUptime
    }

    /// n 回目（1 始まり）の再起動前に待つ時間
    public func delay(forAttempt attempt: Int) -> TimeInterval {
        let d = initialDelay * pow(multiplier, Double(max(attempt - 1, 0)))
        return min(d, maxDelay)
    }
}

/// スーパーバイザーが通知するイベント
public enum EngineEvent {
    /// 起動・初期化に成功（再起動後を含む）
    case started(InitializeResult, isRestart: Bool)
    /// 通信路が閉じた（クラッシュ等）。`attempt` 回目の再起動を `delay` 秒後に行う
    case crashed(attempt: Int, delay: TimeInterval)
    /// 再起動回数が閾値を超えたため minimal で起動する（ユーザーへ通知すること）
    case enteredMinimalMode(restarts: Int)
    /// 再起動後の `restore_session` に成功した。フロントは結果の preedit を描画する
    case restored(sessionId: String, result: ProcessKeyResult)
    /// 復元に失敗した。フロントは表示中の文字列をそのまま確定すること（設計書 §3.3）
    case restoreFailed(sessionId: String, error: Error)
    /// 非同期通知 `update_preedit`（古い revision は破棄済み）
    case updatePreedit(UpdatePreeditParams)
    /// 非同期通知 `update_candidates`（古い revision は破棄済み）
    case updateCandidates(UpdateCandidatesParams)
    /// `notify`（設定エラー・警告等）
    case notify(NotifyParams)
    /// 起動または初期化に失敗した（再起動は自動で予約される）
    case launchFailed(Error)
}

/// 空の params（`ping` / `shutdown` / `get_status` 等の引数なし要求）
struct EmptyParams: Encodable {}

/// エンジン子プロセスの管理: 起動・クラッシュ検知・指数バックオフ再起動・`restore_session`。
/// あわせて revision による古い通知の破棄と、型付きのメソッド呼び出しを提供する。
public final class EngineSupervisor: @unchecked Sendable {
    public struct Configuration {
        public var clientName: String
        public var clientVersion: String
        public var os: String
        /// `initialize` のタイムアウト（コールドスタート < 2s が目標。モデルロードを見込んで長め）
        public var initializeTimeout: TimeInterval
        /// 通常要求のタイムアウト（設計書 §3.3: 500ms）
        public var requestTimeout: TimeInterval
        public var backoff: BackoffPolicy

        public init(
            clientName: String = "nagi-macos", clientVersion: String, os: String = "macos",
            initializeTimeout: TimeInterval = 10, requestTimeout: TimeInterval = 0.5,
            backoff: BackoffPolicy = BackoffPolicy()
        ) {
            self.clientName = clientName
            self.clientVersion = clientVersion
            self.os = os
            self.initializeTimeout = initializeTimeout
            self.requestTimeout = requestTimeout
            self.backoff = backoff
        }
    }

    private let launcher: EngineLauncher
    private let scheduler: EngineScheduler
    private let configuration: Configuration
    private let gate = RevisionGate()

    /// 再起動後に復元すべきセッション（フロントが保持している最新の preedit）を返す。
    /// 任意のスレッドから呼ばれる
    public var restoreProvider: () -> [RestoreSessionParams] = { [] }
    /// イベントの受け手。任意のスレッドから呼ばれる
    public var onEvent: ((EngineEvent) -> Void)?

    private let lock = NSLock()
    private var client: JsonRpcClient?
    private var generation = 0
    private var restartCount = 0
    private var minimalMode = false
    private var launchedAt = Date()
    private var stopped = false
    private var hasStartedOnce = false

    public init(
        launcher: EngineLauncher, configuration: Configuration,
        scheduler: EngineScheduler = DispatchEngineScheduler()
    ) {
        self.launcher = launcher
        self.configuration = configuration
        self.scheduler = scheduler
    }

    // MARK: ライフサイクル

    /// 最初の起動。失敗した場合は自動再起動を予約したうえでエラーを投げる
    @discardableResult
    public func start() async throws -> InitializeResult {
        markRunning()
        do {
            return try await connect()
        } catch {
            handleFailure(error, generation: currentGeneration())
            throw error
        }
    }

    /// 正常終了（`shutdown` を送ってから閉じる）。以後は自動再起動しない
    public func stop() async {
        let c = markStopped()
        if let c {
            let _: EmptyResult? = try? await c.request(
                RpcMethod.shutdown, params: EmptyParams(), timeout: configuration.requestTimeout)
            c.close()
        }
    }

    /// スリープ復帰時などの強制再起動。クラッシュ扱いにはせず、バックオフも増やさない
    public func restartNow() async {
        let detached = detachForRestart()
        guard detached.ok else { return }
        detached.old?.close()
        do {
            try await connect()
        } catch {
            handleFailure(error, generation: currentGeneration())
        }
    }

    /// 現在エンジンと接続しているか
    public var isConnected: Bool {
        lock.lock()
        defer { lock.unlock() }
        return client != nil
    }

    // MARK: 接続・再起動

    // 非同期関数から NSLock を直接触らないための同期ヘルパー

    private func markRunning() {
        lock.lock()
        stopped = false
        lock.unlock()
    }

    private func markStopped() -> JsonRpcClient? {
        lock.lock()
        defer { lock.unlock() }
        stopped = true
        let c = client
        client = nil
        generation += 1
        return c
    }

    /// 強制再起動のため現在の接続を切り離す。停止済みなら nil
    private func detachForRestart() -> (old: JsonRpcClient?, ok: Bool) {
        lock.lock()
        defer { lock.unlock() }
        if stopped { return (nil, false) }
        let old = client
        client = nil
        generation += 1
        return (old, true)
    }

    private func beginConnect() -> (Int, Bool, Bool) {
        lock.lock()
        defer { lock.unlock() }
        generation += 1
        return (generation, minimalMode, hasStartedOnce)
    }

    /// 初期化済みの接続を登録する。世代が変わっていた・停止済みなら false
    private func install(_ rpc: JsonRpcClient, generation gen: Int) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if generation != gen || stopped { return false }
        client = rpc
        launchedAt = Date()
        hasStartedOnce = true
        return true
    }

    private func currentGeneration() -> Int {
        lock.lock()
        defer { lock.unlock() }
        return generation
    }

    @discardableResult
    private func connect() async throws -> InitializeResult {
        let (gen, minimal, isRestart) = beginConnect()

        let connection = try launcher.makeConnection(minimal: minimal)
        let rpc = JsonRpcClient(connection: connection, defaultTimeout: configuration.requestTimeout)
        rpc.onNotification = { [weak self] method, params in
            self?.handleNotification(method: method, params: params, generation: gen)
        }
        // onClose は初期化に成功して client を登録した後で設定する。
        // 初期化中の切断は request のエラーとして呼び出し元（start / 再起動処理）が扱う
        try rpc.start()

        let result: InitializeResult
        do {
            result = try await rpc.request(
                RpcMethod.initialize,
                params: InitializeParams(
                    protocol_version: nagiProtocolVersion, client_name: configuration.clientName,
                    client_version: configuration.clientVersion, os: configuration.os),
                timeout: configuration.initializeTimeout)
        } catch {
            rpc.close()
            throw error
        }

        guard install(rpc, generation: gen) else {
            rpc.close()
            throw NagiClientError.connectionClosed
        }

        rpc.onClose = { [weak self] in
            self?.handleFailure(NagiClientError.connectionClosed, generation: gen)
        }
        if rpc.isClosed { handleFailure(NagiClientError.connectionClosed, generation: gen) }

        gate.reset()
        onEvent?(.started(result, isRestart: isRestart))
        if isRestart { await restoreSessions(using: rpc, generation: gen) }
        return result
    }

    private func restoreSessions(using rpc: JsonRpcClient, generation gen: Int) async {
        for params in restoreProvider() {
            guard currentGeneration() == gen else { return }
            do {
                let result: ProcessKeyResult = try await rpc.request(
                    RpcMethod.restoreSession, params: params, timeout: configuration.initializeTimeout)
                gate.record(sessionId: params.session_id, revision: result.revision)
                onEvent?(.restored(sessionId: params.session_id, result: result))
            } catch {
                onEvent?(.restoreFailed(sessionId: params.session_id, error: error))
            }
        }
    }

    /// 通信路が閉じた・起動に失敗した: 世代が古ければ無視し、バックオフ後に再起動を予約する
    private func handleFailure(_ error: Error, generation gen: Int) {
        lock.lock()
        if generation != gen || stopped {
            lock.unlock()
            return
        }
        // 同じ世代の失敗を二重に処理しない（世代を進めて以後の通知を無効化）
        generation += 1
        let old = client
        client = nil
        let uptime = Date().timeIntervalSince(launchedAt)
        if old != nil, uptime >= configuration.backoff.stableUptime { restartCount = 0 }
        restartCount += 1
        let attempt = restartCount
        var enteredMinimal = false
        if attempt >= configuration.backoff.minimalAfterRestarts, !minimalMode {
            minimalMode = true
            enteredMinimal = true
        }
        lock.unlock()

        old?.close()
        let delay = configuration.backoff.delay(forAttempt: attempt)
        if (error as? NagiClientError) == .connectionClosed {
            onEvent?(.crashed(attempt: attempt, delay: delay))
        } else {
            onEvent?(.launchFailed(error))
            onEvent?(.crashed(attempt: attempt, delay: delay))
        }
        if enteredMinimal { onEvent?(.enteredMinimalMode(restarts: attempt)) }

        scheduler.schedule(after: delay) { [weak self] in
            guard let self else { return }
            Task {
                do {
                    try await self.connect()
                } catch {
                    self.handleFailure(error, generation: self.currentGeneration())
                }
            }
        }
    }

    // MARK: 通知

    private func handleNotification(method: String, params: Data?, generation gen: Int) {
        guard currentGeneration() == gen, let params else { return }
        let decoder = JSONDecoder()
        switch method {
        case RpcMethod.updatePreedit:
            guard let p = try? decoder.decode(UpdatePreeditParams.self, from: params),
                gate.accept(sessionId: p.session_id, revision: p.revision)
            else { return }
            onEvent?(.updatePreedit(p))
        case RpcMethod.updateCandidates:
            guard let p = try? decoder.decode(UpdateCandidatesParams.self, from: params),
                gate.accept(sessionId: p.session_id, revision: p.revision)
            else { return }
            onEvent?(.updateCandidates(p))
        case RpcMethod.notify:
            guard let p = try? decoder.decode(NotifyParams.self, from: params) else { return }
            onEvent?(.notify(p))
        default:
            break
        }
    }

    // MARK: 型付きメソッド

    private func connectedClient() throws -> JsonRpcClient {
        lock.lock()
        defer { lock.unlock() }
        guard let client else { throw NagiClientError.notConnected }
        return client
    }

    private func diff<P: Encodable>(_ method: String, _ params: P, sessionId: String) async throws
        -> ProcessKeyResult
    {
        let c = try connectedClient()
        let result: ProcessKeyResult = try await c.request(method, params: params)
        gate.record(sessionId: sessionId, revision: result.revision)
        return result
    }

    public func processKey(_ params: ProcessKeyParams) async throws -> ProcessKeyResult {
        try await diff(RpcMethod.processKey, params, sessionId: params.session_id)
    }

    /// 実サーバーは描画差分（`ProcessKeyResult`）を返す（Protocol.swift のコメントは `EmptyResult`）
    public func focusIn(sessionId: String) async throws -> ProcessKeyResult {
        try await diff(RpcMethod.focusIn, SessionParams(session_id: sessionId), sessionId: sessionId)
    }

    /// 表示中の surface を確定して返す
    public func focusOut(sessionId: String) async throws -> ProcessKeyResult {
        try await diff(RpcMethod.focusOut, SessionParams(session_id: sessionId), sessionId: sessionId)
    }

    public func reset(sessionId: String) async throws -> ProcessKeyResult {
        try await diff(RpcMethod.reset, SessionParams(session_id: sessionId), sessionId: sessionId)
    }

    public func selectCandidate(_ params: SelectCandidateParams) async throws -> ProcessKeyResult {
        try await diff(RpcMethod.selectCandidate, params, sessionId: params.session_id)
    }

    public func reconvert(_ params: ReconvertParams) async throws -> ProcessKeyResult {
        try await diff(RpcMethod.reconvert, params, sessionId: params.session_id)
    }

    public func setContext(_ params: SetContextParams) throws {
        try connectedClient().notify(RpcMethod.setContext, params: params)
    }

    public func setEnvironment(_ params: SetEnvironmentParams) throws {
        try connectedClient().notify(RpcMethod.setEnvironment, params: params)
    }

    public func getStatus() async throws -> StatusResult {
        try await connectedClient().request(RpcMethod.getStatus, params: EmptyParams())
    }

    public func reloadConfig() async throws -> ReloadConfigResult {
        try await connectedClient().request(
            RpcMethod.reloadConfig, params: EmptyParams(), timeout: configuration.initializeTimeout)
    }

    /// 設定項目の記述子と現在値を取得する（G-41）
    public func getSettings() async throws -> SettingsSnapshot {
        try await connectedClient().request(
            RpcMethod.getSettings, params: EmptyParams(), timeout: configuration.initializeTimeout)
    }

    /// 設定を更新する。`value` が nil の変更は明示指定を削除して既定へ戻す。
    /// 適用後にコアがホットリロードするため、通常要求より長いタイムアウトで待つ
    public func updateSettings(changes: [SettingChange]) async throws -> UpdateSettingsResult {
        try await connectedClient().request(
            RpcMethod.updateSettings, params: UpdateSettingsParams(changes: changes),
            timeout: configuration.initializeTimeout)
    }

    public func clearLearning() async throws {
        let _: EmptyResult = try await connectedClient().request(
            RpcMethod.clearLearning, params: EmptyParams())
    }

    public func ping() async throws {
        let _: EmptyResult = try await connectedClient().request(RpcMethod.ping, params: EmptyParams())
    }
}
