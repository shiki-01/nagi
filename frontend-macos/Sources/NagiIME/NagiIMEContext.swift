#if os(macOS)
import AppKit
import NagiClient

/// IMKit がコントローラを生成するときに参照する共有状態。アプリ起動時に `NagiIMEHost` が設定する。
/// IMKInputController は IMKit がクラス名から生成するため、依存はここ経由で注入する。
public final class NagiIMEContext {
    /// メインスレッドからのみ読み書きする
    public static var shared: NagiIMEContext?

    public let supervisor: EngineSupervisor
    public let store: SessionStore
    let panel = CandidatePanel()
    /// 同期要求の待ち上限（EngineSupervisor の要求タイムアウト 0.5s より少し長く）
    public var keyTimeout: TimeInterval = 0.6
    /// コアの入力モード → OS の入力モード識別子（Info.plist の ComponentInputModeDict のキー）。
    /// 空なら OS 側のモード同期は行わない
    public var modeIdentifiers: [InputMode: String] = [:]

    private final class WeakController {
        weak var value: NagiInputController?
        init(_ v: NagiInputController) { value = v }
    }
    private var controllers: [String: WeakController] = [:]

    public init(supervisor: EngineSupervisor, store: SessionStore) {
        self.supervisor = supervisor
        self.store = store
    }

    func register(_ controller: NagiInputController) {
        controllers[controller.sessionId] = WeakController(controller)
    }

    func unregister(sessionId: String) {
        controllers[sessionId] = nil
        store.remove(sessionId: sessionId)
    }

    /// EngineSupervisor のイベント（任意スレッドから来るので呼び出し側でメインへ移すこと）
    func handle(_ event: EngineEvent) {
        switch event {
        case .updatePreedit(let p):
            controllers[p.session_id]?.value?.applyAsyncPreedit(p.preedit)
        case .updateCandidates(let p):
            controllers[p.session_id]?.value?.applyAsyncCandidates(p.candidates)
        case .restored(let sessionId, let result):
            controllers[sessionId]?.value?.apply(result)
        case .restoreFailed(let sessionId, _):
            // 復元に失敗した場合のみ、表示中の文字列をそのまま確定する（設計書 §3.3）
            controllers[sessionId]?.value?.commitDisplayedTextLocally()
        case .notify(let n):
            // メッセージはコアの通知（設定エラー等）であり入力内容は含まない
            NSLog("nagi notify [\(n.level.rawValue)]: \(n.message)")
        case .enteredMinimalMode(let restarts):
            NSLog("nagi: engine restarted \(restarts) times; running in minimal mode")
        case .launchFailed(let error):
            NSLog("nagi: engine launch failed: \(error)")
        case .started, .crashed:
            break
        }
    }
}
#endif
