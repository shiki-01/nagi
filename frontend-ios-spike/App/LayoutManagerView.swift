import SwiftUI
import UniformTypeIdentifiers

// レイアウト管理タブ（docs/ios-keyboard-layout.md「実装の段階」4）。
// 同梱プリセットとユーザーレイアウト（App Group の keyboard/layouts/<id>.json）の一覧、
// JSON の貼り付け / ファイル / URL スキーム（nagi://layout?data=<base64url json>）での取り込み、書き出し、削除、
// かな / 英字への割り当て、簡易プレビュー。検証は Shared/KeyboardLayout.swift の `LayoutLoader.decode` を使う。

// MARK: - 保存・取り込み

enum NagiLayoutStore {
    /// 取り込むレイアウトの最大サイズ（バイト）
    static let maxBytes = 512 * 1024

    /// 検証済みで、まだ保存していないレイアウト（確認ダイアログの表示に使う）
    struct Preview {
        let data: Data
        let def: KeyboardLayoutDef
        /// 全行のキー数
        let keyCount: Int
        /// 同じ id のユーザーレイアウトがあり、取り込むと置き換わるか
        let replaced: Bool
        /// 同梱プリセットと同じ id か（ユーザーのものが優先して使われる）
        let overridesPreset: Bool
    }

    struct Imported {
        let def: KeyboardLayoutDef
        /// 同じ id のユーザーレイアウトを置き換えたか
        let replaced: Bool
        /// 同梱プリセットと同じ id か（ユーザーのものが優先して使われる）
        let overridesPreset: Bool
    }

    /// JSON を検証する（保存はしない）。問題があれば日本語の理由つきで投げる。ファイルを読むので
    /// メインスレッドでは呼ばないこと
    static func preview(_ data: Data) throws -> Preview {
        if data.isEmpty { throw NagiLayoutError(message: "JSON が空です") }
        if data.count > maxBytes { throw NagiLayoutError(message: "JSON が大きすぎます（\(maxBytes / 1024) KB まで）") }
        let def: KeyboardLayoutDef
        do {
            def = try LayoutLoader.decode(data, checkTargets: true)
        } catch let error as LayoutLoader.LoadError {
            throw NagiLayoutError(message: error.message)
        }
        guard LayoutLoader.isSafeId(def.id) else {
            throw NagiLayoutError(message: "id は半角の英数字・ハイフン・アンダースコアで 64 文字以内にしてください（\(def.id)）")
        }
        guard let dir = LayoutLoader.userLayoutsDirectory else {
            throw NagiLayoutError(message: "保存先（App Group）が使えません")
        }
        let url = dir.appendingPathComponent("\(def.id).json")
        return Preview(
            data: data,
            def: def,
            keyCount: def.rows.reduce(0) { $0 + $1.keys.count },
            replaced: FileManager.default.fileExists(atPath: url.path),
            overridesPreset: LayoutLoader.bundledIds().contains(def.id)
        )
    }

    /// 検証済みのレイアウトを App Group に保存する
    static func save(_ preview: Preview) throws -> Imported {
        guard let dir = LayoutLoader.userLayoutsDirectory else {
            throw NagiLayoutError(message: "保存先（App Group）が使えません")
        }
        let url = dir.appendingPathComponent("\(preview.def.id).json")
        let replaced = FileManager.default.fileExists(atPath: url.path)
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try preview.data.write(to: url, options: .atomic)
        } catch {
            throw NagiLayoutError(message: "保存できませんでした: \(error.localizedDescription)")
        }
        return Imported(def: preview.def, replaced: replaced, overridesPreset: preview.overridesPreset)
    }

    /// JSON を検証して App Group に保存する。問題があれば日本語の理由つきで投げる。
    /// - allowPresetId: false なら、同梱プリセットと同じ id のものは拒否する（URL 経由の取り込み）
    static func importLayout(_ data: Data, allowPresetId: Bool = true) throws -> Imported {
        let checked = try preview(data)
        if !allowPresetId && checked.overridesPreset {
            throw NagiLayoutError(message: "同梱のレイアウトと同じ id（\(checked.def.id)）は、URL からは取り込めません")
        }
        return try save(checked)
    }

    /// ファイルを読む。大きさを確認できない・上限を超えるファイルは、読み込む前に拒否する
    static func readFile(_ url: URL) throws -> Data {
        guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize else {
            throw NagiLayoutError(message: "ファイルの大きさを確認できません")
        }
        if size > maxBytes { throw NagiLayoutError(message: "ファイルが大きすぎます（\(maxBytes / 1024) KB まで）") }
        return try Data(contentsOf: url)
    }

    /// ユーザーのレイアウトを削除する。割り当てていたモードは既定に戻す
    static func delete(id: String) throws {
        guard LayoutLoader.isSafeId(id), let dir = LayoutLoader.userLayoutsDirectory else { return }
        let url = dir.appendingPathComponent("\(id).json")
        try FileManager.default.removeItem(at: url)
        for mode in [LayoutMode.kana, LayoutMode.alpha] where LayoutAssignment.isExplicit(for: mode) {
            if LayoutAssignment.id(for: mode) == id { LayoutAssignment.set(id: nil, for: mode) }
        }
    }

    static func userURL(id: String) -> URL? {
        guard LayoutLoader.isSafeId(id) else { return nil }
        return LayoutLoader.userLayoutsDirectory?.appendingPathComponent("\(id).json")
    }

    static func bundledURL(id: String) -> URL? {
        guard LayoutLoader.isSafeId(id) else { return nil }
        return Bundle.main.url(forResource: id, withExtension: "json", subdirectory: LayoutLoader.bundledFolder)
            ?? Bundle.main.url(forResource: id, withExtension: "json")
    }

    static func rawData(id: String, isUser: Bool) -> Data? {
        guard let url = isUser ? userURL(id: id) : bundledURL(id: id) else { return nil }
        return try? Data(contentsOf: url)
    }

    /// 書き出し（共有）用の一時ファイル
    static func exportURL(id: String, data: Data) -> URL? {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("\(id).json")
        do {
            try data.write(to: url, options: .atomic)
            return url
        } catch {
            return nil
        }
    }

    /// `nagi://layout?data=<base64url の JSON>` から JSON を取り出す（実装は `Shared/LayoutURL.swift`）
    static func jsonData(fromURL url: URL) throws -> Data {
        try NagiLayoutURL.jsonData(fromURL: url)
    }
}

/// URL スキームからの取り込み。他のアプリや Web ページから任意に起動できるため、取り込む前に必ず確認する。
/// 確認の結果と取り込み結果を、アプリ全体のアラートとレイアウト管理画面の再読み込みに伝える
final class NagiLayoutURLInbox: ObservableObject {
    static let shared = NagiLayoutURLInbox()

    /// 取り込みの確認待ち（nil なら無い）
    struct Pending {
        let data: Data
        let name: String
        let id: String
        let keyCount: Int
        /// 同じ id のユーザーレイアウトを置き換えるか
        let replaced: Bool
    }

    /// 表示するメッセージ（nil なら出さない）
    @Published var message: String?
    /// 確認ダイアログを出す取り込み
    @Published private(set) var pending: Pending?
    /// 取り込みのたびに増える（一覧の再読み込みの合図）
    @Published private(set) var changeCount = 0

    /// 検証・保存は重いので、メインスレッドの外で行う
    private let queue = DispatchQueue(label: "com.shiki.nagi.layoutimport", qos: .userInitiated)

    /// URL を受け取る。保存はせず、検証して確認待ちにするだけ
    func handle(_ url: URL) {
        guard url.scheme?.lowercased() == "nagi", url.host?.lowercased() == "layout" else { return }
        queue.async { [weak self] in
            var checked: NagiLayoutStore.Preview?
            var failure: String?
            do {
                let preview = try NagiLayoutStore.preview(try NagiLayoutURL.jsonData(fromURL: url))
                if preview.overridesPreset {
                    failure = "同梱のレイアウトと同じ id（\(preview.def.id)）は、URL からは取り込めません"
                } else {
                    checked = preview
                }
            } catch {
                failure = error.localizedDescription
            }
            DispatchQueue.main.async {
                guard let self = self else { return }
                if let preview = checked {
                    self.pending = Pending(
                        data: preview.data, name: preview.def.name, id: preview.def.id,
                        keyCount: preview.keyCount, replaced: preview.replaced)
                } else {
                    self.message = "レイアウトを取り込めませんでした。\n\(failure ?? "不明なエラー")"
                }
            }
        }
    }

    /// 確認ダイアログで「取り込む」
    func confirmPending() {
        guard let target = pending else { return }
        pending = nil
        queue.async { [weak self] in
            var text: String
            var changed = false
            do {
                // 確認から保存までの間に同梱 id と衝突しないことを、もう一度確かめる
                let imported = try NagiLayoutStore.importLayout(target.data, allowPresetId: false)
                changed = true
                text = "レイアウト「\(imported.def.name)」を\(imported.replaced ? "更新しました" : "取り込みました")。"
                text += "「レイアウト」タブで、かな / 英字への割り当てができます。"
            } catch {
                text = "レイアウトを取り込めませんでした。\n\(error.localizedDescription)"
            }
            DispatchQueue.main.async {
                guard let self = self else { return }
                if changed { self.changeCount += 1 }
                self.message = text
            }
        }
    }

    /// 確認ダイアログで「キャンセル」
    func cancelPending() {
        pending = nil
    }
}

/// 取り込みの確認と結果のアラート。初回案内のシートが出ている間は出さず（重ねると隠れて取りこぼす）、
/// 閉じてから少し待って出す
struct NagiLayoutInboxAlerts: ViewModifier {
    @ObservedObject private var inbox: NagiLayoutURLInbox
    /// 別のシートなどが前面にあるとき true
    private let suppressed: Bool
    /// シートが閉じてから少し待ったか（最初からシートが出ている場合は、閉じるまで false）
    @State private var settled: Bool

    init(inbox: NagiLayoutURLInbox, suppressed: Bool) {
        self.inbox = inbox
        self.suppressed = suppressed
        _settled = State(initialValue: !suppressed)
    }

    private var canPresent: Bool { settled && !suppressed }

    func body(content: Content) -> some View {
        content
            .onChange(of: suppressed) { nowSuppressed in
                if nowSuppressed {
                    settled = false
                } else {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.7) { settled = true }
                }
            }
            .alert(
                "レイアウトを取り込みますか？",
                isPresented: Binding(
                    get: { canPresent && inbox.pending != nil },
                    // ボタンの処理で pending を空にするので、ここでは何もしない
                    set: { _ in })
            ) {
                Button("取り込む") { inbox.confirmPending() }
                Button("キャンセル", role: .cancel) { inbox.cancelPending() }
            } message: {
                if let pending = inbox.pending {
                    Text("名前: \(pending.name)\nid: \(pending.id)\nキー数: \(pending.keyCount)"
                        + (pending.replaced ? "\n同じ id のレイアウトを置き換えます。" : "")
                        + "\nNagi の外から開かれたリンクです。心当たりがなければキャンセルしてください。")
                }
            }
            .alert(
                "レイアウト",
                isPresented: Binding(
                    get: { canPresent && inbox.pending == nil && inbox.message != nil },
                    set: { if !$0 { inbox.message = nil } })
            ) {
                Button("OK") {}
            } message: {
                Text(inbox.message ?? "")
            }
    }
}

// MARK: - 一覧

private struct NagiLayoutRow: Identifiable {
    let id: String
    let name: String
    let mode: LayoutMode?
    let isUser: Bool
    /// 読み込めない理由（無効なレイアウト）
    let error: String?
    let overridesPreset: Bool

    var modeLabel: String {
        switch mode {
        case .kana?: return "かな"
        case .alpha?: return "英字"
        case .number?: return "数字"
        case .symbol?: return "記号"
        case .emoji?: return "絵文字"
        case .custom?: return "カスタム"
        case nil: return "不明"
        }
    }
}

private func loadRows(ids: [String], isUser: Bool, presetIds: Set<String>) -> [NagiLayoutRow] {
    ids.map { (id: String) -> NagiLayoutRow in
        guard let data = NagiLayoutStore.rawData(id: id, isUser: isUser) else {
            return NagiLayoutRow(id: id, name: id, mode: nil, isUser: isUser, error: "ファイルを読めません", overridesPreset: false)
        }
        do {
            let def = try LayoutLoader.decode(data)
            return NagiLayoutRow(
                id: id, name: def.name, mode: def.mode, isUser: isUser, error: nil,
                overridesPreset: isUser && presetIds.contains(id))
        } catch let error as LayoutLoader.LoadError {
            return NagiLayoutRow(id: id, name: id, mode: nil, isUser: isUser, error: error.message, overridesPreset: false)
        } catch {
            return NagiLayoutRow(id: id, name: id, mode: nil, isUser: isUser, error: "読み取れません", overridesPreset: false)
        }
    }
}

struct LayoutManagerView: View {
    @ObservedObject private var inbox = NagiLayoutURLInbox.shared

    @State private var userRows: [NagiLayoutRow] = []
    @State private var presetRows: [NagiLayoutRow] = []
    @State private var assignedKana = LayoutAssignment.id(for: .kana)
    @State private var assignedAlpha = LayoutAssignment.id(for: .alpha)
    @State private var showingPaste = false
    @State private var showingImporter = false
    @State private var notice: String?
    @State private var pendingDelete: NagiLayoutRow?
    @State private var showingDeleteConfirm = false
    /// 同じ id のレイアウトを上書きする取り込みの確認待ち（ファイル・貼り付け）
    @State private var pendingOverwrite: NagiLayoutStore.Preview?
    @State private var showingOverwriteConfirm = false

    var body: some View {
        NavigationStack {
            List {
                Section("割り当て中") {
                    assignedRow(title: "かな", id: assignedKana)
                    assignedRow(title: "英字", id: assignedAlpha)
                }
                Section {
                    if userRows.isEmpty {
                        Text("まだありません。右上の＋から、サイトで作った JSON を取り込めます。")
                            .font(.footnote)
                            .foregroundColor(.secondary)
                    }
                    ForEach(userRows) { row in
                        NavigationLink(destination: NagiLayoutDetailView(
                            id: row.id, isUser: true, onChange: reload)) {
                            rowLabel(row)
                        }
                        .swipeActions {
                            Button(role: .destructive) {
                                pendingDelete = row
                                showingDeleteConfirm = true
                            } label: {
                                Label("削除", systemImage: "trash")
                            }
                        }
                    }
                } header: {
                    Text("ユーザーのレイアウト")
                } footer: {
                    Text("同梱と同じ id のものは、こちらが優先して使われます。")
                }
                Section("同梱のレイアウト") {
                    ForEach(presetRows) { row in
                        NavigationLink(destination: NagiLayoutDetailView(
                            id: row.id, isUser: false, onChange: reload)) {
                            rowLabel(row)
                        }
                    }
                }
            }
            .navigationTitle("レイアウト")
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Menu {
                        Button {
                            showingPaste = true
                        } label: {
                            Label("JSON を貼り付け", systemImage: "doc.on.clipboard")
                        }
                        Button {
                            showingImporter = true
                        } label: {
                            Label("ファイルから取り込む", systemImage: "folder")
                        }
                    } label: {
                        Image(systemName: "plus")
                    }
                }
            }
            .sheet(isPresented: $showingPaste) {
                NagiLayoutPasteView { text, done in
                    guard let data = text.trimmingCharacters(in: .whitespacesAndNewlines).data(using: .utf8) else {
                        done("文字列を読み取れません")
                        return
                    }
                    prepare({ data }, completion: done)
                }
            }
            .fileImporter(
                isPresented: $showingImporter,
                allowedContentTypes: [UTType.json, UTType.plainText],
                allowsMultipleSelection: false
            ) { result in
                importFile(result)
            }
            .alert("レイアウト", isPresented: Binding(
                get: { notice != nil },
                set: { if !$0 { notice = nil } })
            ) {
                Button("OK") {}
            } message: {
                Text(notice ?? "")
            }
            .alert("同じ id のレイアウトがあります", isPresented: $showingOverwriteConfirm, presenting: pendingOverwrite) { preview in
                Button("上書き", role: .destructive) { commit(preview) }
                Button("キャンセル", role: .cancel) {}
            } message: { preview in
                Text(overwriteMessage(preview))
            }
            .alert("レイアウトを削除", isPresented: $showingDeleteConfirm, presenting: pendingDelete) { row in
                Button("削除", role: .destructive) { delete(row) }
                Button("キャンセル", role: .cancel) {}
            } message: { row in
                Text("「\(row.name)」を削除します。割り当てていた場合は既定に戻ります。")
            }
            .onAppear(perform: reload)
            .onReceive(inbox.$changeCount) { _ in reload() }
        }
    }

    // MARK: 行

    private func assignedRow(title: String, id: String) -> some View {
        HStack {
            Text(title)
            Spacer()
            Text(id).foregroundColor(.secondary)
        }
    }

    private func rowLabel(_ row: NagiLayoutRow) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(row.name)
                if row.id == assignedKana { badge("かな") }
                if row.id == assignedAlpha { badge("英字") }
            }
            Text("\(row.id) ・ \(row.modeLabel)")
                .font(.caption)
                .foregroundColor(.secondary)
            if let error = row.error {
                Text("無効: \(error)（キーボードでは使われず、既定に戻ります）")
                    .font(.caption)
                    .foregroundColor(.red)
            }
            if row.overridesPreset {
                Text("同梱の同名を上書き中")
                    .font(.caption)
                    .foregroundColor(.orange)
            }
        }
    }

    private func badge(_ text: String) -> some View {
        Text(text)
            .font(.caption2)
            .foregroundColor(.white)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Capsule().fill(Color.accentColor))
    }

    // MARK: 操作

    private func reload() {
        let presetIds = LayoutLoader.bundledIds()
        presetRows = loadRows(ids: presetIds, isUser: false, presetIds: [])
        userRows = loadRows(ids: LayoutLoader.userLayoutIds(), isUser: true, presetIds: Set(presetIds))
        assignedKana = LayoutAssignment.id(for: .kana)
        assignedAlpha = LayoutAssignment.id(for: .alpha)
    }

    private func delete(_ row: NagiLayoutRow) {
        do {
            try NagiLayoutStore.delete(id: row.id)
        } catch {
            notice = "削除できませんでした: \(error.localizedDescription)"
        }
        reload()
    }

    /// 検証をメインスレッドの外で行い、結果で保存または上書きの確認に進む。
    /// `completion` は検証の結果（失敗の理由。成功は nil）をメインスレッドで受け取る
    private func prepare(_ load: @escaping () throws -> Data, completion: @escaping (String?) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            let result: Result<NagiLayoutStore.Preview, Error>
            do {
                result = .success(try NagiLayoutStore.preview(try load()))
            } catch {
                result = .failure(error)
            }
            DispatchQueue.main.async {
                switch result {
                case .success(let preview):
                    completion(nil)
                    if preview.replaced || preview.overridesPreset {
                        pendingOverwrite = preview
                        // 貼り付けのシートが閉じる間に重ねると、アラートが出ないことがあるので少し待つ
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { showingOverwriteConfirm = true }
                    } else {
                        commit(preview)
                    }
                case .failure(let error):
                    completion(error.localizedDescription)
                }
            }
        }
    }

    private func commit(_ preview: NagiLayoutStore.Preview) {
        DispatchQueue.global(qos: .userInitiated).async {
            let result: Result<NagiLayoutStore.Imported, Error>
            do {
                result = .success(try NagiLayoutStore.save(preview))
            } catch {
                result = .failure(error)
            }
            DispatchQueue.main.async {
                switch result {
                case .success(let imported):
                    reload()
                    notice = importedMessage(imported)
                case .failure(let error):
                    notice = "取り込めませんでした。\n\(error.localizedDescription)"
                }
            }
        }
    }

    private func overwriteMessage(_ preview: NagiLayoutStore.Preview) -> String {
        var text = "「\(preview.def.name)」（id: \(preview.def.id)、キー数 \(preview.keyCount)）を取り込みます。"
        if preview.replaced { text += "\n同じ id のユーザーレイアウトは置き換えられます。" }
        if preview.overridesPreset { text += "\n同梱の同じ id のレイアウトより、こちらが優先して使われます。" }
        return text
    }

    private func importFile(_ result: Result<[URL], Error>) {
        switch result {
        case .failure(let error):
            notice = "ファイルを開けませんでした: \(error.localizedDescription)"
        case .success(let urls):
            guard let url = urls.first else { return }
            prepare({
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                return try NagiLayoutStore.readFile(url)
            }, completion: { message in
                if let message = message { notice = "取り込めませんでした。\n\(message)" }
            })
        }
    }

    private func importedMessage(_ imported: NagiLayoutStore.Imported) -> String {
        var text = "「\(imported.def.name)」を\(imported.replaced ? "更新しました" : "取り込みました")。一覧から開いて、かな / 英字に割り当てられます。"
        if imported.overridesPreset { text += "\n同梱の「\(imported.def.id)」と同じ id のため、このレイアウトが優先されます。" }
        return text
    }
}

// MARK: - 貼り付け

private struct NagiLayoutPasteView: View {
    @Environment(\.dismiss) private var dismiss
    /// 取り込みを始め、終わったら結果（失敗の理由。成功は nil）をメインスレッドで渡す
    let onImport: (String, @escaping (String?) -> Void) -> Void

    @State private var text = ""
    @State private var error: String?
    @State private var busy = false

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 8) {
                Text("サイト（nagi.shiki-01.com/custom-keyboard）で作ったレイアウトの JSON を貼り付けてください。")
                    .font(.footnote)
                    .foregroundColor(.secondary)
                TextEditor(text: $text)
                    .font(.system(.footnote, design: .monospaced))
                    .autocorrectionDisabled(true)
                    .textInputAutocapitalization(.never)
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color(UIColor.separator)))
                if let error = error {
                    Text(error)
                        .font(.footnote)
                        .foregroundColor(.red)
                }
                HStack {
                    Button("クリップボードから貼り付け") {
                        if let pasted = UIPasteboard.general.string { text = pasted }
                    }
                    .buttonStyle(.bordered)
                    Spacer()
                    Button("取り込む") {
                        busy = true
                        onImport(text) { message in
                            busy = false
                            if let message = message {
                                error = message
                            } else {
                                dismiss()
                            }
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(busy || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .padding()
            .navigationTitle("JSON を貼り付け")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarLeading) {
                    Button("閉じる") { dismiss() }
                }
            }
        }
    }
}

// MARK: - 詳細

private struct NagiLayoutDetailView: View {
    @Environment(\.dismiss) private var dismiss
    let id: String
    let isUser: Bool
    let onChange: () -> Void

    @State private var def: KeyboardLayoutDef?
    @State private var loadError: String?
    @State private var exportURL: URL?
    @State private var assignedKana = LayoutAssignment.id(for: .kana)
    @State private var assignedAlpha = LayoutAssignment.id(for: .alpha)
    @State private var showingDeleteConfirm = false
    @State private var notice: String?

    var body: some View {
        List {
            if let def = def {
                Section("情報") {
                    infoRow("名前", def.name)
                    infoRow("id", def.id)
                    infoRow("モード", def.mode.rawValue)
                    infoRow("行数", "\(def.rows.count)")
                }
                Section("プレビュー") {
                    NagiLayoutPreview(def: def)
                        .padding(.vertical, 4)
                    Text("簡易表示です。フリックの方向やポップアップは省略しています。")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
                Section {
                    if canAssignKana {
                        assignRow(title: "かなモードで使う", current: assignedKana, mode: .kana)
                    }
                    if canAssignAlpha {
                        assignRow(title: "英字モードで使う", current: assignedAlpha, mode: .alpha)
                    }
                    if !canAssignKana && !canAssignAlpha {
                        Text("このモード（\(def.mode.rawValue)）のレイアウトは、かな / 英字には割り当てられません。他のレイアウトのキー（layout）から切り替えて使います。")
                            .font(.footnote)
                            .foregroundColor(.secondary)
                    }
                } header: {
                    Text("割り当て")
                } footer: {
                    Text("キーボードを開き直すと反映されます。")
                }
                Section("書き出し") {
                    if let exportURL = exportURL {
                        ShareLink(item: exportURL) {
                            Label("JSON を共有・保存", systemImage: "square.and.arrow.up")
                        }
                    }
                    Button {
                        if let data = NagiLayoutStore.rawData(id: id, isUser: isUser),
                           let text = String(data: data, encoding: .utf8) {
                            UIPasteboard.general.string = text
                            notice = "JSON をコピーしました"
                        }
                    } label: {
                        Label("JSON をコピー", systemImage: "doc.on.doc")
                    }
                }
            } else {
                Section {
                    Text(loadError.map { "読み込めません: \($0)" } ?? "読み込めません")
                        .foregroundColor(.red)
                }
            }
            if isUser {
                Section {
                    Button(role: .destructive) {
                        showingDeleteConfirm = true
                    } label: {
                        Label("このレイアウトを削除", systemImage: "trash")
                    }
                }
            }
            if let notice = notice {
                Section { Text(notice).font(.footnote).foregroundColor(.secondary) }
            }
        }
        .navigationTitle(def?.name ?? id)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear(perform: load)
        .alert("レイアウトを削除", isPresented: $showingDeleteConfirm) {
            Button("削除", role: .destructive) {
                do {
                    try NagiLayoutStore.delete(id: id)
                    onChange()
                    dismiss()
                } catch {
                    notice = "削除できませんでした: \(error.localizedDescription)"
                }
            }
            Button("キャンセル", role: .cancel) {}
        } message: {
            Text("割り当てていた場合は既定に戻ります。")
        }
    }

    private var canAssignKana: Bool { def?.mode == .kana || def?.mode == .custom }
    private var canAssignAlpha: Bool { def?.mode == .alpha || def?.mode == .custom }

    private func infoRow(_ title: String, _ value: String) -> some View {
        HStack {
            Text(title)
            Spacer()
            Text(value).foregroundColor(.secondary)
        }
    }

    private func assignRow(title: String, current: String, mode: LayoutMode) -> some View {
        HStack {
            Text(title)
            Spacer()
            if current == id {
                Text("使用中").foregroundColor(.secondary)
                Button("既定に戻す") {
                    LayoutAssignment.set(id: nil, for: mode)
                    refreshAssignment()
                }
                .buttonStyle(.bordered)
            } else {
                Button("割り当てる") {
                    LayoutAssignment.set(id: id, for: mode)
                    refreshAssignment()
                }
                .buttonStyle(.borderedProminent)
            }
        }
    }

    private func refreshAssignment() {
        assignedKana = LayoutAssignment.id(for: .kana)
        assignedAlpha = LayoutAssignment.id(for: .alpha)
        onChange()
    }

    private func load() {
        refreshAssignment()
        guard let data = NagiLayoutStore.rawData(id: id, isUser: isUser) else {
            def = nil
            loadError = "ファイルがありません"
            return
        }
        exportURL = NagiLayoutStore.exportURL(id: id, data: data)
        do {
            def = try LayoutLoader.decode(data)
            loadError = nil
        } catch let error as LayoutLoader.LoadError {
            def = nil
            loadError = error.message
        } catch {
            def = nil
            loadError = error.localizedDescription
        }
    }
}

// MARK: - プレビュー

/// レイアウトの簡易プレビュー。行の高さ・キーの幅の比と、ラベル（無ければ中身の先頭）だけを描く
private struct NagiLayoutPreview: View {
    let def: KeyboardLayoutDef

    var body: some View {
        VStack(spacing: 3) {
            ForEach(Array(def.rows.enumerated()), id: \.offset) { _, row in
                previewRow(row)
            }
        }
    }

    private func previewRow(_ row: LayoutRow) -> some View {
        let weights = row.keys.reduce(0.0) { $0 + $1.width }
        return GeometryReader { geo in
            HStack(spacing: 3) {
                ForEach(Array(row.keys.enumerated()), id: \.offset) { _, key in
                    previewKey(key)
                        .frame(width: keyWidth(key, weights: weights, total: geo.size.width, count: row.keys.count))
                }
            }
        }
        .frame(height: 30 * CGFloat(row.height))
    }

    private func keyWidth(_ key: LayoutKey, weights: Double, total: CGFloat, count: Int) -> CGFloat {
        guard weights > 0 else { return 0 }
        let available = total - 3 * CGFloat(max(count - 1, 0))
        return max(0, available * CGFloat(key.width / weights))
    }

    @ViewBuilder
    private func previewKey(_ key: LayoutKey) -> some View {
        if key.kind == .spacer {
            Color.clear
        } else {
            ZStack {
                RoundedRectangle(cornerRadius: 4)
                    .fill(key.style == .function ? Color(UIColor.systemGray4) : Color(UIColor.secondarySystemBackground))
                if key.style == .accent {
                    RoundedRectangle(cornerRadius: 4).fill(Color.accentColor.opacity(0.4))
                }
                if let icon = key.icon, key.label == nil {
                    Image(systemName: icon).font(.system(size: 11))
                } else {
                    Text(label(of: key))
                        .font(.system(size: 11))
                        .lineLimit(1)
                        .minimumScaleFactor(0.5)
                        .padding(.horizontal, 1)
                }
                if key.longPress != nil {
                    // 長押しがあるキーは右上に小さな点
                    Circle().fill(Color.secondary).frame(width: 3, height: 3)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                        .padding(2)
                }
            }
        }
    }

    private func label(of key: LayoutKey) -> String {
        if let label = key.label, !label.isEmpty { return label }
        if let text = key.text { return text }
        if let kana = key.kana { return String(kana.prefix(1)) }
        if let center = key.center?.text { return center }
        if let first = key.cycle?.first { return first }
        if let action = key.action { return action }
        if let target = key.target { return target }
        return key.kind.rawValue
    }
}
