import Foundation
import UIKit

// MARK: - Serial Execution Queue

/// グローバルなシリアルキュー（全プローブの直列実行を保証）
private let probeSerialQueue = DispatchQueue(label: "com.shiki.nagi.probe-serial")

/// プローブ実行状態を管理するアクター（並行実行を防止）
actor ProbeExecutionManager {
    private static let shared = ProbeExecutionManager()
    private var isRunning = false

    static func getInstance() -> ProbeExecutionManager {
        Self.shared
    }

    func acquireLock() -> Bool {
        guard !isRunning else { return false }
        isRunning = true
        return true
    }

    func releaseLock() {
        isRunning = false
    }
}

// MARK: - Probe Protocol

protocol ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult
}

struct ProbeContext {
    let store: SharedStore
    let processType: String // "app" or "keyboard"
    let engine: NagiEngine?
}

// MARK: - Utility Functions

private func getDeviceModel() -> String {
    var systemInfo = utsname()
    uname(&systemInfo)
    let model = withUnsafeBytes(of: &systemInfo.machine) { raw in
        String(decoding: raw.prefix(while: { $0 != 0 }), as: UTF8.self)
    }
    return model.isEmpty ? "Unknown" : model
}

private func getOSVersion() -> String {
    "\(UIDevice.current.systemVersion)"
}

// MARK: - 1. AppGroup Probe

struct AppGroupProbe: ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "appgroup"
        let name = "App Group"
        var metrics: [String: Double] = [:]
        var note = ""
        var ok = false

        let resolution = context.store.appGroupResolution

        // 解決結果を note に記録
        var noteLines: [String] = []
        noteLines.append("Candidates: \(resolution.candidates.joined(separator: ", "))")
        if !resolution.provisionGroups.isEmpty {
            noteLines.append("Provision groups: \(resolution.provisionGroups.joined(separator: ", "))")
        }
        if !resolution.altGroups.isEmpty {
            noteLines.append("ALT groups: \(resolution.altGroups.joined(separator: ", "))")
        }
        if let resolvedID = resolution.resolvedID {
            noteLines.append("Resolved ID: \(resolvedID)")
        } else {
            noteLines.append("Resolved ID: (not available)")
        }
        noteLines.append("Provision readable: \(resolution.provisionReadable)")

        // メトリクスに結果を記録
        metrics["resolved"] = resolution.resolvedID != nil ? 1 : 0
        metrics["candidate_count"] = Double(resolution.candidates.count)

        guard let container = context.store.containerURL else {
            note = noteLines.joined(separator: "\n")
            note += "\n\nI/O test: App Group コンテナが利用不可"
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        // テストファイルを書き込み
        let testFileName = "probe_appgroup_\(UUID().uuidString).txt"
        let testURL = container.appendingPathComponent(testFileName)
        let testContent = "test_\(context.processType)_\(Date().timeIntervalSince1970)"

        do {
            try testContent.write(to: testURL, atomically: true, encoding: .utf8)

            // 読み戻す
            let readContent = try String(contentsOf: testURL, encoding: .utf8)
            if readContent == testContent {
                ok = true
                noteLines.append("\nI/O test: OK")

                // 相手プロセスの最新結果を表示
                if let latest = context.store.latestFromOtherProcess(currentProcess: context.processType) {
                    noteLines.append("Other process (\(latest.process)): \(latest.timestamp.formatted())")
                }
            } else {
                noteLines.append("\nI/O test: Content mismatch")
            }

            // テストファイルを削除
            try? FileManager.default.removeItem(at: testURL)
        } catch {
            noteLines.append("\nI/O test: Error - \(error.localizedDescription)")
        }

        note = noteLines.joined(separator: "\n")
        return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 2. Baseline Memory Probe

struct BaselineMemoryProbe: ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "baseline_memory"
        let name = "Baseline Memory"
        let stats = MemoryProbe.getStats()

        let metrics: [String: Double] = [
            "phys_footprint_mb": stats.physFootprintMB,
            "available_memory_mb": stats.availableMemoryMB
        ]

        let note = "何もロードしない状態のメモリ使用量"
        return makeResult(id: id, name: name, ok: true, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 3. Dict Load Probe

struct DictLoadProbe: ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "dict_load"
        let name = "Dict Load"
        var metrics: [String: Double] = [:]
        var note = ""
        var ok = false

        let (dictPath, dataDirSource) = context.store.dictPath()
        guard let dictPath = dictPath else {
            note = "辞書ファイルが見つかりません。バンドルまたはコンテナの data/dict/nagi-dict.bin をチェックしてください（source: \(dataDirSource)）"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let (dataDir, _) = context.store.dataDir()
        guard let dataDir = dataDir else {
            note = "data ディレクトリが見つかりません"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        // 1秒待機（余計な確保を避ける）
        try? await Task.sleep(nanoseconds: 1_000_000_000)

        let baselineStats = MemoryProbe.getStats()
        let startTime = Date()

        guard let engine = NagiEngine(dataDir: dataDir, userDir: nil, noLLM: true) else {
            note = "NagiEngine の生成に失敗（C ABI エラー）"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let endTime = Date()
        let afterStats = MemoryProbe.getStats()

        let elapsedMs = endTime.timeIntervalSince(startTime) * 1000.0
        let footprintDiffMB = afterStats.physFootprintMB - baselineStats.physFootprintMB

        metrics = [
            "load_time_ms": elapsedMs,
            "footprint_increase_mb": footprintDiffMB,
            "footprint_after_mb": afterStats.physFootprintMB,
            "footprint_since_launch_mb": afterStats.physFootprintMB - LaunchBaseline.footprintMB,
            "launch_footprint_mb": LaunchBaseline.footprintMB,
            "data_dir_source": dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
        ]

        // 辞書状態を notes から抽出
        let engineNotes = engine.notes
        var dictStatus = "不明"
        if engineNotes.contains("辞書: OK") {
            dictStatus = "ロード済み"
        } else if engineNotes.contains("未指定") {
            dictStatus = "未指定"
        } else if engineNotes.contains("読みをそのまま") {
            dictStatus = "ロード失敗"
        }

        // 変換テスト（"konnichiha" を1文字ずつ）
        var conversionOk = false
        let initReq = """
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-ios-spike","client_version":"0.1","os":"ios"}}
        """
        _ = engine.send(line: initReq)

        let testString = "konnichiha"
        for char in testString {
            let charNum = UInt32(char.unicodeScalars.first?.value ?? 0)
            let keyReq = """
            {"jsonrpc":"2.0","id":2,"method":"process_key","params":{"session_id":"dict_test","keysym":\(charNum),"keycode":0,"text":"\(char)","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
            """
            _ = engine.send(line: keyReq)
        }

        // Space キーで変換候補を取得
        let spaceReq = """
        {"jsonrpc":"2.0","id":3,"method":"process_key","params":{"session_id":"dict_test","keysym":32,"keycode":49,"text":" ","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
        """
        let spaceResponse = engine.send(line: spaceReq)
        if let response = spaceResponse.first {
            if let surface = extractFirstCandidateSurface(from: response) {
                if surface != "こんにちは" {  // ひらがなではなく漢字変換されているか
                    conversionOk = true
                    metrics["conversion_ok"] = 1
                } else {
                    metrics["conversion_ok"] = 0
                }
            }
        }

        ok = true
        note = "辞書状態: \(dictStatus) | 変換: \(conversionOk ? "OK" : "未検証") | \(engineNotes.split(separator: "\n").first.map(String.init) ?? "") (source: \(dataDirSource))"

        return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 4. Latency Probe

struct LatencyProbe: ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "latency"
        let name = "Latency"
        var metrics: [String: Double] = [:]
        var note = ""
        var ok = false

        guard let engine = createEngine(context: context) else {
            let (_, dataDirSource) = context.store.dataDir()
            note = "エンジン生成失敗"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let (_, dataDirSource) = context.store.dataDir()

        var latencies: [Double] = []
        var spaceLatencies: [Double] = []
        var dictStatus = ""
        var conversionOk = false

        for i in 0..<200 {
            let sessionId = "session_\(i)"

            // initialize
            let initReq = """
            {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-ios-spike","client_version":"0.1","os":"ios"}}
            """
            _ = engine.send(line: initReq)

            let testString = "konnichiha"
            for char in testString {
                let charNum = UInt32(char.unicodeScalars.first?.value ?? 0)
                let keyReq = """
                {"jsonrpc":"2.0","id":2,"method":"process_key","params":{"session_id":"\(sessionId)","keysym":\(charNum),"keycode":0,"text":"\(char)","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
                """

                let startTime = Date()
                let response = engine.send(line: keyReq)
                let elapsed = Date().timeIntervalSince(startTime) * 1000.0
                latencies.append(elapsed)

                // 最初の1回だけ辞書状態を確認
                if i == 0 && dictStatus.isEmpty {
                    let engineNotes = engine.notes
                    if engineNotes.contains("辞書: OK") {
                        dictStatus = "loaded"
                    } else if engineNotes.contains("読みをそのまま") {
                        dictStatus = "failed"
                    }
                }
            }

            // Space キー
            let spaceReq = """
            {"jsonrpc":"2.0","id":3,"method":"process_key","params":{"session_id":"\(sessionId)","keysym":32,"keycode":49,"text":" ","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
            """
            let spaceStart = Date()
            let spaceResponse = engine.send(line: spaceReq)
            let spaceElapsed = Date().timeIntervalSince(spaceStart) * 1000.0
            spaceLatencies.append(spaceElapsed)

            // 最初の1回だけ変換結果を確認
            if i == 0, let response = spaceResponse.first {
                if let surface = extractFirstCandidateSurface(from: response) {
                    if surface != "こんにちは" {
                        conversionOk = true
                    }
                }
            }

            // Enter
            let enterReq = """
            {"jsonrpc":"2.0","id":4,"method":"process_key","params":{"session_id":"\(sessionId)","keysym":65293,"keycode":36,"text":"","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
            """
            _ = engine.send(line: enterReq)
        }

        if !latencies.isEmpty {
            latencies.sort()
            let p50 = latencies[latencies.count / 2]
            let p95 = latencies[Int(Double(latencies.count) * 0.95)]
            let maxLatency = latencies.last ?? 0

            metrics["hiragana_p50_ms"] = p50
            metrics["hiragana_p95_ms"] = p95
            metrics["hiragana_max_ms"] = maxLatency
        }

        if !spaceLatencies.isEmpty {
            spaceLatencies.sort()
            let p50 = spaceLatencies[spaceLatencies.count / 2]
            let p95 = spaceLatencies[Int(Double(spaceLatencies.count) * 0.95)]
            let maxLatency = spaceLatencies.last ?? 0

            metrics["space_p50_ms"] = p50
            metrics["space_p95_ms"] = p95
            metrics["space_max_ms"] = maxLatency
        }

        metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
        metrics["conversion_ok"] = conversionOk ? 1 : 0

        ok = !latencies.isEmpty
        note = "辞書: \(dictStatus.isEmpty ? "?" : dictStatus) | 変換: \(conversionOk ? "OK" : "NG?") | 目標: ひらがな p95 < 16ms, Space p95 < 150ms (source: \(dataDirSource))"

        return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 5. Memory Ramp Probe

struct MemoryRampProbe: ProbeRunner {
    let maxLimitMB: Int  // ホストアプリ: 3072, キーボード拡張: 1024

    init(maxLimitMB: Int = 3072) {
        self.maxLimitMB = maxLimitMB
    }

    func run(context: ProbeContext) async -> ProbeResult {
        let id = "memory_ramp"
        let name = "Memory Ramp"
        var metrics: [String: Double] = [:]
        var note = "メモリ段階的確保。上限: \(maxLimitMB)MB（強制終了対応テスト。再起動後に最後の記録値を確認可能）"
        if context.processType == "app" {
            note = "ホストアプリで実行中。拡張の上限を測るものではありません。\n" + note
        }
        var ok = false

        let (_, dataDirSource) = context.store.dataDir()
        let startStats = MemoryProbe.getStats()
        metrics["start_available_mb"] = startStats.availableMemoryMB
        metrics["start_footprint_mb"] = startStats.physFootprintMB
        metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)

        let chunkSizeMB = 8
        var allocations: [Data] = []
        var maxReachedMB = 0

        do {
            try context.store.append(result: ProbeResult(
                id: "memory_ramp_started",
                name: "Memory Ramp Started",
                process: context.processType,
                ok: true,
                metrics: [:],
                note: "メモリランプテスト開始",
                timestamp: Date(),
                deviceModel: getDeviceModel(),
                osVersion: getOSVersion()
            ))
        } catch {
            note += "\n進捗記録に失敗: \(error.localizedDescription)"
        }

        while maxReachedMB < maxLimitMB {
            do {
                // 進捗を記録する前にメモリ測定
                let beforeStats = MemoryProbe.getStats()

                // 進捗を同期的に記録
                let progressReq = ProbeResult(
                    id: "memory_ramp_progress",
                    name: "Memory Ramp Progress",
                    process: context.processType,
                    ok: true,
                    metrics: ["reached_mb": Double(maxReachedMB), "footprint_mb": beforeStats.physFootprintMB],
                    note: "到達: \(maxReachedMB) MB",
                    timestamp: Date(),
                    deviceModel: getDeviceModel(),
                    osVersion: getOSVersion()
                )
                try context.store.append(result: progressReq)

                // メモリ確保（全ページに書き込み）
                var data = Data(count: chunkSizeMB * 1024 * 1024)
                data.withUnsafeMutableBytes { buffer in
                    for i in stride(from: 0, to: buffer.count, by: 4096) {
                        buffer.storeBytes(of: UInt8(i % 256), toByteOffset: i, as: UInt8.self)
                    }
                }
                allocations.append(data)
                maxReachedMB += chunkSizeMB

                // 少し待機
                try await Task.sleep(nanoseconds: 100_000_000) // 100ms
            } catch {
                note += "\nメモリ確保終了: \(error.localizedDescription)"
                break
            }
        }

        let endStats = MemoryProbe.getStats()
        metrics["end_footprint_mb"] = endStats.physFootprintMB
        metrics["end_available_mb"] = endStats.availableMemoryMB
        metrics["max_reached_mb"] = Double(maxReachedMB)

        ok = true
        return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 6. Coldstart Probe

struct ColdstartProbe: ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "coldstart"
        let name = "Coldstart"
        var metrics: [String: Double] = [:]
        var note = ""
        var ok = false

        let (dictPath, dataDirSource) = context.store.dictPath()
        guard let dictPath = dictPath else {
            note = "辞書ファイルが見つかりません（source: \(dataDirSource)）"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let startTime = Date()
        let (dataDir, _) = context.store.dataDir()
        guard let dataDir = dataDir else {
            note = "data ディレクトリが見つかりません"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        guard let engine = NagiEngine(dataDir: dataDir, userDir: nil, noLLM: true) else {
            note = "エンジン生成失敗"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let initReq = """
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-ios-spike","client_version":"0.1","os":"ios"}}
        """
        let keyReq = """
        {"jsonrpc":"2.0","id":2,"method":"process_key","params":{"session_id":"coldstart","keysym":97,"keycode":0,"text":"a","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
        """

        _ = engine.send(line: initReq)
        let keyStart = Date()
        let keyResponse = engine.send(line: keyReq)
        let keyElapsed = Date().timeIntervalSince(keyStart) * 1000.0

        let totalElapsed = Date().timeIntervalSince(startTime) * 1000.0

        metrics["engine_creation_ms"] = totalElapsed - keyElapsed
        metrics["first_key_ms"] = keyElapsed
        metrics["total_ms"] = totalElapsed
        metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)

        // 辞書状態確認
        let engineNotes = engine.notes
        var dictStatus = "?"
        if engineNotes.contains("辞書: OK") {
            dictStatus = "OK"
        } else if engineNotes.contains("読みをそのまま") {
            dictStatus = "NG"
        }

        // 変換確認
        var conversionOk = false
        if let response = keyResponse.first, let surface = extractFirstCandidateSurface(from: response) {
            if surface == "あ" {
                conversionOk = true
            }
        }

        ok = true
        note = "辞書: \(dictStatus) | 変換: \(conversionOk ? "OK" : "NG?") | 目標: < 2000ms（light は < 300ms）(source: \(dataDirSource))"

        return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 7. LLM Load Probe

struct LLMLoadProbe: ProbeRunner {
    /// Probe の ID（`llm_load` / `llm_load_light`）
    var probeId = "llm_load"
    /// 固定する環境プロファイル（`balanced` / `light`）。連続変換の応答時間とメモリが変わる
    var profile = "balanced"
    /// LLM の推論を同期で待つ最長時間（ms）。超えた分は辞書の結果で先に応答し、完了を非同期通知で差し替える
    var syncBudgetMs = 30

    func run(context: ProbeContext) async -> ProbeResult {
        let id = probeId
        let name = "LLM Load (\(profile))"
        var metrics: [String: Double] = [:]
        var note = ""
        var ok = false

        #if NAGI_LLM
        let appGroupId = context.store.appGroupId
        let runner = context.processType == "keyboard" ? "keyboard（拡張）" : "app（ホスト）"

        // 1. モデルの配置状況を原因別に判定する
        let modelsDirHint = "ホストアプリのホーム画面「モデルをインポート」で .gguf を選んでください"
            + "（\(DataPaths.defaultModelFileName) 約70MB、軽量版 zenz-v3.1-xsmall-Q5_K_M.gguf 約20MB）"
        let found: (dir: String, files: [DataPaths.ModelFile], source: String)
        switch DataPaths.modelStatus(appGroupId: appGroupId) {
        case .noContainer:
            note = "(a) App Group コンテナが取得できません（appgroup プローブで Resolved ID を確認。"
                + "SideStore で再署名した後に App Group が無効になっていないか確認してください）"
            return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
        case .noModelsDir(let path):
            note = "(b) App Group コンテナはありますが \(path) がありません（モデル未インポート）。\(modelsDirHint)"
            return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
        case .noGGUF(let path):
            note = "(c) \(path) に .gguf がありません。\(modelsDirHint)"
            return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
        case .found(let dir, let files, let source):
            found = (dir, files, source)
        }

        guard let model = DataPaths.selectModel(
            from: found.files,
            preferredName: DataPaths.preferredModelName(appGroupId: appGroupId)
        ) else {
            note = "(c) .gguf を選べませんでした"
            return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
        }
        let modelSizeMB = Double(model.sizeBytes) / 1024.0 / 1024.0
        let modelDesc = "\(model.name) (\(String(format: "%.1f", modelSizeMB)) MB, source: \(found.source), 配置数: \(found.files.count))"
        metrics["model_size_mb"] = modelSizeMB
        metrics["data_dir_source"] = found.source == "appgroup" ? 1 : 2

        // 2. data ディレクトリ（コアは辞書が読めないと LLM を読み込まない）。モデルの親が data/
        let dataDir = URL(fileURLWithPath: found.dir).deletingLastPathComponent().path
        if found.source == "appgroup" {
            do {
                if try !DataPaths.ensureDictInAppGroup(appGroupId: appGroupId) {
                    note = "(d) 辞書がありません（\(dataDir)/dict/nagi-dict.bin もバンドルにも無い）。LLM は辞書が無いと読み込まれません。"
                        + "ホストアプリの「辞書をバンドルからコピー」を実行してください [\(modelDesc)]"
                    return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
                }
            } catch {
                note = "(d) 辞書の配置に失敗: \(error.localizedDescription) [\(modelDesc)]"
                return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
            }
        }

        // 3. コアの既定は固定ファイル名なので、選んだモデルを絶対パスで config.toml に指定する
        let probeUserDir = FileManager.default.temporaryDirectory.appendingPathComponent("nagi-llm-probe", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: probeUserDir, withIntermediateDirectories: true)
            // プロファイルを固定する。自動切替だと、最初の変換が遅いと minimal に落ちて、以降の変換で
            // LLM が動かず、推論中のメモリが測れない（連続変換の計測を LLM 有効のまま行うため）
            let config = "[conversion]\nmodel = '\(model.path)'\nsync_budget_ms = \(syncBudgetMs)\n\n[profile]\nmode = \"\(profile)\"\nauto_switch = false\n"
            try config.write(to: probeUserDir.appendingPathComponent("config.toml"), atomically: true, encoding: .utf8)
        } catch {
            note = "(d) 設定ファイルの作成に失敗: \(error.localizedDescription) [\(modelDesc)]"
            return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
        }

        // ロード中に強制終了された場合に原因を追えるよう、開始を先に記録する
        do {
            try context.store.append(result: ProbeResult(
                id: "llm_load_started",
                name: "LLM Load Started",
                process: context.processType,
                ok: true,
                metrics: ["model_size_mb": modelSizeMB],
                note: "LLM ロード開始 実行元: \(runner) モデル: \(modelDesc)",
                timestamp: Date(),
                deviceModel: getDeviceModel(),
                osVersion: getOSVersion()
            ))
        } catch {
            // 記録失敗はプローブ自体の失敗にしない
        }

        let baselineStats = MemoryProbe.getStats()
        let startTime = Date()

        guard let engine = NagiEngine(dataDir: dataDir, userDir: probeUserDir.path, noLLM: false) else {
            note = "(d) エンジン生成失敗（nagi_server_new が NULL）[\(modelDesc)]"
            return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
        }

        let loadTime = Date().timeIntervalSince(startTime) * 1000.0
        let afterStats = MemoryProbe.getStats()

        metrics["load_time_ms"] = loadTime
        metrics["footprint_increase_mb"] = afterStats.physFootprintMB - baselineStats.physFootprintMB
        metrics["footprint_after_mb"] = afterStats.physFootprintMB
        metrics["available_after_mb"] = afterStats.availableMemoryMB

        // エンジンは LLM のロードに失敗しても辞書のみで起動する。notes で成否を判定する
        let engineNotes = engine.notes
        let failureLines = engineNotes.split(separator: "\n").filter {
            $0.contains("辞書のみで起動します") || $0.contains("読みをそのまま")
        }
        if !failureLines.isEmpty {
            note = "(d) LLM のロードに失敗（コアの応答: \(failureLines.joined(separator: " / "))）"
                + " 実行元: \(runner) モデル: \(modelDesc)"
            return makeResult(id: id, name: name, ok: false, metrics: metrics, note: note, process: context.processType)
        }

        // Space 変換テスト（ローマ字を打ってから Space）
        let initReq = """
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-ios-spike","client_version":"0.1","os":"ios"}}
        """
        _ = engine.send(line: initReq)

        var nextId = 2
        for ch in "kyouhaiitenkidesune" {
            let code = ch.asciiValue.map { Int($0) } ?? 0
            let req = "{\"jsonrpc\":\"2.0\",\"id\":\(nextId),\"method\":\"process_key\",\"params\":{\"session_id\":\"llm_test\",\"keysym\":\(code),\"keycode\":0,\"text\":\"\(ch)\",\"modifiers\":{\"shift\":false,\"ctrl\":false,\"alt\":false,\"meta\":false},\"kind\":\"down\"}}"
            _ = engine.send(line: req)
            nextId += 1
        }

        let spaceStart = Date()
        let spaceReq = """
        {"jsonrpc":"2.0","id":\(nextId),"method":"process_key","params":{"session_id":"llm_test","keysym":32,"keycode":49,"text":" ","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
        """
        let spaceResponse = engine.send(line: spaceReq)
        metrics["first_space_ms"] = Date().timeIntervalSince(spaceStart) * 1000.0
        let surface = spaceResponse.compactMap { extractFirstCandidateSurface(from: $0) }.first ?? "(候補なし)"

        // 連続変換（実使用に近い負荷）。推論中のメモリ増加と Space 応答時間を見る。
        // 途中で強制終了された場合は、拡張の診断ログ（前回 llm_load の実行中に終了）で分かる
        func keyRequest(id: Int, session: String, keysym: Int, keycode: Int, text: String) -> String {
            "{\"jsonrpc\":\"2.0\",\"id\":\(id),\"method\":\"process_key\",\"params\":{\"session_id\":\"\(session)\",\"keysym\":\(keysym),\"keycode\":\(keycode),\"text\":\"\(text)\",\"modifiers\":{\"shift\":false,\"ctrl\":false,\"alt\":false,\"meta\":false},\"kind\":\"down\"}}"
        }
        let loopSentences = [
            "kyouhaiitenkidesune",
            "watashihagakkoudeheikouwosuru",
            "konnitiwasekai",
            "asitanoyoteiwonaosu",
            "nihongonohenkanwotesutosimasu"
        ]
        let loopRounds = 20
        // 同期の待ち時間を超えた推論の完了通知（update_preedit / update_candidates）が届くかを数える
        let updateCounter = NotificationCounter()
        engine.startNotifications { lines in
            updateCounter.add(lines.filter {
                $0.contains("\"update_preedit\"") || $0.contains("\"update_candidates\"")
            }.count)
        }
        var loopSpaceTimes: [Double] = []
        var peakFootprint = afterStats.physFootprintMB
        var minAvailable = afterStats.availableMemoryMB
        for round in 0..<loopRounds {
            let session = "llm_loop_\(round)"
            for ch in loopSentences[round % loopSentences.count] {
                let code = ch.asciiValue.map { Int($0) } ?? 0
                nextId += 1
                _ = engine.send(line: keyRequest(id: nextId, session: session, keysym: code, keycode: 0, text: String(ch)))
            }
            nextId += 1
            let start = Date()
            _ = engine.send(line: keyRequest(id: nextId, session: session, keysym: 32, keycode: 49, text: " "))
            loopSpaceTimes.append(Date().timeIntervalSince(start) * 1000.0)
            nextId += 1
            _ = engine.send(line: keyRequest(id: nextId, session: session, keysym: 65293, keycode: 36, text: ""))

            let stats = MemoryProbe.getStats()
            peakFootprint = max(peakFootprint, stats.physFootprintMB)
            minAvailable = min(minAvailable, stats.availableMemoryMB)
        }
        // 入力を続けない変換を 1 回行い、同期の待ち時間を超えた推論の完了が非同期通知で届くかを確かめる。
        // ループ中は直後の次のキー入力・確定で変換が変わるため、差し替えは意図的に捨てられる（通知は 0 件が正常）
        // （ループで使った文は結果がキャッシュされていて同期で返るため、別の文を使う）
        let countBeforeTail = updateCounter.count
        for ch in "watashinonamaehananagidesu" {
            let code = ch.asciiValue.map { Int($0) } ?? 0
            nextId += 1
            _ = engine.send(line: keyRequest(id: nextId, session: "llm_tail", keysym: code, keycode: 0, text: String(ch)))
        }
        nextId += 1
        let tailStart = Date()
        _ = engine.send(line: keyRequest(id: nextId, session: "llm_tail", keysym: 32, keycode: 49, text: " "))
        metrics["tail_space_ms"] = Date().timeIntervalSince(tailStart) * 1000.0
        var tailArrivalMs = -1.0
        while Date().timeIntervalSince(tailStart) < 10 {
            try? await Task.sleep(nanoseconds: 50_000_000)
            if updateCounter.count > countBeforeTail {
                tailArrivalMs = Date().timeIntervalSince(tailStart) * 1000.0
                break
            }
        }
        metrics["tail_async_arrival_ms"] = tailArrivalMs
        let waitedMs = Int(Date().timeIntervalSince(tailStart) * 1000.0)
        engine.stopNotifications()
        let afterWait = MemoryProbe.getStats()
        peakFootprint = max(peakFootprint, afterWait.physFootprintMB)
        minAvailable = min(minAvailable, afterWait.availableMemoryMB)
        metrics["loop_async_updates"] = Double(updateCounter.count)
        metrics["loop_async_wait_ms"] = Double(waitedMs)
        loopSpaceTimes.sort()
        metrics["loop_rounds"] = Double(loopRounds)
        metrics["loop_space_p50_ms"] = loopSpaceTimes[loopSpaceTimes.count / 2]
        metrics["loop_space_p95_ms"] = loopSpaceTimes[min(loopSpaceTimes.count - 1, Int(Double(loopSpaceTimes.count) * 0.95))]
        metrics["loop_space_max_ms"] = loopSpaceTimes.last ?? 0
        metrics["loop_footprint_peak_mb"] = peakFootprint
        let profileLine = engine.notes.split(separator: "\n").last.map(String.init) ?? ""
        metrics["loop_available_min_mb"] = minAvailable

        ok = true
        note = "LLM ロード成功 実行元: \(runner) モデル: \(modelDesc) | 変換結果: kyouhaiitenkidesune → \(surface) | \(profileLine)"
        #else
        note = "LLM 非対応ビルド（NAGI_LLM フラグが無い）"
        ok = false
        #endif

        return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 8. Thermal Battery Probe

struct ThermalBatteryProbe: ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "thermal_battery"
        let name = "Thermal & Battery"

        let thermalState = ProcessInfo.processInfo.thermalState
        let thermalStr: String
        switch thermalState {
        case .nominal:
            thermalStr = "nominal"
        case .fair:
            thermalStr = "fair"
        case .serious:
            thermalStr = "serious"
        case .critical:
            thermalStr = "critical"
        @unknown default:
            thermalStr = "unknown"
        }

        let lowPowerMode = ProcessInfo.processInfo.isLowPowerModeEnabled
        let device = UIDevice.current
        device.isBatteryMonitoringEnabled = true
        let batteryLevel = device.batteryLevel
        let batteryState = device.batteryState

        let batteryStateStr: String
        switch batteryState {
        case .unknown:
            batteryStateStr = "unknown"
        case .unplugged:
            batteryStateStr = "unplugged"
        case .charging:
            batteryStateStr = "charging"
        case .full:
            batteryStateStr = "full"
        @unknown default:
            batteryStateStr = "unknown"
        }

        let metrics: [String: Double] = [
            "battery_level": Double(batteryLevel)
        ]

        let note = "ThermalState: \(thermalStr), LowPowerMode: \(lowPowerMode), BatteryState: \(batteryStateStr)"

        return makeResult(id: id, name: name, ok: true, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - Helper Functions

private func createEngine(context: ProbeContext) -> NagiEngine? {
    let (dataDir, _) = context.store.dataDir()
    guard let dataDir = dataDir else { return nil }
    return NagiEngine(dataDir: dataDir, userDir: nil, noLLM: true)
}

/// JSON-RPC レスポンスの最初の候補の surface（表示文字列）を抽出
private func extractFirstCandidateSurface(from jsonLine: String) -> String? {
    guard let data = jsonLine.data(using: .utf8),
          let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let result = json["result"] as? [String: Any],
          let candidates = result["candidates"] as? [String: Any],
          let items = candidates["items"] as? [[String: Any]],
          let firstCandidate = items.first,
          let surface = firstCandidate["surface"] as? String else {
        // 候補が無い場合（Composing 中の応答など）は preedit.text を返す
        if let data = jsonLine.data(using: .utf8),
           let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let result = json["result"] as? [String: Any],
           let preedit = result["preedit"] as? [String: Any],
           let text = preedit["text"] as? String {
            return text
        }
        return nil
    }
    return surface
}

private func makeResult(id: String, name: String, ok: Bool, metrics: [String: Double], note: String, process: String) -> ProbeResult {
    ProbeResult(
        id: id,
        name: name,
        process: process,
        ok: ok,
        metrics: metrics,
        note: note,
        timestamp: Date(),
        deviceModel: getDeviceModel(),
        osVersion: getOSVersion()
    )
}

// MARK: - All Probes

let allProbes: [String: (ProbeContext) async -> ProbeResult] = [
    "appgroup": { await AppGroupProbe().run(context: $0) },
    "baseline_memory": { await BaselineMemoryProbe().run(context: $0) },
    "dict_load": { await DictLoadProbe().run(context: $0) },
    "latency": { await LatencyProbe().run(context: $0) },
    "memory_ramp": { await MemoryRampProbe(maxLimitMB: $0.processType == "keyboard" ? 1024 : 3072).run(context: $0) },
    "coldstart": { await ColdstartProbe().run(context: $0) },
    "llm_load": { await LLMLoadProbe().run(context: $0) },
    "llm_load_light": { await LLMLoadProbe(probeId: "llm_load_light", profile: "light").run(context: $0) },
    "thermal_battery": { await ThermalBatteryProbe().run(context: $0) }
]

/// 全プローブから memory_ramp を除外（単独実行のみ）
let allProbesExcludingMemoryRamp: [String: (ProbeContext) async -> ProbeResult] = [
    "appgroup": allProbes["appgroup"]!,
    "baseline_memory": allProbes["baseline_memory"]!,
    "dict_load": allProbes["dict_load"]!,
    "latency": allProbes["latency"]!,
    "coldstart": allProbes["coldstart"]!,
    "llm_load": allProbes["llm_load"]!,
    "llm_load_light": allProbes["llm_load_light"]!,
    "thermal_battery": allProbes["thermal_battery"]!
]

/// 通知の件数を別スレッドから数える（`NagiEngine.startNotifications` のハンドラ用）
final class NotificationCounter {
    private let lock = NSLock()
    private var value = 0

    func add(_ n: Int) {
        lock.lock()
        value += n
        lock.unlock()
    }

    var count: Int {
        lock.lock()
        defer { lock.unlock() }
        return value
    }
}
