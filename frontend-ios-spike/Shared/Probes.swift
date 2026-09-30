import Foundation
import UIKit

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
    let model = withUnsafeBytes(of: &systemInfo.machine) {
        $0.lazy.prefix(while: { $0 != 0 }).map(String.init(UnicodeScalar.init))
            .joined()
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

        guard let dictPath = context.store.dictPath() else {
            note = "辞書ファイルが見つかりません。コンテナの data/dict/nagi-dict.bin をチェックしてください"
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let baselineStats = MemoryProbe.getStats()
        let startTime = Date()

        guard let engine = NagiEngine(dataDir: context.store.containerURL!.path + "/data", userDir: nil, noLLM: true) else {
            note = "NagiEngine の生成に失敗（C ABI エラー）"
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let endTime = Date()
        let afterStats = MemoryProbe.getStats()

        let elapsedMs = endTime.timeIntervalSince(startTime) * 1000.0
        let footprintDiffMB = afterStats.physFootprintMB - baselineStats.physFootprintMB

        metrics = [
            "load_time_ms": elapsedMs,
            "footprint_increase_mb": footprintDiffMB,
            "footprint_after_mb": afterStats.physFootprintMB
        ]

        ok = true
        note = "Profile: \(engine.notes)"

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
            note = "エンジン生成失敗"
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        var latencies: [Double] = []
        var spaceLatencies: [Double] = []

        for i in 0..<200 {
            let sessionId = "session_\(i)"

            // initialize
            let initReq = """
            {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-ios-spike","client_version":"0.1","os":"ios"}}
            """
            _ = engine.send(line: initReq)

            let testString = "konnnichiha"
            for char in testString {
                let charNum = UInt32(char.unicodeScalars.first?.value ?? 0)
                let keyReq = """
                {"jsonrpc":"2.0","id":2,"method":"process_key","params":{"session_id":"\(sessionId)","keysym":\(charNum),"keycode":0,"text":"\(char)","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
                """

                let startTime = Date()
                _ = engine.send(line: keyReq)
                let elapsed = Date().timeIntervalSince(startTime) * 1000.0
                latencies.append(elapsed)
            }

            // Space キー
            let spaceReq = """
            {"jsonrpc":"2.0","id":3,"method":"process_key","params":{"session_id":"\(sessionId)","keysym":32,"keycode":49,"text":" ","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}
            """
            let spaceStart = Date()
            _ = engine.send(line: spaceReq)
            let spaceElapsed = Date().timeIntervalSince(spaceStart) * 1000.0
            spaceLatencies.append(spaceElapsed)

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

        ok = !latencies.isEmpty
        note = "目標: ひらがな p95 < 16ms, Space p95 < 150ms"

        return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
    }
}

// MARK: - 5. Memory Ramp Probe

struct MemoryRampProbe: ProbeRunner {
    func run(context: ProbeContext) async -> ProbeResult {
        let id = "memory_ramp"
        let name = "Memory Ramp"
        var metrics: [String: Double] = [:]
        var note = "メモリ段階的確保（強制終了対応テスト。再起動後に最後の記録値を確認可能）"
        var ok = false

        let startStats = MemoryProbe.getStats()
        metrics["start_available_mb"] = startStats.availableMemoryMB
        metrics["start_footprint_mb"] = startStats.physFootprintMB

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

        while maxReachedMB < 500 {
            do {
                // 進捗を同期的に記録
                let progressReq = ProbeResult(
                    id: "memory_ramp_progress",
                    name: "Memory Ramp Progress",
                    process: context.processType,
                    ok: true,
                    metrics: ["reached_mb": Double(maxReachedMB)],
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

        guard let dictPath = context.store.dictPath() else {
            note = "辞書ファイルが見つかりません"
            return makeResult(id: id, name: name, ok: ok, metrics: metrics, note: note, process: context.processType)
        }

        let startTime = Date()
        guard let engine = NagiEngine(dataDir: context.store.containerURL!.path + "/data", userDir: nil, noLLM: true) else {
            note = "エンジン生成失敗"
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
        _ = engine.send(line: keyReq)
        let keyElapsed = Date().timeIntervalSince(keyStart) * 1000.0

        let totalElapsed = Date().timeIntervalSince(startTime) * 1000.0

        metrics["engine_creation_ms"] = totalElapsed - keyElapsed
        metrics["first_key_ms"] = keyElapsed
        metrics["total_ms"] = totalElapsed

        ok = true
        note = "目標: < 2000ms（light は < 300ms）"

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
        guard let modelsDir = context.store.modelsDir() else {
            note = "モデルディレクトリが見つかりません"
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

        guard let engine = NagiEngine(dataDir: context.store.containerURL!.path + "/data", userDir: nil, noLLM: false) else {
            note = "LLM エンジン生成失敗"
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

        ok = true
        note = "LLM ロード成功。推論エンジンが有効です"
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
    guard let container = context.store.containerURL else { return nil }
    return NagiEngine(dataDir: container.path + "/data", userDir: nil, noLLM: true)
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
    "memory_ramp": { await MemoryRampProbe().run(context: $0) },
    "coldstart": { await ColdstartProbe().run(context: $0) },
    "llm_load": { await LLMLoadProbe().run(context: $0) },
    "thermal_battery": { await ThermalBatteryProbe().run(context: $0) }
]
