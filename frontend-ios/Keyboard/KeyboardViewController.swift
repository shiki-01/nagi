import UIKit

/// `UITextDocumentProxy` へ出力するアダプタ
final class ProxyOutput: HostTextOutput {
    private weak var host: UIInputViewController?

    init(host: UIInputViewController) {
        self.host = host
    }

    func insertText(_ text: String) {
        host?.textDocumentProxy.insertText(text)
    }

    func deleteBackward() {
        host?.textDocumentProxy.deleteBackward()
    }

    func setMarkedText(_ text: String, cursorUTF16: Int) {
        host?.textDocumentProxy.setMarkedText(text, selectedRange: NSRange(location: cursorUTF16, length: 0))
    }

    func unmarkText() {
        host?.textDocumentProxy.unmarkText()
    }

    func moveCursor(by offset: Int) {
        host?.textDocumentProxy.adjustTextPosition(byCharacterOffset: offset)
    }
}

final class KeyboardViewController: UIInputViewController, KeyboardLayoutViewDelegate {
    private var controller: NagiInputController!
    private let barView = CandidateBarView()
    private let layoutView = KeyboardLayoutView()
    private var heightConstraint: NSLayoutConstraint?

    private var layouts: [KeyboardLayout] = []
    private var layoutErrors: [String] = []
    private var currentLayout: KeyboardLayout?
    private var shift: ShiftState = .off
    private var lastShiftTap = Date.distantPast

    private let paths = NagiPaths.resolve()
    private var configOutcome = ConfigBootstrap.Outcome.noUserDir
    /// 入力欄の種類（メール・URL・数字など）から、日本語変換を使わないと判断したか
    private var fieldIsASCII = false
    private var fieldIsSecure = false

    private let barHeight: CGFloat = 44
    private let rowHeight: CGFloat = 52
    private let lastLayoutKey = "nagi.lastLayoutID"

    // MARK: - ライフサイクル

    override func viewDidLoad() {
        LaunchBaseline.capture()
        super.viewDidLoad()

        controller = NagiInputController(output: ProxyOutput(host: self))
        controller.headerProvider = { [weak self] in
            guard let self = self else { return "" }
            var lines: [String] = [self.paths.summary]
            lines.append("config_toml: \(self.configOutcome.rawValue)")
            lines.append("layout: \(self.currentLayout?.id ?? "none")")
            lines.append("layout_errors: \(self.layoutErrors.count)")
            lines.append(String(format: "phys_footprint_mb: %.0f", MemoryProbe.getPhysFootprintMB()))
            lines.append(String(format: "available_memory_mb: %.0f", MemoryProbe.getAvailableMemoryMB()))
            return lines.joined(separator: "\n")
        }
        controller.onBarChange = { [weak self] state in
            self?.barView.apply(state)
        }
        if let userDir = paths.userDir {
            DiagnosticsLog.shared.persistURL = URL(fileURLWithPath: userDir)
                .appendingPathComponent("diagnostics-keyboard.log")
        }
        DiagnosticsLog.shared.add(
            "keyboard loaded data_dir_source=\(paths.dataSource) app_group=\(paths.appGroup.resolvedID ?? "none")"
        )

        buildUI()
        loadLayouts()
        startEngine()
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        refreshFieldTraits()
        controller.focusIn(secure: fieldIsSecure)
        applyFieldPolicy()
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        controller.focusOut()
    }

    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        DiagnosticsLog.shared.add(String(format: "memory warning phys=%.0fMB", MemoryProbe.getPhysFootprintMB()))
        controller.persistDiagnostics()
    }

    // MARK: - 構築

    private func buildUI() {
        view.backgroundColor = UIColor.systemGray6
        barView.translatesAutoresizingMaskIntoConstraints = false
        layoutView.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(barView)
        view.addSubview(layoutView)
        NSLayoutConstraint.activate([
            barView.topAnchor.constraint(equalTo: view.topAnchor),
            barView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            barView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            barView.heightAnchor.constraint(equalToConstant: barHeight),
            layoutView.topAnchor.constraint(equalTo: barView.bottomAnchor),
            layoutView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            layoutView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            layoutView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        let height = view.heightAnchor.constraint(equalToConstant: barHeight + rowHeight * 4)
        height.priority = UILayoutPriority(rawValue: 999)
        height.isActive = true
        heightConstraint = height

        layoutView.delegate = self

        barView.onSelectCandidate = { [weak self] index in
            self?.controller.handle(.selectCandidate(index))
        }
        barView.onMoveFocus = { [weak self] direction in
            self?.controller.handle(.engineAction(direction < 0 ? "MoveFocus:Left" : "MoveFocus:Right"))
        }
        barView.onResize = { [weak self] direction in
            self?.controller.handle(.engineAction(direction < 0 ? "ResizeSegment:Left" : "ResizeSegment:Right"))
        }
        barView.onInfoTap = { [weak self] in
            self?.insertDiagnostics()
        }
        barView.onInfoLongPress = { [weak self] in
            self?.toggleMarkedText()
        }
    }

    private func loadLayouts() {
        let result = LayoutStore.load(bundle: Bundle.main, userDir: paths.userDir)
        layouts = result.layouts
        layoutErrors = result.errors
        for error in result.errors {
            DiagnosticsLog.shared.add("layout error: \(error)")
        }
        let saved = UserDefaults.standard.string(forKey: lastLayoutKey)
        showLayout(id: saved ?? LayoutStore.defaultLayoutID, remember: false)
    }

    private func startEngine() {
        let dataDir = paths.dataDir
        let userDir = paths.userDir
        configOutcome = ConfigBootstrap.ensureConfig(userDir: userDir)
        controller.startEngine(factory: {
            return NagiEngine(dataDir: dataDir, userDir: userDir, noLLM: true)
        })
    }

    // MARK: - 入力欄の種類

    private func refreshFieldTraits() {
        // keyboardType / isSecureTextEntry は Optional 要件のため、Bool? / UIKeyboardType? に受ける
        let secure: Bool? = textDocumentProxy.isSecureTextEntry
        let type: UIKeyboardType? = textDocumentProxy.keyboardType
        fieldIsSecure = secure ?? false
        fieldIsASCII = false
        if let type = type {
            switch type {
            case .asciiCapable, .URL, .emailAddress, .numberPad, .phonePad, .decimalPad,
                 .namePhonePad, .numbersAndPunctuation, .asciiCapableNumberPad:
                fieldIsASCII = true
            default:
                break
            }
        }
    }

    private func isNumericField() -> Bool {
        let type: UIKeyboardType? = textDocumentProxy.keyboardType
        guard let type = type else { return false }
        switch type {
        case .numberPad, .phonePad, .decimalPad, .namePhonePad, .numbersAndPunctuation, .asciiCapableNumberPad:
            return true
        default:
            return false
        }
    }

    /// 入力欄の種類に応じてレイアウトと変換の有無を決める
    private func applyFieldPolicy() {
        if fieldIsSecure || fieldIsASCII {
            controller.commitComposition()
            let id = isNumericField() ? "symbols-direct" : "qwerty-direct"
            showLayout(id: id, remember: false)
        } else {
            let saved = UserDefaults.standard.string(forKey: lastLayoutKey) ?? LayoutStore.defaultLayoutID
            showLayout(id: saved, remember: false)
        }
        updateDirectInput()
    }

    private func updateDirectInput() {
        controller.directInput = (currentLayout?.isDirect ?? false) || fieldIsASCII || fieldIsSecure
    }

    // MARK: - レイアウトの切替

    private func showLayout(id: String, remember: Bool) {
        var target = layouts.first(where: { $0.id == id })
        if target == nil { target = layouts.first(where: { $0.id == LayoutStore.defaultLayoutID }) }
        if target == nil { target = layouts.first }
        guard let layout = target else {
            controller.showMessage("レイアウトを読み込めませんでした", seconds: 3)
            return
        }
        if currentLayout?.id == layout.id { return }
        currentLayout = layout
        shift = .off
        layoutView.shiftState = .off
        layoutView.setLayout(layout)
        heightConstraint?.constant = barHeight + rowHeight * CGFloat(layout.rows.count)
        if remember {
            UserDefaults.standard.set(layout.id, forKey: lastLayoutKey)
        }
        updateDirectInput()
    }

    // MARK: - KeyboardLayoutViewDelegate

    func layoutView(_ view: KeyboardLayoutView, didProduce input: KeyboardInput) {
        switch input {
        case .special(.shift):
            toggleShift()
        case .special(.globe):
            advanceToNextInputMode()
        case .switchLayout(let id):
            // 切替先が英数（direct）なら、未確定の文字列を先に確定する
            if let target = layouts.first(where: { $0.id == id }), target.isDirect {
                controller.commitComposition()
            }
            showLayout(id: id, remember: true)
        default:
            controller.handle(input)
            if case .char = input, shift == .once {
                shift = .off
                layoutView.shiftState = .off
            }
        }
    }

    private func toggleShift() {
        let now = Date()
        switch shift {
        case .off:
            shift = .once
        case .once:
            // 素早く 2 回押したら Caps Lock
            shift = now.timeIntervalSince(lastShiftTap) < 0.4 ? .locked : .off
        case .locked:
            shift = .off
        }
        lastShiftTap = now
        layoutView.shiftState = shift
    }

    // MARK: - 診断

    private func insertDiagnostics() {
        controller.commitComposition()
        textDocumentProxy.insertText(controller.diagnosticsText())
        controller.persistDiagnostics()
    }

    private func toggleMarkedText() {
        let next = !controller.useMarkedText
        controller.setUseMarkedText(next)
        controller.showMessage("未確定文字列の表示: \(next ? "入力欄内（下線）" : "このバーのみ")", seconds: 2.5)
    }
}
