import SwiftUI
import UniformTypeIdentifiers

// ユーザー辞書・スニペットの登録・編集画面（docs/ios-settings.md §2.4）。
// ファイルの形式と読み書きは `Shared/UserDictStore.swift`。ここは状態と画面だけを持つ。

/// 画面の状態。状態はメインスレッドでだけ触り、ファイルの読み書きは専用の直列キューで行う
final class NagiUserDictModel: ObservableObject {
    /// 編集できる登録（読み順）
    @Published private(set) var own: [NagiUserDictEntry] = []
    /// 読み取り専用の登録（ほかのファイル）
    @Published private(set) var readOnly: [NagiUserDictEntry] = []
    /// 検索で絞り込んだもの
    @Published private(set) var visibleOwn: [NagiUserDictEntry] = []
    @Published private(set) var visibleReadOnly: [NagiUserDictEntry] = []
    @Published private(set) var ownIssues: [NagiUserDictIssue] = []
    @Published private(set) var otherIssues: [NagiUserDictIssue] = []
    @Published private(set) var loaded = false
    @Published private(set) var busy = false
    /// 保存先を決められない・編集対象のファイルを読めないなど、書き込めない理由
    @Published private(set) var blockedReason: String?
    @Published private(set) var exportURL: URL?
    @Published private(set) var appGroupAvailable = true
    @Published var message: String?
    @Published var query = "" {
        didSet { refilter() }
    }

    private let queue = DispatchQueue(label: "com.shiki.nagi.userdict")
    /// 保存先（読み込み後に決まる）
    private var location: (userDict: URL, userDir: URL)?

    var canWrite: Bool {
        loaded && !busy && location != nil && blockedReason == nil
    }

    // MARK: - 読み込み

    func start() {
        busy = true
        appGroupAvailable = SharedStore.shared.containerURL != nil
        queue.async { [weak self] in
            let located = NagiUserDictStore.locate()
            var snapshot = NagiUserDictSnapshot()
            if let dir = located.userDict {
                snapshot = NagiUserDictStore.load(userDict: dir)
            }
            let sorted = Self.sortedByReading(snapshot.own)
            let export = NagiUserDictStore.writeExport(sorted)
            DispatchQueue.main.async {
                guard let self = self else { return }
                if let dir = located.userDict, let userDir = located.userDir {
                    self.location = (dir, userDir)
                } else {
                    self.location = nil
                }
                self.own = sorted
                self.readOnly = snapshot.readOnly
                self.ownIssues = snapshot.ownIssues
                self.otherIssues = snapshot.otherIssues
                self.exportURL = export
                self.blockedReason = located.message ?? snapshot.ownError
                self.loaded = true
                self.busy = false
                self.refilter()
            }
        }
    }

    private static func sortedByReading(_ entries: [NagiUserDictEntry]) -> [NagiUserDictEntry] {
        // 同じ読みの並び（スニペットの候補順）を保つため、元の位置を同順位の比較に使う
        entries.enumerated()
            .sorted { a, b in
                a.element.reading == b.element.reading ? a.offset < b.offset : a.element.reading < b.element.reading
            }
            .map { $0.element }
    }

    private func refilter() {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if q.isEmpty {
            visibleOwn = own
            visibleReadOnly = readOnly
            return
        }
        let hira = NagiUserDictStore.toHiragana(q)
        let match: (NagiUserDictEntry) -> Bool = { e in
            e.reading.contains(hira) || e.surface.localizedCaseInsensitiveContains(q)
                || e.comment.localizedCaseInsensitiveContains(q)
        }
        visibleOwn = own.filter(match)
        visibleReadOnly = readOnly.filter(match)
    }

    // MARK: - 書き込み

    /// 書き込めない理由（なければ nil）
    private func writeBlock() -> String? {
        if !loaded { return "読み込み中です" }
        if busy { return "処理中です。少し待ってからやり直してください" }
        if let reason = blockedReason { return reason }
        if !ownIssues.isEmpty {
            return "\(NagiUserDictStore.ownFileName) に読めない行があります。保存すると失われるため、先に画面上部の「問題のある行を削除して保存し直す」を実行してください"
        }
        if location == nil { return "保存先がありません" }
        return nil
    }

    /// 新しい内容をファイルに書き、成功したら画面の状態を置き換える
    private func commit(_ next: [NagiUserDictEntry], completion: @escaping (String?) -> Void) {
        guard let location = location else {
            completion("保存先がありません")
            return
        }
        let sorted = Self.sortedByReading(next)
        busy = true
        queue.async { [weak self] in
            var failure: String?
            do {
                try NagiUserDictStore.writeOwn(sorted, userDict: location.userDict, userDir: location.userDir)
            } catch {
                failure = "保存できませんでした: \(error.localizedDescription)"
            }
            let export = failure == nil ? NagiUserDictStore.writeExport(sorted) : nil
            DispatchQueue.main.async {
                guard let self = self else { return }
                self.busy = false
                if failure == nil {
                    self.own = sorted
                    self.ownIssues = []
                    self.exportURL = export
                    self.refilter()
                }
                completion(failure)
            }
        }
    }

    /// 追加または更新（`entry.id` が既存の行と同じなら更新）。エラーは日本語の文言で返す
    func save(_ entry: NagiUserDictEntry, completion: @escaping (String?) -> Void) {
        if let block = writeBlock() {
            completion(block)
            return
        }
        var e = entry
        if let error = NagiUserDictStore.validate(&e) {
            completion(error)
            return
        }
        let key = e.duplicateKey
        if own.contains(where: { $0.id != e.id && $0.duplicateKey == key }) {
            completion("同じ読みと単語の登録がすでにあります")
            return
        }
        if let other = readOnly.first(where: { $0.duplicateKey == key }) {
            completion("読み取り専用の \(other.file) に同じ登録があります")
            return
        }
        var next = own
        if let index = next.firstIndex(where: { $0.id == e.id }) {
            next[index] = e
        } else {
            if next.count >= NagiUserDictStore.maxOwnEntries {
                completion("登録は \(NagiUserDictStore.maxOwnEntries) 件までです")
                return
            }
            next.append(e)
        }
        commit(next, completion: completion)
    }

    func delete(ids: Set<UUID>) {
        if let block = writeBlock() {
            message = block
            return
        }
        let next = own.filter { !ids.contains($0.id) }
        commit(next) { [weak self] error in
            if let error = error { self?.message = error }
        }
    }

    /// 読めない行を捨てて、読めた行だけで書き直す
    func repair() {
        if !loaded || busy || location == nil { return }
        if let reason = blockedReason {
            message = reason
            return
        }
        commit(own) { [weak self] error in
            self?.message = error ?? "読めない行を取り除いて保存し直しました"
        }
    }

    // MARK: - 取り込み

    /// - startedAt: 渡すと、結果の文言にそこからの所要時間を足す（性能確認用）
    func importFile(_ url: URL, startedAt: Date? = nil) {
        if let block = writeBlock() {
            message = block
            return
        }
        busy = true
        // 取り込み中は busy で他の書き込みが止まるので、この時点の内容を判定に使ってよい
        let ownSnapshot = own
        let readOnlySnapshot = readOnly
        let elapsedText: () -> String = {
            guard let startedAt = startedAt else { return "" }
            return String(format: "\n所要時間（生成を含む）: %.2f 秒", Date().timeIntervalSince(startedAt))
        }
        queue.async { [weak self] in
            let accessing = url.startAccessingSecurityScopedResource()
            defer { if accessing { url.stopAccessingSecurityScopedResource() } }
            var prepared: (added: [NagiUserDictEntry], summary: String)?
            var failure: String?
            do {
                // 大きさを取れないファイルは 0 扱いにせず拒否する（上限の検査をすり抜けさせない）
                guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize else {
                    throw NagiUserDictImportError.sizeUnknown
                }
                if size > NagiUserDictStore.maxImportBytes {
                    failure = "ファイルが大きすぎます（上限 \(NagiUserDictStore.maxImportBytes / 1024 / 1024) MB）"
                } else {
                    let data = try Data(contentsOf: url)
                    // 検証・重複判定も重いので、このキューの上で済ませる
                    let parsed = NagiUserDictStore.parse(NagiUserDictStore.decodeText(data), file: url.lastPathComponent)
                    prepared = Self.prepareImport(
                        parsed.entries, parseIssues: parsed.issues, own: ownSnapshot, readOnly: readOnlySnapshot
                    )
                }
            } catch NagiUserDictImportError.sizeUnknown {
                failure = "ファイルの大きさを確認できないため取り込めません"
            } catch {
                failure = "ファイルを読み込めませんでした: \(error.localizedDescription)"
            }
            DispatchQueue.main.async {
                guard let self = self else { return }
                self.busy = false
                guard let prepared = prepared else {
                    self.message = failure ?? "ファイルを読み込めませんでした"
                    return
                }
                if prepared.added.isEmpty {
                    self.message = prepared.summary + elapsedText()
                    return
                }
                self.commit(self.own + prepared.added) { [weak self] error in
                    self?.message = error ?? (prepared.summary + elapsedText())
                }
            }
        }
    }

    private enum NagiUserDictImportError: Error {
        case sizeUnknown
    }

    /// 取り込む行を検証し、重複・上限を除いて、追加する行と結果の文言を作る（状態には触らない）
    private static func prepareImport(
        _ entries: [NagiUserDictEntry],
        parseIssues: [NagiUserDictIssue],
        own: [NagiUserDictEntry],
        readOnly: [NagiUserDictEntry]
    ) -> (added: [NagiUserDictEntry], summary: String) {
        var keys = Set<String>()
        for e in own { keys.insert(e.duplicateKey) }
        for e in readOnly { keys.insert(e.duplicateKey) }
        var added: [NagiUserDictEntry] = []
        var problems = parseIssues.map { $0.text }
        var duplicates = 0
        var overLimit = 0
        for entry in entries {
            var e = entry
            if let error = NagiUserDictStore.validate(&e) {
                problems.append("\(entry.line) 行目: \(error)")
                continue
            }
            let key = e.duplicateKey
            if keys.contains(key) {
                duplicates += 1
                continue
            }
            if own.count + added.count >= NagiUserDictStore.maxOwnEntries {
                overLimit += 1
                continue
            }
            keys.insert(key)
            e.line = 0
            added.append(e)
        }

        var summary = added.isEmpty ? "取り込める登録がありませんでした" : "\(added.count) 件を取り込みました"
        if duplicates > 0 { summary += "\n重複のため除外: \(duplicates) 件" }
        if overLimit > 0 { summary += "\n上限（\(NagiUserDictStore.maxOwnEntries) 件）を超えたため除外: \(overLimit) 件" }
        if !problems.isEmpty {
            summary += "\n読めなかった行: \(problems.count) 件"
            for line in problems.prefix(5) { summary += "\n  \(line)" }
            if problems.count > 5 { summary += "\n  …ほか \(problems.count - 5) 件" }
        }
        return (added, summary)
    }
}

// MARK: - 画面

private struct NagiUserDictEditTarget: Identifiable {
    let id = UUID()
    let entry: NagiUserDictEntry
}

struct NagiUserDictionaryView: View {
    @StateObject private var model = NagiUserDictModel()
    @ObservedObject private var developer = NagiDeveloperMode.shared
    @State private var editing: NagiUserDictEditTarget?
    @State private var showingImporter = false
    // テスト用（テストビルドのみ）
    @State private var sampleURLs: [NagiTestSamples.DictSample: URL] = [:]
    @State private var bulkProgress: Double?
    @State private var bulkLabel = ""
    @State private var confirmingClear = false

    var body: some View {
        List {
            statusSection

            if !model.ownIssues.isEmpty || !model.otherIssues.isEmpty {
                issuesSection
            }

            Section {
                if model.visibleOwn.isEmpty {
                    Text(model.query.isEmpty ? "まだ登録がありません。右上の「＋」から追加できます" : "一致する登録がありません")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                }
                ForEach(model.visibleOwn) { entry in
                    Button {
                        editing = NagiUserDictEditTarget(entry: entry)
                    } label: {
                        NagiUserDictRow(entry: entry)
                    }
                    .buttonStyle(.plain)
                }
                .onDelete { offsets in
                    let ids = Set(offsets.map { model.visibleOwn[$0].id })
                    model.delete(ids: ids)
                }
            } header: {
                Text("登録（タップで編集・左へスワイプで削除）")
            }

            if !model.visibleReadOnly.isEmpty {
                Section {
                    ForEach(model.visibleReadOnly) { entry in
                        NagiUserDictRow(entry: entry)
                    }
                } header: {
                    Text("読み取り専用（ほかのファイル）")
                } footer: {
                    Text("user_dict/ にある \(NagiUserDictStore.ownFileName) 以外のファイルです。ここでは編集できません。")
                }
            }

            Section {
                Button("Mozc TSV を取り込む") { showingImporter = true }
                    .buttonStyle(.borderless)
                    .disabled(!model.canWrite)
                if let url = model.exportURL {
                    ShareLink(item: url) {
                        Label("Mozc TSV として書き出す", systemImage: "square.and.arrow.up")
                    }
                }
            } header: {
                Text("取り込み・書き出し")
            } footer: {
                Text("取り込みは \(NagiUserDictStore.ownFileName) に追記します（同じ読み・単語は除外）。書き出しはこのアプリで編集できる登録だけです。品詞が「スニペット」の行は定型文（本文の改行は \\n）として扱います。")
            }

            // 実機での確認用（TestFlight・開発用ビルドで、開発者モードがオンのときだけ。App Store 版には出さない）
            if developer.showsTestTools {
                testSamplesSection
                testToolsSection
            }
        }
        .navigationTitle("ユーザー辞書")
        .searchable(text: $model.query, prompt: "読み・単語・コメントで検索")
        .toolbar {
            ToolbarItem(placement: .navigationBarTrailing) {
                Button {
                    editing = NagiUserDictEditTarget(
                        entry: NagiUserDictEntry(reading: "", surface: "", pos: NagiUserDictStore.posChoices[0], comment: "")
                    )
                } label: {
                    Image(systemName: "plus")
                }
                .disabled(!model.canWrite)
                .accessibilityLabel("登録を追加")
            }
        }
        .onAppear {
            model.start()
            if developer.showsTestTools { prepareSampleFiles() }
        }
        .fileImporter(
            isPresented: $showingImporter,
            allowedContentTypes: [.tabSeparatedText, .plainText, .data],
            onCompletion: { result in
                switch result {
                case .success(let url):
                    model.importFile(url)
                case .failure(let error):
                    model.message = "ファイルを選べませんでした: \(error.localizedDescription)"
                }
            }
        )
        .sheet(item: $editing) { target in
            NagiUserDictEditView(model: model, entry: target.entry) { editing = nil }
        }
        .alert("ユーザー辞書", isPresented: Binding(
            get: { model.message != nil },
            set: { if !$0 { model.message = nil } }
        )) {
            Button("OK") {}
        } message: {
            Text(model.message ?? "")
        }
    }

    // MARK: - テスト用（テストビルドのみ）

    private var testSamplesSection: some View {
        Section {
            ForEach(NagiTestSamples.DictSample.allCases, id: \.self) { sample in
                VStack(alignment: .leading, spacing: 4) {
                    if let url = sampleURLs[sample] {
                        ShareLink(item: url) {
                            Label("\(sample.title) を共有・保存", systemImage: "square.and.arrow.up")
                        }
                    } else {
                        Text("\(sample.title)（準備中）").font(.subheadline)
                    }
                    Text("期待結果: \(sample.expectation)")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
            }
        } header: {
            Text("テスト用: サンプル TSV")
        } footer: {
            Text("共有シートの「“ファイル”に保存」で保存し、上の「Mozc TSV を取り込む」で選びます。")
        }
    }

    private var testToolsSection: some View {
        Section {
            Button("500 件の辞書を生成して取り込む") { runBulk(count: 500) }
                .buttonStyle(.borderless)
                .disabled(!model.canWrite || bulkProgress != nil)
            Button("5000 件の辞書を生成して取り込む") { runBulk(count: 5000) }
                .buttonStyle(.borderless)
                .disabled(!model.canWrite || bulkProgress != nil)
            if let progress = bulkProgress {
                VStack(alignment: .leading, spacing: 4) {
                    ProgressView(value: progress)
                    Text(bulkLabel).font(.caption)
                }
            } else if model.busy {
                HStack {
                    ProgressView()
                    Text("取り込み中…").font(.caption)
                }
            }
            Button("読み取り専用サンプル（\(NagiTestSamples.readonlySampleFileName)）を作る") { createReadonlySample() }
                .buttonStyle(.borderless)
            Button("読み取り専用サンプルを削除") { deleteReadonlySample() }
                .buttonStyle(.borderless)
            Button("user_dict/ を空にする", role: .destructive) { confirmingClear = true }
                .buttonStyle(.borderless)
                .confirmationDialog(
                    "user_dict/ のファイルをすべて削除しますか？", isPresented: $confirmingClear, titleVisibility: .visible
                ) {
                    Button("削除", role: .destructive) { clearUserDict() }
                    Button("キャンセル", role: .cancel) {}
                } message: {
                    Text("\(NagiUserDictStore.ownFileName) を含む、登録がすべて消えます。")
                }
        } header: {
            Text("テスト用: 性能・読み取り専用・初期化")
        } footer: {
            Text("一括生成は同じ読みを重複として除外するため、2 回目以降は「重複のため除外」になります。読み取り専用サンプルは、編集できないファイルとして「読み取り専用」欄に出ます。")
        }
    }

    /// 共有用のサンプルファイルを一時ディレクトリに作る（バックグラウンド）
    private func prepareSampleFiles() {
        DispatchQueue.global(qos: .utility).async {
            let dir = FileManager.default.temporaryDirectory.appendingPathComponent("NagiUserDictSamples", isDirectory: true)
            var urls: [NagiTestSamples.DictSample: URL] = [:]
            do {
                try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                for sample in NagiTestSamples.DictSample.allCases {
                    let url = dir.appendingPathComponent(sample.fileName)
                    try NagiTestSamples.dictSampleData(sample).write(to: url, options: .atomic)
                    urls[sample] = url
                }
            } catch {
                // 作れたものだけ出す
            }
            DispatchQueue.main.async { sampleURLs = urls }
        }
    }

    /// `count` 件の TSV をバックグラウンドで生成し、ファイルの取り込みと同じ経路で取り込む
    private func runBulk(count: Int) {
        guard model.canWrite, bulkProgress == nil else { return }
        let startedAt = Date()
        bulkProgress = 0
        bulkLabel = "\(count) 件を生成中…"
        DispatchQueue.global(qos: .userInitiated).async {
            let text = NagiTestSamples.bulkTSV(count: count) { value in
                DispatchQueue.main.async { bulkProgress = value }
            }
            let url = FileManager.default.temporaryDirectory.appendingPathComponent("nagi-bulk-\(count).tsv")
            do {
                try Data(text.utf8).write(to: url, options: .atomic)
            } catch {
                DispatchQueue.main.async {
                    bulkProgress = nil
                    model.message = "一時ファイルを作れませんでした: \(error.localizedDescription)"
                }
                return
            }
            DispatchQueue.main.async {
                bulkProgress = nil
                model.importFile(url, startedAt: startedAt)
            }
        }
    }

    private func createReadonlySample() {
        DispatchQueue.global(qos: .userInitiated).async {
            let located = NagiUserDictStore.locate()
            var result: String
            if let dir = located.userDict, let userDir = located.userDir {
                do {
                    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                    try Data(NagiTestSamples.readonlySampleText.utf8)
                        .write(to: dir.appendingPathComponent(NagiTestSamples.readonlySampleFileName), options: .atomic)
                    NagiUserDictStore.notifyKeyboard(userDir: userDir)
                    result = "\(NagiTestSamples.readonlySampleFileName) を作りました。「読み取り専用」欄に 2 件出れば成功です"
                } catch {
                    result = "作れませんでした: \(error.localizedDescription)"
                }
            } else {
                result = located.message ?? "保存先がありません"
            }
            DispatchQueue.main.async {
                model.message = result
                model.start()
            }
        }
    }

    private func deleteReadonlySample() {
        DispatchQueue.global(qos: .userInitiated).async {
            let located = NagiUserDictStore.locate()
            var result: String
            if let dir = located.userDict, let userDir = located.userDir {
                let url = dir.appendingPathComponent(NagiTestSamples.readonlySampleFileName)
                if FileManager.default.fileExists(atPath: url.path) {
                    do {
                        try FileManager.default.removeItem(at: url)
                        NagiUserDictStore.notifyKeyboard(userDir: userDir)
                        result = "\(NagiTestSamples.readonlySampleFileName) を削除しました"
                    } catch {
                        result = "削除できませんでした: \(error.localizedDescription)"
                    }
                } else {
                    result = "\(NagiTestSamples.readonlySampleFileName) はありません"
                }
            } else {
                result = located.message ?? "保存先がありません"
            }
            DispatchQueue.main.async {
                model.message = result
                model.start()
            }
        }
    }

    /// user_dict/ の中身をすべて削除する（ディレクトリ自体は残す）
    private func clearUserDict() {
        DispatchQueue.global(qos: .userInitiated).async {
            let located = NagiUserDictStore.locate()
            var result: String
            if let dir = located.userDict, let userDir = located.userDir {
                let fileManager = FileManager.default
                var removed = 0
                var failure: String?
                let names = (try? fileManager.contentsOfDirectory(atPath: dir.path)) ?? []
                for name in names {
                    do {
                        try fileManager.removeItem(at: dir.appendingPathComponent(name))
                        removed += 1
                    } catch {
                        failure = "\(name) を削除できませんでした: \(error.localizedDescription)"
                    }
                }
                NagiUserDictStore.notifyKeyboard(userDir: userDir)
                result = failure ?? "user_dict/ の \(removed) 件を削除しました"
            } else {
                result = located.message ?? "保存先がありません"
            }
            DispatchQueue.main.async {
                model.message = result
                model.start()
            }
        }
    }

    private var statusSection: some View {
        Section {
            if !model.loaded {
                HStack {
                    ProgressView()
                    Text("読み込み中…").font(.footnote)
                }
            } else {
                Text("登録 \(model.own.count) / \(NagiUserDictStore.maxOwnEntries) 件"
                    + (model.readOnly.isEmpty ? "" : "（読み取り専用 \(model.readOnly.count) 件）"))
                    .font(.footnote)
            }
            if !model.appGroupAvailable {
                Text("App Group を使えないため、ここでの登録はキーボードに届きません。")
                    .font(.footnote)
                    .foregroundColor(.orange)
            }
            if let reason = model.blockedReason {
                Text(reason).font(.footnote).foregroundColor(.red)
            }
        } footer: {
            Text("変更はキーボードを次に開いたときに反映されます。")
        }
    }

    private var issuesSection: some View {
        Section {
            ForEach(Array((model.ownIssues + model.otherIssues).prefix(5))) { issue in
                Text(issue.text).font(.caption).foregroundColor(.orange)
            }
            let total = model.ownIssues.count + model.otherIssues.count
            if total > 5 {
                Text("…ほか \(total - 5) 件").font(.caption).foregroundColor(.secondary)
            }
            if !model.ownIssues.isEmpty {
                Button("問題のある行を削除して保存し直す", role: .destructive) { model.repair() }
                    .buttonStyle(.borderless)
                    .disabled(model.busy)
            }
        } header: {
            Text("読み込めなかった行")
        } footer: {
            Text("読めない行は変換に使われません。\(NagiUserDictStore.ownFileName) の行は、削除して保存し直すまで編集できません。")
        }
    }
}

private struct NagiUserDictRow: View {
    let entry: NagiUserDictEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(entry.reading).font(.headline)
                Spacer()
                Text(entry.pos)
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(entry.isSnippet ? Color.blue.opacity(0.15) : Color.gray.opacity(0.15))
                    .cornerRadius(4)
            }
            Text(entry.surface)
                .font(.subheadline)
                .lineLimit(2)
            if !entry.comment.isEmpty {
                Text(entry.comment).font(.caption).foregroundColor(.secondary).lineLimit(1)
            }
            if !entry.file.isEmpty {
                Text(entry.file).font(.caption2).foregroundColor(.secondary)
            }
        }
        .padding(.vertical, 2)
        .contentShape(Rectangle())
    }
}

/// 追加・編集のシート
private struct NagiUserDictEditView: View {
    @ObservedObject var model: NagiUserDictModel
    private let original: NagiUserDictEntry
    private let onClose: () -> Void

    @State private var isSnippet: Bool
    @State private var reading: String
    @State private var surface: String
    @State private var pos: String
    @State private var comment: String
    @State private var errorText: String?
    @State private var saving = false

    init(model: NagiUserDictModel, entry: NagiUserDictEntry, onClose: @escaping () -> Void) {
        self.model = model
        self.original = entry
        self.onClose = onClose
        _isSnippet = State(initialValue: entry.isSnippet)
        _reading = State(initialValue: entry.reading)
        _surface = State(initialValue: entry.surface)
        _pos = State(initialValue: entry.isSnippet ? NagiUserDictStore.posChoices[0] : entry.pos)
        _comment = State(initialValue: entry.comment)
    }

    /// 品詞の選択肢。取り込んだ登録が一覧にない品詞なら、それも選べるようにして品詞を保つ
    private var posOptions: [String] {
        NagiUserDictStore.posChoices.contains(pos) ? NagiUserDictStore.posChoices : [pos] + NagiUserDictStore.posChoices
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("種類", selection: $isSnippet) {
                        Text("単語").tag(false)
                        Text("スニペット").tag(true)
                    }
                    .pickerStyle(.segmented)
                } footer: {
                    Text(isSnippet ? "短い読みから定型文（複数行可）を候補に出します。" : "読みに対する変換候補として出します。")
                }

                Section("読み（ひらがな）") {
                    TextField("例: きょうと", text: $reading)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                }

                if isSnippet {
                    Section("本文（改行可・\(NagiUserDictStore.maxSnippetChars) 文字以内）") {
                        TextEditor(text: $surface)
                            .frame(minHeight: 120)
                            .autocorrectionDisabled()
                        Text("\(surface.count) / \(NagiUserDictStore.maxSnippetChars) 文字")
                            .font(.caption)
                            .foregroundColor(surface.count > NagiUserDictStore.maxSnippetChars ? .red : .secondary)
                    }
                } else {
                    Section("単語") {
                        TextField("例: 京都", text: $surface)
                            .autocorrectionDisabled()
                            .textInputAutocapitalization(.never)
                    }
                    Section("品詞") {
                        Picker("品詞", selection: $pos) {
                            ForEach(posOptions, id: \.self) { Text($0).tag($0) }
                        }
                    }
                }

                Section("コメント（任意）") {
                    TextField("メモ", text: $comment)
                }

                if let errorText = errorText {
                    Section {
                        Text(errorText).font(.footnote).foregroundColor(.red)
                    }
                }
            }
            .navigationTitle(original.reading.isEmpty ? "登録を追加" : "登録を編集")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("キャンセル") { onClose() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") { save() }
                        .disabled(saving)
                }
            }
        }
    }

    private func save() {
        var entry = original
        entry.reading = reading
        entry.surface = surface
        entry.comment = comment
        entry.pos = isSnippet ? NagiUserDictStore.snippetPos : pos
        saving = true
        errorText = nil
        model.save(entry) { error in
            saving = false
            if let error = error {
                errorText = error
            } else {
                onClose()
            }
        }
    }
}

#Preview {
    NavigationStack { NagiUserDictionaryView() }
}
