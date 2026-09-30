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
    private let appGroupId = "group.com.shiki.nagi.spike"
    private let resultsFileName = "results.jsonl"

    /// App Group コンテナへのパス。取得不可な場合は nil
    var containerURL: URL? {
        fileManager.containerURL(forSecurityApplicationGroupIdentifier: appGroupId)
    }

    /// 辞書ファイルのパス（コンテナ内の data/dict/nagi-dict.bin）
    func dictPath() -> String? {
        guard let container = containerURL else { return nil }
        let path = container.appendingPathComponent("data/dict/nagi-dict.bin").path
        return fileManager.fileExists(atPath: path) ? path : nil
    }

    /// モデルディレクトリのパス（コンテナ内の data/models）
    func modelsDir() -> String? {
        guard let container = containerURL else { return nil }
        let path = container.appendingPathComponent("data/models").path
        let isDir = UnsafeMutablePointer<ObjCBool>.allocate(capacity: 1)
        defer { isDir.deallocate() }

        guard fileManager.fileExists(atPath: path, isDirectory: isDir),
              isDir.pointee.boolValue else { return nil }
        return path
    }

    /// 結果を追記（同期的に fsync）
    func append(result: ProbeResult) throws {
        guard let container = containerURL else {
            throw NSError(domain: "SharedStore", code: 1, userInfo: [NSLocalizedDescriptionKey: "App Group コンテナが利用不可"])
        }

        let resultsURL = container.appendingPathComponent(resultsFileName)

        // ディレクトリを作成（必要に応じて）
        try fileManager.createDirectory(at: container, withIntermediateDirectories: true)

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
    func read(last: Int = 100) -> [ProbeResult] {
        guard let container = containerURL else { return [] }

        let resultsURL = container.appendingPathComponent(resultsFileName)
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
        guard let container = containerURL else { return }
        let resultsURL = container.appendingPathComponent(resultsFileName)
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
