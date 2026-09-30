import Foundation

/// `--register` の結果。判定と表示文言は OS 非依存（テスト可能）。
public struct RegistrationReport: Equatable {
    /// `TISRegisterInputSource` の OSStatus
    public var registerStatus: Int32
    /// 検索で見つかった入力ソースの数
    public var foundSourceCount: Int
    /// 見つかった各入力ソースへの `TISEnableInputSource` の OSStatus
    public var enableStatuses: [Int32]
    /// 0件だった場合の追加診断（全入力ソース列挙の結果など）。表示文言にそのまま出す
    public var diagnostics: [String]

    public init(
        registerStatus: Int32, foundSourceCount: Int, enableStatuses: [Int32],
        diagnostics: [String] = []
    ) {
        self.registerStatus = registerStatus
        self.foundSourceCount = foundSourceCount
        self.enableStatuses = enableStatuses
        self.diagnostics = diagnostics
    }

    public var succeeded: Bool {
        registerStatus == 0 && foundSourceCount > 0 && enableStatuses.count == foundSourceCount
            && enableStatuses.allSatisfy { $0 == 0 }
    }

    public var text: String {
        var lines = [
            "入力ソースの登録: \(registerStatus == 0 ? "成功" : "失敗") (OSStatus \(registerStatus))",
            "見つかった入力ソース数: \(foundSourceCount)",
        ]
        for (i, s) in enableStatuses.enumerated() {
            lines.append("有効化 #\(i + 1): \(s == 0 ? "成功" : "失敗") (OSStatus \(s))")
        }
        if foundSourceCount == 0 {
            lines.append("入力ソースが見つかりません（Info.plist の ComponentInputModeDict と配置場所を確認してください）")
        }
        lines.append(contentsOf: diagnostics)
        lines.append(succeeded ? "結果: 成功" : "結果: 失敗")
        return lines.joined(separator: "\n")
    }
}

#if os(macOS)
import Carbon

/// TIS（Text Input Services）への明示登録。~/Library/Input Methods に置いただけでは
/// 入力ソース一覧に出ない場合に、インストーラから呼ぶ。
public enum InputSourceRegistrar {
    /// 実行中バンドルの ID（Info.plist の CFBundleIdentifier）
    public static var bundleID: String { BundleIdentity.resolve(Bundle.main.bundleIdentifier) }
    /// 入力モード ID（`<bundle-id>.Japanese`）
    public static var inputSourceID: String { BundleIdentity.inputSourceID(bundleID: bundleID) }

    // kTISProperty* の値（SDK ヘッダの文字列定数と同じ）。定数の Swift 側の型
    // （CFString! / Unmanaged）が SDK で異なりうるため、リテラルで指定する。
    private static let keyInputSourceID = "TISPropertyInputSourceID"
    private static let keyBundleID = "TISPropertyBundleID"

    /// 検索リトライの間隔（秒）と最大回数（約 5 秒）
    private static let retryInterval: TimeInterval = 0.5
    private static let maxAttempts = 10

    public static func register(appURL: URL) -> RegistrationReport {
        let registerStatus = TISRegisterInputSource(appURL as CFURL)

        // 登録の反映に時間がかかることがあるため、見つかるまでリトライする
        var sources = findTargets()
        var attempt = 1
        while sources.isEmpty && attempt < maxAttempts {
            Thread.sleep(forTimeInterval: retryInterval)
            sources = findTargets()
            attempt += 1
        }

        var diagnostics: [String] = []
        if sources.isEmpty {
            diagnostics = diagnoseAll(attempts: attempt)
        }

        let statuses = sources.map { TISEnableInputSource($0) }
        return RegistrationReport(
            registerStatus: registerStatus, foundSourceCount: sources.count,
            enableStatuses: statuses, diagnostics: diagnostics)
    }

    /// モード ID 一致と bundle id 一致の和集合（重複は入力ソース ID で除く）
    private static func findTargets() -> [TISInputSource] {
        var sources: [TISInputSource] = []
        var seen = Set<String>()
        for filter in [[keyInputSourceID: inputSourceID], [keyBundleID: bundleID]] {
            for source in find(filter as CFDictionary) {
                let id = identifier(of: source)
                if seen.insert(id).inserted { sources.append(source) }
            }
        }
        return sources
    }

    /// 全インストール済み入力ソースを列挙し、"nagi" を含むものを診断として返す。
    private static func diagnoseAll(attempts: Int) -> [String] {
        var lines = ["検索を \(attempts) 回試行しても見つかりませんでした。全入力ソースを列挙します"]
        let all = find(nil)
        let matches = all.compactMap { source -> String? in
            let id = property(source, keyInputSourceID) ?? "(ID なし)"
            let bundle = property(source, keyBundleID) ?? "(bundle ID なし)"
            let hit = id.lowercased().contains("nagi") || bundle.lowercased().contains("nagi")
            return hit ? "  \(id) (bundle: \(bundle))" : nil
        }
        if matches.isEmpty {
            lines.append("\"nagi\" を含む入力ソースはありません（列挙できた入力ソースの総数: \(all.count)）")
            lines.append(
                "ヒント: Info.plist の内容を `plutil -lint` で検査し、システム設定を開き直してください")
        } else {
            lines.append("\"nagi\" を含む入力ソース:")
            lines.append(contentsOf: matches)
        }
        return lines
    }

    private static func find(_ filter: CFDictionary?) -> [TISInputSource] {
        // includeAllInstalled: true（無効な入力ソースも対象にする）
        guard let list = TISCreateInputSourceList(filter, true)?.takeRetainedValue() else {
            return []
        }
        let count = CFArrayGetCount(list)
        var result: [TISInputSource] = []
        for i in 0..<count {
            guard let p = CFArrayGetValueAtIndex(list, i) else { continue }
            result.append(Unmanaged<TISInputSource>.fromOpaque(p).takeUnretainedValue())
        }
        return result
    }

    private static func property(_ source: TISInputSource, _ key: String) -> String? {
        guard let raw = TISGetInputSourceProperty(source, key as CFString) else { return nil }
        return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
    }

    private static func identifier(of source: TISInputSource) -> String {
        property(source, keyInputSourceID) ?? UUID().uuidString
    }
}
#endif
