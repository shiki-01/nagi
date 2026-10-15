import UIKit
import SwiftUI

/// 製品版キーボードと検証用の画面（Probe）の切り替え
final class KeyboardScreen: ObservableObject {
    @Published var showProbes = false
}

class KeyboardViewController: UIInputViewController, KeyboardHost {
    private let store = SharedStore.shared
    private var model: KeyboardModel?
    private let screen = KeyboardScreen()
    private var hostingController: UIHostingController<KeyboardRoot>?
    /// キーボード全体の高さの制約（設定の高さの倍率で、focusIn のたびに更新する）
    private var heightConstraint: NSLayoutConstraint?
    /// 直近に見た入力欄の識別子（`textDocumentProxy.documentIdentifier`）。変わったら入力欄が切り替わっている
    private var lastDocumentIdentifier: UUID?

    override func viewDidLoad() {
        LaunchBaseline.capture()
        super.viewDidLoad()

        setupUI()
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        // ホストアプリのセットアップ状況表示用のマーカー（OnboardingView.swift の NagiSetupMarker が読む）
        if let defaults = UserDefaults(suiteName: SharedStore.shared.appGroupId) {
            let now = Date().timeIntervalSince1970
            defaults.set(now, forKey: "keyboardLastSeen")
            if hasFullAccess { defaults.set(now, forKey: "fullAccessLastSeen") }
        }
        lastDocumentIdentifier = textDocumentProxy.documentIdentifier
        model?.focusIn()
        // 触覚・クリック音: フルアクセスの有無を渡し、focusIn で読み直した設定でジェネレータを準備する
        NagiKeyFeedback.fullAccess = hasFullAccess
        NagiKeyFeedback.prepare()
        // focusIn で読み直した設定の高さを反映する
        heightConstraint?.constant = CGFloat(NagiKeyMetrics.totalHeight)
        applyTheme()
    }

    /// 設定 `keyboard.theme` の外観を反映する（system は端末に従う）
    private func applyTheme() {
        hostingController?.overrideUserInterfaceStyle = NagiTheme.interfaceStyle(NagiKeyMetrics.current.theme)
    }

    override func viewWillDisappear(_ animated: Bool) {
        // 変換中の文字列は確定してから離れる
        model?.focusOut()
        super.viewWillDisappear(animated)
    }

    private func setupUI() {
        // コアのセッション。辞書やモデルが無ければ、画面にメッセージを出す
        let model = KeyboardModel(outcome: ProductSession.make(store: store))
        model.host = self
        self.model = model

        let probeView = KeyboardSwiftUIView(
            store: store,
            insertTextHandler: { [weak self] text in
                self?.textDocumentProxy.insertText(text)
            },
            deleteBackwardHandler: { [weak self] in
                self?.textDocumentProxy.deleteBackward()
            },
            advanceToNextInputModeHandler: { [weak self] in
                self?.advanceToNextInputMode()
            }
        )
        // 触覚ジェネレータを結びつけるビュー（iOS 17.5 以降）
        NagiKeyFeedback.anchorView = view
        let root = KeyboardRoot(model: model, screen: screen, probeView: probeView)
        let hostView = UIHostingController(rootView: root)
        hostingController = hostView
        addChild(hostView)

        self.view.addSubview(hostView.view)
        hostView.view.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            hostView.view.leadingAnchor.constraint(equalTo: self.view.leadingAnchor),
            hostView.view.trailingAnchor.constraint(equalTo: self.view.trailingAnchor),
            hostView.view.topAnchor.constraint(equalTo: self.view.topAnchor),
            hostView.view.bottomAnchor.constraint(equalTo: self.view.bottomAnchor)
        ])
        hostView.didMove(toParent: self)
        applyTheme()

        // キーボードの高さ（候補バー 44 + キー 4 行）。優先度を下げて OS の制約と衝突しないようにする
        let height = view.heightAnchor.constraint(equalToConstant: CGFloat(NagiKeyMetrics.totalHeight))
        height.priority = UILayoutPriority(999)
        height.isActive = true
        heightConstraint = height
    }

    // MARK: - KeyboardHost

    func hostInsertText(_ text: String) {
        textDocumentProxy.insertText(text)
    }

    func hostDeleteBackward() {
        textDocumentProxy.deleteBackward()
    }

    func hostSetMarkedText(_ text: String, cursor: Int) {
        let position = min(max(cursor, 0), text.utf16.count)
        textDocumentProxy.setMarkedText(text, selectedRange: NSRange(location: position, length: 0))
    }

    func hostUnmarkText() {
        textDocumentProxy.unmarkText()
    }

    func hostClearMarkedText() {
        textDocumentProxy.setMarkedText("", selectedRange: NSRange(location: 0, length: 0))
        textDocumentProxy.unmarkText()
    }

    func hostAdjustCursor(by offset: Int) {
        textDocumentProxy.adjustTextPosition(byCharacterOffset: offset)
    }

    func hostAdvanceToNextInputMode() {
        advanceToNextInputMode()
    }

    func hostDismissKeyboard() {
        dismissKeyboard()
    }

    func hostContextBeforeInput() -> String? {
        textDocumentProxy.documentContextBeforeInput
    }

    func hostIsSecure() -> Bool {
        let proxy = textDocumentProxy
        if proxy.isSecureTextEntry ?? false { return true }
        // パスワード・ワンタイムコードの入力欄（secure でなくても文脈・学習の対象にしない）
        let type = proxy.textContentType
        return type == UITextContentType.password
            || type == UITextContentType.newPassword
            || type == UITextContentType.oneTimeCode
    }

    // MARK: - メモリ警告

    /// OS からのメモリ警告。コアに `memory_pressure: critical` を送って LLM を手放させ、キャッシュを捨てる
    /// （強制終了される前に軽くする）
    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        model?.handleMemoryWarning()
    }

    // MARK: - ホストの入力欄の変化

    /// 入力欄の内容やカーソルが変わった（外部からの変更、カーソル移動、自分の操作の結果）。
    /// 変換中でなければ、直前のテキストを文脈としてコアに渡し直す
    override func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        // 入力欄そのものが切り替わったときは、状態を初期化する（入力中の文字は前の欄で確定扱い）
        let identifier = textDocumentProxy.documentIdentifier
        if let last = lastDocumentIdentifier, last != identifier {
            lastDocumentIdentifier = identifier
            model?.hostDocumentChanged()
        } else if lastDocumentIdentifier == nil {
            lastDocumentIdentifier = identifier
        }
        model?.hostTextChanged()
    }
}

/// 製品版キーボードと検証用の画面を切り替えるルート
struct KeyboardRoot: View {
    @ObservedObject var model: KeyboardModel
    @ObservedObject var screen: KeyboardScreen
    let probeView: KeyboardSwiftUIView

    var body: some View {
        if screen.showProbes && DiagLog.isTestBuild {
            ZStack(alignment: .topTrailing) {
                probeView
                Button("閉じる") { screen.showProbes = false }
                    .font(.system(size: 13))
                    .padding(8)
            }
        } else {
            NagiKeyboardView(model: model, onShowProbes: { screen.showProbes = true })
        }
    }
}

// MARK: - SwiftUI 部分

struct KeyboardSwiftUIView: View {
    let store: SharedStore
    let insertTextHandler: (String) -> Void
    let deleteBackwardHandler: () -> Void
    let advanceToNextInputModeHandler: () -> Void

    @State private var results: [ProbeResult] = []
    @State private var selectedProbe: String?
    @State private var isRunning = false
    @State private var showInsertResultsAlert = false
    @State private var hasRunAutoProbe = false
    /// 画面に出す診断ログ（保存や挿入に失敗しても状況が見えるようにする）
    @State private var logLines: [String] = []
    @State private var runningProbe: String?

    /// 実行中の Probe 名を保存するキー。次回起動時に残っていれば、実行中に強制終了されたと判断する
    private static let runningMarkerKey = "nagi.keyboard.runningProbe"

    var body: some View {
        VStack(spacing: 0) {
            // ヘッダ
            VStack {
                Text("Nagi Keyboard Probe")
                    .font(.headline)
                Text(runningProbe.map { "実行中: \($0)" } ?? "各ボタンで検証項目を実行")
                    .font(.caption)
                    .foregroundColor(.gray)
                Text(statusLine())
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundColor(.gray)
            }
            .frame(maxWidth: .infinity)
            .padding()
            .background(Color(.systemGray6))

            // 診断ログ（直近 8 行）
            if !logLines.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(logLines.enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(.system(size: 10, design: .monospaced))
                            .foregroundColor(.black)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.horizontal)
                .padding(.vertical, 4)
                .background(Color(.systemYellow).opacity(0.25))
            }

            // Probe ボタン
            ScrollView {
                VStack(spacing: 8) {
                    ForEach(Array(allProbesExcludingMemoryRamp.keys.sorted()), id: \.self) { probeId in
                        Button(action: { runProbe(probeId) }) {
                            HStack {
                                Text(probeId)
                                Spacer()
                                if let latest = results.last(where: { $0.id == probeId }) {
                                    Image(systemName: latest.ok ? "checkmark.circle" : "xmark.circle")
                                        .foregroundColor(latest.ok ? .green : .red)
                                }
                            }
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color(.systemGray5))
                            .foregroundColor(.black)
                            .cornerRadius(8)
                        }
                        .disabled(isRunning)
                    }

                    Button(action: { runAllProbes() }) {
                        Text("すべて実行（memory_ramp 除外）")
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color.blue)
                            .foregroundColor(.white)
                            .cornerRadius(8)
                    }
                    .disabled(isRunning)

                    Button(action: { runProbe("memory_ramp") }) {
                        Text("Memory Ramp（警告: 強制終了の可能性）")
                            .frame(maxWidth: .infinity)
                            .padding()
                            .background(Color.orange)
                            .foregroundColor(.white)
                            .cornerRadius(8)
                    }
                    .disabled(isRunning)
                }
                .padding()
            }

            Divider()

            Divider()

            // テスト入力ボタン・結果挿入ボタン
            VStack(spacing: 8) {
                HStack(spacing: 4) {
                    Button("あ") { insertTextHandler("あ") }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("か") { insertTextHandler("か") }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("Space") { insertTextHandler(" ") }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("⌫") { deleteBackwardHandler() }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)

                    Button("Next") { advanceToNextInputModeHandler() }
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color(.systemGray5))
                        .cornerRadius(4)
                }

                // 入力欄への挿入は DEBUG / TestFlight のビルドだけ（App Store 版では出さない）
                if DiagLog.isTestBuild {
                    Button(action: { insertResultsToTextField() }) {
                        Text("結果を入力欄に挿入")
                            .frame(maxWidth: .infinity)
                            .padding(8)
                            .background(Color.green)
                            .foregroundColor(.white)
                            .cornerRadius(4)
                    }
                }
            }
            .padding()
        }
        .onAppear {
            reportPreviousTermination()
            loadResults()
            autoRunBaselineMemory()
        }
    }

    // MARK: - 診断

    private func appendLog(_ line: String) {
        logLines.append(line)
        if logLines.count > 8 { logLines.removeFirst(logLines.count - 8) }
    }

    private func statusLine() -> String {
        let stats = MemoryProbe.getStats()
        let group = store.containerURL != nil ? "AppGroup:OK" : "AppGroup:NG"
        return String(format: "%@ footprint=%.0fMB available=%.0fMB", group, stats.physFootprintMB, stats.availableMemoryMB)
    }

    private func setRunningMarker(_ probeId: String?) {
        let defaults = UserDefaults.standard
        if let probeId {
            defaults.set(probeId, forKey: Self.runningMarkerKey)
        } else {
            defaults.removeObject(forKey: Self.runningMarkerKey)
        }
        defaults.synchronize()
    }

    /// 前回の実行が終わらないまま拡張が終了していれば、その Probe 名を表示する
    private func reportPreviousTermination() {
        let defaults = UserDefaults.standard
        if let probeId = defaults.string(forKey: Self.runningMarkerKey) {
            appendLog("前回 \(probeId) の実行中に終了（メモリ超過の疑い）")
            defaults.removeObject(forKey: Self.runningMarkerKey)
            defaults.synchronize()
        }
    }

    /// Probe を 1 件実行し、開始・終了・保存失敗を画面ログに残す
    private func execute(_ probeId: String, context: ProbeContext) async {
        runningProbe = probeId
        setRunningMarker(probeId)
        appendLog("開始: \(probeId)")
        let result: ProbeResult
        if probeId == "memory_ramp" {
            result = await MemoryRampProbe(maxLimitMB: 1024).run(context: context)
        } else if let probeRunner = allProbes[probeId] {
            result = await probeRunner(context)
        } else {
            appendLog("未知の Probe: \(probeId)")
            setRunningMarker(nil)
            runningProbe = nil
            return
        }
        setRunningMarker(nil)
        runningProbe = nil
        let metrics = result.metrics
            .sorted { $0.key < $1.key }
            .map { "\($0.key)=\(String(format: "%.1f", $0.value))" }
            .joined(separator: " ")
        appendLog("\(result.ok ? "OK" : "NG"): \(probeId) \(metrics)")
        do {
            try store.append(result: result)
        } catch {
            appendLog("保存失敗: \(error.localizedDescription)")
        }
    }

    private func runProbe(_ probeId: String) {
        isRunning = true
        Task {
            let context = ProbeContext(store: store, processType: "keyboard", engine: nil)
            await execute(probeId, context: context)
            loadResults()
            isRunning = false
        }
    }

    private func runAllProbes() {
        isRunning = true
        Task {
            for probeId in allProbesExcludingMemoryRamp.keys.sorted() {
                let context = ProbeContext(store: store, processType: "keyboard", engine: nil)
                await execute(probeId, context: context)
            }
            loadResults()
            isRunning = false
        }
    }

    private func autoRunBaselineMemory() {
        guard !hasRunAutoProbe else { return }
        hasRunAutoProbe = true

        Task {
            let context = ProbeContext(store: store, processType: "keyboard", engine: nil)
            let result = await BaselineMemoryProbe().run(context: context)
            do {
                try store.append(result: result)
            } catch {
                appendLog("保存失敗(baseline): \(error.localizedDescription)")
            }
            loadResults()
        }
    }

    private func loadResults() {
        results = store.read(last: 100).filter { $0.process == "keyboard" }
    }

    private func insertResultsToTextField() {
        let resultLines = results.map { result in
            let status = result.ok ? "ok" : "ng"
            let metricsStr = result.metrics
                .sorted { $0.key < $1.key }
                .map { "\($0.key)=\(String(format: "%.2f", $0.value))" }
                .joined(separator: ", ")
            return "\(result.id) | \(result.process) | \(status) | \(metricsStr)"
        }

        let insertText = (logLines + resultLines).joined(separator: "\n")
        if !insertText.isEmpty {
            // テキスト入力（行が長い場合は複数行に分割）
            let lines = insertText.split(separator: "\n", omittingEmptySubsequences: true)
            for (index, line) in lines.enumerated() {
                if index > 0 {
                    // 複数行の場合は前の行の後に改行を挿入（省略）
                }
            }
            // 一度に全て挿入
            insertTextHandler(insertText)
        }
    }
}

#Preview {
    KeyboardSwiftUIView(
        store: SharedStore.shared,
        insertTextHandler: { _ in },
        deleteBackwardHandler: { },
        advanceToNextInputModeHandler: { }
    )
}
