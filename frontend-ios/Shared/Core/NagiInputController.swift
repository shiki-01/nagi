import Foundation

/// ホストアプリのテキスト入力欄への出力（実体は `UITextDocumentProxy`。テストでは差し替える）
protocol HostTextOutput: AnyObject {
    func insertText(_ text: String)
    func deleteBackward()
    /// 未確定文字列（marked text）を設定する。`cursorUTF16` は未確定文字列内の UTF-16 位置
    func setMarkedText(_ text: String, cursorUTF16: Int)
    /// 未確定文字列を、その位置に確定した状態にする
    func unmarkText()
    func moveCursor(by offset: Int)
}

/// 候補バーに表示するセグメント（文節）
struct BarSegment: Equatable {
    let surface: String
    let focused: Bool
}

/// 候補バーに表示する候補
struct BarCandidate: Equatable {
    let surface: String
    /// 曖昧入力の補正候補（`CandidateSource.corrected`）
    let corrected: Bool
}

/// 候補バーの表示状態
struct BarState: Equatable {
    var preedit = ""
    var segments: [BarSegment] = []
    var candidates: [BarCandidate] = []
    var selectedCandidate = 0
    var isConverting = false
    var message: String?
}

enum EngineState: String {
    /// 起動中（入力は溜めておき、準備ができたら順に処理する）
    case starting
    case ready
    /// 失敗からの再初期化中（入力は溜める）
    case recovering
    /// 使えない（入力はそのまま挿入する）
    case unavailable
}

/// タッチ入力とコア（JSON-RPC）の間の制御。UIKit に依存しない。
///
/// - コアの応答（`commit` / `preedit` / `candidates` / `delete_before`）をホストへ反映する
/// - コアが `consumed = false` を返した入力（Direct 状態の Space・Enter・Backspace 等）は OS 標準の動作に回す
/// - コアの呼び出しに失敗したら、入力を溜めてエンジンを再初期化し、`restore_session` で preedit を復元する。
///   再初期化できなければ入力をそのまま挿入する（入力を失わない）
/// - 呼び出しはすべてメインスレッドで行う（エンジンの生成だけ別スレッド）
final class NagiInputController {
    private let output: HostTextOutput
    private var rpc: NagiRpcClient?
    private var factory: (() -> LineTransport?)?
    private let sessionId = "ios-keyboard"

    private(set) var state: EngineState = .starting
    private(set) var profile = ""
    private(set) var serverVersion = ""

    /// 候補バーの表示が変わったとき
    var onBarChange: ((BarState) -> Void)?
    /// true ならコアを通さず、押したキーをそのまま挿入する（英数レイアウト・数字欄・secure 入力欄）
    var directInput = false
    /// true なら preedit を `setMarkedText` でホストに出す。false ならバーのみに表示し、確定時だけ挿入する
    private(set) var useMarkedText = true
    /// 診断テキストの先頭に付ける情報（パス・メモリ等。呼び出し側が組み立てる）
    var headerProvider: (() -> String)?

    private var lastPreedit: PreeditView?
    private var lastCandidates: CandidatesView?
    private var lastCommit = ""
    private var markedActive = false
    private var queue: [KeyboardInput] = []
    private let maxQueue = 64
    private var recoveryCount = 0
    private var successesSinceRecovery = 0
    private var secureInput = false
    private var message: String?

    init(output: HostTextOutput) {
        self.output = output
    }

    // MARK: - エンジンの起動

    /// エンジンを別スレッドで生成して初期化する。完了までの入力は溜めておく
    func startEngine(factory: @escaping () -> LineTransport?) {
        self.factory = factory
        state = .starting
        publishBar(message: "辞書を読み込み中…")
        spawnEngine(isRecovery: false)
    }

    private func spawnEngine(isRecovery: Bool) {
        guard let factory = factory else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let transport = factory()
            DispatchQueue.main.async {
                self?.engineCreated(transport, isRecovery: isRecovery)
            }
        }
    }

    /// 生成したエンジンを初期化して使い始める（テストからも呼ぶ）
    func engineCreated(_ transport: LineTransport?, isRecovery: Bool = false) {
        guard let transport = transport else {
            becomeUnavailable("エンジンを生成できませんでした")
            return
        }
        let client = NagiRpcClient(transport: transport)
        do {
            let params = InitializeParams(
                protocol_version: 1,
                client_name: "nagi-ios",
                client_version: "0.1",
                os: "ios"
            )
            let result: InitializeResult = try client.call("initialize", params: params)
            profile = result.profile
            serverVersion = result.server_version
            rpc = client
            state = .ready
            DiagnosticsLog.shared.add(
                "engine ready profile=\(result.profile) version=\(result.server_version) recovery=\(isRecovery)"
            )
            sendContext()
            if isRecovery, let preedit = lastPreedit, !preedit.segments.isEmpty {
                restoreSession(preedit)
            }
            let focus: ProcessKeyResult = try client.call("focus_in", params: SessionParams(session_id: sessionId))
            apply(focus)
            publishBar(message: nil)
            replayQueue()
        } catch {
            DiagnosticsLog.shared.add("engine init failed: \(describe(error))")
            becomeUnavailable("エンジンの初期化に失敗しました")
        }
    }

    private func sendContext() {
        let params = SetContextParams(
            session_id: sessionId,
            surrounding_text: nil,
            app_id: nil,
            secure_input: secureInput
        )
        rpc?.notify("set_context", params: params)
    }

    /// 再初期化後に、保持していた preedit を `restore_session` で復元する（pending のローマ字は復元されない）
    private func restoreSession(_ preedit: PreeditView) {
        guard let rpc = rpc else { return }
        var segments: [RestoreSegment] = []
        var total = 0
        for seg in preedit.segments {
            segments.append(RestoreSegment(reading: seg.reading, surface: seg.surface, kind: seg.kind))
            total += seg.reading.unicodeScalars.count
        }
        let params = RestoreSessionParams(
            session_id: sessionId,
            segments: segments,
            cursor: UInt32(total),
            focus: nil,
            converting: false
        )
        do {
            let result: ProcessKeyResult = try rpc.call("restore_session", params: params)
            apply(result)
            DiagnosticsLog.shared.add("restore_session ok segments=\(segments.count)")
        } catch {
            DiagnosticsLog.shared.add("restore_session failed: \(describe(error))")
            finalizeMarkedAsPlain()
        }
    }

    private func becomeUnavailable(_ reason: String) {
        state = .unavailable
        rpc = nil
        DiagnosticsLog.shared.add("engine unavailable: \(reason)")
        finalizeMarkedAsPlain()
        let pending = queue
        queue.removeAll()
        for input in pending { passthrough(input) }
        publishBar(message: "\(reason)（標準入力で動作中）")
    }

    private func replayQueue() {
        let pending = queue
        queue.removeAll()
        for input in pending { handle(input) }
    }

    // MARK: - ライフサイクル

    /// 入力欄にフォーカスしたとき。secure 入力欄ならコアに伝えて学習を止める（変換しない設定は呼び出し側が `directInput` で行う）
    func focusIn(secure: Bool) {
        secureInput = secure
        guard state == .ready, let rpc = rpc else { return }
        sendContext()
        do {
            let result: ProcessKeyResult = try rpc.call("focus_in", params: SessionParams(session_id: sessionId))
            apply(result)
        } catch {
            DiagnosticsLog.shared.add("focus_in failed: \(describe(error))")
        }
    }

    /// キーボードが隠れるとき。未確定の文字列を確定して入力を失わない
    func focusOut() {
        if state == .ready, let rpc = rpc {
            do {
                let result: ProcessKeyResult = try rpc.call("focus_out", params: SessionParams(session_id: sessionId))
                apply(result)
            } catch {
                DiagnosticsLog.shared.add("focus_out failed: \(describe(error))")
                finalizeMarkedAsPlain()
            }
        }
        persistDiagnostics()
    }

    /// 未確定の状態を確定せずに捨てる（コアの `reset`）
    func reset() {
        if state == .ready, let rpc = rpc {
            do {
                let result: ProcessKeyResult = try rpc.call("reset", params: SessionParams(session_id: sessionId))
                if markedActive {
                    output.setMarkedText("", cursorUTF16: 0)
                    output.unmarkText()
                    markedActive = false
                }
                lastPreedit = nil
                lastCandidates = nil
                apply(result)
            } catch {
                DiagnosticsLog.shared.add("reset failed: \(describe(error))")
            }
        }
    }

    /// 未確定の文字列があれば、そのまま確定する（レイアウト切替・診断挿入の前に呼ぶ）
    func commitComposition() {
        guard let preedit = lastPreedit, !preedit.text.isEmpty else { return }
        handle(.engineAction("Commit"))
    }

    /// 未確定文字列の出し方を切り替える。切替前に確定して、二重に挿入されないようにする
    func setUseMarkedText(_ value: Bool) {
        guard value != useMarkedText else { return }
        commitComposition()
        useMarkedText = value
    }

    func persistDiagnostics() {
        DiagnosticsLog.shared.persist(header: diagnosticsText(includeLog: false))
    }

    /// 診断テキスト（入力内容は含まない）
    func diagnosticsText(includeLog: Bool = true) -> String {
        var lines: [String] = ["Nagi iOS 診断"]
        if let header = headerProvider?() { lines.append(header) }
        lines.append("engine_state: \(state.rawValue)")
        lines.append("profile: \(profile)")
        lines.append("server_version: \(serverVersion)")
        lines.append("marked_text: \(useMarkedText ? "on" : "off")")
        lines.append("direct_input: \(directInput)")
        lines.append("recoveries: \(recoveryCount)")
        lines.append("latency: \(DiagnosticsLog.shared.latencySummary())")
        if includeLog {
            lines.append("--- log ---")
            lines.append(DiagnosticsLog.shared.logText())
        }
        return lines.joined(separator: "\n")
    }

    // MARK: - 入力

    /// 入力 1 件を処理する
    func handle(_ input: KeyboardInput) {
        if shouldHandleDirectly(input) {
            handleDirect(input)
            return
        }
        switch state {
        case .starting, .recovering:
            if queue.count < maxQueue { queue.append(input) }
            return
        case .unavailable:
            passthrough(input)
            return
        case .ready:
            break
        }
        let start = DispatchTime.now().uptimeNanoseconds
        do {
            try process(input)
            let ms = Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000.0
            DiagnosticsLog.shared.recordLatency(ms)
            noteSuccess()
        } catch let error as NagiRpcError {
            handleFailure(error, input: input)
        } catch {
            handleFailure(.decodeFailed(method: "unknown"), input: input)
        }
    }

    private func shouldHandleDirectly(_ input: KeyboardInput) -> Bool {
        guard directInput else { return false }
        switch input {
        case .engineAction, .selectCandidate: return false
        default: return true
        }
    }

    /// コアを通さない入力（英数レイアウト等）
    private func handleDirect(_ input: KeyboardInput) {
        switch input {
        case .char(let text, _): output.insertText(text)
        case .kana(let reading, _): output.insertText(reading)
        case .special(let key): passthroughSpecial(key)
        default: break
        }
    }

    private func process(_ input: KeyboardInput) throws {
        guard let rpc = rpc else { throw NagiRpcError.emptyResponse(method: "none") }
        switch input {
        case .char(let text, let alternatives):
            let isUpper = text.count == 1 && text != text.lowercased()
            let result = try rpc.processKey(
                sessionId: sessionId,
                keysym: Keysym.forText(text),
                text: text,
                shift: isUpper,
                alternatives: alternatives
            )
            apply(result)
            if !result.consumed { output.insertText(text) }

        case .kana(let reading, let alternatives):
            let result = try rpc.insertReading(sessionId: sessionId, reading: reading, alternatives: alternatives)
            apply(result)
            if !result.consumed { output.insertText(reading) }

        case .special(let key):
            try processSpecial(key, rpc: rpc)

        case .engineAction(let name):
            let result = try rpc.performAction(sessionId: sessionId, action: name)
            apply(result)

        case .selectCandidate(let index):
            try selectCandidate(index, rpc: rpc)

        case .switchLayout:
            break
        }
    }

    private func processSpecial(_ key: SpecialKey, rpc: NagiRpcClient) throws {
        switch key {
        case .space:
            let result = try rpc.processKey(
                sessionId: sessionId, keysym: Keysym.space, text: " ", shift: false, alternatives: []
            )
            apply(result)
            if !result.consumed { output.insertText(" ") }
        case .enter:
            let result = try rpc.processKey(
                sessionId: sessionId, keysym: Keysym.enter, text: "", shift: false, alternatives: []
            )
            apply(result)
            if !result.consumed { output.insertText("\n") }
        case .backspace:
            let result = try rpc.processKey(
                sessionId: sessionId, keysym: Keysym.backspace, text: "", shift: false, alternatives: []
            )
            apply(result)
            if !result.consumed { output.deleteBackward() }
        case .left:
            let result = try rpc.processKey(
                sessionId: sessionId, keysym: Keysym.left, text: "", shift: false, alternatives: []
            )
            apply(result)
            if !result.consumed { output.moveCursor(by: -1) }
        case .right:
            let result = try rpc.processKey(
                sessionId: sessionId, keysym: Keysym.right, text: "", shift: false, alternatives: []
            )
            apply(result)
            if !result.consumed { output.moveCursor(by: 1) }
        case .dakuten:
            try applyDakuten(rpc: rpc)
        case .shift, .globe:
            break
        }
    }

    /// 12 キーの「小゛゜」: 末尾のかなを 1 つ消して、次の形を挿入し直す（変換中は何もしない）
    private func applyDakuten(rpc: NagiRpcClient) throws {
        guard let preedit = lastPreedit,
              preedit.focus == nil,
              preedit.pending == nil,
              !preedit.text.isEmpty,
              Int(preedit.cursor) == preedit.text.utf16.count,
              let last = preedit.text.last,
              let next = KanaTransform.next(after: last)
        else { return }
        let removed = try rpc.processKey(
            sessionId: sessionId, keysym: Keysym.backspace, text: "", shift: false, alternatives: []
        )
        apply(removed)
        let inserted = try rpc.insertReading(sessionId: sessionId, reading: String(next), alternatives: [])
        apply(inserted)
    }

    /// 候補バーのタップ。変換中は候補を選び、最後の文節なら確定、そうでなければ次の文節へ進む。
    /// 未変換（Composing）のときは、読みをそのまま確定する
    private func selectCandidate(_ index: Int, rpc: NagiRpcClient) throws {
        guard let preedit = lastPreedit, !preedit.text.isEmpty else { return }
        guard let focus = preedit.focus else {
            apply(try rpc.performAction(sessionId: sessionId, action: "Commit"))
            return
        }
        let params = SelectCandidateParams(session_id: sessionId, index: UInt32(max(0, index)))
        let selected: ProcessKeyResult = try rpc.call("select_candidate", params: params)
        apply(selected)
        let isLast = Int(focus) >= preedit.segments.count - 1
        let next = isLast ? "Commit" : "MoveFocus:Right"
        apply(try rpc.performAction(sessionId: sessionId, action: next))
    }

    // MARK: - 応答の反映

    private func apply(_ result: ProcessKeyResult) {
        if let n = result.delete_before, n > 0 {
            let count = TextMath.deleteCount(utf16Length: Int(n), endingOf: lastCommit)
            for _ in 0..<count { output.deleteBackward() }
            lastCommit = ""
        }
        var committed = false
        if let commit = result.commit, !commit.isEmpty {
            finishMarked(with: commit)
            lastCommit = commit
            committed = true
        }
        if let preedit = result.preedit { lastPreedit = preedit }
        if let candidates = result.candidates { lastCandidates = candidates }
        if result.preedit != nil || committed { updateMarked() }
        publishBar(message: nil)
    }

    /// 確定文字列を未確定文字列の位置に置いて確定する（未確定が無ければそのまま挿入）
    private func finishMarked(with text: String) {
        if markedActive {
            output.setMarkedText(text, cursorUTF16: text.utf16.count)
            output.unmarkText()
            markedActive = false
        } else {
            output.insertText(text)
        }
    }

    /// 現在の preedit をホストの未確定文字列に反映する
    private func updateMarked() {
        let text = lastPreedit?.text ?? ""
        if text.isEmpty {
            if markedActive {
                output.setMarkedText("", cursorUTF16: 0)
                output.unmarkText()
                markedActive = false
            }
            return
        }
        guard useMarkedText else { return }
        let cursor = min(Int(lastPreedit?.cursor ?? 0), text.utf16.count)
        output.setMarkedText(text, cursorUTF16: cursor)
        markedActive = true
    }

    /// 未確定文字列をそのまま確定扱いにして、コアの状態の保持をやめる
    private func finalizeMarkedAsPlain() {
        if markedActive {
            output.unmarkText()
            markedActive = false
        }
        lastPreedit = nil
        lastCandidates = nil
        publishBar(message: nil)
    }

    // MARK: - 失敗・フォールバック

    private func handleFailure(_ error: NagiRpcError, input: KeyboardInput) {
        DiagnosticsLog.shared.add("failure: \(error)")
        if !error.isFatal {
            // コアは生きている。この入力だけ OS 標準の動作に回す
            passthrough(input)
            return
        }
        recoveryCount += 1
        successesSinceRecovery = 0
        if recoveryCount > 3 {
            becomeUnavailable("エンジンが繰り返し失敗しました")
            passthrough(input)
            return
        }
        state = .recovering
        rpc = nil
        if queue.count < maxQueue { queue.append(input) }
        publishBar(message: "エンジンを再起動中…")
        spawnEngine(isRecovery: true)
    }

    private func noteSuccess() {
        successesSinceRecovery += 1
        if successesSinceRecovery >= 100 { recoveryCount = 0 }
    }

    /// コアを使えないときの入力（OS 標準と同じ素の挿入）。未確定の文字列は先に確定する
    private func passthrough(_ input: KeyboardInput) {
        finalizeMarkedAsPlain()
        switch input {
        case .char(let text, _): output.insertText(text)
        case .kana(let reading, _): output.insertText(reading)
        case .special(let key): passthroughSpecial(key)
        default: break
        }
    }

    private func passthroughSpecial(_ key: SpecialKey) {
        switch key {
        case .space: output.insertText(" ")
        case .enter: output.insertText("\n")
        case .backspace: output.deleteBackward()
        case .left: output.moveCursor(by: -1)
        case .right: output.moveCursor(by: 1)
        case .shift, .globe, .dakuten: break
        }
    }

    // MARK: - 表示

    /// バーに一時的なメッセージを出す（`seconds` 後に消す）
    func showMessage(_ text: String, seconds: TimeInterval) {
        publishBar(message: text)
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { [weak self] in
            self?.publishBar(message: nil)
        }
    }

    private func publishBar(message newMessage: String?) {
        message = newMessage
        var bar = BarState()
        bar.message = newMessage
        if let preedit = lastPreedit {
            bar.preedit = preedit.text
            bar.isConverting = preedit.focus != nil
            for (i, seg) in preedit.segments.enumerated() {
                let focused: Bool
                if let f = preedit.focus { focused = Int(f) == i } else { focused = false }
                bar.segments.append(BarSegment(surface: seg.surface, focused: focused))
            }
        }
        if let candidates = lastCandidates, candidates.visible {
            for item in candidates.items {
                bar.candidates.append(BarCandidate(surface: item.surface, corrected: item.sources.contains(.corrected)))
            }
            bar.selectedCandidate = Int(candidates.selected)
        }
        onBarChange?(bar)
    }

    private func describe(_ error: Error) -> String {
        if let e = error as? NagiRpcError { return e.description }
        return "error"
    }
}
