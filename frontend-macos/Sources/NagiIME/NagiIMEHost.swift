#if os(macOS)
import AppKit
import InputMethodKit
import NagiClient

/// アプリ起動時の組み立て: EngineSupervisor の生成・起動、IMKServer の起動、スリープ復帰時の再起動。
public final class NagiIMEHost {
    /// Info.plist に `InputMethodConnectionName` が無いときの既定値（バンドル ID から派生）
    public static var defaultConnectionName: String {
        BundleIdentity.connectionName(bundleID: defaultBundleIdentifier)
    }
    public static var defaultBundleIdentifier: String {
        BundleIdentity.resolve(Bundle.main.bundleIdentifier)
    }

    public enum HostError: Error {
        case engineNotFound
    }

    private let options: HostOptions
    private var server: IMKServer?
    private var context: NagiIMEContext?
    private var wakeObserver: NSObjectProtocol?

    public init(options: HostOptions) {
        self.options = options
    }

    /// エンジン実行ファイルの解決: `--engine` > アプリバンドル同梱（Contents/MacOS/imserver）
    private func resolveEngineURL() throws -> URL {
        if let path = options.enginePath { return URL(fileURLWithPath: path) }
        if let url = Bundle.main.url(forAuxiliaryExecutable: "imserver") { return url }
        throw HostError.engineNotFound
    }

    private static func defaultDataDir() -> String {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSHomeDirectory())
        return base.appendingPathComponent("com.shiki.nagi/data").path
    }

    /// メインスレッドで呼ぶこと（`NSApplication.shared` を作成済みであること）
    public func start() throws {
        let engineURL = try resolveEngineURL()
        let launcher = ProcessEngineLauncher(
            configuration: ProcessEngineConfiguration(
                executable: engineURL,
                arguments: options.engineArguments(defaultDataDir: Self.defaultDataDir()),
                // `--no-llm` が既に引数にある場合、minimal 用の追加で二重指定にならないようにする
                minimalExtraArguments: options.noLLM ? [] : ["--no-llm"]))
        let version =
            Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? "0.0.0"
        let supervisor = EngineSupervisor(
            launcher: launcher, configuration: EngineSupervisor.Configuration(clientVersion: version))

        let store = SessionStore()
        let context = NagiIMEContext(supervisor: supervisor, store: store)
        self.context = context
        NagiIMEContext.shared = context
        supervisor.restoreProvider = { store.restoreParams() }
        supervisor.onEvent = { [weak context] event in
            // イベントは任意スレッドから来る。IMKit / AppKit の操作はメインで行う
            DispatchQueue.main.async { context?.handle(event) }
        }

        let connectionName =
            options.connectionName
            ?? (Bundle.main.object(forInfoDictionaryKey: "InputMethodConnectionName") as? String)
            ?? Self.defaultConnectionName
        server = IMKServer(
            name: connectionName,
            bundleIdentifier: Self.defaultBundleIdentifier)

        // スリープ復帰でパイプが破棄される事象への対処（設計書 §3.3）
        wakeObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification, object: nil, queue: .main
        ) { _ in
            Task { await supervisor.restartNow() }
        }

        // 失敗時は EngineSupervisor が自動再起動を予約する
        Task {
            do {
                try await supervisor.start()
            } catch {
                NSLog("nagi: initial engine start failed: \(error)")
            }
        }
    }
}
#endif
