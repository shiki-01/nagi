import SwiftUI

/// ホストアプリのテキスト入力欄への操作（`UIInputViewController` が実装する）
protocol KeyboardHost: AnyObject {
    func hostInsertText(_ text: String)
    func hostDeleteBackward()
    /// 変換中の文字列（下線付き）を表示する。`cursor` はその中のカーソル位置（UTF-16）
    func hostSetMarkedText(_ text: String, cursor: Int)
    /// 変換中の表示を消す（確定した文字列は別に `hostInsertText` で入れる）
    func hostClearMarkedText()
    /// 変換中でないときのカーソル移動（文字数単位）
    func hostAdjustCursor(by offset: Int)
    func hostAdvanceToNextInputMode()
}

/// 候補バーに出す候補 1 つ
struct CandidateItem: Identifiable {
    let id: Int
    let surface: String
    /// 入力中（変換前）の候補で、先頭の何文字分の読みを置き換えるか。nil は読み全体
    let covers: Int?
}

/// キーボードのレイアウト
enum KeyboardLayout {
    case kana
    case qwerty
}

/// キーボードの状態と操作。ロジック（文字の決定・変換・確定）はコアが持ち、ここはキー操作をコアへ送り、
/// 返ってきた描画差分（確定・プリエディット・候補）をホストと画面に反映するだけにする
final class KeyboardModel: ObservableObject {
    /// 変換中の文字列（候補バーの左に出す）
    @Published private(set) var preedit = ""
    /// 候補
    @Published private(set) var candidates: [CandidateItem] = []
    /// 候補の一覧を広げているか（候補が無くなれば自動で閉じる）
    @Published var isExpanded = false
    /// 選択中の候補（なければ -1）
    @Published private(set) var selectedCandidate = -1
    @Published private(set) var layout: KeyboardLayout = .kana
    /// QWERTY のシフト（1 文字ごとに解除）
    @Published private(set) var isShifted = false
    /// 画面に出すメッセージ（辞書が無いなど）
    @Published private(set) var message: String?
    /// 動作はするが画面に出しておきたい注意（設定の保存先を作れないなど）
    @Published private(set) var warning: String?

    weak var host: KeyboardHost?

    private let session: NagiSession?
    /// 同じキーの連続タップでトグル入力にするか（false ならフリック入力で、タップは常に先頭の文字）
    private let toggleInput: Bool
    private var lastRevision: UInt32 = 0
    /// ホストに表示している変換中の文字列とカーソル位置
    private var shownMarked = ""
    private var shownCursor = 0

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

    /// 入力中（変換前のかな、または変換中）か
    var hasPreedit: Bool { !preedit.isEmpty }
    /// 変換中（注目文節がある）か。←/→ は、変換前はカーソル、変換中は注目文節の移動になる
    @Published private(set) var isConverting = false

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

    /// ←/→: 変換中は注目文節、変換前の入力中はカーソル、未入力ならホストの入力欄のカーソルを動かす
    func arrow(_ direction: TouchDirection) {
        if isConverting {
            send { $0.perform(.moveFocus(direction)) }
        } else if hasPreedit {
            send { $0.perform(.moveCursor(direction)) }
        } else {
            host?.hostAdjustCursor(by: direction == .left ? -1 : 1)
        }
    }

    /// かな ⇄ 英字（QWERTY）。入力中の文字列は確定してから切り替える
    func switchLayout() {
        if hasPreedit, let session = session, let result = session.perform(.commit) {
            apply(result)
        }
        layout = (layout == .kana) ? .qwerty : .kana
        isShifted = false
    }

    func toggleShift() {
        isShifted.toggle()
    }

    /// 英字・記号をそのままホストに入れる（変換は通さない）
    func typeLetter(_ letter: String) {
        host?.hostInsertText(isShifted ? letter.uppercased() : letter)
        isShifted = false
    }

    func nextKeyboard() {
        host?.hostAdvanceToNextInputMode()
    }

    /// 候補を選ぶ。変換中は選んでから確定し、入力中（変換前）は選んだ候補が置き換える読みの部分を
    /// コアが確定する（部分変換。残りは入力中のまま続く）
    func selectCandidate(_ index: Int) {
        guard let session = session else { return }
        let converting = isConverting
        isExpanded = false
        if let result = session.selectCandidate(index) { apply(result) }
        if converting, let result = session.perform(.commit) { apply(result) }
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
            // 変換中の表示（下線）を消してから、確定した文字列を通常のテキストとして入れる
            host?.hostClearMarkedText()
            host?.hostInsertText(commit)
            shownMarked = ""
            shownCursor = 0
        }
        if let preedit = result.preedit { showPreedit(preedit) }
        if let candidates = result.candidates { showCandidates(candidates) }
    }

    private func applyPreeditUpdate(_ update: UpdatePreeditParams) {
        // 古い通知は捨てる
        guard update.revision >= lastRevision else { return }
        lastRevision = update.revision
        showPreedit(update.preedit)
    }

    private func applyCandidatesUpdate(_ update: UpdateCandidatesParams) {
        guard update.revision >= lastRevision else { return }
        lastRevision = update.revision
        showCandidates(update.candidates)
    }

    private func showPreedit(_ view: PreeditView) {
        preedit = view.text
        isConverting = view.focus != nil
        let cursor = Int(view.cursor)
        guard view.text != shownMarked || cursor != shownCursor else { return }
        if view.text.isEmpty {
            host?.hostClearMarkedText()
        } else {
            host?.hostSetMarkedText(view.text, cursor: cursor)
        }
        shownMarked = view.text
        shownCursor = cursor
    }

    private func showCandidates(_ view: CandidatesView) {
        candidates = view.items.enumerated().map { index, item in
            CandidateItem(id: index, surface: item.surface, covers: item.covers.map { Int($0) })
        }
        selectedCandidate = candidates.isEmpty ? -1 : Int(view.selected)
        if candidates.isEmpty { isExpanded = false }
    }
}
