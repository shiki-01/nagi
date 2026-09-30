#if os(macOS)
import AppKit
import Carbon
import InputMethodKit
import NagiClient

/// IMKit のコントローラ。OS イベントの正規化・描画・セッション境界のみを担い、ロジックはコアに委ねる。
/// クライアント（入力先アプリ）1 接続につき 1 インスタンス = 1 セッション。
@objc(NagiInputController)
final class NagiInputController: IMKInputController {
    /// コアとのセッション ID
    let sessionId: String
    private var textClient: (any IMKTextInput)?
    private var hasMarked = false
    private var lastCandidates: CandidatePage?

    private static let notFound = NSRange(location: NSNotFound, length: NSNotFound)

    private var context: NagiIMEContext? { NagiIMEContext.shared }

    override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        sessionId = UUID().uuidString
        super.init(server: server, delegate: delegate, client: inputClient)
        textClient = inputClient as? IMKTextInput
        context?.register(self)
    }

    deinit {
        // IMKit はメインスレッドで破棄する
        context?.unregister(sessionId: sessionId)
    }

    // MARK: IMKit エントリポイント

    /// flagsChanged も受け取る（右⌘単独タップ等をコアが判定するため）
    override func recognizedEvents(_ sender: Any!) -> Int {
        Int(NSEvent.EventTypeMask([.keyDown, .flagsChanged]).rawValue)
    }

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event, event.type == .keyDown || event.type == .flagsChanged,
            let context
        else { return false }
        if let c = sender as? IMKTextInput { textClient = c }

        let isFlags = event.type == .flagsChanged
        // flagsChanged の NSEvent から characters を読むと例外になる
        let raw = RawKeyInput(
            keyCode: event.keyCode,
            characters: isFlags ? "" : (event.characters ?? ""),
            charactersIgnoringModifiers: isFlags ? "" : (event.charactersIgnoringModifiers ?? ""),
            modifierFlags: event.modifierFlags.rawValue, isFlagsChanged: isFlags)
        guard let key = KeyNormalizer.normalize(raw) else { return false }

        let params = ProcessKeyParams(
            session_id: sessionId, keysym: key.keysym, keycode: key.keycode, text: key.text,
            modifiers: key.modifiers, kind: key.kind)
        let supervisor = context.supervisor
        switch SyncBridge.wait(timeout: context.keyTimeout, { try await supervisor.processKey(params) }) {
        case .success(let result):
            apply(result)
            return result.consumed
        case .failure:
            // エンジン異常（設計書 §3.3）: Direct 状態ならキーは素通し、それ以外は消費する
            return context.store.preedit(for: sessionId) != nil
        }
    }

    override func activateServer(_ sender: Any!) {
        super.activateServer(sender)
        if let c = sender as? IMKTextInput { textClient = c }
        guard let context else { return }
        let supervisor = context.supervisor
        let id = sessionId
        // secure input / アプリ識別子を先に伝える（学習停止の判断はコア）。Surrounding Text は送らない
        let ctx = SetContextParams(
            session_id: id, surrounding_text: nil, app_id: textClient?.bundleIdentifier(),
            secure_input: IsSecureEventInputEnabled())
        try? supervisor.setContext(ctx)
        if case .success(let result) = SyncBridge.wait(
            timeout: context.keyTimeout, { try await supervisor.focusIn(sessionId: id) })
        {
            apply(result)
        }
    }

    override func deactivateServer(_ sender: Any!) {
        if let c = sender as? IMKTextInput { textClient = c }
        if let context {
            let supervisor = context.supervisor
            let id = sessionId
            switch SyncBridge.wait(timeout: context.keyTimeout, { try await supervisor.focusOut(sessionId: id) }) {
            case .success(let result):
                apply(result)
            case .failure:
                commitDisplayedTextLocally()
            }
        }
        hideCandidates()
        super.deactivateServer(sender)
    }

    /// アプリ側からの確定要求（フォーカス移動・クリック等）。表示中の文字列をそのまま確定する
    override func commitComposition(_ sender: Any!) {
        if let c = sender as? IMKTextInput { textClient = c }
        commitDisplayedTextLocally()
        guard let context else { return }
        let supervisor = context.supervisor
        let id = sessionId
        if case .success(let result) = SyncBridge.wait(
            timeout: context.keyTimeout, { try await supervisor.reset(sessionId: id) })
        {
            apply(result)
        }
    }

    // MARK: 入力メニュー

    /// メニューバーの入力メニューに出る項目（「設定…」）
    override func menu() -> NSMenu! {
        let menu = NSMenu(title: "Nagi")
        let item = NSMenuItem(
            title: "設定…", action: #selector(openSettings(_:)), keyEquivalent: "")
        item.target = self
        menu.addItem(item)
        return menu
    }

    @objc func openSettings(_ sender: Any?) {
        context?.showSettings()
    }

    // MARK: 描画差分の適用

    /// 応答（描画差分）を反映する。順序: 削除 → 確定 → preedit → 候補 → モード
    func apply(_ result: ProcessKeyResult) {
        if let n = result.delete_before, n > 0 { deleteBeforeCaret(Int(n)) }
        let committed = result.commit.map { !$0.isEmpty } ?? false
        if let commit = result.commit, committed {
            textClient?.insertText(commit, replacementRange: Self.notFound)
            hasMarked = false  // 確定でマークテキストは置き換わる
        }
        if let preedit = result.preedit { renderPreedit(preedit) }
        if let candidates = result.candidates { showCandidates(CandidatePaging.page(for: candidates)) }
        if let mode = result.mode { syncMode(mode) }
    }

    /// `update_preedit` 通知（ライブ変換等）
    func applyAsyncPreedit(_ preedit: PreeditView) {
        renderPreedit(preedit)
        if lastCandidates != nil { showCandidates(lastCandidates) }
    }

    /// `update_candidates` 通知
    func applyAsyncCandidates(_ candidates: CandidatesView) {
        showCandidates(CandidatePaging.page(for: candidates))
    }

    /// 復元失敗・エンジン異常時: 保持している preedit の表示文字列をそのまま確定して状態を捨てる
    func commitDisplayedTextLocally() {
        if let preedit = context?.store.preedit(for: sessionId), !preedit.text.isEmpty {
            textClient?.insertText(preedit.text, replacementRange: Self.notFound)
            hasMarked = false
        } else if hasMarked {
            textClient?.setMarkedText(
                "", selectionRange: NSRange(location: 0, length: 0), replacementRange: Self.notFound)
            hasMarked = false
        }
        context?.store.update(sessionId: sessionId, preedit: nil)
        hideCandidates()
    }

    private func renderPreedit(_ preedit: PreeditView) {
        context?.store.update(sessionId: sessionId, preedit: preedit)
        guard let client = textClient else { return }
        if preedit.text.isEmpty {
            if hasMarked {
                client.setMarkedText(
                    "", selectionRange: NSRange(location: 0, length: 0),
                    replacementRange: Self.notFound)
                hasMarked = false
            }
            return
        }
        let attributed = NSMutableAttributedString(string: preedit.text)
        let full = NSRange(location: 0, length: attributed.length)
        attributed.addAttributes(Self.attributes(for: .normal), range: full)
        for run in PreeditStyler.runs(for: preedit) {
            attributed.addAttributes(Self.attributes(for: run.style), range: run.range)
        }
        let cursor = min(Int(preedit.cursor), attributed.length)
        client.setMarkedText(
            attributed, selectionRange: NSRange(location: cursor, length: 0),
            replacementRange: Self.notFound)
        hasMarked = true
    }

    /// 下線スタイルで区別する（アプリによっては属性が無視されるため色・背景には頼らない）
    private static func attributes(for style: MarkStyle) -> [NSAttributedString.Key: Any] {
        let underline: NSUnderlineStyle
        switch style {
        case .normal: underline = .single
        case .focused, .selected: underline = .thick
        case .fixed: underline = [.single, .patternDot]
        case .styled: underline = [.single, .patternDash]
        case .pending: underline = [.single, .patternDot]
        }
        return [.underlineStyle: underline.rawValue]
    }

    /// 確定取消: 直前に挿入した確定文字列を UTF-16 単位で削除する
    private func deleteBeforeCaret(_ length: Int) {
        guard let client = textClient else { return }
        let selection = client.selectedRange()
        guard selection.location != NSNotFound, selection.location >= length else { return }
        client.insertText(
            "", replacementRange: NSRange(location: selection.location - length, length: length))
    }

    private func syncMode(_ mode: InputMode) {
        guard let identifier = context?.modeIdentifiers[mode] else { return }
        textClient?.selectMode(identifier)
    }

    // MARK: 候補ウィンドウ

    private func showCandidates(_ page: CandidatePage?) {
        lastCandidates = page
        guard let page, let context else {
            context?.panel.hide()
            return
        }
        var rect = NSRect.zero
        _ = textClient?.attributes(forCharacterIndex: 0, lineHeightRectangle: &rect)
        context.panel.show(page: page, near: rect)
    }

    private func hideCandidates() {
        lastCandidates = nil
        context?.panel.hide()
    }
}
#endif
