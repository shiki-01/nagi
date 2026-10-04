import Foundation

/// データ・ユーザーデータの置き場の解決結果。
/// スパイクの `DataPaths` と同じ考え方（App Group があればそれを使い、無ければバンドル / プロセス固有の領域）。
/// App Group の ID は `AppGroupResolver` が実行時に解決する（SideStore の再署名で ID が変わる場合に備える）
struct NagiPaths {
    /// App Group の解決結果
    let appGroup: AppGroupResolution
    /// コアへ渡す `data_dir`（`dict/nagi-dict.bin` を含むもの）。辞書が見つからなければ nil
    let dataDir: String?
    /// `data_dir` の出所: `appgroup` / `bundle` / `app_plugin` / `none`
    let dataSource: String
    /// 辞書ファイルのパス（存在するとき）
    let dictPath: String?
    /// コアへ渡す `user_dir`（`config.toml`・学習・ユーザー辞書の置き場）
    let userDir: String?
    /// `user_dir` の出所: `appgroup` / `process` / `none`
    let userDirSource: String

    /// 本体アプリのバンドル内にあるキーボード拡張の名前（辞書は拡張にだけ同梱し、本体アプリからはここを参照する）
    static let keyboardAppexName = "NagiKeyboard.appex"

    static func resolve() -> NagiPaths {
        let fileManager = FileManager.default
        let group = AppGroupResolver.resolve()

        // 辞書の探索順: (a) App Group の data/ → (b) 自プロセスのバンドル → (c) 本体アプリ内の拡張のバンドル
        var candidates: [(dir: String, source: String)] = []
        if let container = group.containerURL {
            candidates.append((container.appendingPathComponent("data").path, "appgroup"))
        }
        if let resource = Bundle.main.resourceURL {
            candidates.append((resource.appendingPathComponent("data").path, "bundle"))
        }
        if let plugIns = Bundle.main.builtInPlugInsURL {
            let appex = plugIns.appendingPathComponent(keyboardAppexName)
            candidates.append((appex.appendingPathComponent("data").path, "app_plugin"))
        }

        var dataDir: String?
        var dataSource = "none"
        var dictPath: String?
        for candidate in candidates {
            let dict = URL(fileURLWithPath: candidate.dir)
                .appendingPathComponent("dict")
                .appendingPathComponent("nagi-dict.bin").path
            if fileManager.fileExists(atPath: dict) {
                dataDir = candidate.dir
                dataSource = candidate.source
                dictPath = dict
                break
            }
        }

        // user_dir: App Group があればその中、無ければプロセス固有の Application Support
        var userDir: String?
        var userDirSource = "none"
        if let container = group.containerURL {
            let url = container.appendingPathComponent("user")
            if (try? fileManager.createDirectory(at: url, withIntermediateDirectories: true)) != nil {
                userDir = url.path
                userDirSource = "appgroup"
            }
        }
        if userDir == nil,
           let support = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            let url = support.appendingPathComponent("com.shiki.nagi")
            if (try? fileManager.createDirectory(at: url, withIntermediateDirectories: true)) != nil {
                userDir = url.path
                userDirSource = "process"
            }
        }

        return NagiPaths(
            appGroup: group,
            dataDir: dataDir,
            dataSource: dataSource,
            dictPath: dictPath,
            userDir: userDir,
            userDirSource: userDirSource
        )
    }

    /// 診断用の複数行テキスト（パスは含めるが入力内容は含まない）
    var summary: String {
        var lines: [String] = []
        lines.append("app_group: \(appGroup.resolvedID ?? "unavailable")")
        lines.append("app_group_candidates: \(appGroup.candidates.joined(separator: ","))")
        lines.append("data_dir_source: \(dataSource)")
        lines.append("dict: \(dictPath == nil ? "missing" : "present")")
        lines.append("user_dir_source: \(userDirSource)")
        return lines.joined(separator: "\n")
    }
}
