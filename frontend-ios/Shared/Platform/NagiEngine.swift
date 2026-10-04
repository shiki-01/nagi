import Foundation
import NagiCore

/// コア（Rust）の C ABI のラッパー。スパイクの `NagiEngine` を、data_dir が無い場合も許すように拡張したもの。
/// JSON-RPC の 1 行を渡して応答の行を受け取る。呼び出しはメインスレッドから行う（同時呼び出し禁止）
final class NagiEngine: LineTransport {
    private var handle: OpaquePointer?

    /// エンジンを生成。失敗時は nil を返す。`dataDir` が nil なら辞書なし（読みをそのまま返す）
    init?(dataDir: String?, userDir: String?, noLLM: Bool) {
        let noLLMFlag: Int32 = noLLM ? 1 : 0
        self.handle = NagiEngine.withOptionalCString(dataDir) { (dataPtr: UnsafePointer<CChar>?) -> OpaquePointer? in
            NagiEngine.withOptionalCString(userDir) { (userPtr: UnsafePointer<CChar>?) -> OpaquePointer? in
                nagi_server_new(dataPtr, userPtr, noLLMFlag)
            }
        }
        guard handle != nil else { return nil }
    }

    private static func withOptionalCString<R>(
        _ string: String?,
        _ body: (UnsafePointer<CChar>?) -> R
    ) -> R {
        if let string = string {
            return string.withCString { (ptr: UnsafePointer<CChar>) -> R in
                body(ptr)
            }
        }
        return body(nil)
    }

    /// JSON-RPC 行を送信して応答行の配列を取得。失敗時（FFI 側のパニック等）は空配列
    func send(line: String) -> [String] {
        guard let handle = handle else { return [] }
        return line.withCString { (cLine: UnsafePointer<CChar>) -> [String] in
            guard let responsePtr = nagi_handle_line(handle, cLine) else { return [String]() }
            defer { nagi_string_free(responsePtr) }
            let responseString = String(cString: responsePtr)
            return responseString.split(separator: "\n").map(String.init)
        }
    }

    /// 起動時の注記（プロファイル名など）
    var notes: String {
        guard let handle = handle else { return "" }
        guard let notesPtr = nagi_last_notes(handle) else { return "" }
        defer { nagi_string_free(notesPtr) }
        return String(cString: notesPtr)
    }

    deinit {
        if let handle = handle {
            nagi_server_free(handle)
        }
    }

    /// コアのバージョン
    static var version: String {
        guard let versionPtr = nagi_version() else { return "unknown" }
        return String(cString: versionPtr)
    }
}
