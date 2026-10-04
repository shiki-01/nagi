import Foundation

/// レイアウト JSON の読み込み。バンドル同梱の `Layouts/*.json` を読み、
/// ユーザー定義（`<user_dir>/layouts/*.json`）を後から重ねる（同じ `id` は上書き、新しい `id` は追加）
enum LayoutStore {
    /// バンドル内のレイアウトのフォルダ名（XcodeGen の folder リソース）
    static let bundleFolder = "Layouts"
    /// ユーザー定義のレイアウトを置くフォルダ名（`user_dir` の下）
    static let userFolder = "layouts"
    /// 既定のレイアウト ID
    static let defaultLayoutID = "qwerty-romaji"

    struct LoadResult {
        var layouts: [KeyboardLayout]
        /// 読み込めなかったファイルの理由（入力内容は含まない）
        var errors: [String]
    }

    static func decode(_ data: Data) throws -> KeyboardLayout {
        return try JSONDecoder().decode(KeyboardLayout.self, from: data)
    }

    static func load(bundle: Bundle, userDir: String?) -> LoadResult {
        var layouts: [KeyboardLayout] = []
        var errors: [String] = []

        if let urls = bundle.urls(forResourcesWithExtension: "json", subdirectory: bundleFolder) {
            for url in urls.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
                append(url: url, into: &layouts, errors: &errors)
            }
        } else {
            errors.append("バンドルに \(bundleFolder)/ が見つかりません")
        }

        if let userDir = userDir {
            let dir = URL(fileURLWithPath: userDir).appendingPathComponent(userFolder)
            if let urls = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) {
                for url in urls.sorted(by: { $0.lastPathComponent < $1.lastPathComponent })
                where url.pathExtension.lowercased() == "json" {
                    append(url: url, into: &layouts, errors: &errors)
                }
            }
        }
        return LoadResult(layouts: layouts, errors: errors)
    }

    private static func append(url: URL, into layouts: inout [KeyboardLayout], errors: inout [String]) {
        let name = url.lastPathComponent
        guard let data = try? Data(contentsOf: url) else {
            errors.append("\(name): 読み込めません")
            return
        }
        let layout: KeyboardLayout
        do {
            layout = try decode(data)
        } catch {
            errors.append("\(name): JSON の形式が不正です")
            return
        }
        let problems = layout.validationErrors()
        if !problems.isEmpty {
            errors.append("\(name): \(problems.joined(separator: " / "))")
            return
        }
        if let i = layouts.firstIndex(where: { $0.id == layout.id }) {
            layouts[i] = layout
        } else {
            layouts.append(layout)
        }
    }

    /// ユーザー定義のレイアウトとして保存する（`<user_dir>/layouts/<id>.json`）。
    /// 検査に通らなければ理由の配列を返す。保存できたら空配列
    static func saveUserLayout(jsonText: String, userDir: String) -> [String] {
        guard let data = jsonText.data(using: .utf8) else { return ["文字コードが不正です"] }
        let layout: KeyboardLayout
        do {
            layout = try decode(data)
        } catch {
            return ["JSON の形式が不正です（レイアウト定義として読めません）"]
        }
        let problems = layout.validationErrors()
        if !problems.isEmpty { return problems }
        // ファイル名に使える文字だけを許す（パス区切りの混入を防ぐ）
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_"))
        if layout.id.unicodeScalars.contains(where: { !allowed.contains($0) }) {
            return ["id には英数字・-・_ だけを使ってください: \(layout.id)"]
        }
        let dir = URL(fileURLWithPath: userDir).appendingPathComponent(userFolder)
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try data.write(to: dir.appendingPathComponent(layout.id + ".json"), options: .atomic)
        } catch {
            return ["保存できません: \(error.localizedDescription)"]
        }
        return []
    }
}
