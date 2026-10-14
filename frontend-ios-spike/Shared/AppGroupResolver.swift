import Foundation

/// App Group ID の実行時解決結果
struct AppGroupResolution {
    /// 候補 ID の一覧（重複排除）
    let candidates: [String]
    /// embedded.mobileprovision から読み取った application-groups
    let provisionGroups: [String]
    /// Bundle.main.infoDictionary の ALTAppGroups
    let altGroups: [String]
    /// 実際に使える App Group ID（nil の場合は利用不可）
    let resolvedID: String?
    /// コンテナ URL（resolvedID が非 nil なら必ず非 nil）
    let containerURL: URL?
    /// embedded.mobileprovision が読み取り可能だったか
    let provisionReadable: Bool
}

/// App Group ID を実行時に解決し、キャッシュする
class AppGroupResolver {
    private static let shared = AppGroupResolver()
    private var cachedResolution: AppGroupResolution?
    private let fileManager = FileManager.default

    static func resolve() -> AppGroupResolution {
        Self.shared.resolveOnce()
    }

    private func resolveOnce() -> AppGroupResolution {
        if let cached = cachedResolution {
            return cached
        }

        let result = performResolve()
        cachedResolution = result
        return result
    }

    private func performResolve() -> AppGroupResolution {
        var candidates: [String] = []
        var provisionGroups: [String] = []
        var altGroups: [String] = []
        var provisionReadable = false

        // (a) embedded.mobileprovision からアプリケーショングループを抽出
        let bundlePath = Bundle.main.bundlePath
        let provisionPath = (bundlePath as NSString).appendingPathComponent("embedded.mobileprovision")
        if fileManager.fileExists(atPath: provisionPath) {
            provisionReadable = true
            if let groups = extractGroupsFromProvisioning(path: provisionPath) {
                provisionGroups = groups
                candidates.append(contentsOf: groups)
            }
        }

        // (b) Bundle.main.infoDictionary から ALTAppGroups を取得
        if let altGroupsArray = Bundle.main.infoDictionary?["ALTAppGroups"] as? [String] {
            altGroups = altGroupsArray
            candidates.append(contentsOf: altGroupsArray)
        }

        // (c) 固定値を候補に追加
        let fixedID = "group.com.shiki01.nagi"
        candidates.append(fixedID)

        // 重複排除（順序を保持）
        candidates = Array(Set(candidates)).sorted()

        // 各候補について containerURL を試す
        var resolvedID: String? = nil
        var containerURL: URL? = nil

        for candidate in candidates {
            if let container = fileManager.containerURL(forSecurityApplicationGroupIdentifier: candidate) {
                resolvedID = candidate
                containerURL = container
                break
            }
        }

        return AppGroupResolution(
            candidates: candidates,
            provisionGroups: provisionGroups,
            altGroups: altGroups,
            resolvedID: resolvedID,
            containerURL: containerURL,
            provisionReadable: provisionReadable
        )
    }

    /// embedded.mobileprovision から Entitlements の application-groups を抽出
    /// - Returns: グループ ID の配列、またはパース失敗時は nil
    private func extractGroupsFromProvisioning(path: String) -> [String]? {
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)) else {
            return nil
        }

        // <?xml から </plist> までのバイト列を抽出
        guard let xmlStart = data.range(of: Data("<?xml".utf8)) else {
            return nil
        }
        guard let xmlEnd = data.range(of: Data("</plist>".utf8), in: xmlStart.lowerBound..<data.endIndex) else {
            return nil
        }

        let xmlData = data.subdata(in: xmlStart.lowerBound..<xmlEnd.upperBound)

        do {
            if let plist = try PropertyListSerialization.propertyList(from: xmlData, options: [], format: nil) as? [String: Any],
               let entitlements = plist["Entitlements"] as? [String: Any] {

                // com.apple.security.application-groups または application-groups から取得
                if let groups = entitlements["com.apple.security.application-groups"] as? [String] {
                    return groups
                } else if let groups = entitlements["application-groups"] as? [String] {
                    return groups
                }
            }
        } catch {
            return nil
        }

        return nil
    }
}
