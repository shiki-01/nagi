import SwiftUI

/// ホストアプリのテキスト入力欄への操作（`UIInputViewController` が実装する）
protocol KeyboardHost: AnyObject {
    func hostInsertText(_ text: String)
    func hostDeleteBackward()
    /// 変換中の文字列（下線付き）を表示する。`cursor` はその中のカーソル位置（UTF-16）
    func hostSetMarkedText(_ text: String, cursor: Int)
    /// 変換中の表示を消す（確定した文字列は別に `hostInsertText` で入れる）
    func hostClearMarkedText()
    func hostReplaceMarkedAndCommit(_ text: String)
    /// 変換中の範囲を確定扱いにする（文字は消さない）
    func hostUnmarkText()
    /// 変換中でないときのカーソル移動（文字数単位）
    func hostAdjustCursor(by offset: Int)
    func hostAdvanceToNextInputMode()
    /// キーボードを閉じる
    func hostDismissKeyboard()
    /// カーソル手前のテキスト（`documentContextBeforeInput`。変換中の文字列を含みうる）
    func hostContextBeforeInput() -> String?
    /// パスワード欄など、学習・文脈の送信・ログを止めるべき入力欄か
    func hostIsSecure() -> Bool
}

/// 候補バーに出す候補 1 つ
struct CandidateItem: Identifiable {
    let id: Int
    let surface: String
    /// 入力中（変換前）の候補で、先頭の何文字分の読みを置き換えるか。nil は読み全体
    let covers: Int?
}

/// 英字のシフトの状態
enum ShiftState {
    /// 小文字
    case off
    /// 次の 1 文字だけ大文字
    case once
    /// Caps Lock（解除するまで大文字）
    case caps
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
    /// 表示中のレイアウト（JSON から読んだもの。`Shared/KeyboardLayout.swift`）
    @Published private(set) var layoutDef: KeyboardLayoutDef
    /// 英字のシフト（1 回で次の 1 文字、2 回連続で Caps Lock）
    @Published private(set) var shiftState: ShiftState = .off
    /// 画面に出すメッセージ（辞書が無いなど）
    @Published private(set) var message: String?
    /// 動作はするが画面に出しておきたい注意（設定の保存先を作れないなど）
    @Published private(set) var warning: String?
    /// iOS 固有の設定（`ios-settings.json`。focusIn のたびに読み直す）。
    /// 描画が参照する寸法は `NagiKeyMetrics.current` にも同じ値を入れる
    @Published private(set) var ios = NagiIOSSettings()

    weak var host: KeyboardHost?

    private let session: NagiSession?
    /// 同じキーの連続タップでトグル入力にするか（false ならフリック入力で、タップは常に先頭の文字）。
    /// 設定（`input.toggle_input`）で決まる
    private var toggleInput: Bool
    /// コアの `config.toml`。focusIn で更新時刻が変わっていたら `reload_config` を送る
    private let configURL: URL?
    private var lastConfigStamp: Date?
    /// 学習データの全削除の合図（ホストアプリが更新する）を最後に処理した値
    private var lastLearningClearSignal: String?
    /// 直前に反映した、かな・英字モードのレイアウトの割り当て（変わったら表示中のレイアウトを切り替える）
    private var appliedAssignment: (kana: String, alpha: String)
    private var lastRevision: UInt32 = 0
    /// ホストに表示している変換中の文字列とカーソル位置
    private var shownMarked = ""
    private var shownCursor = 0
    /// 読み込み済みレイアウトの控え（id → 定義）。フォーカスが戻るたびに破棄して読み直す
    private var layoutCache: [String: KeyboardLayoutDef] = [:]
    /// シフトを直前に押した時刻（2 回連続の判定）
    private var lastShiftTap: Date?
    /// 2 回連続とみなす間隔（秒）
    private let doubleTapInterval: TimeInterval = 0.5

    /// 直前のトグルキー（`toggle`）の押下。同じキーを時間内に続けて押すと、直前の文字を次の候補に置き換える。
    /// 他のキー操作・レイアウト切替・フォーカス変化で破棄する（= 確定）
    private struct ToggleState {
        let keyId: String
        let layoutId: String
        var index: Int
        /// 直前に入れた文字列（置き換えで消す文字数の元）
        var last: String
        /// 英字で大文字にして入れたか（置き換えでも同じ大文字・小文字にする）
        var upper: Bool
        var at: Date
    }
    private var toggleState: ToggleState?

    // 文脈（set_context）
    private var contextScheduled = false
    /// 最後にコアへ送った文脈（同じ内容を送り直さない）。nil は未送信
    private var lastSentContext: String?
    /// コアに渡す文脈の最大文字数
    private let contextLength = 20

    /// - toggleInput: 設定ファイルが読めないときの既定（通常は設定 `input.toggle_input` で上書きされる）
    init(outcome: ProductSession.Outcome, toggleInput: Bool = false) {
        self.session = outcome.session
        self.message = outcome.message
        self.warning = outcome.warning
        self.toggleInput = toggleInput
        self.configURL = outcome.configURL
        self.lastConfigStamp = Self.modificationDate(outcome.configURL)
        self.lastLearningClearSignal = NagiLearningClearSignal.current()
        self.appliedAssignment = (kana: LayoutAssignment.id(for: .kana), alpha: LayoutAssignment.id(for: .alpha))
        self.layoutDef = LayoutLoader.loadAssigned(mode: .kana)
        session?.onPreeditUpdate = { [weak self] update in self?.applyPreeditUpdate(update) }
        session?.onCandidatesUpdate = { [weak self] update in self?.applyCandidatesUpdate(update) }
        session?.startNotifications()
        reloadIOSSettings()
    }

    private static func modificationDate(_ url: URL?) -> Date? {
        guard let url = url else { return nil }
        return (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
    }

    /// `ios-settings.json` を読み直して反映する（不正な値は既定に丸められている）
    func reloadIOSSettings() {
        let settings = NagiIOSSettingsStore.load()
        // 描画が参照する値を先に更新してから、@Published で再描画を起こす
        NagiKeyMetrics.current = settings
        toggleInput = settings.toggleInput
        if settings != ios { ios = settings }
    }

    var isAvailable: Bool { session != nil }

    /// 入力中（変換前のかな、または変換中）か
    var hasPreedit: Bool { !preedit.isEmpty }
    /// 変換中（注目文節がある）か。←/→ は、変換前はカーソル、変換中は注目文節の移動になる
    @Published private(set) var isConverting = false

    // MARK: - キー操作（レイアウトのキー）

    /// `text` キー: 文字を送る。かなモードはコアのローマ字変換に、それ以外はホストへそのまま入れる
    func inputText(_ text: String, shift: String?) {
        toggleState = nil
        if layoutDef.mode == .kana {
            insertReading(text)
            return
        }
        let output: String
        switch shiftState {
        case .off: output = text
        case .once, .caps: output = shift ?? text.uppercased()
        }
        hostInsert(output)
        if shiftState == .once { shiftState = .off }
    }

    /// かなモードの文字入力: かな直接 / ASCII（ローマ字）はコアが解決する。コアが無い・消費しないときはそのまま入れる
    private func insertReading(_ text: String) {
        if !send("text \(logText(text))", { $0.insert(.text(text)) }) {
            hostInsert(text)
        }
    }

    /// `toggle` キー（ガラケー式）: 押すたびに `cycle` の次の文字に切り替わる。
    /// 時間内（設定 `input.toggle_timeout_ms`、無ければ 1000ms）に同じキーをもう一度押すと、直前の文字を消して次を入れる。
    /// かなモードは、直前の文字をコアの backspace で消してから `insert_reading(text)` を送る
    /// （`ReadingInput.key12` のトグルはコアが 12 キーの行の表で解決するので、任意の cycle には使えない）。
    /// かな以外はホストの deleteBackward + insertText
    func toggle(_ key: LayoutKey) {
        let cycle = key.cycle ?? []
        guard !cycle.isEmpty else { return }
        let now = Date()
        var index = 0
        var replaceCount = 0
        var upper = false
        if let state = toggleState, state.keyId == key.id, state.layoutId == layoutDef.id, cycle.count > 1,
           now.timeIntervalSince(state.at) * 1000 < Double(ios.toggleTimeoutMs) {
            index = (state.index + 1) % cycle.count
            replaceCount = state.last.count
            upper = state.upper
        } else if layoutDef.mode != .kana {
            upper = shiftState != .off
            if shiftState == .once { shiftState = .off }
        }
        let base = cycle[index]
        let text = (layoutDef.mode != .kana && upper) ? base.uppercased() : base
        if layoutDef.mode == .kana {
            for _ in 0..<replaceCount {
                if !send("backspace(toggle)", { $0.perform(.backspace) }) { hostDeleteBackward(1) }
            }
            insertReading(text)
        } else {
            hostDeleteBackward(replaceCount)
            hostInsert(text)
        }
        toggleState = ToggleState(keyId: key.id, layoutId: layoutDef.id, index: index, last: text, upper: upper, at: now)
    }

    /// 入力中の文字列を確定する（なければ何もしない）
    private func commitPreedit() {
        if hasPreedit, let session = session, let result = session.perform(.commit) {
            apply(result, origin: "response(commit)")
        }
    }

    /// 片手モードの左右切替（キーボード上のボタン）。`side` は `left` / `right` / `off`。
    /// 書くのは本来ホストアプリだが、キーボード上のボタンなので拡張から App Group の設定ファイルへ書く
    func setOneHanded(_ side: String) {
        var latest = NagiIOSSettingsStore.load()
        latest.set(NagiIOSSettings.Key.keyboardOneHanded, .string(side))
        do {
            try NagiIOSSettingsStore.save(latest)
        } catch {
            DiagLog.log("setOneHanded save failed: \(error.localizedDescription)")
        }
        NagiKeyMetrics.current = latest
        ios = latest
    }

    /// `flick` キーの操作。`kana` 指定ならコアに任せ、文字を直接指定したキーはその文字を送る
    func flick(_ key: LayoutKey, _ gesture: KeyGesture) {
        toggleState = nil
        if let kana = key.kana {
            guard let entry = KanaKeyTable.entry(forName: kana) else { return }
            self.key(entry.key, gesture)
            return
        }
        let char: FlickChar?
        switch gesture {
        case .tap, .toggle: char = key.center
        case .flickLeft: char = key.left
        case .flickUp: char = key.up
        case .flickRight: char = key.right
        case .flickDown: char = key.down
        }
        guard let text = char?.text else { return }
        inputText(text, shift: nil)
    }

    /// `action` として実行できる名前（`perform(action:)` と同じ）
    private static let knownActions: Set<String> = [
        "space", "enter", "backspace", "shift", "capslock", "convert", "moveLeft", "moveRight",
        "dakuten", "smallKana", "modeSwitch", "globe", "dismiss",
    ]

    /// `shortcut` キー: 入力中の文字列を確定してから、手順を順に実行する。
    /// `text` はコアに通さずそのままホストへ入れる（定型文の挿入）。
    /// 未知の action や、action も text も無い手順があれば、そこで中断する（それまでの手順は実行済み）
    func shortcut(_ steps: [ShortcutStep]) {
        toggleState = nil
        commitPreedit()
        for (index, step) in steps.enumerated() {
            if let action = step.action {
                guard Self.knownActions.contains(action) else {
                    DiagLog.log("shortcut aborted at step \(index + 1): unknown action \(action)")
                    return
                }
                perform(action: action)
            } else if let text = step.text, !text.isEmpty {
                hostInsert(text)
            } else {
                DiagLog.log("shortcut aborted at step \(index + 1): empty step")
                return
            }
        }
    }

    /// `action` キー。未知の名前は無視する（将来の拡張で壊れないように）
    func perform(action: String) {
        toggleState = nil
        switch action {
        case "space": space()
        case "enter": enter()
        case "backspace": backspace()
        case "shift": tapShift()
        case "capslock": toggleCapsLock()
        case "convert": send("start_convert") { $0.perform(.startConvert) }
        case "moveLeft": arrow(.left)
        case "moveRight": arrow(.right)
        case "dakuten", "smallKana": variant()
        case "modeSwitch": switchMode()
        case "globe": nextKeyboard()
        case "dismiss": host?.hostDismissKeyboard()
        default:
            DiagLog.log("unknown action \(action)")
        }
    }

    // MARK: - キー操作（コアへ）

    func key(_ key: KanaKey, _ gesture: KeyGesture) {
        toggleState = nil
        // フリック入力では、タップは常に先頭の文字。トグル入力では連続タップを置き換えにする
        let effective: KeyGesture = (gesture == .tap && toggleInput) ? .toggle : gesture
        send("key12 \(key.rawValue) \(effective.rawValue)") {
            $0.insert(.key12(ReadingInputKey12Inner(key: key, gesture: effective)))
        }
    }

    func variant() {
        toggleState = nil
        send("variant") { $0.insert(.variant) }
    }

    func space() {
        toggleState = nil
        if !send("space", { $0.perform(.space) }) { hostInsert(" ") }
    }

    func enter() {
        toggleState = nil
        if !send("enter", { $0.perform(.enter) }) { hostInsert("\n") }
    }

    /// ⌫: 入力中ならコアの backspace、未入力ならホストの 1 文字削除（長押しの連打でも 1 回ごとにこれを呼ぶ）
    func backspace() {
        toggleState = nil
        if !send("backspace", { $0.perform(.backspace) }) { hostDeleteBackward(1) }
    }

    /// ←/→: 変換中は注目文節、変換前の入力中はカーソル、未入力ならホストの入力欄のカーソルを動かす
    func arrow(_ direction: TouchDirection) {
        toggleState = nil
        if isConverting {
            send("move_focus \(direction.rawValue)") { $0.perform(.moveFocus(direction)) }
        } else if hasPreedit {
            send("move_cursor \(direction.rawValue)") { $0.perform(.moveCursor(direction)) }
        } else {
            DiagLog.log("host adjustCursor \(direction == .left ? -1 : 1)")
            host?.hostAdjustCursor(by: direction == .left ? -1 : 1)
        }
    }

    // MARK: - レイアウトの切替

    /// かな ⇄ 英字。かな以外（英字・数字・記号・絵文字）からは、かなに戻る
    func switchMode() {
        switchLayout(target: layoutDef.mode == .kana ? "alpha" : "kana")
    }

    /// 別のレイアウトへ。`target` は `kana` / `alpha`（各モードに割り当てたレイアウト）またはレイアウトの id。
    /// 入力中の文字列は確定してから切り替える
    func switchLayout(target: String) {
        let next: KeyboardLayoutDef?
        switch target {
        case "kana": next = cachedLayout(assigned: .kana)
        case "alpha": next = cachedLayout(assigned: .alpha)
        default: next = cachedLayout(id: target)
        }
        guard let next = next else {
            DiagLog.log("layout target not found: \(target)")
            return
        }
        toggleState = nil
        if hasPreedit, let session = session, let result = session.perform(.commit) {
            apply(result, origin: "response(commit before layout switch)")
        }
        layoutDef = next
        shiftState = .off
        lastShiftTap = nil
        isExpanded = false
        DiagLog.log("layout -> \(next.id) mode=\(next.mode.rawValue)")
    }

    private func cachedLayout(id: String) -> KeyboardLayoutDef? {
        if let cached = layoutCache[id] { return cached }
        guard let def = LayoutLoader.load(id: id) else { return nil }
        layoutCache[id] = def
        return def
    }

    private func cachedLayout(assigned mode: LayoutMode) -> KeyboardLayoutDef {
        let id = LayoutAssignment.id(for: mode)
        if let cached = layoutCache[id] { return cached }
        let def = LayoutLoader.loadAssigned(mode: mode)
        layoutCache[id] = def
        return def
    }

    /// シフトキー: 1 回で次の 1 文字のみ大文字、0.5 秒以内の 2 回連続で Caps Lock、Caps Lock 中の 1 回で解除
    func tapShift() {
        let now = Date()
        switch shiftState {
        case .off:
            shiftState = .once
            lastShiftTap = now
        case .once:
            if let last = lastShiftTap, now.timeIntervalSince(last) < doubleTapInterval {
                shiftState = .caps
            } else {
                shiftState = .off
            }
            lastShiftTap = nil
        case .caps:
            shiftState = .off
            lastShiftTap = nil
        }
    }

    func toggleCapsLock() {
        shiftState = (shiftState == .caps) ? .off : .caps
        lastShiftTap = nil
    }

    func nextKeyboard() {
        host?.hostAdvanceToNextInputMode()
    }

    /// 候補を選ぶ。変換中は選んでから確定し、入力中（変換前）は選んだ候補が置き換える読みの部分を
    /// コアが確定する（部分変換。残りは入力中のまま続く）
    func selectCandidate(_ index: Int) {
        guard let session = session else { return }
        toggleState = nil
        let converting = isConverting
        isExpanded = false
        DiagLog.log("select_candidate index=\(index) converting=\(converting)")
        if let result = session.selectCandidate(index) { apply(result, origin: "response(select_candidate)") }
        if converting, let result = session.perform(.commit) { apply(result, origin: "response(commit)") }
    }

    func focusIn() {
        toggleState = nil
        reloadIOSSettings()
        applyCoreSettingsChanges()
        // レイアウトのファイルが変わっていても追随する（ホストアプリでの割り当て・取り込み）
        layoutCache.removeAll()
        if let fresh = cachedLayout(id: layoutDef.id) { layoutDef = fresh }
        applyLayoutAssignment()
        lastSentContext = nil
        DiagLog.log("focusIn layout=\(layoutDef.id)")
        if let result = session?.focusIn() { apply(result, origin: "response(focus_in)") }
        scheduleContextUpdate(delay: 0)
    }

    /// ホストアプリでの変更をコアに反映する。`config.toml` の更新時刻が変わっていれば `reload_config`
    /// （コアも要求のたびに更新を見て自動で読み直すが、focusIn の時点で確実に反映する）。
    /// 学習データの全削除の合図が変わっていれば `clear_learning`（拡張のメモリ上の学習データが、
    /// 削除済みのファイルへ書き戻されて復活しないようにする）
    private func applyCoreSettingsChanges() {
        guard let session = session else { return }
        if NagiLearningClearSignal.current() != lastLearningClearSignal {
            lastLearningClearSignal = NagiLearningClearSignal.current()
            _ = session.clearLearning()
            DiagLog.log("clear_learning (signal from host app)")
        }
        let stamp = Self.modificationDate(configURL)
        if stamp != lastConfigStamp {
            lastConfigStamp = stamp
            if let result = session.reloadConfig() {
                DiagLog.log("reload_config ok=\(result.ok) errors=\(result.errors.count)")
            }
        }
    }

    /// かな・英字の割り当てが変わっていたら、表示中のレイアウトをその新しい割り当てに切り替える
    private func applyLayoutAssignment() {
        let kana = LayoutAssignment.id(for: .kana)
        let alpha = LayoutAssignment.id(for: .alpha)
        let kanaChanged = kana != appliedAssignment.kana
        let alphaChanged = alpha != appliedAssignment.alpha
        guard kanaChanged || alphaChanged else { return }
        appliedAssignment = (kana: kana, alpha: alpha)
        if kanaChanged && layoutDef.mode == .kana {
            switchLayout(target: "kana")
        } else if alphaChanged && layoutDef.mode == .alpha {
            switchLayout(target: "alpha")
        }
    }

    /// フォーカスを失うとき: 変換中の文字列を確定する
    func focusOut() {
        toggleState = nil
        DiagLog.log("focusOut")
        if let result = session?.focusOut() { apply(result, origin: "response(focus_out)") }
    }

    // MARK: - 文脈（set_context）

    /// ホストの入力欄が変わった（外部からの変更・カーソル移動・自分の操作の結果）
    func hostTextChanged() {
        scheduleContextUpdate(delay: 0.05)
    }

    /// 文脈の送信を予約する。連続した変更は 1 回にまとめる
    private func scheduleContextUpdate(delay: TimeInterval) {
        guard session != nil, !contextScheduled else { return }
        contextScheduled = true
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self = self else { return }
            self.contextScheduled = false
            self.sendContext()
        }
    }

    /// カーソル手前の末尾（最大 `contextLength` 文字）をコアに渡す。変換中の入力は含めない
    /// （ホストの文脈には変換中の文字列も入るため、入力中は送らず、確定・消去のあとで送る）。
    /// パスワード欄などでは文脈を送らず、secure であることだけ伝える
    private func sendContext() {
        guard let session = session, let host = host else { return }
        guard shownMarked.isEmpty, !hasPreedit else { return }
        let secure = host.hostIsSecure()
        let tail: String? = secure ? nil : String((host.hostContextBeforeInput() ?? "").suffix(contextLength))
        let signature = secure ? "\u{0}secure" : (tail ?? "")
        guard signature != lastSentContext else { return }
        lastSentContext = signature
        session.setContext(before: tail, secure: secure)
        DiagLog.log("set_context secure=\(secure) tail=\(DiagLog.quote(tail ?? "", redact: secure))")
    }

    // MARK: - コアとのやり取り

    /// コアに要求を送り、結果を反映する。コアが操作を消費したら true（セッションが無ければ false）
    @discardableResult
    private func send(_ label: String, _ call: (NagiSession) -> ProcessKeyResult?) -> Bool {
        guard let session = session, let result = call(session) else {
            DiagLog.log("send \(label) -> no result")
            return false
        }
        DiagLog.log("send \(label)")
        apply(result, origin: "response")
        return result.consumed
    }

    private func apply(_ result: ProcessKeyResult, origin: String) {
        if DiagLog.isEnabled {
            var parts: [String] = ["\(origin) consumed=\(result.consumed) rev=\(result.revision)"]
            if let commit = result.commit { parts.append("commit=\(logText(commit))") }
            if let n = result.delete_before { parts.append("delete_before=\(n)") }
            if let preedit = result.preedit {
                parts.append("preedit=\(logText(preedit.text)) cursor=\(preedit.cursor) focus=\(preedit.focus != nil)")
            }
            if let candidates = result.candidates { parts.append("candidates=\(candidates.items.count)") }
            DiagLog.log(parts.joined(separator: " "))
        }
        lastRevision = max(lastRevision, result.revision)
        if let n = result.delete_before, n > 0 {
            hostDeleteBackward(Int(n))
        }
        if let commit = result.commit, !commit.isEmpty {
            // 変換中の表示（下線）を消してから、確定した文字列を通常のテキストとして入れる
            if shownMarked.isEmpty {
                hostInsert(commit)
            } else if let rest = result.preedit?.text, !rest.isEmpty {
                // 部分確定（確定と残りの入力中が同時に来る）。ホスト側の変換中の範囲の追跡に頼ると
                // 実機で範囲がずれて二重になるため、本文を確定的に直す:
                // 範囲を確定扱いにして、表示中の文字数だけ戻して消し、確定文字列を入れる
                // （残りは直後の showPreedit が新しい変換中として入れる）
                DiagLog.log("host partialCommit \(logText(commit)) delete=\(shownMarked.count) shownMarked=\(logText(shownMarked))")
                host?.hostUnmarkText()
                for _ in 0..<shownMarked.count { host?.hostDeleteBackward() }
                host?.hostInsertText(commit)
            } else {
                // 変換中の範囲を確定文字列で置き換えて確定する（残りの入力中の文字列が
                // 平文として残らないよう、挿入ではなく置換で扱う）
                DiagLog.log("host replaceMarkedAndCommit \(logText(commit)) shownMarked=\(logText(shownMarked))")
                host?.hostReplaceMarkedAndCommit(commit)
            }
            shownMarked = ""
            shownCursor = 0
            // 確定した文字列を文脈に含めるため、ホストが反映したあとに送る
            scheduleContextUpdate(delay: 0.15)
        }
        if let preedit = result.preedit { showPreedit(preedit) }
        if let candidates = result.candidates { showCandidates(candidates) }
        DiagLog.log("shownMarked=\(logText(shownMarked)) cursor=\(shownCursor)")
    }

    private func applyPreeditUpdate(_ update: UpdatePreeditParams) {
        // 古い通知は捨てる
        guard update.revision >= lastRevision else {
            DiagLog.log("notify update_preedit dropped rev=\(update.revision) < \(lastRevision)")
            return
        }
        DiagLog.log("notify update_preedit rev=\(update.revision) text=\(logText(update.preedit.text))")
        lastRevision = update.revision
        showPreedit(update.preedit)
        DiagLog.log("shownMarked=\(logText(shownMarked)) cursor=\(shownCursor)")
    }

    private func applyCandidatesUpdate(_ update: UpdateCandidatesParams) {
        guard update.revision >= lastRevision else {
            DiagLog.log("notify update_candidates dropped rev=\(update.revision) < \(lastRevision)")
            return
        }
        DiagLog.log("notify update_candidates rev=\(update.revision) candidates=\(update.candidates.items.count)")
        lastRevision = update.revision
        showCandidates(update.candidates)
    }

    private func showPreedit(_ view: PreeditView) {
        preedit = view.text
        isConverting = view.focus != nil
        let cursor = Int(view.cursor)
        guard view.text != shownMarked || cursor != shownCursor else { return }
        if view.text.isEmpty {
            DiagLog.log("host clearMarked")
            host?.hostClearMarkedText()
            scheduleContextUpdate(delay: 0.15)
        } else {
            DiagLog.log("host setMarked \(logText(view.text)) cursor=\(cursor)")
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

    // MARK: - ホストへの操作（ログ付き）

    private func hostInsert(_ text: String) {
        DiagLog.log("host insertText \(logText(text))")
        host?.hostInsertText(text)
    }

    private func hostDeleteBackward(_ count: Int) {
        DiagLog.log("host deleteBackward x\(count)")
        for _ in 0..<count { host?.hostDeleteBackward() }
    }

    /// ログ用の文字列。パスワード欄などでは内容を伏せる
    private func logText(_ text: String) -> String {
        DiagLog.quote(text, redact: host?.hostIsSecure() ?? false)
    }
}
