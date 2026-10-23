import CryptoKit
import Foundation

/// 任意データ（英語の語彙・カタカナ語の原語）の取得元と配置の論理。コア・ネットワークに依存しない
/// （取得元の一覧そのものはコア `nagi_optional_data_sources()` が JSON で返し、ここで解釈する）。
/// 取得（URLSession）と索引の組み立て（コアの FFI）は `App/OptionalDataInstaller.swift`
enum OptionalDataKind: String, CaseIterable {
    case english
    case loanword

    var title: String {
        switch self {
        case .english: return "英語の語彙"
        case .loanword: return "カタカナ語の原語データ"
        }
    }

    /// 入っていないと何が起きるか
    var missingEffect: String {
        switch self {
        case .english: return "英語の自動判定が弱くなります"
        case .loanword: return "カタカナ語の原語の候補（コンピューター → computer など）が出ません"
        }
    }
}

struct OptionalDataSource: Equatable {
    let kind: OptionalDataKind
    /// `<data>/` 下のディレクトリ名
    let dir: String
    /// 取得したアーカイブの保存名
    let file: String
    let url: String
    /// アーカイブの SHA-256。nil は配布元のハッシュを固定していない（取得してはいけない）
    let sha256: String?
    /// 組み立てた索引のファイル名
    let indexFile: String

    var isDownloadable: Bool { sha256 != nil }
}

enum OptionalData {
    /// コアが返す JSON を解釈する。読めない種別は含めない
    static func parseSources(_ json: String) -> [OptionalDataKind: OptionalDataSource] {
        guard let data = json.data(using: .utf8),
            let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return [:] }
        var result: [OptionalDataKind: OptionalDataSource] = [:]
        for kind in OptionalDataKind.allCases {
            guard let item = root[kind.rawValue] as? [String: Any],
                let dir = item["dir"] as? String,
                let file = item["file"] as? String,
                let url = item["url"] as? String,
                let indexFile = item["index_file"] as? String
            else { continue }
            result[kind] = OptionalDataSource(
                kind: kind, dir: dir, file: file, url: url,
                sha256: item["sha256"] as? String, indexFile: indexFile)
        }
        return result
    }

    static func directory(_ source: OptionalDataSource, in dataDir: URL) -> URL {
        dataDir.appendingPathComponent(source.dir, isDirectory: true)
    }

    static func archiveURL(_ source: OptionalDataSource, in dataDir: URL) -> URL {
        directory(source, in: dataDir).appendingPathComponent(source.file)
    }

    static func indexURL(_ source: OptionalDataSource, in dataDir: URL) -> URL {
        directory(source, in: dataDir).appendingPathComponent(source.indexFile)
    }

    /// 索引が置かれているか（キーボード拡張が読めるのはこれだけ）
    static func isInstalled(_ source: OptionalDataSource, in dataDir: URL) -> Bool {
        FileManager.default.fileExists(atPath: indexURL(source, in: dataDir).path)
    }

    /// ファイルの SHA-256（16 進、小文字）。読めなければ nil。全体をメモリに載せない
    static func sha256Hex(of url: URL) -> String? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        var hasher = SHA256()
        while true {
            // read(upToCount:) は末尾で空の Data ではなく nil を返す（読み込みの失敗は throw）
            let chunk: Data?
            do { chunk = try handle.read(upToCount: 1 << 20) } catch { return nil }
            guard let chunk, !chunk.isEmpty else { break }
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    /// 次にすること。取得済みで SHA-256 が一致し、索引もあれば何もしない
    enum Step: Equatable {
        /// 何もしない（取得済み）
        case skip
        /// 取得して、索引を組み立てる
        case download
        /// アーカイブは取得済み（SHA-256 一致）。索引だけ組み立てる
        case buildOnly
        /// 取得してはいけない（配布元のハッシュが未固定）
        case unavailable
    }

    static func step(for source: OptionalDataSource, in dataDir: URL) -> Step {
        guard let expected = source.sha256 else { return .unavailable }
        let archive = archiveURL(source, in: dataDir)
        let archiveOK = sha256Hex(of: archive) == expected
        if archiveOK {
            return isInstalled(source, in: dataDir) ? .skip : .buildOnly
        }
        return .download
    }

    /// 一覧に出す状態の文言
    static func statusText(_ source: OptionalDataSource?, in dataDir: URL?) -> String {
        guard let source, let dataDir else { return "確認できません" }
        if isInstalled(source, in: dataDir) { return "あり" }
        return source.isDownloadable ? "なし" : "なし（配布元のハッシュが未固定のため、まだ入れられません）"
    }
}
