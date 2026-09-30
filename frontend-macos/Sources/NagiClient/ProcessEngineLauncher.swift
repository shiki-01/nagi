import Foundation

/// 子プロセス（imserver）を stdin/stdout の JSON-RPC 通信路として扱う実装
public final class ProcessEngineConnection: EngineConnection, @unchecked Sendable {
    private let process = Process()
    private let stdinPipe = Pipe()
    private let stdoutPipe = Pipe()
    private let lock = NSLock()
    private var onLine: ((Data) -> Void)?
    private var onClose: (() -> Void)?
    private var closeNotified = false
    private var lineBuffer = LineBuffer()

    public init(executable: URL, arguments: [String], environment: [String: String]? = nil) {
        process.executableURL = executable
        process.arguments = arguments
        if let environment { process.environment = environment }
        process.standardInput = stdinPipe
        process.standardOutput = stdoutPipe
        // stderr は親から継承する（サーバーは入力内容をログに出さない方針）
    }

    public func setHandlers(onLine: @escaping (Data) -> Void, onClose: @escaping () -> Void) {
        lock.lock()
        self.onLine = onLine
        self.onClose = onClose
        lock.unlock()
    }

    public func start() throws {
        stdoutPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            guard let self else { return }
            let data = handle.availableData
            if data.isEmpty {
                // EOF = 子プロセスの終了
                handle.readabilityHandler = nil
                self.notifyClose()
                return
            }
            self.lock.lock()
            let lines = self.lineBuffer.append(data)
            let handler = self.onLine
            self.lock.unlock()
            for line in lines { handler?(line) }
        }
        try process.run()
    }

    public func send(_ message: Data) throws {
        var payload = message
        payload.append(0x0A)
        do {
            try stdinPipe.fileHandleForWriting.write(contentsOf: payload)
        } catch {
            throw NagiClientError.sendFailed("\(error)")
        }
    }

    public func terminate() {
        stdoutPipe.fileHandleForReading.readabilityHandler = nil
        try? stdinPipe.fileHandleForWriting.close()
        if process.isRunning { process.terminate() }
        notifyClose()
    }

    private func notifyClose() {
        lock.lock()
        if closeNotified {
            lock.unlock()
            return
        }
        closeNotified = true
        let handler = onClose
        lock.unlock()
        handler?()
    }
}

/// 子プロセスの起動設定
public struct ProcessEngineConfiguration {
    public var executable: URL
    public var arguments: [String]
    /// minimal プロファイル（モデル起因のクラッシュループ回避）で起動するときに追加する引数
    public var minimalExtraArguments: [String]
    public var environment: [String: String]?

    public init(
        executable: URL, arguments: [String] = [],
        minimalExtraArguments: [String] = ["--no-llm"], environment: [String: String]? = nil
    ) {
        self.executable = executable
        self.arguments = arguments
        self.minimalExtraArguments = minimalExtraArguments
        self.environment = environment
    }
}

public final class ProcessEngineLauncher: EngineLauncher {
    private let configuration: ProcessEngineConfiguration

    public init(configuration: ProcessEngineConfiguration) {
        self.configuration = configuration
        // 死んだ子プロセスの stdin への書き込みで SIGPIPE により本体が落ちるのを防ぐ
        // （書き込みは EPIPE エラーとして throw される）
        signal(SIGPIPE, SIG_IGN)
    }

    public func makeConnection(minimal: Bool) throws -> EngineConnection {
        let args = configuration.arguments + (minimal ? configuration.minimalExtraArguments : [])
        return ProcessEngineConnection(
            executable: configuration.executable, arguments: args, environment: configuration.environment)
    }
}
