import Foundation

/// 本体アプリの「エンジン自己テスト」。実機でコア・辞書が動いているかを確かめる（入力内容は扱わない）
enum SelfTest {
    /// ローマ字 `nihongo` を送って Space で変換し、結果を複数行のテキストで返す。時間のかかる処理なので別スレッドで呼ぶ
    static func run(paths: NagiPaths) -> String {
        var lines: [String] = []
        let outcome = ConfigBootstrap.ensureConfig(userDir: paths.userDir)
        lines.append("config.toml: \(outcome.rawValue)")
        let memoryBefore = MemoryProbe.getPhysFootprintMB()
        let started = Date()
        guard let engine = NagiEngine(dataDir: paths.dataDir, userDir: paths.userDir, noLLM: true) else {
            lines.append("エンジンの生成に失敗しました（nagi_server_new が NULL）")
            return lines.joined(separator: "\n")
        }
        let loadMs = Date().timeIntervalSince(started) * 1000
        lines.append(String(format: "engine load: %.0f ms", loadMs))
        lines.append("notes: \(engine.notes.replacingOccurrences(of: "\n", with: " / "))")

        let client = NagiRpcClient(transport: engine)
        let sessionId = "selftest"
        do {
            let initResult: InitializeResult = try client.call(
                "initialize",
                params: InitializeParams(protocol_version: 1, client_name: "nagi-ios-app", client_version: "0.1", os: "ios")
            )
            lines.append("profile: \(initResult.profile)  touch_input: \(initResult.capabilities.touch_input ?? false)")

            var maxKeyMs = 0.0
            var lastPreedit = ""
            for ch in "nihongo" {
                let t0 = Date()
                let r = try client.processKey(
                    sessionId: sessionId,
                    keysym: Keysym.forText(String(ch)),
                    text: String(ch),
                    shift: false,
                    alternatives: []
                )
                maxKeyMs = max(maxKeyMs, Date().timeIntervalSince(t0) * 1000)
                if let p = r.preedit { lastPreedit = p.text }
            }
            lines.append("preedit after typing: \(lastPreedit)")
            let t1 = Date()
            let converted = try client.processKey(
                sessionId: sessionId, keysym: Keysym.space, text: " ", shift: false, alternatives: []
            )
            let convertMs = Date().timeIntervalSince(t1) * 1000
            let surface = converted.preedit?.text ?? lastPreedit
            let candidateCount = converted.candidates?.items.count ?? 0
            lines.append("converted: \(surface)  candidates: \(candidateCount)")
            lines.append(String(format: "max key latency: %.1f ms  convert: %.1f ms", maxKeyMs, convertMs))
            // 学習を汚さないよう、確定せずに破棄する
            let _: ProcessKeyResult = try client.call("reset", params: SessionParams(session_id: sessionId))
            let ok = (paths.dictPath != nil) && surface != lastPreedit
            lines.append(ok ? "結果: OK（辞書で変換できました）" : "結果: 辞書変換は確認できませんでした（辞書なし、または変換結果が読みと同じ）")
        } catch {
            lines.append("結果: NG \(error)")
        }
        let memoryAfter = MemoryProbe.getPhysFootprintMB()
        lines.append(String(format: "phys_footprint: %.0f MB -> %.0f MB", memoryBefore, memoryAfter))
        return lines.joined(separator: "\n")
    }
}
