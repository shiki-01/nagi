import Foundation

/// キーボード拡張が使うコアのセッションを、App Group の辞書・モデルから作る
enum ProductSession {
    /// 結果。`session` が nil のときは `message` を画面に出す
    struct Outcome {
        let session: NagiSession?
        let message: String?
        /// 動作はするが、設定の保存先が作れないなど、画面に出しておきたい注意
        var warning: String? = nil
        /// コアが読む `config.toml`（キーボードの focusIn で更新を検出して `reload_config` を送る）
        var configURL: URL? = nil
    }

    /// 同期の待ち時間（ms）。これを超えた LLM の推論は裏で続け、完了を非同期通知で差し替える
    /// （decisions.md H-10。xsmall は収まり、small は約 0.6 秒後に差し替わる）
    static let syncBudgetMs = 30

    /// - noLLM: true なら LLM を読み込まず辞書のみで動く（メモリ警告のあとの作り直しに使う）
    static func make(store: SharedStore, noLLM: Bool = false) -> Outcome {
        let (dataDirPath, _) = store.dataDir()
        guard let dataDir = dataDirPath else {
            return Outcome(session: nil, message: "辞書が見つかりません。Nagi アプリを開いて辞書をコピーしてください")
        }
        // 設定の保存先が作れなくても、変換は使える（設定と学習は既定値・保存なし）。理由は画面に出す
        let prepared = prepareUserDir(store: store, modelPath: preferredModelPath(store: store))
        // 上限が小さい端末では、コアが LLM を読み込まず辞書のみで動く（decisions.md H-8）
        guard let engine = NagiEngine(
            dataDir: dataDir,
            userDir: prepared.path,
            noLLM: noLLM,
            memoryBudgetMB: MemoryProbe.processBudgetMB()
        ), let session = NagiSession(engine: engine) else {
            return Outcome(session: nil, message: "変換エンジンを起動できませんでした（保存先: \(prepared.path ?? "なし")）")
        }
        let warning = prepared.path == nil
            ? "設定の保存先を作れません（\(prepared.detail)）。既定の設定で動作します"
            : nil
        let configURL = prepared.path.map { URL(fileURLWithPath: $0).appendingPathComponent("config.toml") }
        return Outcome(session: session, message: nil, warning: warning, configURL: configURL)
    }

    /// 使うモデルのパス。ユーザーの選択（`preferredModelName`）を優先し、未選択なら xsmall
    /// （待ち時間内に結果が返り、差し替えのちらつきが無い。H-10）、無ければ既定の選び方で選ぶ
    static func preferredModelPath(store: SharedStore) -> String? {
        guard case .found(_, let files, _) = DataPaths.modelStatus(appGroupId: store.appGroupId) else {
            return nil
        }
        // 未選択のときだけ xsmall が優先される（`DataPaths.selectModel`。ホームの表示と同じ決め方）
        return DataPaths.selectModel(
            from: files,
            preferredName: DataPaths.preferredModelName(appGroupId: store.appGroupId)
        )?.path
    }

    /// 自動生成した設定ファイルの目印（1 行目）。続けて ` body=<本文のハッシュ>` を付ける。
    /// 本文がハッシュと一致する（誰も編集していない）ファイルだけ、起動のたびに作り直す。
    /// 設定画面（コアの `update_settings`）が書き換えたファイルは、本文が変わるのでハッシュが合わず、
    /// ユーザーの設定として保たれる（コメントの 1 行目は残るため、目印だけでは判別できない）
    private static let generatedMarker = "# nagi:generated"

    /// 本文のハッシュ（FNV-1a 64 bit。プロセスをまたいで同じ値になる）
    private static func bodyHash(_ body: String) -> String {
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        for byte in body.utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 0x100_0000_01b3
        }
        return String(hash, radix: 16)
    }

    /// 自動生成のままか。目印が無ければ false。ハッシュの無い旧形式の目印は、従来どおり生成物として扱う
    private static func isPristineGenerated(_ text: String) -> Bool {
        guard text.hasPrefix(generatedMarker) else { return false }
        let firstLineEnd = text.firstIndex(of: "\n") ?? text.endIndex
        let firstLine = String(text[text.startIndex..<firstLineEnd])
        let body = firstLineEnd < text.endIndex ? String(text[text.index(after: firstLineEnd)...]) : ""
        guard let range = firstLine.range(of: " body=") else { return true }
        return String(firstLine[range.upperBound...]) == bodyHash(body)
    }

    /// ユーザーデータのディレクトリを用意し、`config.toml` を既定値で作る。
    /// 候補は順に `<App Group>/user`、拡張自身の Application Support、一時ディレクトリ。
    /// どれも作れなければ `path` が nil で、`detail` に各候補の失敗理由を入れる
    static func prepareUserDir(store: SharedStore, modelPath: String?) -> (path: String?, detail: String) {
        let fileManager = FileManager.default
        var candidates: [URL] = []
        var failures: [String] = []
        if let root = DataPaths.resolveUserDir(appGroupId: store.appGroupId) {
            candidates.append(URL(fileURLWithPath: root).appendingPathComponent("user", isDirectory: true))
        } else {
            failures.append("App Group の保存先なし")
        }
        if let support = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            candidates.append(support.appendingPathComponent("Nagi/user", isDirectory: true))
        }
        candidates.append(fileManager.temporaryDirectory.appendingPathComponent("nagi-user", isDirectory: true))

        for dir in candidates {
            do {
                try fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
                try writeConfigIfGenerated(in: dir, modelPath: modelPath)
                return (dir.path, "")
            } catch {
                failures.append("\(dir.lastPathComponent): \(error.localizedDescription)")
            }
        }
        return (nil, failures.joined(separator: " / "))
    }

    /// `config.toml` が無い、または自動生成のままなら、既定値で作り直す。
    /// ユーザーが編集した設定（設定画面での変更を含む）は書き換えない。ただしモデルのパスだけは、
    /// `[conversion]` の `model = '...'` の行が既にあれば、配置済みのモデルに追随させる
    private static func writeConfigIfGenerated(in dir: URL, modelPath: String?) throws {
        let configURL = dir.appendingPathComponent("config.toml")
        let existing = try? String(contentsOf: configURL, encoding: .utf8)
        // ファイルはあるが読めない（権限・文字コードなど）ときは、ユーザーの設定かもしれないので上書きしない
        if existing == nil, FileManager.default.fileExists(atPath: configURL.path) {
            return
        }
        // 書き込みの直前に読み直し、読んだあとに誰か（設定画面・別プロセス）が書き換えていたら諦める
        // （次の起動で再評価される）。読み取りから書き込みまでの隙間を縮めるだけで、完全な排他ではない
        func writeIfUnchanged(_ text: String) throws {
            let current = try? String(contentsOf: configURL, encoding: .utf8)
            guard current == existing else { return }
            try text.write(to: configURL, atomically: true, encoding: .utf8)
        }
        if let existing = existing, !isPristineGenerated(existing) {
            if let modelPath = modelPath, let updated = replacingModelLine(in: existing, with: modelPath), updated != existing {
                try writeIfUnchanged(updated)
            }
            return
        }
        // キーボード拡張はネットワークを使わない（更新確認を切る）
        var body = "[update]\nauto_check = false\n\n"
        body += "[conversion]\nsync_budget_ms = \(syncBudgetMs)\n"
        if let modelPath = modelPath {
            body += "model = '\(modelPath)'\n"
        }
        let config = "\(generatedMarker) body=\(bodyHash(body))\n" + body
        // 内容が同じなら書かない（ユーザー辞書画面を開くたびに更新時刻を進めて reload_config を誘発しない）
        if config == existing { return }
        try writeIfUnchanged(config)
    }

    /// `[conversion]` テーブルの `model = '...'` の行を差し替える。該当の行が無ければ nil
    private static func replacingModelLine(in text: String, with modelPath: String) -> String? {
        var inConversion = false
        var replaced = false
        var lines: [String] = []
        for line in text.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("[") {
                inConversion = (trimmed == "[conversion]")
            }
            if inConversion, !replaced, trimmed.hasPrefix("model"), trimmed.contains("='") || trimmed.contains("= '") {
                lines.append("model = '\(modelPath)'")
                replaced = true
            } else {
                lines.append(line)
            }
        }
        return replaced ? lines.joined(separator: "\n") : nil
    }
}
