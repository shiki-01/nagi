import Foundation

// レイアウト取り込みの URL スキーム（nagi://layout?data=<base64url の JSON>）の解釈。
// アプリの画面・保存先に依存しない純粋な処理なので、単体テスト（Tests/LayoutURLTests.swift）の対象にしている。

struct NagiLayoutError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

enum NagiLayoutURL {
    /// URL の `data`（base64url 文字列）の長さの上限。デコード前に検査する
    /// （`assets` を含まない JSON は実質 512KB 以下なので、base64 で約 683K 文字になる分に余裕を持たせた 1MB。
    /// `assets` を含むレイアウトはこの上限を超えうるので、URL ではなくファイル / コピーで渡す）
    static let maxDataChars = 1024 * 1024

    /// `nagi://layout?data=<base64url の JSON>` から JSON を取り出す
    static func jsonData(fromURL url: URL) throws -> Data {
        guard url.scheme?.lowercased() == "nagi", url.host?.lowercased() == "layout" else {
            throw NagiLayoutError(message: "対応していない URL です")
        }
        // クエリ全体の長さも先に見る（巨大な URL を解析・コピーする前に弾く）
        if url.absoluteString.utf8.count > maxDataChars + 4096 {
            throw NagiLayoutError(message: "data が大きすぎます（\(maxDataChars / 1024) KB まで）")
        }
        guard let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
              let encoded = items.first(where: { $0.name == "data" })?.value, !encoded.isEmpty else {
            throw NagiLayoutError(message: "URL に data がありません")
        }
        if encoded.utf8.count > maxDataChars {
            throw NagiLayoutError(message: "data が大きすぎます（\(maxDataChars / 1024) KB まで）")
        }
        var base64 = encoded.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while base64.count % 4 != 0 { base64 += "=" }
        guard let data = Data(base64Encoded: base64) else {
            throw NagiLayoutError(message: "data を base64url として読み取れません")
        }
        return data
    }

    /// JSON から `nagi://layout?data=...` の URL を作る（パディングなしの base64url。サイト側と同じ形）。テスト用
    static func url(forJSON data: Data) -> URL? {
        let encoded = data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
        return URL(string: "nagi://layout?data=\(encoded)")
    }
}
