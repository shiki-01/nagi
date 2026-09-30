import Foundation

/// `--register` の結果。判定と表示文言は OS 非依存（テスト可能）。
public struct RegistrationReport: Equatable {
    /// `TISRegisterInputSource` の OSStatus
    public var registerStatus: Int32
    /// 検索で見つかった入力ソースの数
    public var foundSourceCount: Int
    /// 見つかった各入力ソースへの `TISEnableInputSource` の OSStatus
    public var enableStatuses: [Int32]

    public init(registerStatus: Int32, foundSourceCount: Int, enableStatuses: [Int32]) {
        self.registerStatus = registerStatus
        self.foundSourceCount = foundSourceCount
        self.enableStatuses = enableStatuses
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
        lines.append(succeeded ? "結果: 成功" : "結果: 失敗")
        return lines.joined(separator: "\n")
    }
}

#if os(macOS)
import Carbon

/// TIS（Text Input Services）への明示登録。~/Library/Input Methods に置いただけでは
/// 入力ソース一覧に出ない場合に、インストーラから呼ぶ。
public enum InputSourceRegistrar {
    public static let inputSourceID = "com.shiki.nagi.Japanese"
    public static let bundleID = "com.shiki.nagi"

    // kTISProperty* の値（SDK ヘッダの文字列定数と同じ）。定数の Swift 側の型
    // （CFString! / Unmanaged）が SDK で異なりうるため、リテラルで指定する。
    private static let keyInputSourceID = "TISPropertyInputSourceID"
    private static let keyBundleID = "TISPropertyBundleID"

    public static func register(appURL: URL) -> RegistrationReport {
        let registerStatus = TISRegisterInputSource(appURL as CFURL)

        // モード ID 一致と bundle id 一致の和集合（重複は入力ソース ID で除く）
        var sources: [TISInputSource] = []
        var seen = Set<String>()
        for filter in [[keyInputSourceID: inputSourceID], [keyBundleID: bundleID]] {
            for source in find(filter as CFDictionary) {
                let id = identifier(of: source)
                if seen.insert(id).inserted { sources.append(source) }
            }
        }

        let statuses = sources.map { TISEnableInputSource($0) }
        return RegistrationReport(
            registerStatus: registerStatus, foundSourceCount: sources.count,
            enableStatuses: statuses)
    }

    private static func find(_ filter: CFDictionary) -> [TISInputSource] {
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

    private static func identifier(of source: TISInputSource) -> String {
        guard let raw = TISGetInputSourceProperty(source, "TISPropertyInputSourceID" as CFString)
        else { return UUID().uuidString }
        return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
    }
}
#endif
