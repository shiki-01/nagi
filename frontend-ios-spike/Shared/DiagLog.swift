import Foundation

/// 診断ログ（docs/ios-device-checklist.md の D-1）。
///
/// キーボード拡張が、コアとのやり取りとホストへの操作を App Group の `diag/keyboard.log` に追記する。
/// **入力内容を含む**ので、既定は「TestFlight 配布・開発用ビルドでのみ ON、App Store 配布は OFF」
/// （`defaultEnabled`）。App Group の UserDefaults のフラグ `diagLogEnabled` で明示的に切り替えられる
/// （ホストアプリの「入力テスト」タブ・設定）。ホストアプリの「入力テスト」タブで表示・コピー・消去できる。
///
/// 書き込みは直列キューで行う（スレッド安全）。追記だけを行い（読み直さない）、ファイルが `maxBytes` を
/// 超えたら後半の `keepBytes` だけを残して切り詰める（連打中の負荷を小さくするため）
enum DiagLog {
    static let enabledKey = "diagLogEnabled"
    /// ファイルの上限（バイト）。超えたら切り詰める
    static let maxBytes = 64 * 1024
    /// 切り詰めたあとに残す量（バイト。行の途中から始まらないよう、最初の改行の次から）
    private static let keepBytes = 32 * 1024

    private static let queue = DispatchQueue(label: "com.shiki.nagi.diaglog")
    private static let defaults = UserDefaults(suiteName: SharedStore.shared.appGroupId)
    private static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "HH:mm:ss.SSS"
        return f
    }()

    /// 明示の設定が無いときの既定。DEBUG ビルド、または TestFlight / 開発用の署名（レシートが
    /// `sandboxReceipt`）のときだけ ON。App Store 配布は OFF
    static var defaultEnabled: Bool { isTestBuild }

    /// DEBUG ビルド、または TestFlight / 開発用の署名か。App Store 配布では false。
    /// 検証用の機能（候補バーの「検証」・Probe の入力欄挿入など）を、この判定のビルドだけで有効にする
    static var isTestBuild: Bool {
        #if DEBUG
        return true
        #else
        return Bundle.main.appStoreReceiptURL?.lastPathComponent == "sandboxReceipt"
        #endif
    }

    /// ログを記録するか（未設定は `defaultEnabled`）
    static var isEnabled: Bool {
        get { (defaults?.object(forKey: enabledKey) as? Bool) ?? defaultEnabled }
        set { defaults?.set(newValue, forKey: enabledKey) }
    }

    /// 1 行を追記する。無効のときは `message` を評価しない
    static func log(_ message: @autoclosure () -> String) {
        guard isEnabled else { return }
        let line = "\(formatter.string(from: Date())) \(message())"
        queue.async { append(line) }
    }

    /// 全文を読む（古い順）
    static func read() -> String {
        queue.sync {
            guard let url = fileURL else { return "" }
            return (try? String(contentsOf: url, encoding: .utf8)) ?? ""
        }
    }

    /// ログを消す
    static func clear() {
        queue.sync {
            guard let url = fileURL else { return }
            try? FileManager.default.removeItem(at: url)
        }
    }

    /// 文字列をログ向けに整形する（改行などをエスケープし、UTF-16 の長さを付ける）。
    /// `redact` なら内容を伏せて長さだけにする（パスワード欄など）
    static func quote(_ text: String, redact: Bool = false) -> String {
        let count = text.utf16.count
        if redact { return "<secure>(u16=\(count))" }
        var out = ""
        for scalar in text.unicodeScalars {
            switch scalar {
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            case "\"": out += "\\\""
            default: out.unicodeScalars.append(scalar)
            }
        }
        return "\"\(out)\"(u16=\(count))"
    }

    // MARK: - 表示用（ホストアプリの入力テスト）

    /// ログの概要（本文は読まず、サイズと行数、最終更新だけ）
    struct Summary: Equatable {
        var lineCount: Int
        var byteCount: Int
        var modified: Date?

        static let empty = Summary(lineCount: 0, byteCount: 0, modified: nil)
    }

    /// ログの概要を取る。ファイルは最大 `maxBytes` 程度なので、改行を数えるだけ
    static func summary() -> Summary {
        queue.sync {
            guard let url = fileURL, let data = try? Data(contentsOf: url) else { return .empty }
            let modified = (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
            return Summary(lineCount: lineCount(in: data), byteCount: data.count, modified: modified)
        }
    }

    /// 改行で終わる行の数（末尾が改行で終わらない最後の行も 1 行と数える）
    static func lineCount(in data: Data) -> Int {
        if data.isEmpty { return 0 }
        var count = 0
        for byte in data where byte == 0x0A { count += 1 }
        return data.last == 0x0A ? count : count + 1
    }

    /// 概要を 1 行の文言にする（時刻は `timeZone` で表示。既定は端末のタイムゾーン）
    static func summaryText(_ summary: Summary, timeZone: TimeZone = .current) -> String {
        if summary.lineCount == 0 && summary.byteCount == 0 { return "ログは空です" }
        let size: String
        if summary.byteCount < 1024 {
            size = "\(summary.byteCount) B"
        } else {
            size = String(format: "%.1f KB", Double(summary.byteCount) / 1024)
        }
        var text = "\(summary.lineCount) 行 / \(size)"
        if let modified = summary.modified {
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX")
            f.timeZone = timeZone
            f.dateFormat = "yyyy-MM-dd HH:mm:ss"
            text += " / 最終更新 \(f.string(from: modified))"
        }
        return text
    }

    /// 全文の末尾の `maxLines` 行（古い順）。末尾の改行で空行が増えないようにする
    static func tailLines(of text: String, maxLines: Int) -> [String] {
        guard maxLines > 0, !text.isEmpty else { return [] }
        var lines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        if lines.last == "" { lines.removeLast() }
        return Array(lines.suffix(maxLines))
    }

    /// 共有用に、ログのコピーを一時ディレクトリに作る（ログが空なら nil）
    static func exportCopy() -> URL? {
        queue.sync {
            guard let url = fileURL, let data = try? Data(contentsOf: url), !data.isEmpty else { return nil }
            let dest = FileManager.default.temporaryDirectory.appendingPathComponent("nagi-keyboard.log")
            do {
                try data.write(to: dest, options: .atomic)
                return dest
            } catch {
                return nil
            }
        }
    }

    // MARK: - 内部

    /// App Group の `diag/keyboard.log`。App Group が無ければプロセス固有の Application Support
    /// （この場合ホストアプリからは読めない）
    private static var fileURL: URL? {
        let base: URL?
        if let container = SharedStore.shared.containerURL {
            base = container
        } else {
            base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
        }
        return base?.appendingPathComponent("diag", isDirectory: true).appendingPathComponent("keyboard.log")
    }

    /// キューの上で呼ぶ。末尾に追記するだけ。上限を超えたら切り詰める
    private static func append(_ line: String) {
        guard let url = fileURL, let data = (line + "\n").data(using: .utf8) else { return }
        let fileManager = FileManager.default
        if !fileManager.fileExists(atPath: url.path) {
            try? fileManager.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            fileManager.createFile(atPath: url.path, contents: data)
            return
        }
        guard let handle = try? FileHandle(forWritingTo: url) else { return }
        defer { try? handle.close() }
        do {
            try handle.seekToEnd()
            try handle.write(contentsOf: data)
            if try handle.offset() > UInt64(maxBytes) { trim(url) }
        } catch {
            // ログの失敗は無視する
        }
    }

    /// 後半の `keepBytes` だけを残す（原子的に書き換える）
    private static func trim(_ url: URL) {
        guard let all = try? Data(contentsOf: url), all.count > keepBytes else { return }
        var tail = all.suffix(keepBytes)
        if let newline = tail.firstIndex(of: 0x0A) {
            tail = tail[tail.index(after: newline)...]
        }
        try? Data(tail).write(to: url, options: .atomic)
    }
}
