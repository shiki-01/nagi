import SwiftUI

struct ContentView: View {
    @State private var selectedTab = 0
    @State private var store = SharedStore.shared
    @State private var deviceInfo = ""
    @State private var dataStatus = ""
    @State private var showingDocumentPicker = false
    @State private var copyingDict = false
    @State private var allResults: [ProbeResult] = []
    @State private var showingAlert = false
    @State private var alertMessage = ""

    var body: some View {
        TabView(selection: $selectedTab) {
            // Tab 1: ホーム・設定
            NavigationStack {
                List {
                    Section("端末情報") {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(deviceInfo).font(.system(.body, design: .monospaced))
                        }
                        .onAppear { updateDeviceInfo() }
                    }

                    Section("App Group") {
                        VStack(alignment: .leading, spacing: 8) {
                            if let container = store.containerURL {
                                Text("✓ 利用可能: \(container.path)").foregroundColor(.green)
                            } else {
                                Text("✗ 利用不可（無料アカウント？）").foregroundColor(.red)
                            }
                        }
                    }

                    Section("データ") {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(dataStatus).font(.system(.body, design: .monospaced))

                            Button("辞書をバンドルからコピー") {
                                copyDictFromBundle()
                            }
                            .disabled(!canCopyDict())

                            if !copyingDict {
                                Button("モデル(GGUF)をファイルから取り込む") {
                                    showingDocumentPicker = true
                                }
                            }
                        }
                        .onAppear { updateDataStatus() }
                    }

                    Section("キーボード設定") {
                        Text("設定 > 一般 > キーボード > 新しいキーボード で NagiSpikeKeyboard を有効化してください")
                            .font(.caption)
                    }
                }
                .navigationTitle("ホーム")
                .fileImporter(
                    isPresented: $showingDocumentPicker,
                    allowedContentTypes: [.init(filenameExtension: "gguf")!],
                    onCompletion: { result in
                        if case .success(let url) = result {
                            importModel(from: url)
                        }
                    }
                )
            }
            .tabItem {
                Label("ホーム", systemImage: "house.fill")
            }
            .tag(0)

            // Tab 2: Probes
            NavigationStack {
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
                        }
                    }

                    Section {
                        Button("すべての Probe を実行") {
                            runAllProbes()
                        }
                        .frame(maxWidth: .infinity)
                    }
                }
                .navigationTitle("Probes")
                .onAppear { loadResults() }
            }
            .tabItem {
                Label("Probes", systemImage: "checkmark.square.fill")
            }
            .tag(1)

            // Tab 3: 結果
            NavigationStack {
                List {
                    Section("すべての結果") {
                        ForEach(allResults.sorted { $0.timestamp > $1.timestamp }, id: \.id) { result in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text("\(result.name) (\(result.process))")
                                        .font(.headline)
                                    Spacer()
                                    Image(systemName: result.ok ? "checkmark.circle.fill" : "xmark.circle.fill")
                                        .foregroundColor(result.ok ? .green : .red)
                                }
                                Text(result.timestamp.formatted())
                                    .font(.caption)
                                    .foregroundColor(.gray)
                                if !result.metrics.isEmpty {
                                    Text("メトリクス: \(result.metrics.count) 個")
                                        .font(.caption)
                                }
                            }
                        }
                    }

                    Section {
                        Button("エクスポート (JSONL)") {
                            exportResults()
                        }

                        Button("すべてクリア", role: .destructive) {
                            clearResults()
                        }
                    }
                }
                .navigationTitle("結果")
                .onAppear { loadResults() }
            }
            .tabItem {
                Label("結果", systemImage: "doc.text.fill")
            }
            .tag(2)
        }
        .alert("メッセージ", isPresented: $showingAlert) {
            Button("OK") { }
        } message: {
            Text(alertMessage)
        }
    }

    private func updateDeviceInfo() {
        let model = UIDevice.current.model
        let systemVersion = UIDevice.current.systemVersion
        deviceInfo = "端末: \(model)\niOS: \(systemVersion)"
    }

    private func updateDataStatus() {
        var status = ""

        if let dictPath = store.dictPath() {
            let fileSize = (try? FileManager.default.attributesOfItem(atPath: dictPath))?[.size] as? Int64 ?? 0
            status += "辞書: あり (\(formatBytes(fileSize)))\n"
        } else {
            status += "辞書: なし\n"
        }

        if let modelsDir = store.modelsDir() {
            let files = try? FileManager.default.contentsOfDirectory(atPath: modelsDir)
            let ggufFiles = files?.filter { $0.hasSuffix(".gguf") } ?? []
            if !ggufFiles.isEmpty {
                status += "モデル: あり (\(ggufFiles.count) 個)\n"
            } else {
                status += "モデル: なし\n"
            }
        } else {
            status += "モデル: なし\n"
        }

        dataStatus = status
    }

    private func canCopyDict() -> Bool {
        if let bundleDict = Bundle.main.url(forResource: "nagi-dict", withExtension: "bin") {
            return FileManager.default.fileExists(atPath: bundleDict.path)
        }
        return false
    }

    private func copyDictFromBundle() {
        guard let bundleDict = Bundle.main.url(forResource: "nagi-dict", withExtension: "bin") else {
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
        guard url.startAccessingSecurityScopedResource() else { return }
        defer { url.stopAccessingSecurityScopedResource() }

        guard let container = store.containerURL else {
            alertMessage = "App Group コンテナが利用不可"
            showingAlert = true
            return
        }

        let modelsDir = container.appendingPathComponent("data/models", isDirectory: true)
        let fileName = url.lastPathComponent
        let destPath = modelsDir.appendingPathComponent(fileName)

        copyingDict = true

        DispatchQueue.global().async {
            do {
                try FileManager.default.createDirectory(at: modelsDir, withIntermediateDirectories: true)
                try FileManager.default.copyItem(at: url, to: destPath)
                DispatchQueue.main.async {
                    copyingDict = false
                    alertMessage = "モデルをインポートしました: \(fileName)"
                    showingAlert = true
                    updateDataStatus()
                }
            } catch {
                DispatchQueue.main.async {
                    copyingDict = false
                    alertMessage = "インポート失敗: \(error.localizedDescription)"
                    showingAlert = true
                }
            }
        }
    }

    private func runAllProbes() {
        Task {
            for (probeId, probeRunner) in allProbes {
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
                                    Text(String(format: "%.2f", value)).font(.caption).monospaced()
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
            if let probeRunner = allProbes[probeId] {
                let context = ProbeContext(
                    store: store,
                    processType: "app",
                    engine: nil
                )
                let newResult = await probeRunner(context)
                try? store.append(result: newResult)
                result = newResult
            }
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
