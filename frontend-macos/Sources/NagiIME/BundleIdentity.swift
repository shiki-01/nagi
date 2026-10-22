import Foundation
import NagiClient

/// バンドル ID から入力モード ID・接続名を派生する純粋関数群。
/// `cargo xtask bundle-macos --bundle-id` の派生規則（`<id>.Japanese` / `<id>.Roman` / `<id>_Connection`）と一致させること。
public enum BundleIdentity {
    /// `Bundle.main.bundleIdentifier` が取れないときの既定値。
    /// 入力メソッドとして認識されるには `.inputmethod.` を含む必要がある。
    /// 設定/データの識別子（`com.shiki.nagi`。Application Support のディレクトリ名）とは別物
    public static let fallbackBundleID = "com.shiki.inputmethod.Nagi"

    /// 実行中バンドルの ID（nil・空なら既定値）
    public static func resolve(_ bundleIdentifier: String?) -> String {
        guard let id = bundleIdentifier, !id.isEmpty else { return fallbackBundleID }
        return id
    }

    /// 入力モード ID（`<bundle-id>.Japanese`）。コアのひらがなモードに対応する
    public static func inputSourceID(bundleID: String) -> String {
        "\(bundleID).Japanese"
    }

    /// 英数モードの入力モード ID（`<bundle-id>.Roman`）。コアの英数モードに対応する
    public static func romanInputSourceID(bundleID: String) -> String {
        "\(bundleID).Roman"
    }

    /// コアの入力モード → OS の入力モード ID（Info.plist の ComponentInputModeDict のキー）
    public static func modeIdentifiers(bundleID: String) -> [InputMode: String] {
        [
            .hiragana: inputSourceID(bundleID: bundleID),
            .alphanumeric: romanInputSourceID(bundleID: bundleID),
        ]
    }

    /// IMKServer の接続名（`<bundle-id>_Connection`）
    public static func connectionName(bundleID: String) -> String {
        "\(bundleID)_Connection"
    }
}
