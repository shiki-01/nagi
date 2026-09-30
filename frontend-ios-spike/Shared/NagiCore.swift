import Foundation
import NagiCore

/// NagiEngine: C ABI のラッパー
class NagiEngine {
    private var handle: UnsafeMutableRawPointer?
    private let queue = DispatchQueue(label: "com.shiki.nagi.engine")

    /// エンジンを生成。失敗時は nil を返す
    init?(dataDir: String, userDir: String? = nil, noLLM: Bool = false) {
        let safedataDir = dataDir.utf8CString
        let safeuserDir = (userDir ?? "").utf8CString

        self.handle = safedataDir.withUnsafeBytes { dataDirBytes in
            safeuserDir.withUnsafeBytes { userDirBytes in
                nagi_server_new(
                    dataDirBytes.baseAddress?.assumingMemoryBound(to: CChar.self),
                    userDirBytes.baseAddress?.assumingMemoryBound(to: CChar.self),
                    noLLM ? 1 : 0
                )
            }
        }

        guard handle != nil else { return nil }
    }

    /// JSON-RPC 行を送信して応答行の配列を取得
    func send(line: String) -> [String] {
        guard let handle = handle else { return [] }

        let lineC = line.utf8CString
        return lineC.withUnsafeBytes { lineBytes in
            let cLine = lineBytes.baseAddress?.assumingMemoryBound(to: CChar.self)
            guard let responsePtr = nagi_handle_line(handle, cLine) else { return [] }
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
