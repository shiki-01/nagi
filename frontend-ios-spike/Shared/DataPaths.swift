import Foundation

/// データディレクトリとユーザーディレクトリの解決ロジック
/// 優先順位: (a) App Group コンテナ（実在する場合）→ (b) Bundle リソース（実在する場合）→ (c) なし
struct DataPaths {
    /// data_dir の解決と情報
    /// - Returns: (path: String?, source: String) source は "appgroup" / "bundle" / "none"
    static func resolveDataDir(appGroupId: String = "group.com.shiki.nagi.spike") -> (path: String?, source: String) {
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
    static func resolveUserDir(appGroupId: String = "group.com.shiki.nagi.spike") -> String? {
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
    static func dictPath(appGroupId: String = "group.com.shiki.nagi.spike") -> (path: String?, source: String) {
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
    static func modelsDir(appGroupId: String = "group.com.shiki.nagi.spike") -> (path: String?, source: String) {
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
}
