import Foundation

/// 診断ログ（docs/ios-device-checklist.md の D-1）。
///
/// キーボード拡張が、コアとのやり取りとホストへの操作を App Group の `diag/keyboard.log` に追記する。
/// 直近 `maxLines` 行だけ残す。**入力内容を含む**ので、App Group の UserDefaults のフラグ
/// `diagLogEnabled`（既定 ON。TestFlight 期間中のみ。配布前に既定を OFF にする）で切り替える。
/// ホストアプリの「入力テスト」タブで表示・コピー・消去できる。
///
/// 書き込みは直列キューで行う（スレッド安全）。ファイルは毎回、読み直して切り詰め、原子的に書き換える
/// （ホストアプリが消去しても古い内容が戻らない。200 行程度なので十分小さい）
enum DiagLog {
    static let enabledKey = "diagLogEnabled"
    static let maxLines = 200

    private static let queue = DispatchQueue(label: "com.shiki.nagi.diaglog")
    private static let defaults = UserDefaults(suiteName: SharedStore.shared.appGroupId)
    private static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "HH:mm:ss.SSS"
        return f
    }()

    /// ログを記録するか（未設定は ON）
    static var isEnabled: Bool {
        get { (defaults?.object(forKey: enabledKey) as? Bool) ?? true }
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

    /// キューの上で呼ぶ
    private static func append(_ line: String) {
        guard let url = fileURL else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        var lines: [String] = []
        if let existing = try? String(contentsOf: url, encoding: .utf8) {
            lines = existing.split(separator: "\n", omittingEmptySubsequences: true).map(String.init)
        }
        lines.append(line)
        if lines.count > maxLines {
            lines.removeFirst(lines.count - maxLines)
        }
        try? (lines.joined(separator: "\n") + "\n").write(to: url, atomically: true, encoding: .utf8)
    }
}
