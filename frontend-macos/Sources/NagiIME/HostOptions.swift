import Foundation

/// アプリ起動時の設定（コマンドライン引数 > 環境変数）。エンジンのパス・データ/ユーザーディレクトリを注入する。
public struct HostOptions: Equatable {
    /// imserver 実行ファイル（`--engine` / `NAGI_ENGINE_PATH`）
    public var enginePath: String?
    /// `--data-dir` / `NAGI_DATA_DIR`
    public var dataDir: String?
    /// `--user-dir` / `NAGI_USER_DIR`（未指定ならコア側が OS の設定ディレクトリを使う）
    public var userDir: String?
    /// `--config` / `NAGI_CONFIG_FILE`
    public var configFile: String?
    /// `--connection-name`（Info.plist の InputMethodConnectionName を上書き）
    public var connectionName: String?
    /// `--no-llm`
    public var noLLM: Bool
    /// `--register`（TIS への入力ソース登録・有効化だけ行って終了する）
    public var register: Bool
    /// `--diagnose`（Nagi と標準の英字キーボードの入力ソースの属性を表示して終了する。英字の判定の調査用）
    public var diagnose: Bool
    /// `--settings`（IMKServer を起動せず、設定ウィンドウだけ開く。開発時の確認用）
    public var settings: Bool

    public init(
        enginePath: String? = nil, dataDir: String? = nil, userDir: String? = nil,
        configFile: String? = nil, connectionName: String? = nil, noLLM: Bool = false,
        register: Bool = false, settings: Bool = false, diagnose: Bool = false
    ) {
        self.enginePath = enginePath
        self.dataDir = dataDir
        self.userDir = userDir
        self.configFile = configFile
        self.connectionName = connectionName
        self.noLLM = noLLM
        self.register = register
        self.diagnose = diagnose
        self.settings = settings
    }

    public static func parse(arguments: [String], environment: [String: String]) -> HostOptions {
        var o = HostOptions(
            enginePath: environment["NAGI_ENGINE_PATH"], dataDir: environment["NAGI_DATA_DIR"],
            userDir: environment["NAGI_USER_DIR"], configFile: environment["NAGI_CONFIG_FILE"])
        var i = 0
        func value() -> String? {
            guard i + 1 < arguments.count else { return nil }
            i += 1
            return arguments[i]
        }
        while i < arguments.count {
            switch arguments[i] {
            case "--engine": o.enginePath = value() ?? o.enginePath
            case "--data-dir": o.dataDir = value() ?? o.dataDir
            case "--user-dir": o.userDir = value() ?? o.userDir
            case "--config": o.configFile = value() ?? o.configFile
            case "--connection-name": o.connectionName = value() ?? o.connectionName
            case "--no-llm": o.noLLM = true
            case "--register": o.register = true
            case "--diagnose": o.diagnose = true
            case "--settings": o.settings = true
            default: break
            }
            i += 1
        }
        return o
    }

    /// imserver に渡す引数。`--data-dir` は必須なので未指定時は `defaultDataDir`
    public func engineArguments(defaultDataDir: String) -> [String] {
        var args = ["--data-dir", dataDir ?? defaultDataDir]
        if let userDir { args += ["--user-dir", userDir] }
        if let configFile { args += ["--config", configFile] }
        if noLLM { args.append("--no-llm") }
        return args
    }
}
