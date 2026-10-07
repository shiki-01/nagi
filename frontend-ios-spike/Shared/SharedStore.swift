import Foundation

/// プローブの結果
struct ProbeResult: Codable {
    let id: String
    let name: String
    let process: String // "app" or "keyboard"
    let ok: Bool
    let metrics: [String: Double]
    let note: String
    let timestamp: Date
    let deviceModel: String
    let osVersion: String

    enum CodingKeys: String, CodingKey {
        case id, name, process, ok, metrics, note, timestamp
        case deviceModel = "device_model"
        case osVersion = "os_version"
    }
}

/// App Group コンテナを使用した結果ストア
class SharedStore {
    static let shared = SharedStore()

    private let fileManager = FileManager.default
    private let resultsFileName = "results.jsonl"
    private let resolution: AppGroupResolution

    init() {
        self.resolution = AppGroupResolver.resolve()
    }

    /// App Group コンテナへのパス。取得不可な場合は nil
    var containerURL: URL? {
        resolution.containerURL
    }

    /// App Group ID 解決結果を取得
    var appGroupResolution: AppGroupResolution {
        resolution
    }

    /// 実際に使う App Group ID（解決できなければ既定値）
    var appGroupId: String {
        resolution.resolvedID ?? "group.com.shiki.nagi.spike"
    }

    /// 辞書ファイルのパス（優先順位: App Group → Bundle）
    /// - Returns: (path: String?, source: String) source は "appgroup" / "bundle" / "none"
    func dictPath() -> (path: String?, source: String) {
        return DataPaths.dictPath(appGroupId: resolution.resolvedID ?? "group.com.shiki.nagi.spike")
    }

    /// モデルディレクトリのパス（優先順位: App Group → Bundle）
    /// - Returns: (path: String?, source: String)
    func modelsDir() -> (path: String?, source: String) {
        return DataPaths.modelsDir(appGroupId: resolution.resolvedID ?? "group.com.shiki.nagi.spike")
    }

    /// data ディレクトリのパス（優先順位: App Group → Bundle）
    /// - Returns: (path: String?, source: String)
    func dataDir() -> (path: String?, source: String) {
        return DataPaths.resolveDataDir(appGroupId: resolution.resolvedID ?? "group.com.shiki.nagi.spike")
    }

    /// 結果を追記（同期的に fsync）
    /// App Group が使えない場合は Application Support の per-process ディレクトリに記録
    func append(result: ProbeResult) throws {
        let resultsURL: URL

        if let container = containerURL {
            // App Group 利用可能
            resultsURL = container.appendingPathComponent(resultsFileName)
            try fileManager.createDirectory(at: container, withIntermediateDirectories: true)
        } else {
            // App Group 利用不可の場合、プロセス固有の Application Support を使用
            // （ホストアプリとキーボード拡張は別ディレクトリになる）
            guard let appSupportURL = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
                throw NSError(domain: "SharedStore", code: 3, userInfo: [NSLocalizedDescriptionKey: "Application Support ディレクトリが見つかりません"])
            }
            let resultsDirURL = appSupportURL.appendingPathComponent("com.shiki.nagi.spike")
            try fileManager.createDirectory(at: resultsDirURL, withIntermediateDirectories: true)
            resultsURL = resultsDirURL.appendingPathComponent(resultsFileName)
        }

        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let jsonData = try encoder.encode(result)
        guard var jsonString = String(data: jsonData, encoding: .utf8) else {
            throw NSError(domain: "SharedStore", code: 2, userInfo: [NSLocalizedDescriptionKey: "JSON エンコード失敗"])
        }
        jsonString += "\n"

        if fileManager.fileExists(atPath: resultsURL.path) {
            if let fileHandle = FileHandle(forWritingAtPath: resultsURL.path) {
                defer { try? fileHandle.close() }
                fileHandle.seekToEndOfFile()
                if let data = jsonString.data(using: .utf8) {
                    fileHandle.write(data)
                    try? fileHandle.synchronizeFile()
                }
            }
        } else {
            try jsonString.write(to: resultsURL, atomically: true, encoding: .utf8)
        }
    }

    /// 結果を読込（最新 N 件）
    /// App Group が使えない場合は per-process ディレクトリから読む
    func read(last: Int = 100) -> [ProbeResult] {
        let resultsURL: URL

        if let container = containerURL {
            resultsURL = container.appendingPathComponent(resultsFileName)
        } else {
            guard let appSupportURL = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
                return []
            }
            let resultsDirURL = appSupportURL.appendingPathComponent("com.shiki.nagi.spike")
            resultsURL = resultsDirURL.appendingPathComponent(resultsFileName)
        }

        guard let content = try? String(contentsOf: resultsURL, encoding: .utf8) else { return [] }

        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601

        let lines = content.split(separator: "\n", omittingEmptySubsequences: true)
        return lines.suffix(last).compactMap { line in
            try? decoder.decode(ProbeResult.self, from: String(line).data(using: .utf8)!)
        }
    }

    /// 最新の相手プロセスの結果を取得（appgroup Probe 用）
    func latestFromOtherProcess(currentProcess: String) -> ProbeResult? {
        let allResults = read(last: 1000)
        let otherProcess = currentProcess == "app" ? "keyboard" : "app"
        return allResults.last { $0.process == otherProcess }
    }

    /// すべての結果をクリア
    func clearAll() throws {
        let resultsURL: URL

        if let container = containerURL {
            resultsURL = container.appendingPathComponent(resultsFileName)
        } else {
            guard let appSupportURL = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
                return
            }
            let resultsDirURL = appSupportURL.appendingPathComponent("com.shiki.nagi.spike")
            resultsURL = resultsDirURL.appendingPathComponent(resultsFileName)
        }

        try fileManager.removeItem(at: resultsURL)
    }

    /// 結果を JSONL として取得
    func exportAsJSONL() -> String {
        let results = read(last: 10000)
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601

        return results.compactMap { result in
            guard let data = try? encoder.encode(result),
                  let json = String(data: data, encoding: .utf8) else { return nil }
            return json
        }.joined(separator: "\n")
    }
}
