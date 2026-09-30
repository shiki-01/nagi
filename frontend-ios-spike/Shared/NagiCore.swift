import Foundation
import NagiCore

/// NagiEngine: C ABI のラッパー
class NagiEngine {
    private var handle: OpaquePointer?
    private let queue = DispatchQueue(label: "com.shiki.nagi.engine")

    /// エンジンを生成。失敗時は nil を返す
    init?(dataDir: String, userDir: String? = nil, noLLM: Bool = false) {
        let noLLMFlag: Int32 = noLLM ? 1 : 0
        self.handle = dataDir.withCString { (dataDirPtr: UnsafePointer<CChar>) -> OpaquePointer? in
            if let userDir = userDir {
                return userDir.withCString { (userDirPtr: UnsafePointer<CChar>) -> OpaquePointer? in
                    nagi_server_new(dataDirPtr, userDirPtr, noLLMFlag)
                }
            }
            return nagi_server_new(dataDirPtr, nil, noLLMFlag)
        }

        guard handle != nil else { return nil }
    }

    /// JSON-RPC 行を送信して応答行の配列を取得
    func send(line: String) -> [String] {
        guard let handle = handle else { return [] }

        return line.withCString { (cLine: UnsafePointer<CChar>) -> [String] in
            guard let responsePtr = nagi_handle_line(handle, cLine) else { return [String]() }
            defer { nagi_string_free(responsePtr) }

            let responseString = String(cString: responsePtr)
            return responseString.split(separator: "\n").map(String.init)
        }
    }

    /// ビルド時の注記（プロファイル名など）を取得
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

    /// エンジンのバージョンを取得
    static var version: String {
        guard let versionPtr = nagi_version() else { return "unknown" }
        return String(cString: versionPtr)
    }
}
