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
    private var isActive = false
    /// 画面へ反映済みの revision（確定のあとに、それより前の非同期更新が反映されるのを防ぐ）
    private var applied = AppliedRevision()
    /// コアのこのセッションの入力モード（応答の `mode` で更新する。nil = 未受信で初期モードのまま）
    private var coreMode: InputMode?
    /// 次にマークテキストを置くときに置き換える範囲（再変換の対象として選ばれていたテキスト）
    private var pendingReplacement: NSRange?

    private static let notFound = NSRange(location: NSNotFound, length: NSNotFound)
    /// コアへ送る直前テキストの長さ（UTF-16。コアは末尾 40 文字だけを文脈に使う）
    private static let contextLength = 64
    /// 再変換できる選択の長さの上限（UTF-16）
    private static let reconvertMaxLength = 200

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
        // 入力を始める前に、直前のテキスト（変換の文脈）と secure input の状態を伝える
        if !isFlags, currentMode == .hiragana, context.store.preedit(for: sessionId)?.text.isEmpty ?? true {
            reportContext()
        }

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
        // secure input / アプリ識別子 / 直前のテキストを先に伝える（学習停止の判断はコア）
        reportContext()
        if case .success(let result) = SyncBridge.wait(
            timeout: context.keyTimeout, { try await supervisor.focusIn(sessionId: id) })
        {
            apply(result)
        }
        isActive = true
        adoptSelectedMode()
        // 辞書が無ければ取得を勧める（activate の処理を終えてから出す）
        Task { @MainActor [weak context] in context?.offerDataInstallIfNeeded() }
    }

    override func deactivateServer(_ sender: Any!) {
        isActive = false
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

    /// OS 側で入力モードが選ばれた（入力メニューの「あ / A」、英数/かなキー等）。コアのモードを合わせる
    override func setValue(_ value: Any!, forTag tag: Int, client sender: Any!) {
        guard tag == Int(kTextServiceInputModePropertyTag) else {
            super.setValue(value, forTag: tag, client: sender)
            return
        }
        if let c = sender as? IMKTextInput { textClient = c }
        guard let identifier = value as? String, let mode = context?.mode(forIdentifier: identifier)
        else { return }
        requestMode(mode)
    }

    /// エンジンの起動・再起動後: コアのセッションは初期モードに戻るため、使用中なら OS のモードに合わせ直す
    func engineStarted() {
        applied.reset()
        coreMode = nil
        if isActive { adoptSelectedMode() }
    }

    // MARK: 文脈

    /// アプリ識別子・secure input・直前のテキストをコアへ伝える。secure input 中は直前のテキストを読まない
    private func reportContext() {
        guard let context else { return }
        let secure = IsSecureEventInputEnabled()
        let ctx = SetContextParams(
            session_id: sessionId, surrounding_text: secure ? nil : textBeforeCaret(),
            app_id: textClient?.bundleIdentifier(), secure_input: secure)
        try? context.supervisor.setContext(ctx)
    }

    /// キャレット直前のテキスト。アプリが対応していなければ nil（コアは前回の文脈を維持する）
    private func textBeforeCaret() -> SurroundingText? {
        guard let client = textClient else { return nil }
        let selection = client.selectedRange()
        guard selection.location != NSNotFound else { return nil }
        let length = min(selection.location, Self.contextLength)
        if length == 0 { return SurroundingText(text: "", cursor: 0) }
        let range = NSRange(location: selection.location - length, length: length)
        guard let text = client.attributedSubstring(from: range)?.string else { return nil }
        return SurroundingText(text: text, cursor: UInt32(text.utf16.count))
    }

    // MARK: 入力メニュー

    /// メニューバーの入力メニューに出る項目（状態の表示・設定・再読み込み・学習データの削除）
    override func menu() -> NSMenu! {
        let menu = NSMenu(title: "Nagi")
        for line in statusLines() {
            let item = NSMenuItem(title: line, action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let entries: [(String, Selector)] = [
            ("設定…", #selector(openSettings(_:))),
            ("設定ファイルを再読み込み", #selector(reloadConfig(_:))),
            ("学習データを削除…", #selector(clearLearning(_:))),
            ("辞書とモデルをダウンロード…", #selector(installData(_:))),
            ("アップデートを確認…", #selector(checkForUpdates(_:))),
        ]
        for (title, action) in entries {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = self
            menu.addItem(item)
        }
        return menu
    }

    /// メニューを開いた時点の状態（取得できなければ未接続の 1 行）
    private func statusLines() -> [String] {
        guard let context else { return [StatusMessages.unavailable] }
        let supervisor = context.supervisor
        switch SyncBridge.wait(timeout: context.keyTimeout, { try await supervisor.getStatus() }) {
        case .success(let status): return StatusMessages.menuLines(for: status)
        case .failure: return [StatusMessages.unavailable]
        }
    }

    @objc func openSettings(_ sender: Any?) {
        context?.showSettings()
    }

    @objc func installData(_ sender: Any?) {
        guard let context else { return }
        Task { @MainActor in await context.installData() }
    }

    @objc func checkForUpdates(_ sender: Any?) {
        guard let context else { return }
        Task { @MainActor in await context.checkForUpdates() }
    }

    @objc func reloadConfig(_ sender: Any?) {
        guard let context else { return }
        let supervisor = context.supervisor
        Task { @MainActor in
            let message: String
            do {
                let result = try await supervisor.reloadConfig()
                message =
                    result.ok
                    ? "設定ファイルを再読み込みしました"
                    : "設定ファイルの読み込みに失敗しました（以前の設定で動作中）"
            } catch {
                message = StatusMessages.unavailable
            }
            context.toast.show(message)
        }
    }

    /// 学習データの全削除（確認ダイアログを出す）
    @objc func clearLearning(_ sender: Any?) {
        guard let context else { return }
        let alert = NSAlert()
        alert.messageText = "学習データを削除しますか？"
        alert.informativeText = "変換の学習結果をすべて削除します。ユーザー辞書と設定は残ります。この操作は取り消せません。"
        alert.alertStyle = .warning
        alert.addButton(withTitle: "削除")
        alert.addButton(withTitle: "キャンセル")
        // LSUIElement アプリなので明示的にアクティブ化しないと背面に出る
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let supervisor = context.supervisor
        Task { @MainActor in
            let message: String
            do {
                try await supervisor.clearLearning()
                message = "学習データを削除しました"
            } catch {
                message = "学習データを削除できませんでした"
            }
            context.toast.show(message)
        }
    }

    // MARK: 描画差分の適用

    /// 応答（描画差分）を反映する。順序: 削除 → 確定 → preedit → 候補 → モード
    func apply(_ result: ProcessKeyResult) {
        applied.applied(sync: result.revision)
        if let n = result.delete_before, n > 0 { deleteBeforeCaret(Int(n)) }
        let committed = result.commit.map { !$0.isEmpty } ?? false
        if let commit = result.commit, committed {
            textClient?.insertText(commit, replacementRange: Self.notFound)
            hasMarked = false  // 確定でマークテキストは置き換わる
        }
        if let preedit = result.preedit { renderPreedit(preedit) }
        if let candidates = result.candidates { showCandidates(CandidatePaging.page(for: candidates)) }
        if let mode = result.mode {
            coreMode = mode
            syncMode(mode)
        }
        if result.reconvert_selection == true { reconvertSelection() }
    }

    /// コアの要求（かなキー・変換キー）で、選択中の確定済みテキストを再変換する。選択が無ければ何もしない
    private func reconvertSelection() {
        guard let context, let client = textClient else { return }
        let selection = client.selectedRange()
        guard selection.location != NSNotFound, selection.length > 0,
            selection.length <= Self.reconvertMaxLength,
            let text = client.attributedSubstring(from: selection)?.string, !text.isEmpty
        else { return }
        let params = ReconvertParams(session_id: sessionId, text: text)
        let supervisor = context.supervisor
        let id = sessionId
        // コア側の要求タイムアウトは長めにし、遅れて届いた応答で接続を壊さない
        let requestTimeout = context.reconvertTimeout + 10
        switch SyncBridge.wait(
            timeout: context.reconvertTimeout,
            { try await supervisor.reconvert(params, timeout: requestTimeout) })
        {
        case .success(let result):
            // 再変換の文字列はマークテキストとして選択範囲を置き換える
            pendingReplacement = selection
            apply(result)
            pendingReplacement = nil
        case .failure:
            // 待ちきれなかった場合、コアは遅れて変換中になりうるので取り消す（要求は順に処理される）
            Task { @MainActor [weak self] in
                if let result = try? await supervisor.reset(sessionId: id) { self?.apply(result) }
            }
            context.toast.show("再変換の準備に時間がかかっています。少し待ってからもう一度お試しください")
        }
    }

    /// `update_preedit` 通知（ライブ変換等）。確定・取消のあとに遅れて届いた古いものは反映しない
    func applyAsyncPreedit(_ preedit: PreeditView, revision: UInt32) {
        guard applied.accept(async: revision) else { return }
        renderPreedit(preedit)
        if lastCandidates != nil { showCandidates(lastCandidates) }
    }

    /// `update_candidates` 通知。入力中の文字列が無いのに届いた候補（確定・取消のあとに遅れて届いたもの）は表示しない
    func applyAsyncCandidates(_ candidates: CandidatesView, revision: UInt32) {
        guard applied.accept(async: revision) else { return }
        guard let preedit = context?.store.preedit(for: sessionId), !preedit.text.isEmpty else {
            hideCandidates()
            return
        }
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
        // 入力中の文字列が空になったら、候補は残さない（取り残し防止。コアの応答に依らず閉じる）
        if preedit.text.isEmpty { hideCandidates() }
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
            replacementRange: pendingReplacement ?? Self.notFound)
        pendingReplacement = nil
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

    /// コアの現在のモード（未受信なら新規セッションの初期モード）
    private var currentMode: InputMode { coreMode ?? context?.initialMode ?? .hiragana }

    /// OS で選ばれている入力モードをコアに反映する（新規セッションは初期モードから始まるため）
    private func adoptSelectedMode() {
        guard let identifier = InputSourceRegistrar.currentInputSourceID(),
            let mode = context?.mode(forIdentifier: identifier)
        else { return }
        requestMode(mode)
    }

    /// コアのモードを切り替える。同じモードなら送らない（SetMode は入力中の文字列を確定するため）
    private func requestMode(_ mode: InputMode) {
        guard let context, mode != currentMode else { return }
        let params = PerformActionParams(session_id: sessionId, action: .setMode(mode))
        let supervisor = context.supervisor
        if case .success(let result) = SyncBridge.wait(
            timeout: context.keyTimeout, { try await supervisor.performAction(params) })
        {
            apply(result)
        }
    }

    /// コアのモードを OS の入力モード（メニューバーの表示）へ反映する
    private func syncMode(_ mode: InputMode) {
        guard let identifier = context?.modeIdentifiers[mode] else { return }
        textClient?.selectMode(identifier)
    }

    // MARK: 候補ウィンドウ

    private func showCandidates(_ page: CandidatePage?) {
        lastCandidates = page
        guard let page, let context else {
            hidePanelIfOwner()
            return
        }
        var rect = NSRect.zero
        _ = textClient?.attributes(forCharacterIndex: 0, lineHeightRectangle: &rect)
        context.panelOwner = sessionId
        context.panel.show(page: page, near: rect)
    }

    private func hideCandidates() {
        lastCandidates = nil
        hidePanelIfOwner()
    }

    /// 候補パネルは全入力欄で共有しているので、自分が出したものだけを閉じる
    /// （別の入力欄が出した候補を、遅れて来た deactivate で消してしまわないように）
    private func hidePanelIfOwner() {
        guard let context else { return }
        if context.panelOwner == nil || context.panelOwner == sessionId {
            context.panelOwner = nil
            context.panel.hide()
        }
    }
}
#endif
