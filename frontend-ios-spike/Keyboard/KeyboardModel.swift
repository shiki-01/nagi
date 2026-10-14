import SwiftUI

/// ホストアプリのテキスト入力欄への操作（`UIInputViewController` が実装する）
protocol KeyboardHost: AnyObject {
    func hostInsertText(_ text: String)
    func hostDeleteBackward()
    /// 変換中の文字列（下線付き）を表示する。空文字列なら表示を消す
    func hostSetMarkedText(_ text: String)
    func hostUnmarkText()
    func hostAdvanceToNextInputMode()
}

/// キーボードの状態と操作。ロジック（文字の決定・変換・確定）はコアが持ち、ここはキー操作をコアへ送り、
/// 返ってきた描画差分（確定・プリエディット・候補）をホストと画面に反映するだけにする
final class KeyboardModel: ObservableObject {
    /// 変換中の文字列（候補バーの左に出す）
    @Published private(set) var preedit = ""
    /// 候補
    @Published private(set) var candidates: [String] = []
    /// 選択中の候補（なければ -1）
    @Published private(set) var selectedCandidate = -1
    @Published private(set) var mode: InputMode = .hiragana
    /// 画面に出すメッセージ（辞書が無いなど）
    @Published private(set) var message: String?
    /// 動作はするが画面に出しておきたい注意（設定の保存先を作れないなど）
    @Published private(set) var warning: String?

    weak var host: KeyboardHost?

    private let session: NagiSession?
    /// 同じキーの連続タップでトグル入力にするか（false ならフリック入力で、タップは常に先頭の文字）
    private let toggleInput: Bool
    private var lastRevision: UInt32 = 0
    /// ホストに表示している変換中の文字列
    private var shownMarked = ""

    init(outcome: ProductSession.Outcome, toggleInput: Bool = false) {
        self.session = outcome.session
        self.message = outcome.message
        self.warning = outcome.warning
        self.toggleInput = toggleInput
        session?.onPreeditUpdate = { [weak self] update in self?.applyPreeditUpdate(update) }
        session?.onCandidatesUpdate = { [weak self] update in self?.applyCandidatesUpdate(update) }
        session?.startNotifications()
    }

    var isAvailable: Bool { session != nil }

    /// 変換中か（候補が出ている間は、空白で次の候補・確定ボタンで確定）
    var hasPreedit: Bool { !preedit.isEmpty }

    // MARK: - キー操作

    func key(_ key: KanaKey, _ gesture: KeyGesture) {
        // フリック入力では、タップは常に先頭の文字。トグル入力では連続タップを置き換えにする
        let effective: KeyGesture = (gesture == .tap && toggleInput) ? .toggle : gesture
        send { $0.insert(.key12(ReadingInputKey12Inner(key: key, gesture: effective))) }
    }

    func variant() {
        send { $0.insert(.variant) }
    }

    func space() {
        if !send({ $0.perform(.space) }) { host?.hostInsertText(" ") }
    }

    func enter() {
        if !send({ $0.perform(.enter) }) { host?.hostInsertText("\n") }
    }

    func backspace() {
        if !send({ $0.perform(.backspace) }) { host?.hostDeleteBackward() }
    }

    func moveCursor(_ direction: TouchDirection) {
        send { $0.perform(.moveCursor(direction)) }
    }

    func toggleMode() {
        send { $0.perform(.toggleMode) }
    }

    func nextKeyboard() {
        host?.hostAdvanceToNextInputMode()
    }

    /// 候補を選んで確定する
    func selectCandidate(_ index: Int) {
        guard let session = session else { return }
        if let result = session.selectCandidate(index) { apply(result) }
        if let result = session.perform(.commit) { apply(result) }
    }

    func focusIn() {
        if let result = session?.focusIn() { apply(result) }
    }

    /// フォーカスを失うとき: 変換中の文字列を確定する
    func focusOut() {
        if let result = session?.focusOut() { apply(result) }
    }

    // MARK: - コアとのやり取り

    /// コアに要求を送り、結果を反映する。コアが操作を消費したら true（セッションが無ければ false）
    @discardableResult
    private func send(_ call: (NagiSession) -> ProcessKeyResult?) -> Bool {
        guard let session = session, let result = call(session) else { return false }
        apply(result)
        return result.consumed
    }

    private func apply(_ result: ProcessKeyResult) {
        lastRevision = max(lastRevision, result.revision)
        if let n = result.delete_before, n > 0 {
            for _ in 0..<n { host?.hostDeleteBackward() }
        }
        if let commit = result.commit, !commit.isEmpty {
            // 変換中の表示を確定した文字列に置き換えてから、確定する
            host?.hostSetMarkedText(commit)
            host?.hostUnmarkText()
            shownMarked = ""
        }
        if let preedit = result.preedit { showPreedit(preedit.text) }
        if let candidates = result.candidates { showCandidates(candidates) }
        if let mode = result.mode { self.mode = mode }
    }

    private func applyPreeditUpdate(_ update: UpdatePreeditParams) {
        // 古い通知は捨てる
        guard update.revision >= lastRevision else { return }
        lastRevision = update.revision
        showPreedit(update.preedit.text)
    }

    private func applyCandidatesUpdate(_ update: UpdateCandidatesParams) {
        guard update.revision >= lastRevision else { return }
        lastRevision = update.revision
        showCandidates(update.candidates)
    }

    private func showPreedit(_ text: String) {
        preedit = text
        guard text != shownMarked else { return }
        if text.isEmpty {
            host?.hostSetMarkedText("")
            host?.hostUnmarkText()
        } else {
            host?.hostSetMarkedText(text)
        }
        shownMarked = text
    }

    private func showCandidates(_ view: CandidatesView) {
        candidates = view.items.map { $0.surface }
        selectedCandidate = candidates.isEmpty ? -1 : Int(view.selected)
    }
}
