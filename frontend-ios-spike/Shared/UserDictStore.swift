import Foundation

// ユーザー辞書（Mozc TSV 互換）のファイルの読み書き。
// 置き場所は `<user_dir>/user_dict/`（コアが `*.txt` / `*.tsv` をファイル名順にすべて読む）。
// このアプリが編集するのは `nagi-user.tsv` だけで、ほかのファイルは読み取り専用で表示する。

// 解析・書き出し・検証（Mozc TSV の純粋な部分）は `MozcTSV.swift`。ここはファイルと場所の扱い。

enum NagiUserDictStore {
    /// スニペットを表す品詞名（core/dict の `SNIPPET_POS`）
    static let snippetPos = NagiMozcTSV.snippetPos
    /// このアプリが編集するファイル
    static let ownFileName = "nagi-user.tsv"
    /// 編集対象ファイルの件数の上限（`userdict.max_entries` の既定。docs/ios-settings.md §2.4）
    static let maxOwnEntries = 10000
    /// スニペット本文の上限（文字数。docs/ios-settings.md §2.4）
    static let maxSnippetChars = NagiMozcTSV.maxSnippetChars
    /// 取り込むファイルの大きさの上限（バイト）
    static let maxImportBytes = 16 * 1024 * 1024

    /// 画面で選べる品詞（コアの `pos_patterns` が対応づける Mozc の品詞名）
    static let posChoices = [
        "名詞", "固有名詞", "人名", "姓", "名", "地名", "組織", "顔文字", "記号",
        "副詞", "接続詞", "感動詞", "連体詞", "形容詞", "名詞サ変",
    ]

    // MARK: - 場所

    /// `<user_dir>/user_dict/` と `<user_dir>`。キーボード拡張と同じ `<App Group>/user` を使う
    static func locate() -> (userDict: URL?, userDir: URL?, message: String?) {
        let store = SharedStore.shared
        let prepared = ProductSession.prepareUserDir(
            store: store,
            modelPath: ProductSession.preferredModelPath(store: store)
        )
        guard let path = prepared.path else {
            return (nil, nil, "保存先を作れません（\(prepared.detail)）")
        }
        let userDir = URL(fileURLWithPath: path, isDirectory: true)
        return (userDir.appendingPathComponent("user_dict", isDirectory: true), userDir, nil)
    }

    // MARK: - Mozc TSV（`NagiMozcTSV` への委譲）

    static func decodeText(_ data: Data) -> String { NagiMozcTSV.decodeText(data) }
    static func toHiragana(_ s: String) -> String { NagiMozcTSV.toHiragana(s) }
    static func parse(_ text: String, file: String) -> (entries: [NagiUserDictEntry], issues: [NagiUserDictIssue]) {
        NagiMozcTSV.parse(text, file: file)
    }
    static func serialize(_ entries: [NagiUserDictEntry]) -> String { NagiMozcTSV.serialize(entries) }
    static func validate(_ entry: inout NagiUserDictEntry) -> String? { NagiMozcTSV.validate(&entry) }

    // MARK: - ファイル

    private static func isDictFile(_ url: URL) -> Bool {
        let ext = url.pathExtension.lowercased()
        return ext == "txt" || ext == "tsv"
    }

    /// `user_dir/user_dict/` を読む。ディレクトリが無ければ空。
    /// ディレクトリはあるのに一覧を取れないときは `ownError` を立てる（書き込みを止め、空で上書きしない）
    static func load(userDict dir: URL) -> NagiUserDictSnapshot {
        var snapshot = NagiUserDictSnapshot()
        let fileManager = FileManager.default
        var isDir: ObjCBool = false
        guard fileManager.fileExists(atPath: dir.path, isDirectory: &isDir) else { return snapshot }
        guard isDir.boolValue else {
            snapshot.ownError = "user_dict がディレクトリではありません"
            return snapshot
        }
        let urls: [URL]
        do {
            urls = try fileManager.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        } catch {
            snapshot.ownError = "user_dict を読み込めません: \(error.localizedDescription)"
            return snapshot
        }
        let files = urls.filter { isDictFile($0) }.sorted { $0.lastPathComponent < $1.lastPathComponent }
        for url in files {
            let name = url.lastPathComponent
            var isDirectory: ObjCBool = false
            if fileManager.fileExists(atPath: url.path, isDirectory: &isDirectory), isDirectory.boolValue { continue }
            let isOwn = name == ownFileName
            let data: Data
            do {
                data = try Data(contentsOf: url)
            } catch {
                if isOwn {
                    snapshot.ownError = "\(name) を読み込めません: \(error.localizedDescription)"
                } else {
                    snapshot.otherIssues.append(NagiUserDictIssue(file: name, line: 0, message: "読み込めません: \(error.localizedDescription)"))
                }
                continue
            }
            let parsed = parse(decodeText(data), file: name)
            if isOwn {
                snapshot.own = parsed.entries
                snapshot.ownIssues = parsed.issues
            } else {
                snapshot.readOnly += parsed.entries.map { e -> NagiUserDictEntry in
                    var copy = e
                    copy.file = name
                    return copy
                }
                snapshot.otherIssues += parsed.issues
            }
        }
        return snapshot
    }

    /// 編集対象のファイルを原子的に書く（一時ファイル → 置き換え）。
    /// 一時ファイルは `user_dict/` の外（`<user_dir>`）に作る。コアは `user_dict/` の中身の変化を見て読み直す
    static func writeOwn(_ entries: [NagiUserDictEntry], userDict dir: URL, userDir: URL) throws {
        let fileManager = FileManager.default
        try fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent(ownFileName)
        let temp = userDir.appendingPathComponent(".nagi-user.tsv.tmp")
        try? fileManager.removeItem(at: temp)
        try Data(serialize(entries).utf8).write(to: temp)
        do {
            if fileManager.fileExists(atPath: dest.path) {
                _ = try fileManager.replaceItemAt(dest, withItemAt: temp)
            } else {
                try fileManager.moveItem(at: temp, to: dest)
            }
        } catch {
            try? fileManager.removeItem(at: temp)
            throw error
        }
        notifyKeyboard(userDir: userDir)
    }

    /// 拡張に変更を知らせる。コアは要求のたびに `user_dict/` の変化を見て読み直すが（`Reloadable::changed`）、
    /// 拡張は focusIn で `config.toml` の更新時刻が変わっていれば `reload_config`（設定とユーザー辞書を読み直す）
    /// を送るので、その更新時刻だけを進める（内容は変えない）
    static func notifyKeyboard(userDir: URL) {
        let config = userDir.appendingPathComponent("config.toml")
        if FileManager.default.fileExists(atPath: config.path) {
            try? FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: config.path)
        }
    }

    /// 共有用の書き出しファイル（Mozc TSV）を作る。空なら nil
    static func writeExport(_ entries: [NagiUserDictEntry]) -> URL? {
        guard !entries.isEmpty else { return nil }
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("NagiUserDictExport", isDirectory: true)
        let url = dir.appendingPathComponent("nagi-user-dict.tsv")
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try Data(serialize(entries).utf8).write(to: url, options: .atomic)
            return url
        } catch {
            return nil
        }
    }
}
