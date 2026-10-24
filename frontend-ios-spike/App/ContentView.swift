import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @State private var selectedTab = 0
    @State private var store = SharedStore.shared
    @State private var deviceInfo = ""
    @State private var dataStatus = ""
    /// ホームのデータ状況の再読み込みの合図（取り込み・モデル選択のあとに増やす）
    @State private var dataReloadToken = 0
    @State private var showingDocumentPicker = false
    @State private var copyingDict = false
    @State private var modelFiles: [DataPaths.ModelFile] = []
    @State private var selectedModelName = ""
    @State private var allResults: [ProbeResult] = []
    @State private var showingAlert = false
    @State private var alertMessage = ""
    @State private var probesRunning = false
    // 初回案内（初回のみ自動表示。ホームから再表示できる）
    @AppStorage("nagi.onboarding.completed") private var onboardingCompleted = false
    // 初回は最初の描画からシートを出す（URL 取り込みの確認アラートが、あとから出るシートと重ならないように）
    @State private var showingOnboarding = !UserDefaults.standard.bool(forKey: "nagi.onboarding.completed")
    @ObservedObject private var layoutInbox = NagiLayoutURLInbox.shared

    var body: some View {
        TabView(selection: $selectedTab) {
            // Tab 1: ホーム（セットアップ・データの状況と取り込み。端末情報・App Group は About の開発者向け）
            NavigationStack {
                List {
                    NagiSetupStatusSection(onShowGuide: { showingOnboarding = true })

                    // 辞書・モデル・メモリ・LLM の見込み（取り込みは下の「取り込み」）
                    NagiDataStatusSections(reloadToken: dataReloadToken)

                    // 英語の語彙・カタカナ語の原語データ（任意。入れると英語の自動判定・原語の候補が働く）
                    NagiOptionalDataSection(onChanged: { dataReloadToken += 1 })

                    Section("ユーザー辞書") {
                        NavigationLink("ユーザー辞書・スニペット") { NagiUserDictionaryView() }
                    }

                    Section("取り込み") {
                        // ボタンごとに別の行にし、.borderless でボタン本体だけをタップ領域にする。
                        // 同じ行に複数の Button があると、List では行全体がタップ領域になり、
                        // 無効なボタンを押しても同じ行の有効なボタンが発火するため
                        if store.containerURL == nil {
                            Text("App Group コンテナが取得できていません。辞書はバンドルのものを自動的に使います。モデルはこのアプリ専用の領域（Application Support）にインポートされ、キーボード拡張からは見えません")
                                .font(.caption)
                                .foregroundColor(.orange)
                        } else {
                            Button("辞書をバンドルからコピー") {
                                copyDictFromBundle()
                            }
                            .buttonStyle(.borderless)
                            .disabled(!canCopyDict())

                            if !canCopyDict() {
                                Text("バンドルに辞書ファイルがないためコピーできません")
                                    .font(.caption)
                                    .foregroundColor(.orange)
                            }
                        }

                        if copyingDict {
                            HStack {
                                ProgressView()
                                Text("モデルをコピー中…（数十秒かかることがあります）").font(.caption)
                            }
                        } else {
                            Button("モデルをインポート（.gguf を選択）") {
                                showingDocumentPicker = true
                            }
                            .buttonStyle(.borderless)
                        }

                        if modelFiles.count > 1 {
                            Picker("llm_load で使うモデル", selection: Binding(
                                get: { selectedModelName },
                                set: { name in
                                    selectedModelName = name
                                    DataPaths.setPreferredModelName(name, appGroupId: store.appGroupId)
                                    updateDataStatus()
                                }
                            )) {
                                ForEach(modelFiles, id: \.name) { file in
                                    Text("\(file.name) (\(formatBytes(file.sizeBytes)))").tag(file.name)
                                }
                            }
                        }
                    }
                }
                .navigationTitle("ホーム")
                .onAppear { updateDataStatus() }
                .fileImporter(
                    isPresented: $showingDocumentPicker,
                    // .gguf は未登録の拡張子なので、動的型に加えて .data も許可し、拡張子は importModel で検査する
                    allowedContentTypes: [UTType(filenameExtension: "gguf") ?? .data, .data],
                    onCompletion: { result in
                        switch result {
                        case .success(let url):
                            importModel(from: url)
                        case .failure(let error):
                            alertMessage = "ファイルを選べませんでした: \(error.localizedDescription)"
                            showingAlert = true
                        }
                    }
                )
            }
            .tabItem {
                Label("ホーム", systemImage: "house.fill")
            }
            .tag(0)

            // Tab 2: キーボードの入力テスト
            KeyboardTestView()
                .tabItem {
                    Label("入力テスト", systemImage: "keyboard")
                }
                .tag(3)

            // Tab 3: 設定
            SettingsView { developerView }
                .tabItem {
                    Label("設定", systemImage: "gearshape")
                }
                .tag(4)

            // Tab 4: レイアウト
            LayoutManagerView().tabItem { Label("レイアウト", systemImage: "square.grid.2x2") }.tag(22)

            // Tab 5: About（端末情報・App Group・Probes・結果は、設定の「開発者」から）
            NagiAboutView()
                .tabItem {
                    Label("About", systemImage: "info.circle")
                }
                .tag(21)
        }
        .alert("メッセージ", isPresented: $showingAlert) {
            Button("OK") { }
        } message: {
            Text(alertMessage)
        }
        .sheet(isPresented: $showingOnboarding, onDismiss: { onboardingCompleted = true }) {
            NagiOnboardingView(
                onClose: { showingOnboarding = false },
                onOpenData: { selectedTab = 0 },
                onOpenInputTest: { selectedTab = 3 }
            )
        }
        // nagi://layout の取り込みの確認と結果。初回案内のシートが出ている間は出さない
        .modifier(NagiLayoutInboxAlerts(inbox: layoutInbox, suppressed: showingOnboarding))
        .onAppear {
            if !onboardingCompleted { showingOnboarding = true }
        }
    }

    // MARK: - 検証（Probes・結果。設定の「開発者」から開く。開発者モードがオンのときだけ）

    private var developerView: some View {
        List {
            Section("端末情報") {
                Text(deviceInfo).font(.system(.body, design: .monospaced))
            }

            Section("App Group") {
                VStack(alignment: .leading, spacing: 8) {
                    let resolution = store.appGroupResolution
                    if let resolvedID = resolution.resolvedID {
                        Text("✓ 利用可能: \(resolvedID)").foregroundColor(.green)
                    } else {
                        Text("✗ 利用不可（無料アカウント？）").foregroundColor(.red)
                    }

                    Divider()

                    Text("解決結果").font(.caption).fontWeight(.semibold)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Candidates: \(resolution.candidates.joined(separator: ", "))")
                            .font(.caption2)
                            .textSelection(.enabled)

                        if !resolution.provisionGroups.isEmpty {
                            Text("Provision: \(resolution.provisionGroups.joined(separator: ", "))")
                                .font(.caption2)
                                .textSelection(.enabled)
                        }

                        if !resolution.altGroups.isEmpty {
                            Text("ALT: \(resolution.altGroups.joined(separator: ", "))")
                                .font(.caption2)
                                .textSelection(.enabled)
                        }

                        Text("Provision readable: \(resolution.provisionReadable)")
                            .font(.caption2)
                    }
                }
            }

            Section("データの配置") {
                Text(dataStatus)
                    .font(.system(.body, design: .monospaced))
                    .textSelection(.enabled)
            }

            Section("検証") {
                NavigationLink("Probes") { probesView }
                NavigationLink("結果") { resultsView }
            }
        }
        .navigationTitle("開発者向け")
        .onAppear {
            updateDeviceInfo()
            updateDataStatus()
        }
    }

    private var probesView: some View {
        List {
            Section("Probes") {
                ForEach(Array(allProbes.keys.sorted()), id: \.self) { probeId in
                    let probeName = allProbes[probeId].map { _ in probeId } ?? probeId
                    NavigationLink(destination: ProbeDetailView(probeId: probeId)) {
                        HStack {
                            Text(probeName)
                            Spacer()
                            let results = allResults.filter { $0.id == probeId && $0.process == "app" }
                            if let latest = results.last {
                                Image(systemName: latest.ok ? "checkmark.circle.fill" : "xmark.circle.fill")
                                    .foregroundColor(latest.ok ? .green : .red)
                            }
                        }
                    }
                    .disabled(probesRunning)
                }
            }

            Section("実行") {
                Button("すべての Probe を実行（memory_ramp 除外）") {
                    runAllProbesExcludingMemoryRamp()
                }
                .frame(maxWidth: .infinity)
                .disabled(probesRunning)

                Button("Memory Ramp を実行（警告: 強制終了の可能性あり）") {
                    runMemoryRamp()
                }
                .frame(maxWidth: .infinity)
                .disabled(probesRunning)
                .foregroundColor(.orange)
            }
        }
        .navigationTitle("Probes")
        .onAppear { loadResults() }
    }

    private var resultsView: some View {
        VStack(spacing: 0) {
            // サマリ表示（1行テキスト選択可能）
            VStack(alignment: .leading, spacing: 4) {
                Text("結果サマリ")
                    .font(.headline)
                    .padding()

                Text(resultsSummary())
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .padding()
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color(.systemGray6))
            }

            Divider()

            // 詳細表示（TextEditor）
            VStack(alignment: .leading, spacing: 4) {
                Text("詳細（すべての結果）")
                    .font(.headline)
                    .padding()

                TextEditor(text: .constant(resultsFullText()))
                    .font(.system(.caption, design: .monospaced))
                    .disabled(true)
                    .padding()
            }

            Divider()

            // アクション
            VStack(spacing: 8) {
                HStack(spacing: 8) {
                    Button(action: {
                        UIPasteboard.general.string = resultsSummary()
                        alertMessage = "サマリをコピーしました"
                        showingAlert = true
                    }) {
                        Label("コピー", systemImage: "doc.on.doc")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)

                    ShareLink(item: resultsSummary()) {
                        Label("共有", systemImage: "square.and.arrow.up")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)
                }

                Button(action: { exportResults() }) {
                    Label("エクスポート (JSONL)", systemImage: "arrow.down.doc")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)

                Button("すべてクリア", role: .destructive) {
                    clearResults()
                }
                .frame(maxWidth: .infinity)
            }
            .padding()
        }
        .navigationTitle("結果")
        .onAppear { loadResults() }
    }

    private func updateDeviceInfo() {
        let model = UIDevice.current.model
        let systemVersion = UIDevice.current.systemVersion
        deviceInfo = "端末: \(model)\niOS: \(systemVersion)"
    }

    private func updateDataStatus() {
        var status = ""

        let (dictPath, dictSource) = store.dictPath()
        if let dictPath = dictPath {
            let fileSize = (try? FileManager.default.attributesOfItem(atPath: dictPath))?[.size] as? Int64 ?? 0
            status += "辞書: あり (\(formatBytes(fileSize))) [\(dictSource)]\n"
        } else {
            status += "辞書: なし [\(dictSource)]\n"
        }

        switch DataPaths.modelStatus(appGroupId: store.appGroupId) {
        case .noContainer:
            status += "モデル: なし (a) App Group コンテナが取得できません（「モデルをインポート」でこのアプリ専用の領域に置けます）\n"
            modelFiles = []
        case .noModelsDir(let path):
            status += "モデル: なし (b) \(path) がありません。「モデルをインポート」で .gguf を選んでください\n"
            modelFiles = []
        case .noGGUF(let path):
            status += "モデル: なし (c) \(path) に .gguf がありません。「モデルをインポート」で選んでください\n"
            modelFiles = []
        case .found(let dir, let files, let source):
            let preferred = DataPaths.preferredModelName(appGroupId: store.appGroupId)
            let used = DataPaths.selectModel(
                from: files,
                preferredName: preferred,
                budgetMB: DataPaths.selectionBudgetMB(appGroupId: store.appGroupId)
            )
            status += "モデル: あり (\(files.count) 個) [\(source)]\n  \(dir)\n"
            for file in files {
                status += "  - \(file.name) (\(formatBytes(file.sizeBytes)))\(file.name == used?.name ? " ← llm_load で使用" : "")\n"
            }
            modelFiles = files
            if let used = used, selectedModelName != used.name { selectedModelName = used.name }
        }

        dataStatus = status
        dataReloadToken += 1
    }

    private func canCopyDict() -> Bool {
        // App Group が利用不可の場合はコピーできない
        guard store.containerURL != nil else { return false }
        return DataPaths.bundleDictURL() != nil
    }

    private func copyDictFromBundle() {
        guard let bundleDict = DataPaths.bundleDictURL() else {
            alertMessage = "バンドルに辞書ファイルが見つかりません"
            showingAlert = true
            return
        }

        guard let container = store.containerURL else {
            alertMessage = "App Group コンテナが利用不可"
            showingAlert = true
            return
        }

        let destDir = container.appendingPathComponent("data/dict", isDirectory: true)
        let destPath = destDir.appendingPathComponent("nagi-dict.bin")

        do {
            try FileManager.default.createDirectory(at: destDir, withIntermediateDirectories: true)
            if FileManager.default.fileExists(atPath: destPath.path) {
                try FileManager.default.removeItem(at: destPath)
            }
            try FileManager.default.copyItem(at: bundleDict, to: destPath)
            alertMessage = "辞書をコピーしました"
            showingAlert = true
            updateDataStatus()
        } catch {
            alertMessage = "コピー失敗: \(error.localizedDescription)"
            showingAlert = true
        }
    }

    private func importModel(from url: URL) {
        guard url.pathExtension.lowercased() == "gguf" else {
            alertMessage = "拡張子が .gguf のファイルを選んでください（選択: \(url.lastPathComponent)）"
            showingAlert = true
            return
        }

        // App Group が取れなければ、このアプリ専用の領域に置く（キーボード拡張からは見えない）
        guard let modelsDir = store.containerURL?.appendingPathComponent("data/models", isDirectory: true)
            ?? DataPaths.hostLocalModelsDir() else {
            alertMessage = "モデルの保存先（App Group / Application Support）を取得できません"
            showingAlert = true
            return
        }
        let savedInAppGroup = store.containerURL != nil
        let fileName = url.lastPathComponent
        let destURL = modelsDir.appendingPathComponent(fileName)
        let tempURL = modelsDir.appendingPathComponent(fileName + ".partial")
        let appGroupId = store.appGroupId

        copyingDict = true

        DispatchQueue.global(qos: .userInitiated).async {
            // コピーが終わるまでアクセス権を保持する（false でも読める場合があるので結果で中断しない）
            let accessing = url.startAccessingSecurityScopedResource()
            defer { if accessing { url.stopAccessingSecurityScopedResource() } }

            let fileManager = FileManager.default
            do {
                try fileManager.createDirectory(at: modelsDir, withIntermediateDirectories: true)
                // copyItem はファイル全体をメモリに載せずにコピーする。途中失敗で壊れたモデルを残さないよう一時名に書く
                try? fileManager.removeItem(at: tempURL)
                try fileManager.copyItem(at: url, to: tempURL)
                if fileManager.fileExists(atPath: destURL.path) {
                    _ = try fileManager.replaceItemAt(destURL, withItemAt: tempURL)
                } else {
                    try fileManager.moveItem(at: tempURL, to: destURL)
                }
                let size = (try? fileManager.attributesOfItem(atPath: destURL.path))?[.size] as? Int64 ?? 0

                // コアは辞書が無いと LLM を読み込まないので、辞書も App Group に揃える
                var dictNote = savedInAppGroup
                    ? ""
                    : "\n注意: App Group が使えないため、このアプリ専用の領域に保存しました。キーボード拡張からは見えません"
                if fileName.hasPrefix("ggml-model") {
                    dictNote += "\n注意: HuggingFace の既定名のままです。small と xsmall を両方置く場合は、「ファイル」アプリで zenz-v3.1-small-Q5_K_M.gguf / zenz-v3.1-xsmall-Q5_K_M.gguf に名前を変えてからインポートしてください（同名だと上書きされます）"
                }
                if savedInAppGroup {
                    do {
                        if try !DataPaths.ensureDictInAppGroup(appGroupId: appGroupId) {
                            dictNote += "\n注意: 辞書が App Group にもバンドルにもありません。llm_load には辞書が必要です"
                        }
                    } catch {
                        dictNote += "\n注意: 辞書の配置に失敗: \(error.localizedDescription)"
                    }
                }

                DispatchQueue.main.async {
                    copyingDict = false
                    alertMessage = "モデルをインポートしました: \(fileName) (\(formatBytes(size)))\(dictNote)"
                    showingAlert = true
                    updateDataStatus()
                }
            } catch {
                try? fileManager.removeItem(at: tempURL)
                DispatchQueue.main.async {
                    copyingDict = false
                    alertMessage = "インポート失敗: \(error.localizedDescription)"
                    showingAlert = true
                }
            }
        }
    }

    private func runAllProbesExcludingMemoryRamp() {
        Task {
            probesRunning = true
            defer { probesRunning = false }

            for (probeId, probeRunner) in allProbesExcludingMemoryRamp.sorted(by: { $0.key < $1.key }) {
                let context = ProbeContext(
                    store: store,
                    processType: "app",
                    engine: nil
                )
                let result = await probeRunner(context)
                try? store.append(result: result)
            }
            loadResults()
        }
    }

    private func runMemoryRamp() {
        Task {
            probesRunning = true
            defer { probesRunning = false }

            let context = ProbeContext(
                store: store,
                processType: "app",
                engine: nil
            )
            let result = await MemoryRampProbe(maxLimitMB: 3072).run(context: context)
            try? store.append(result: result)
            loadResults()
        }
    }

    private func loadResults() {
        allResults = store.read(last: 1000)
    }

    private func exportResults() {
        let jsonl = store.exportAsJSONL()
        let pasteboard = UIPasteboard.general
        pasteboard.string = jsonl
        alertMessage = "JSONL をクリップボードにコピーしました"
        showingAlert = true
    }

    private func clearResults() {
        do {
            try store.clearAll()
            allResults = []
            alertMessage = "結果をクリアしました"
            showingAlert = true
        } catch {
            alertMessage = "クリア失敗: \(error.localizedDescription)"
            showingAlert = true
        }
    }

    private func formatBytes(_ bytes: Int64) -> String {
        let formatter = ByteCountFormatter()
        return formatter.string(fromByteCount: bytes)
    }

    private func resultsSummary() -> String {
        let sorted = allResults.sorted { $0.timestamp > $1.timestamp }
        return sorted.map { result in
            let status = result.ok ? "ok" : "ng"
            let metricsStr = result.metrics
                .sorted { $0.key < $1.key }
                .map { "\($0.key)=\(String(format: "%.2f", $0.value))" }
                .joined(separator: ", ")
            return "\(result.id) | \(result.process) | \(status) | \(metricsStr)"
        }.joined(separator: "\n")
    }

    private func resultsFullText() -> String {
        let sorted = allResults.sorted { $0.timestamp > $1.timestamp }
        var lines: [String] = []
        for result in sorted {
            lines.append("[\(result.timestamp.formatted())] \(result.id) (\(result.process))")
            lines.append("  status: \(result.ok ? "ok" : "ng")")
            if !result.metrics.isEmpty {
                let metricsStr = result.metrics
                    .sorted { $0.key < $1.key }
                    .map { "    \($0.key): \(String(format: "%.2f", $0.value))" }
                    .joined(separator: "\n")
                lines.append("  metrics:")
                lines.append(metricsStr)
            }
            if !result.note.isEmpty {
                lines.append("  note: \(result.note)")
            }
            lines.append("")
        }
        return lines.joined(separator: "\n")
    }
}

// MARK: - Probe Detail View

struct ProbeDetailView: View {
    let probeId: String
    @State private var isRunning = false
    @State private var result: ProbeResult?
    @State private var store = SharedStore.shared

    var body: some View {
        List {
            Section("実行") {
                Button("実行") {
                    runProbe()
                }
                .disabled(isRunning)
            }

            if let result = result {
                Section("結果") {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Text("OK").font(.headline)
                            Spacer()
                            Image(systemName: result.ok ? "checkmark.circle.fill" : "xmark.circle.fill")
                                .foregroundColor(result.ok ? .green : .red)
                        }

                        Text("タイムスタンプ: \(result.timestamp.formatted())")
                            .font(.caption)

                        if !result.metrics.isEmpty {
                            Divider()
                            ForEach(Array(result.metrics.sorted { $0.key < $1.key }), id: \.key) { key, value in
                                HStack {
                                    Text(key).font(.caption)
                                    Spacer()
                                    Text(String(format: "%.2f", value)).font(.caption.monospaced())
                                }
                            }
                        }

                        if !result.note.isEmpty {
                            Divider()
                            Text(result.note).font(.caption)
                        }
                    }
                }
            }
        }
        .navigationTitle(probeId)
        .onAppear { loadLatestResult() }
    }

    private func runProbe() {
        isRunning = true
        Task {
            let context = ProbeContext(
                store: store,
                processType: "app",
                engine: nil
            )
            let newResult: ProbeResult

            if probeId == "memory_ramp" {
                newResult = await MemoryRampProbe(maxLimitMB: 3072).run(context: context)
            } else if let probeRunner = allProbes[probeId] {
                newResult = await probeRunner(context)
            } else {
                isRunning = false
                return
            }

            try? store.append(result: newResult)
            result = newResult
            isRunning = false
        }
    }

    private func loadLatestResult() {
        let allResults = store.read(last: 1000)
        result = allResults.last { $0.id == probeId && $0.process == "app" }
    }
}

#Preview {
    ContentView()
}
