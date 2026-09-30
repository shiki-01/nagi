import Foundation
import NagiClient

/// セッションごとの最新の preedit を保持する（設計書 §3.3「入力を失わない」）。
/// エンジン再起動時の `restore_session` の材料と、復元失敗時の「表示中の文字列の確定」に使う。
public final class SessionStore: @unchecked Sendable {
    private let lock = NSLock()
    private var preedits: [String: PreeditView] = [:]

    public init() {}

    /// 空の preedit（text が空）は「未確定なし」として削除する
    public func update(sessionId: String, preedit: PreeditView?) {
        lock.lock()
        defer { lock.unlock() }
        if let preedit, !preedit.text.isEmpty {
            preedits[sessionId] = preedit
        } else {
            preedits[sessionId] = nil
        }
    }

    public func preedit(for sessionId: String) -> PreeditView? {
        lock.lock()
        defer { lock.unlock() }
        return preedits[sessionId]
    }

    public func remove(sessionId: String) {
        update(sessionId: sessionId, preedit: nil)
    }

    /// `EngineSupervisor.restoreProvider` に渡す。segments が空のもの（保留ローマ字のみ）は復元対象外
    public func restoreParams() -> [RestoreSessionParams] {
        lock.lock()
        let snapshot = preedits
        lock.unlock()
        return snapshot.compactMap { id, p in
            guard !p.segments.isEmpty else { return nil }
            return RestoreSessionParams.make(sessionId: id, preedit: p, converting: p.focus != nil)
        }
    }
}
