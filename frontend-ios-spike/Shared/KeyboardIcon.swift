import Foundation

// キーの icon の書式と、Iconify から取り込んだアイコンの保存先・ライセンス情報
// （docs/ios-keyboard-layout.md「アイコン」、docs/decisions.md H-36）。
// 通信・SVG の変換はホストアプリ（App/IconifyResolver.swift）だけが行う。ここは書式の解釈と保存先・
// ライセンスの読み書きだけを持つ純粋な処理で、単体テスト（Tests/KeyboardIconTests.swift）の対象にしている。

/// `icon` の解釈結果
enum KeyboardIconRef: Equatable {
    /// SF Symbols（`sf:<名>` と、接頭辞なしの従来の書式）
    case symbol(String)
    /// Iconify（`iconify:<prefix>:<name>`）
    case iconify(prefix: String, name: String)

    /// Iconify の prefix / name の書式（Iconify の規則: 小文字英数字とハイフン区切り）
    static let iconifyPartPattern = "[a-z0-9]+(-[a-z0-9]+)*"
    static let iconifyPattern = "^iconify:\(iconifyPartPattern):\(iconifyPartPattern)\\z"
    static let symbolPattern = "^sf:[A-Za-z0-9._-]+\\z"

    /// 書式が不正なら理由（日本語）。`sf:` / `iconify:` で始まらない文字列は従来どおり SF Symbols 名として何でも通す
    static func formatError(_ raw: String) -> String? {
        if raw.hasPrefix("iconify:") {
            return raw.range(of: iconifyPattern, options: .regularExpression) == nil
                ? "iconify: の書式は iconify:<prefix>:<name>（小文字英数字とハイフン。例 iconify:mdi:home）にしてください" : nil
        }
        if raw.hasPrefix("sf:") {
            return raw.range(of: symbolPattern, options: .regularExpression) == nil
                ? "sf: の後ろは SF Symbols 名（英数字と . _ -）にしてください" : nil
        }
        return nil
    }

    /// 書式が不正なら nil
    static func parse(_ raw: String) -> KeyboardIconRef? {
        if formatError(raw) != nil { return nil }
        if raw.hasPrefix("iconify:") {
            let parts = raw.split(separator: ":", omittingEmptySubsequences: false)
            return .iconify(prefix: String(parts[1]), name: String(parts[2]))
        }
        if raw.hasPrefix("sf:") { return .symbol(String(raw.dropFirst(3))) }
        return .symbol(raw)
    }
}

/// レイアウト内の Iconify アイコンの使用箇所
struct IconifyUse: Equatable {
    let prefix: String
    let name: String
    /// 使っているキー（エラー表示用。長押しの項目は「親キーの id」）
    let keyId: String
    /// フォールバックできる label を持つか
    let hasLabel: Bool

    var id: String { "\(prefix):\(name)" }
}

extension KeyboardLayoutDef {
    /// このレイアウトが使う Iconify アイコン（キーと長押しの項目。出現順、重複は使用箇所ごとに残す）
    func iconifyUses() -> [IconifyUse] {
        var result: [IconifyUse] = []
        func visit(_ key: LayoutKeyDef, owner: String) {
            if let raw = key.icon, case .iconify(let prefix, let name)? = KeyboardIconRef.parse(raw) {
                result.append(IconifyUse(prefix: prefix, name: name, keyId: owner, hasLabel: !(key.label ?? "").isEmpty))
            }
            for item in key.longPress ?? [] { visit(item, owner: owner) }
        }
        for row in rows { for key in row.keys { visit(key, owner: key.id) } }
        return result
    }
}

/// Iconify から取り込んだアイコンのライセンス情報（CC BY 系の表示義務のために、アイコンと一緒に保存する）
struct IconSetLicense: Codable, Equatable {
    let prefix: String
    /// アイコンセット名（例 Material Design Icons）
    let name: String
    /// ライセンス名（例 Apache 2.0）
    let licenseTitle: String
    let licenseSpdx: String?
    let licenseUrl: String?
    let authorName: String?
    let authorUrl: String?
}

enum KeyboardIconStore {
    /// 取り込んだアイコンのフォルダ（App Group の `keyboard/assets/icons/`）
    static var directory: URL? {
        SharedStore.shared.containerURL?
            .appendingPathComponent("keyboard", isDirectory: true)
            .appendingPathComponent("assets", isDirectory: true)
            .appendingPathComponent("icons", isDirectory: true)
    }

    /// prefix / name は検査済みの文字（小文字英数字とハイフン）だけなので、そのままファイル名にできる
    static func fileName(prefix: String, name: String) -> String { "\(prefix)--\(name).png" }

    static func fileURL(prefix: String, name: String) -> URL? {
        directory?.appendingPathComponent(fileName(prefix: prefix, name: name))
    }

    static func exists(prefix: String, name: String) -> Bool {
        guard let url = fileURL(prefix: prefix, name: name) else { return false }
        return FileManager.default.fileExists(atPath: url.path)
    }

    static let licensesFileName = "licenses.json"

    private static var licensesURL: URL? { directory?.appendingPathComponent(licensesFileName) }

    /// 保存済みのライセンス情報（prefix 順）
    static func licenses() -> [IconSetLicense] {
        guard let url = licensesURL, let data = try? Data(contentsOf: url),
              let map = try? JSONDecoder().decode([String: IconSetLicense].self, from: data) else { return [] }
        return map.values.sorted { $0.prefix < $1.prefix }
    }

    static func hasLicense(prefix: String) -> Bool {
        licenses().contains { $0.prefix == prefix }
    }

    static func saveLicense(_ license: IconSetLicense) throws {
        guard let dir = directory, let url = licensesURL else { throw NagiLayoutError(message: "保存先（App Group）が使えません") }
        var map = Dictionary(uniqueKeysWithValues: licenses().map { ($0.prefix, $0) })
        map[license.prefix] = license
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(map).write(to: url, options: .atomic)
    }
}

/// アイコンを取得できなかった理由の区分（取り込み結果の文言を分けるため）
enum IconifyFailure: Equatable {
    /// HTTP 404（アイコン名またはアイコンセットが存在しない）。取り込み直しても解決しない
    case notFound
    /// 通信失敗・429・その他。通信できる状態で取り込み直すと取得できる可能性がある
    case other

    static func forHTTPStatus(_ status: Int) -> IconifyFailure { status == 404 ? .notFound : .other }
}

/// 取得できず label で代替するキーの説明（取り込みは続行する）
struct IconifyFallback: Equatable {
    let keyIds: [String]
    let icon: String
    let reason: String
    let failure: IconifyFailure

    var line: String { "・キー \(keyIds.joined(separator: "、")): \(icon)（\(reason)）" }

    /// 取り込み結果に添える案内。理由の区分ごとに文言を分ける（無ければ空文字）
    static func note(_ fallbacks: [IconifyFallback]) -> String {
        var text = ""
        let notFound = fallbacks.filter { $0.failure == .notFound }
        let other = fallbacks.filter { $0.failure == .other }
        if !notFound.isEmpty {
            text += "\n\n次のキーのアイコンは、アイコン名が見つかりません。名前を確認してください。それまでは label を表示します。\n"
                + notFound.map { $0.line }.joined(separator: "\n")
        }
        if !other.isEmpty {
            text += "\n\n次のキーのアイコンは取得できなかったため、label を表示します。通信できる状態で同じレイアウトを取り込み直すと、アイコンを取得します。\n"
                + other.map { $0.line }.joined(separator: "\n")
        }
        return text
    }
}

/// Iconify API の URL と応答の解釈（通信そのものは App/IconifyResolver.swift）
enum IconifyAPI {
    static let host = "api.iconify.design"
    /// ラスタライズの高さ（px）。@3x で 32pt 相当
    static let rasterHeight = 96

    static func svgURL(prefix: String, name: String) -> URL? {
        URL(string: "https://\(host)/\(prefix)/\(name).svg?height=\(rasterHeight)")
    }

    static func collectionURL(prefix: String) -> URL? {
        URL(string: "https://\(host)/collection?prefix=\(prefix)&info=true")
    }

    /// `/collection` の応答からライセンス情報を取り出す。`info` の中にある形とトップレベルにある形の両方を受ける。
    /// ライセンス名が読めなければ nil（表示義務を満たせないので、呼び出し側は取り込みを失敗扱いにする）
    static func parseCollection(prefix: String, data: Data) -> IconSetLicense? {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return nil }
        let info = (root["info"] as? [String: Any]) ?? root
        guard let license = info["license"] as? [String: Any] else { return nil }
        let title = (license["title"] as? String) ?? (license["spdx"] as? String)
        guard let licenseTitle = title, !licenseTitle.isEmpty else { return nil }
        let author = info["author"] as? [String: Any]
        let setName = (info["name"] as? String) ?? (root["title"] as? String) ?? prefix
        return IconSetLicense(
            prefix: prefix,
            name: setName,
            licenseTitle: licenseTitle,
            licenseSpdx: license["spdx"] as? String,
            licenseUrl: license["url"] as? String,
            authorName: author?["name"] as? String,
            authorUrl: author?["url"] as? String
        )
    }

    /// SVG の応答として受け入れるか（大きさと先頭の確認）。SVG は `<img>` 経由でしか描画しないため
    /// スクリプトや外部参照は実行・読み込みされないが、想定外の応答は保存しない
    static func isAcceptableSVG(_ data: Data) -> Bool {
        guard data.count > 0, data.count <= 256 * 1024, let text = String(data: data, encoding: .utf8) else { return false }
        return text.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("<svg")
    }
}
