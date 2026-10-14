import Foundation

/// キーボード拡張が使うコアのセッションを、App Group の辞書・モデルから作る
enum ProductSession {
    /// 結果。`session` が nil のときは `message` を画面に出す
    struct Outcome {
        let session: NagiSession?
        let message: String?
    }

    /// 同期の待ち時間（ms）。これを超えた LLM の推論は裏で続け、完了を非同期通知で差し替える
    /// （decisions.md H-10。xsmall は収まり、small は約 0.6 秒後に差し替わる）
    static let syncBudgetMs = 30

    static func make(store: SharedStore) -> Outcome {
        let (dataDirPath, _) = store.dataDir()
        guard let dataDir = dataDirPath else {
            return Outcome(session: nil, message: "辞書が見つかりません。Nagi アプリを開いて辞書をコピーしてください")
        }
        guard let userDir = prepareUserDir(store: store, modelPath: preferredModelPath(store: store)) else {
            return Outcome(session: nil, message: "設定の保存先を作れませんでした")
        }
        // 上限が小さい端末では、コアが LLM を読み込まず辞書のみで動く（decisions.md H-8）
        guard let engine = NagiEngine(
            dataDir: dataDir,
            userDir: userDir,
            noLLM: false,
            memoryBudgetMB: MemoryProbe.processBudgetMB()
        ), let session = NagiSession(engine: engine) else {
            return Outcome(session: nil, message: "変換エンジンを起動できませんでした")
        }
        return Outcome(session: session, message: nil)
    }

    /// 使うモデルのパス。既定は xsmall（待ち時間内に結果が返り、差し替えのちらつきが無い。H-10）で、
    /// 無ければ配置済みのモデルから既定の選び方で選ぶ
    private static func preferredModelPath(store: SharedStore) -> String? {
        guard case .found(_, let files, _) = DataPaths.modelStatus(appGroupId: store.appGroupId) else {
            return nil
        }
        if let xsmall = files.first(where: { $0.name.contains("xsmall") }) {
            return xsmall.path
        }
        return DataPaths.selectModel(
            from: files,
            preferredName: DataPaths.preferredModelName(appGroupId: store.appGroupId)
        )?.path
    }

    /// 自動生成した設定ファイルの目印。この行があるファイルだけ、起動のたびに作り直す
    /// （モデルを入れ替えても追随する）。ユーザーが編集した設定（この行が無いもの）は書き換えない
    private static let generatedMarker = "# nagi:generated"

    /// ユーザーデータのディレクトリ（`<App Group>/user`）を用意し、`config.toml` を既定値で作る
    private static func prepareUserDir(store: SharedStore, modelPath: String?) -> String? {
        guard let root = DataPaths.resolveUserDir(appGroupId: store.appGroupId) else { return nil }
        let dir = URL(fileURLWithPath: root).appendingPathComponent("user", isDirectory: true)
        let fileManager = FileManager.default
        do {
            try fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        } catch {
            return nil
        }
        let configURL = dir.appendingPathComponent("config.toml")
        let existing = try? String(contentsOf: configURL, encoding: .utf8)
        if existing == nil || existing?.hasPrefix(generatedMarker) == true {
            // キーボード拡張はネットワークを使わない（更新確認を切る）
            var config = "\(generatedMarker)\n[update]\nauto_check = false\n\n"
            config += "[conversion]\nsync_budget_ms = \(syncBudgetMs)\n"
            if let modelPath = modelPath {
                config += "model = '\(modelPath)'\n"
            }
            try? config.write(to: configURL, atomically: true, encoding: .utf8)
        }
        return dir.path
    }
}
