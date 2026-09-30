import Foundation

/// セッションごとの revision を管理し、古い非同期通知を破棄する（設計書 §3.2）。
///
/// 同期応答（`process_key` 等）の revision を `record` で記録し、
/// 非同期通知（`update_preedit` / `update_candidates`）は `accept` が true のときだけ反映する。
public final class RevisionGate: @unchecked Sendable {
    private let lock = NSLock()
    private var latest: [String: UInt32] = [:]

    public init() {}

    /// 同期応答の revision を記録する（小さい値では巻き戻さない）
    public func record(sessionId: String, revision: UInt32) {
        lock.lock()
        defer { lock.unlock() }
        if revision >= (latest[sessionId] ?? 0) { latest[sessionId] = revision }
    }

    /// 通知を反映してよいか。現在より古ければ false。新しい・同じなら記録して true
    public func accept(sessionId: String, revision: UInt32) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if let current = latest[sessionId], revision < current { return false }
        latest[sessionId] = revision
        return true
    }

    /// 全セッションの記録を捨てる（エンジン再起動時。revision は 0 から始まり直すため）
    public func reset() {
        lock.lock()
        latest = [:]
        lock.unlock()
    }
}
