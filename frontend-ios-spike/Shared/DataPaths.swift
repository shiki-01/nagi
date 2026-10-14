import Foundation

/// データディレクトリとユーザーディレクトリの解決ロジック
/// 優先順位: (a) App Group コンテナ（実在する場合）→ (b) Bundle リソース（実在する場合）→ (c) なし
struct DataPaths {
    /// data_dir の解決と情報
    /// - Returns: (path: String?, source: String) source は "appgroup" / "bundle" / "none"
    static func resolveDataDir(appGroupId: String = "group.com.shiki01.nagi") -> (path: String?, source: String) {
        let fileManager = FileManager.default

        // (a) App Group コンテナの data ディレクトリ（実在する場合）
        if let containerURL = fileManager.containerURL(forSecurityApplicationGroupIdentifier: appGroupId) {
            let dataPath = containerURL.appendingPathComponent("data").path
            if fileManager.fileExists(atPath: dataPath) {
                return (dataPath, "appgroup")
            }
        }

        // (b) Bundle リソースの data ディレクトリ（実在する場合）
        if let bundleDataURL = Bundle.main.resourceURL?.appendingPathComponent("data") {
            let bundleDataPath = bundleDataURL.path
            if fileManager.fileExists(atPath: bundleDataPath) {
                return (bundleDataPath, "bundle")
            }
        }

        // (c) なし
        return (nil, "none")
    }

    /// user_dir の解決
    /// App Group が利用可能ならそのコンテナを使用。不可ならプロセス固有の Application Support を使用
    /// - Returns: path: String?
    static func resolveUserDir(appGroupId: String = "group.com.shiki01.nagi") -> String? {
        let fileManager = FileManager.default

        // App Group コンテナが利用可能ならそれを使う
        if let containerURL = fileManager.containerURL(forSecurityApplicationGroupIdentifier: appGroupId) {
            return containerURL.path
        }

        // App Group 不可の場合、プロセス固有の Application Support
        if let appSupportURL = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            let userDirURL = appSupportURL.appendingPathComponent("com.shiki.nagi")
            do {
                try fileManager.createDirectory(at: userDirURL, withIntermediateDirectories: true)
                return userDirURL.path
            } catch {
                return nil
            }
        }

        return nil
    }

    /// 辞書ファイルのパスを取得
    /// - Returns: (path: String?, source: String) source は "appgroup" / "bundle" / "none"
    static func dictPath(appGroupId: String = "group.com.shiki01.nagi") -> (path: String?, source: String) {
        let fileManager = FileManager.default
        let (dataDir, source) = resolveDataDir(appGroupId: appGroupId)

        guard let dataDir = dataDir else {
            return (nil, source)
        }

        let dictPath = URL(fileURLWithPath: dataDir)
            .appendingPathComponent("dict")
            .appendingPathComponent("nagi-dict.bin")
            .path

        if fileManager.fileExists(atPath: dictPath) {
            return (dictPath, source)
        }

        return (nil, source)
    }

    /// モデルディレクトリのパスを取得
    /// - Returns: (path: String?, source: String)
    static func modelsDir(appGroupId: String = "group.com.shiki01.nagi") -> (path: String?, source: String) {
        let fileManager = FileManager.default
        let (dataDir, source) = resolveDataDir(appGroupId: appGroupId)

        guard let dataDir = dataDir else {
            return (nil, source)
        }

        let modelsPath = URL(fileURLWithPath: dataDir)
            .appendingPathComponent("models")
            .path

        let isDir = UnsafeMutablePointer<ObjCBool>.allocate(capacity: 1)
        defer { isDir.deallocate() }

        if fileManager.fileExists(atPath: modelsPath, isDirectory: isDir), isDir.pointee.boolValue {
            return (modelsPath, source)
        }

        return (nil, source)
    }

    // MARK: - モデル（GGUF）の探索

    /// コアの既定モデルファイル名（core/server/src/setup.rs の DEFAULT_MODEL_FILE）
    static let defaultModelFileName = "zenz-v3.1-small-Q5_K_M.gguf"

    /// 配置済みの GGUF ファイル
    struct ModelFile {
        let name: String
        let path: String
        let sizeBytes: Int64
    }

    /// モデルの配置状況（原因別）
    enum ModelStatus {
        /// (a) App Group コンテナが取得できない（かつバンドルにもモデルが無い）
        case noContainer
        /// (b) コンテナはあるが data/models/ が無い（パスは作成予定の場所）
        case noModelsDir(String)
        /// (c) ディレクトリはあるが .gguf が無い
        case noGGUF(String)
        /// 1 個以上ある。source は "appgroup" / "bundle"
        case found(dir: String, files: [ModelFile], source: String)
    }

    /// このプロセス専用の Application Support 配下のモデル置き場（App Group が使えないときのフォールバック）。
    /// ホストアプリとキーボード拡張でパスが別になるため、拡張からはホストが置いたモデルは見えない
    static func hostLocalModelsDir() -> URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("com.shiki.nagi/data/models", isDirectory: true)
    }

    /// モデルの配置状況を調べる（App Group → このプロセス専用領域 → バンドルの順）。
    /// `resolveDataDir` と違い、data/ だけ無い場合や辞書の有無に影響されず、原因を区別できる
    static func modelStatus(appGroupId: String = "group.com.shiki01.nagi") -> ModelStatus {
        let fileManager = FileManager.default
        let container = fileManager.containerURL(forSecurityApplicationGroupIdentifier: appGroupId)

        var candidates: [(dir: URL, source: String)] = []
        if let container = container {
            candidates.append((container.appendingPathComponent("data/models", isDirectory: true), "appgroup"))
        }
        if container == nil, let local = hostLocalModelsDir() {
            candidates.append((local, "local"))
        }
        if let resourceURL = Bundle.main.resourceURL {
            candidates.append((resourceURL.appendingPathComponent("data/models", isDirectory: true), "bundle"))
        }

        var firstExistingDir: String?
        for candidate in candidates {
            var isDir: ObjCBool = false
            guard fileManager.fileExists(atPath: candidate.dir.path, isDirectory: &isDir), isDir.boolValue else {
                continue
            }
            if firstExistingDir == nil { firstExistingDir = candidate.dir.path }
            let names = ((try? fileManager.contentsOfDirectory(atPath: candidate.dir.path)) ?? [])
                .filter { $0.lowercased().hasSuffix(".gguf") }
                .sorted()
            let files = names.map { name -> ModelFile in
                let path = candidate.dir.appendingPathComponent(name).path
                let size = (try? fileManager.attributesOfItem(atPath: path))?[.size] as? Int64 ?? 0
                return ModelFile(name: name, path: path, sizeBytes: size)
            }
            if !files.isEmpty {
                return .found(dir: candidate.dir.path, files: files, source: candidate.source)
            }
        }

        if let dir = firstExistingDir { return .noGGUF(dir) }
        if let container = container {
            return .noModelsDir(container.appendingPathComponent("data/models").path)
        }
        return .noContainer
    }

    /// 使用するモデルの選び方: 設定で選んだファイル名 → 端末の予算に合う推奨モデル
    /// （`NagiModelRecommendation`。decisions.md H-22）→ 既定名 → 最大サイズ。
    /// 予算が分かっていて推奨が無い（どれも載らない）ときは、もっとも小さいモデルにする。
    /// `budgetMB` が nil（予算を測れない）なら推奨を使わず、既定名（small）→ 最大サイズの順。
    /// ホームの「使用中」表示・Picker と、拡張が実際に読むモデルを一致させるため、全員がこの関数を使う
    static func selectModel(from files: [ModelFile], preferredName: String?, budgetMB: UInt64? = nil) -> ModelFile? {
        if let preferredName = preferredName, let hit = files.first(where: { $0.name == preferredName }) {
            return hit
        }
        if let budgetMB = budgetMB {
            if let recommended = NagiModelRecommendation.recommend(budgetMB: budgetMB, available: files) {
                return recommended
            }
            return files.min { $0.sizeBytes < $1.sizeBytes }
        }
        if let hit = files.first(where: { $0.name == defaultModelFileName }) {
            return hit
        }
        return files.max { $0.sizeBytes < $1.sizeBytes }
    }

    // MARK: - 拡張のメモリ予算（推奨モデルの計算用）

    private static let extensionBudgetKey = "extension_budget_mb"

    /// キーボード拡張が自分の予算（`MemoryProbe.processBudgetMB()`）を記録する。
    /// ホストアプリは数 GB と報告するため、拡張に載せるモデルの推奨にはこの値を使う。0 は記録しない
    static func recordExtensionBudgetMB(_ mb: UInt64, appGroupId: String = "group.com.shiki01.nagi") {
        guard mb > 0 else { return }
        UserDefaults(suiteName: appGroupId)?.set(Int(clamping: mb), forKey: extensionBudgetKey)
    }

    /// モデル選択に使う予算（MB）: 拡張が記録した値 → 実測に基づく仮定値（H-6 の約 177MB）。
    /// ホストアプリ・Probes（ホスト側）はこちらを使う
    static func selectionBudgetMB(appGroupId: String = "group.com.shiki01.nagi") -> UInt64 {
        if let stored = UserDefaults(suiteName: appGroupId)?.object(forKey: extensionBudgetKey) as? Int, stored > 0 {
            return UInt64(stored)
        }
        return NagiModelRecommendation.assumedExtensionBudgetMB
    }

    private static let modelPreferenceKey = "llm_model_file"

    /// 使用するモデルのファイル名の選択（App Group の UserDefaults に保存し、ホストと拡張で共有）
    static func preferredModelName(appGroupId: String = "group.com.shiki01.nagi") -> String? {
        UserDefaults(suiteName: appGroupId)?.string(forKey: modelPreferenceKey)
    }

    static func setPreferredModelName(_ name: String?, appGroupId: String = "group.com.shiki01.nagi") {
        UserDefaults(suiteName: appGroupId)?.set(name, forKey: modelPreferenceKey)
    }

    // MARK: - 辞書

    /// バンドル内の辞書（Resources/data/dict/nagi-dict.bin。フォルダ参照なので subdirectory が必要）
    static func bundleDictURL() -> URL? {
        guard let url = Bundle.main.resourceURL?.appendingPathComponent("data/dict/nagi-dict.bin"),
              FileManager.default.fileExists(atPath: url.path) else { return nil }
        return url
    }

    /// App Group の data/dict/ に辞書が無ければバンドルからコピーする。
    /// コアは辞書が読めないと LLM を読み込まないため、モデルを置く前後に必要
    /// - Returns: 配置済み（または今回コピーした）なら true。バンドルにも無ければ false
    @discardableResult
    static func ensureDictInAppGroup(appGroupId: String = "group.com.shiki01.nagi") throws -> Bool {
        let fileManager = FileManager.default
        guard let container = fileManager.containerURL(forSecurityApplicationGroupIdentifier: appGroupId) else {
            return false
        }
        let destDir = container.appendingPathComponent("data/dict", isDirectory: true)
        let dest = destDir.appendingPathComponent("nagi-dict.bin")
        if fileManager.fileExists(atPath: dest.path) { return true }
        guard let source = bundleDictURL() else { return false }
        try fileManager.createDirectory(at: destDir, withIntermediateDirectories: true)
        try fileManager.copyItem(at: source, to: dest)
        return true
    }
}
