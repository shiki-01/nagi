import Foundation

/// NagiClient が投げるエラー
public enum NagiClientError: Error, Equatable {
    /// エンジンと接続していない（起動前・再起動待ち）
    case notConnected
    /// 通信路が閉じた（エンジンのクラッシュ等）。未応答の要求はこれで失敗する
    case connectionClosed
    /// 応答が制限時間内に来なかった
    case timeout
    /// サーバーが JSON-RPC エラーを返した
    case rpc(code: Int, message: String)
    /// 応答の形式が不正
    case invalidResponse(String)
    /// 送信・エンコードに失敗
    case sendFailed(String)
}

/// 設計書 §3.2 / core/protocol の `error_code` と対応するコード
public enum RpcErrorCode {
    public static let parseError = -32700
    public static let invalidRequest = -32600
    public static let methodNotFound = -32601
    public static let invalidParams = -32602
    public static let internalError = -32603
    public static let sessionNotFound = -32001
    public static let notInitialized = -32002
    public static let notImplemented = -32003
    public static let engineFailure = -32004
    public static let unsupportedProtocolVersion = -32005
}

/// メソッド名（core/protocol の `method` モジュールと対応）
public enum RpcMethod {
    public static let initialize = "initialize"
    public static let processKey = "process_key"
    public static let focusIn = "focus_in"
    public static let focusOut = "focus_out"
    public static let setContext = "set_context"
    public static let setEnvironment = "set_environment"
    public static let getStatus = "get_status"
    public static let clearLearning = "clear_learning"
    public static let selectCandidate = "select_candidate"
    public static let reconvert = "reconvert"
    public static let restoreSession = "restore_session"
    public static let reset = "reset"
    public static let reloadConfig = "reload_config"
    public static let getSettings = "get_settings"
    public static let updateSettings = "update_settings"
    public static let ping = "ping"
    public static let shutdown = "shutdown"
    public static let updatePreedit = "update_preedit"
    public static let updateCandidates = "update_candidates"
    public static let notify = "notify"
}

/// プロトコルバージョン（core/protocol の `PROTOCOL_VERSION`）
public let nagiProtocolVersion: UInt32 = 1
