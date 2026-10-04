import Foundation

/// 診断ログ。入力した文字列は記録しない（イベント・エラー・レイテンシ統計のみ）
final class DiagnosticsLog {
    static let shared = DiagnosticsLog()

    private let lock = NSLock()
    private var lines: [String] = []
    private var latenciesMs: [Double] = []
    private var keyCount = 0
    private let maxLines = 200
    private let maxLatencies = 300
    private let formatter = ISO8601DateFormatter()
    /// 設定すると `persist()` でこのファイルへ書き出す（App Group があれば本体アプリからも読める）
    var persistURL: URL?

    func add(_ message: String) {
        lock.lock()
        defer { lock.unlock() }
        lines.append("\(formatter.string(from: Date())) \(message)")
        if lines.count > maxLines { lines.removeFirst(lines.count - maxLines) }
    }

    /// 1 キー分の処理時間（コアの呼び出し 1 回以上を含む）を記録する
    func recordLatency(_ ms: Double) {
        lock.lock()
        defer { lock.unlock() }
        keyCount += 1
        latenciesMs.append(ms)
        if latenciesMs.count > maxLatencies { latenciesMs.removeFirst(latenciesMs.count - maxLatencies) }
    }

    func latencySummary() -> String {
        lock.lock()
        defer { lock.unlock() }
        guard !latenciesMs.isEmpty else { return "keys=0" }
        let sorted = latenciesMs.sorted()
        let p50 = sorted[sorted.count / 2]
        let p95 = sorted[min(sorted.count - 1, Int(Double(sorted.count) * 0.95))]
        let maxMs = sorted[sorted.count - 1]
        return String(format: "keys=%d p50=%.1fms p95=%.1fms max=%.1fms", keyCount, p50, p95, maxMs)
    }

    func logText() -> String {
        lock.lock()
        defer { lock.unlock() }
        return lines.joined(separator: "\n")
    }

    /// ログをファイルへ書き出す（失敗は無視）
    func persist(header: String = "") {
        guard let url = persistURL else { return }
        let text = header.isEmpty ? logText() : header + "\n--- log ---\n" + logText()
        try? text.write(to: url, atomically: true, encoding: .utf8)
    }
}
