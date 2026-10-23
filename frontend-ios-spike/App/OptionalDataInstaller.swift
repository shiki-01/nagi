import Foundation
import NagiCore

/// 任意データ（英語の語彙・カタカナ語の原語）の取得。ネットワークはここ（本体アプリ）だけが使う
/// （キーボード拡張は通信しない。`docs/mobile-design.md` §6.3）。取得元と SHA-256 はコアが返す一覧を使い、
/// 取得したアーカイブの検証 → App Group への配置 → 索引の組み立て（コアの FFI）の順に行う。
/// 取得済みで SHA-256 が一致し、索引もあるものは飛ばす。失敗しても、辞書・モデル・すでにある索引は変わらない。
@MainActor
final class OptionalDataInstaller: ObservableObject {
    @Published private(set) var running = false
    /// 進捗または結果の文言（最後の 1 行）
    @Published private(set) var message: String?

    /// コアが返す取得元の一覧
    static func loadSources() -> [OptionalDataKind: OptionalDataSource] {
        guard let ptr = nagi_optional_data_sources() else { return [:] }
        defer { nagi_string_free(ptr) }
        return OptionalData.parseSources(String(cString: ptr))
    }

    /// 取得済みのアーカイブから索引を組み立てる（コアの FFI）。失敗の理由、成功なら nil
    nonisolated private static func buildIndex(kind: String, archive: String, out: String) -> String? {
        var failure: String?
        kind.withCString { k in
            archive.withCString { a in
                out.withCString { o in
                    if let ptr = nagi_build_optional_index(k, a, o) {
                        failure = String(cString: ptr)
                        nagi_string_free(ptr)
                    }
                }
            }
        }
        return failure
    }

    /// 足りないデータを取得して索引を組み立てる。何か入れたら true（キーボードは次に開いたとき読み込む）
    @discardableResult
    func installMissing(into dataDir: URL) async -> Bool {
        guard !running else { return false }
        running = true
        defer { running = false }
        let sources = Self.loadSources()
        var installedAny = false
        var failures: [String] = []
        var skippedUnpinned: [String] = []

        for kind in OptionalDataKind.allCases {
            guard let source = sources[kind] else { continue }
            let step = OptionalData.step(for: source, in: dataDir)
            if step == .skip { continue }
            if step == .unavailable {
                skippedUnpinned.append(kind.title)
                continue
            }
            do {
                try await install(source, step: step, into: dataDir)
                installedAny = true
            } catch {
                failures.append("\(kind.title): \(error.localizedDescription)")
            }
        }

        if !failures.isEmpty {
            message = "入れられませんでした（\(failures.joined(separator: " / "))）。通信を確認してもう一度お試しください"
        } else if installedAny {
            message = "入れました。キーボードを開き直すと反映されます"
        } else if !skippedUnpinned.isEmpty {
            message = "\(skippedUnpinned.joined(separator: "・"))は、配布元のハッシュが未固定のため、まだ入れられません"
        } else {
            message = "すべて取得済みです"
        }
        return installedAny
    }

    private func install(_ source: OptionalDataSource, step: OptionalData.Step, into dataDir: URL) async throws {
        let fm = FileManager.default
        let dir = OptionalData.directory(source, in: dataDir)
        let archive = OptionalData.archiveURL(source, in: dataDir)
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)

        if step == .download {
            message = "\(source.kind.title)をダウンロードしています"
            guard let url = URL(string: source.url), url.scheme == "https" else {
                throw OptionalDataError.badURL
            }
            // 圧縮の自動展開（Accept-Encoding の既定）を避け、配布元のバイト列そのものを受け取る
            var request = URLRequest(url: url)
            request.setValue("identity", forHTTPHeaderField: "Accept-Encoding")
            let (temp, response) = try await URLSession.shared.download(for: request)
            defer { try? fm.removeItem(at: temp) }
            if let http = response as? HTTPURLResponse, http.statusCode != 200 {
                throw OptionalDataError.http(http.statusCode)
            }
            // 検証に通るまでは最終の場所へ置かない（壊れたデータで今あるものを置き換えない）
            let actual = OptionalData.sha256Hex(of: temp)
            guard actual == source.sha256 else {
                // 原因の切り分け用に、実際のハッシュ・サイズ・応答ヘッダーを残す
                let size = (try? fm.attributesOfItem(atPath: temp.path)[.size] as? Int) ?? 0
                let http = response as? HTTPURLResponse
                let type = http?.value(forHTTPHeaderField: "Content-Type") ?? "-"
                let encoding = http?.value(forHTTPHeaderField: "Content-Encoding") ?? "-"
                throw OptionalDataError.checksumMismatch(
                    "実際: \((actual ?? "読めません").prefix(12))… \(size) バイト, 種別 \(type), 圧縮 \(encoding)")
            }
            try? fm.removeItem(at: archive)
            try fm.moveItem(at: temp, to: archive)
        }

        message = "\(source.kind.title)を組み立てています"
        let kind = source.kind.rawValue
        let archivePath = archive.path
        let outPath = OptionalData.indexURL(source, in: dataDir).path
        let failure = await Task.detached(priority: .userInitiated) {
            Self.buildIndex(kind: kind, archive: archivePath, out: outPath)
        }.value
        if let failure { throw OptionalDataError.build(failure) }
    }
}

enum OptionalDataError: LocalizedError {
    case badURL
    case http(Int)
    case checksumMismatch(String)
    case build(String)

    var errorDescription: String? {
        switch self {
        case .badURL: return "取得元の URL が不正です"
        case .http(let code): return "配布元がエラーを返しました（HTTP \(code)）"
        case .checksumMismatch(let detail): return "SHA-256 が一致しません（\(detail)）"
        case .build(let reason): return "索引を組み立てられません（\(reason)）"
        }
    }
}
