import Foundation
import WebKit

// レイアウト取り込み時の Iconify アイコンの解決（docs/decisions.md H-36）。
// 通信するのはホストアプリのここだけ。キーボード拡張は保存済みの PNG を読むだけで、通信しない。
// 流れ: レイアウトが使うアイコンを 1 件ずつ → 取得済みなら何もしない → ライセンス情報（アイコンセット単位）と
// SVG を取得 → WKWebView の canvas で PNG にして `keyboard/assets/icons/` へ保存。

/// 解決の結果
struct IconifyResolution {
    /// 取得できず、label で代替するキー（取り込みは続行する）
    var fallbacks: [IconifyFallback] = []
}

/// HTTP 200 以外の応答（404 は「名前が無い」として文言を分けるため、状態コードを保つ）
private struct IconifyHTTPError: LocalizedError {
    let statusCode: Int
    var errorDescription: String? { "HTTP \(statusCode)" }
}

enum IconifyResolver {
    /// 1 回の取り込みで新しく取得するアイコンの上限（巨大なレイアウトや悪意ある URL による大量通信を避ける）
    static let maxFetchesPerImport = 64

    private static let session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 10
        config.timeoutIntervalForResource = 20
        config.waitsForConnectivity = false
        config.httpCookieStorage = nil
        return URLSession(configuration: config)
    }()

    /// レイアウトの Iconify アイコンを解決する。
    /// 取得できないアイコンのキーに label があれば `fallbacks` に載せて続行し、label が無ければ投げる
    static func resolve(_ def: KeyboardLayoutDef) async throws -> IconifyResolution {
        var resolution = IconifyResolution()
        let uses = def.iconifyUses()
        // 使用箇所を (prefix, name) ごとにまとめる（出現順）
        var order: [String] = []
        var groups: [String: [IconifyUse]] = [:]
        for use in uses {
            if groups[use.id] == nil { order.append(use.id) }
            groups[use.id, default: []].append(use)
        }
        var fetched = 0
        for id in order {
            guard let group = groups[id], let first = group.first else { continue }
            // 取得済み（アイコンとライセンスが揃っている）なら再取得しない
            if KeyboardIconStore.exists(prefix: first.prefix, name: first.name),
               KeyboardIconStore.hasLicense(prefix: first.prefix) { continue }
            fetched += 1
            if fetched > maxFetchesPerImport {
                throw NagiLayoutError(message: "新しく取得するアイコンが多すぎます（\(maxFetchesPerImport) 種類まで）")
            }
            do {
                try await fetchAndStore(prefix: first.prefix, name: first.name)
            } catch {
                let reason = (error as? NagiLayoutError)?.message ?? error.localizedDescription
                let keys = group.map { $0.keyId }
                let withoutLabel = group.filter { !$0.hasLabel }.map { $0.keyId }
                if !withoutLabel.isEmpty {
                    throw NagiLayoutError(message:
                        "キー \(Array(Set(withoutLabel)).sorted().joined(separator: "、")) のアイコン iconify:\(first.prefix):\(first.name) を取得できず、label も無いため取り込めません（\(reason)）")
                }
                let failure = (error as? IconifyHTTPError).map { IconifyFailure.forHTTPStatus($0.statusCode) } ?? .other
                resolution.fallbacks.append(IconifyFallback(
                    keyIds: Array(Set(keys)).sorted(),
                    icon: "iconify:\(first.prefix):\(first.name)",
                    reason: reason,
                    failure: failure))
            }
        }
        return resolution
    }

    private static func fetchAndStore(prefix: String, name: String) async throws {
        guard let dir = KeyboardIconStore.directory else { throw NagiLayoutError(message: "保存先（App Group）が使えません") }
        // 1. ライセンス情報（アイコンセット単位。無ければアイコンは保存しない）
        var license: IconSetLicense?
        if !KeyboardIconStore.hasLicense(prefix: prefix) {
            guard let url = IconifyAPI.collectionURL(prefix: prefix) else { throw NagiLayoutError(message: "URL を作れません") }
            let data = try await get(url)
            guard let parsed = IconifyAPI.parseCollection(prefix: prefix, data: data) else {
                throw NagiLayoutError(message: "アイコンセット \(prefix) のライセンスを確認できません")
            }
            license = parsed
        }
        // 2. SVG → PNG
        guard let svgURL = IconifyAPI.svgURL(prefix: prefix, name: name) else { throw NagiLayoutError(message: "URL を作れません") }
        let svg = try await get(svgURL)
        guard IconifyAPI.isAcceptableSVG(svg), let text = String(data: svg, encoding: .utf8) else {
            throw NagiLayoutError(message: "アイコンが見つかりません")
        }
        let png = try await IconRasterizer.png(svg: text, side: IconifyAPI.rasterHeight)
        // 3. 保存（アイコンより先にライセンスを書く。アイコンがあればライセンスもある、を保つ）
        if let license = license { try KeyboardIconStore.saveLicense(license) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent(KeyboardIconStore.fileName(prefix: prefix, name: name))
        try png.write(to: file, options: .atomic)
    }

    private static func get(_ url: URL) async throws -> Data {
        do {
            let (data, response) = try await session.data(from: url)
            guard let http = response as? HTTPURLResponse else { throw NagiLayoutError(message: "応答が不正です") }
            guard http.statusCode == 200 else { throw IconifyHTTPError(statusCode: http.statusCode) }
            return data
        } catch let error as NagiLayoutError {
            throw error
        } catch let error as IconifyHTTPError {
            throw error
        } catch {
            throw NagiLayoutError(message: "通信できません（\(error.localizedDescription)）")
        }
    }
}

/// SVG を PNG にする。WKWebView（画面に載せない）で SVG を `<img>` として読み、canvas に描いて取り出す。
/// SVG の解釈は WebKit に任せ、自前のパーサは持たない。`<img>` 経由なので SVG 内のスクリプト・外部参照は動かない。
/// 色は currentColor を黒にして描き、使う側がテンプレート画像として前景色で塗る（アルファだけを使う）
@MainActor
private final class IconRasterizer: NSObject, WKNavigationDelegate {
    private var loaded: CheckedContinuation<Void, Error>?

    static func png(svg: String, side: Int) async throws -> Data {
        let rasterizer = IconRasterizer()
        return try await rasterizer.render(svg: svg, side: side)
    }

    private func render(svg: String, side: Int) async throws -> Data {
        let webView = WKWebView(frame: CGRect(x: 0, y: 0, width: side, height: side), configuration: WKWebViewConfiguration())
        webView.navigationDelegate = self
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            loaded = continuation
            webView.loadHTMLString("<!doctype html><html><body></body></html>", baseURL: nil)
        }
        let b64 = Data(svg.replacingOccurrences(of: "currentColor", with: "#000000").utf8).base64EncodedString()
        let script = """
        return await new Promise((resolve, reject) => {
          const img = new Image();
          img.onload = () => {
            const w = img.naturalWidth, h = img.naturalHeight;
            if (!w || !h) { reject(new Error('size')); return; }
            const canvas = document.createElement('canvas');
            canvas.width = side; canvas.height = side;
            const scale = Math.min(side / w, side / h);
            canvas.getContext('2d').drawImage(img, (side - w * scale) / 2, (side - h * scale) / 2, w * scale, h * scale);
            resolve(canvas.toDataURL('image/png'));
          };
          img.onerror = () => reject(new Error('decode'));
          img.src = 'data:image/svg+xml;base64,' + b64;
        });
        """
        let dataURL: String = try await withTimeout(seconds: 10) {
            let result = try await webView.callAsyncJavaScript(script, arguments: ["b64": b64, "side": side], in: nil, contentWorld: .page)
            return (result as? String) ?? ""
        }
        let prefix = "data:image/png;base64,"
        guard dataURL.hasPrefix(prefix),
              let png = Data(base64Encoded: String(dataURL.dropFirst(prefix.count))), !png.isEmpty else {
            throw NagiLayoutError(message: "画像に変換できません")
        }
        return png
    }

    private func withTimeout<T: Sendable>(seconds: Double, _ body: @escaping () async throws -> T) async throws -> T {
        try await withThrowingTaskGroup(of: T.self) { group in
            group.addTask { try await body() }
            group.addTask {
                try await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
                throw NagiLayoutError(message: "画像への変換が時間切れになりました")
            }
            guard let first = try await group.next() else { throw NagiLayoutError(message: "画像に変換できません") }
            group.cancelAll()
            return first
        }
    }

    nonisolated func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        Task { @MainActor in
            loaded?.resume()
            loaded = nil
        }
    }

    nonisolated func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        Task { @MainActor in
            loaded?.resume(throwing: error)
            loaded = nil
        }
    }

    nonisolated func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        Task { @MainActor in
            loaded?.resume(throwing: error)
            loaded = nil
        }
    }
}
