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

        guard let container = context.store.containerURL else {
            note = "App Group コンテナが利用不可（無料アカウント？）"
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
                note = "App Group コンテナが正常に動作"

                // 相手プロセスの最新結果を表示
                if let latest = context.store.latestFromOtherProcess(currentProcess: context.processType) {
                    note += "\n相手プロセス(\(latest.process)): \(latest.timestamp.formatted())"
                }
            } else {
                note = "読み戻した内容が異なる"
            }

            // テストファイルを削除
            try? FileManager.default.removeItem(at: testURL)
        } catch {
            note = "ファイル I/O エラー: \(error.localizedDescription)"
        }

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
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "llm_load"
        let name = "LLM Load"
        var metrics: [String: Double] = [:]
        var note = ""
        var ok = false

        #if NAGI_LLM
        let (modelsDir, dataDirSource) = context.store.modelsDir()
        guard let modelsDir = modelsDir else {
            note = "モデルディレクトリが見つかりません（source: \(dataDirSource)）"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        do {
            try context.store.append(result: ProbeResult(
                id: "llm_load_started",
                name: "LLM Load Started",
                process: context.processType,
                ok: true,
                metrics: [:],
                note: "LLM ロード開始",
                timestamp: Date(),
                deviceModel: getDeviceModel(),
                osVersion: getOSVersion()
            ))
        } catch {
            note = "進捗記録失敗"
        }

        let baselineStats = MemoryProbe.getStats()
        let startTime = Date()

        let (dataDir, _) = context.store.dataDir()
        guard let dataDir = dataDir else {
            note = "data ディレクトリが見つかりません"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        guard let engine = NagiEngine(dataDir: dataDir, userDir: nil, noLLM: false) else {
            note = "LLM エンジン生成失敗"
            metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let loadTime = Date().timeIntervalSince(startTime) * 1000.0
        let afterStats = MemoryProbe.getStats()

        metrics["load_time_ms"] = loadTime
        metrics["footprint_increase_mb"] = afterStats.physFootprintMB - baselineStats.physFootprintMB

        // Space 変換テスト
        let initReq = """
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-ios-spike","client_version":"0.1","os":"ios"}}
        """
        _ = engine.send(line: initReq)

        let spaceStart = Date()
        let spaceReq = """
        {"jsonrpc":"2.0","id":2,"method":"process_key","params":{"session_id":"llm_test","keysym":32,"keycode":49,"text":" ","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
        """
        _ = engine.send(line: spaceReq)
        metrics["first_space_ms"] = Date().timeIntervalSince(spaceStart) * 1000.0
        metrics["data_dir_source"] = dataDirSource == "none" ? 0 : (dataDirSource == "appgroup" ? 1 : 2)

        ok = true
        note = "LLM ロード成功。推論エンジンが有効です（source: \(dataDirSource)）"
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
          let preedit = result["preedit"] as? [String: Any],
          let candidates = preedit["candidates"] as? [[String: Any]],
          let firstCandidate = candidates.first,
          let surface = firstCandidate["surface"] as? String else {
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
    "thermal_battery": allProbes["thermal_battery"]!
]
