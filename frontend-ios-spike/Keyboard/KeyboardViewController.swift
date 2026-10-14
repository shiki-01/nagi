import UIKit
import SwiftUI

class KeyboardViewController: UIInputViewController {
    private let store = SharedStore.shared
    private var containerView: UIView?
    private var swiftUIView: KeyboardSwiftUIView?

    override func viewDidLoad() {
        LaunchBaseline.capture()
        super.viewDidLoad()

        setupUI()
    }

    private func setupUI() {
        // SwiftUI ホストをコンテナとして使う
        swiftUIView = KeyboardSwiftUIView(
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

        let hostView = UIHostingController(rootView: swiftUIView!)
        addChild(hostView)

        let containerView = UIView()
        self.view.addSubview(containerView)
        containerView.translatesAutoresizingMaskIntoConstraints = false

        NSLayoutConstraint.activate([
            containerView.leadingAnchor.constraint(equalTo: self.view.leadingAnchor),
            containerView.trailingAnchor.constraint(equalTo: self.view.trailingAnchor),
            containerView.topAnchor.constraint(equalTo: self.view.topAnchor),
            containerView.bottomAnchor.constraint(equalTo: self.view.bottomAnchor)
        ])

        self.view.addSubview(hostView.view)
        hostView.view.translatesAutoresizingMaskIntoConstraints = false

        NSLayoutConstraint.activate([
            hostView.view.leadingAnchor.constraint(equalTo: self.view.leadingAnchor),
            hostView.view.trailingAnchor.constraint(equalTo: self.view.trailingAnchor),
            hostView.view.topAnchor.constraint(equalTo: self.view.topAnchor),
            hostView.view.bottomAnchor.constraint(equalTo: self.view.bottomAnchor)
        ])

        hostView.didMove(toParent: self)
        self.containerView = containerView
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

                Button(action: { insertResultsToTextField() }) {
                    Text("結果を入力欄に挿入")
                        .frame(maxWidth: .infinity)
                        .padding(8)
                        .background(Color.green)
                        .foregroundColor(.white)
                        .cornerRadius(4)
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
