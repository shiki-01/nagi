import SwiftUI
import UIKit

// 実機での確認作業用の画面部品（診断ログ・入力欄の種類、ファイル確認）。設定の「開発者モード」をオンにすると出る
// 「開発者」画面（DeveloperView.swift）に置く。入力欄の種類とファイル確認は `DiagLog.isTestBuild`（DEBUG または
// TestFlight）のときだけ表示する。

// MARK: - 診断ログ

/// 診断ログの操作。本文は常時描画せず（ログが増えると重くなる）、概要だけを出して、
/// 本文は「ログを開く」のシートで最新の行だけを軽く表示する
struct NagiDiagLogSection: View {
    @State private var enabled = DiagLog.isEnabled
    @State private var summary = DiagLog.Summary.empty
    @State private var shareURL: URL?
    @State private var copied = false
    @State private var showingViewer = false
    @State private var confirmingClear = false

    var body: some View {
        Section {
            Toggle("キーボードの動作を記録する", isOn: Binding(
                get: { enabled },
                set: { newValue in
                    enabled = newValue
                    DiagLog.isEnabled = newValue
                }
            ))
            Text("入力した文字を含みます。既定では TestFlight・開発用ビルドでのみ有効で、App Store 配布では無効です。直近 \(DiagLog.maxBytes / 1024)KB 程度だけ残ります。")
                .font(.footnote)
                .foregroundColor(.secondary)
            Text(DiagLog.summaryText(summary))
                .font(.footnote)
                .monospacedDigit()
            Button("更新") { refresh() }
                .buttonStyle(.borderless)
            Button(copied ? "コピーしました" : "コピー") {
                UIPasteboard.general.string = DiagLog.read()
                copied = true
            }
            .buttonStyle(.borderless)
            Button("ログを開く（最新 \(NagiDiagLogViewer.maxLines) 行）") { showingViewer = true }
                .buttonStyle(.borderless)
                .sheet(isPresented: $showingViewer) { NagiDiagLogViewer() }
            if let url = shareURL {
                ShareLink(item: url) {
                    Label("共有（ファイル）", systemImage: "square.and.arrow.up")
                }
            } else {
                Text("共有（ログが空です）").font(.footnote).foregroundColor(.secondary)
            }
            Button("消去", role: .destructive) { confirmingClear = true }
                .buttonStyle(.borderless)
                .confirmationDialog("診断ログを消去しますか？", isPresented: $confirmingClear, titleVisibility: .visible) {
                    Button("消去", role: .destructive) {
                        DiagLog.clear()
                        copied = false
                        refresh()
                    }
                    Button("キャンセル", role: .cancel) {}
                }
        } header: {
            Text("診断ログ（D-1）")
        }
        .onAppear { refresh() }
    }

    private func refresh() {
        DispatchQueue.global(qos: .utility).async {
            let latest = DiagLog.summary()
            let url = DiagLog.exportCopy()
            DispatchQueue.main.async {
                summary = latest
                shareURL = url
            }
        }
    }
}

/// 最新の行だけを LazyVStack で出す軽い表示
struct NagiDiagLogViewer: View {
    static let maxLines = 300

    @Environment(\.dismiss) private var dismiss
    @State private var lines: [String] = []
    @State private var loaded = false

    var body: some View {
        NavigationStack {
            ScrollViewReader { proxy in
                ScrollView {
                    if loaded && lines.isEmpty {
                        Text("（空）").font(.footnote).foregroundColor(.secondary).padding()
                    }
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(0..<lines.count, id: \.self) { index in
                            Text(lines[index])
                                .font(.system(size: 10, design: .monospaced))
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .padding(8)
                }
                .onChange(of: lines.count) { count in
                    if count > 0 { proxy.scrollTo(count - 1, anchor: .bottom) }
                }
            }
            .navigationTitle("診断ログ（最新 \(Self.maxLines) 行）")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("閉じる") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("更新") { load() }
                }
            }
            .onAppear { load() }
        }
    }

    private func load() {
        DispatchQueue.global(qos: .userInitiated).async {
            let tail = DiagLog.tailLines(of: DiagLog.read(), maxLines: Self.maxLines)
            DispatchQueue.main.async {
                lines = tail
                loaded = true
            }
        }
    }
}

// MARK: - 入力欄の種類

/// `isSecureTextEntry` を明示した UITextField（SwiftUI の SecureField より確実に属性を指定できる）
struct NagiSecureTextField: UIViewRepresentable {
    @Binding var text: String
    let placeholder: String

    func makeUIView(context: Context) -> UITextField {
        let field = UITextField()
        field.isSecureTextEntry = true
        field.placeholder = placeholder
        field.borderStyle = .none
        field.autocorrectionType = .no
        field.autocapitalizationType = .none
        field.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .editingChanged)
        return field
    }

    func updateUIView(_ field: UITextField, context: Context) {
        if field.text != text { field.text = text }
    }

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    final class Coordinator: NSObject {
        private let text: Binding<String>

        init(text: Binding<String>) { self.text = text }

        @objc func changed(_ field: UITextField) {
            text.wrappedValue = field.text ?? ""
        }
    }
}

/// 入力欄の種類を試す欄（テストビルドのみ）。種類ごとに、キーボード側で期待する動作を 1 行添える
struct NagiInputKindsSection: View {
    @State private var plain = ""
    @State private var secure = ""
    @State private var password = ""
    @State private var newPassword = ""
    @State private var oneTime = ""
    @State private var email = ""
    @State private var url = ""
    @State private var number = ""
    @State private var decimal = ""
    @State private var asciiNumber = ""
    @State private var search = ""
    @State private var editor = ""

    var body: some View {
        Section {
            kind("通常の TextField", "通常どおり変換でき、学習・文脈も使われる。診断ログに入力内容が残る") {
                TextField("通常", text: $plain)
            }
            kind("SecureField（isSecureTextEntry）",
                 "iOS の仕様で標準キーボードになる（他社キーボードは使えない）。Nagi が出た場合は学習・文脈が止まり、診断ログの入力内容が伏せられる") {
                NagiSecureTextField(text: $secure, placeholder: "パスワード（secure）")
                    .frame(height: 32)
            }
            kind("パスワード（.password）",
                 "iOS が標準キーボードに切り替えることがある（パスワードの自動入力のため）。Nagi が出た場合は学習・文脈が止まり、診断ログの入力内容が伏せられる") {
                TextField("パスワード", text: $password)
                    .textContentType(.password)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
            kind("新しいパスワード（.newPassword）",
                 "パスワード欄と同じく学習・文脈が止まり、診断ログの入力内容が伏せられる") {
                TextField("新しいパスワード", text: $newPassword)
                    .textContentType(.newPassword)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
            kind("ワンタイムコード（.oneTimeCode）",
                 "パスワード欄と同じく学習・文脈が止まり、診断ログの入力内容が伏せられる") {
                TextField("ワンタイムコード", text: $oneTime)
                    .textContentType(.oneTimeCode)
            }
            kind("メール（.emailAddress）", "英字側のレイアウトで開き、@ が入力しやすい。学習・文脈は通常どおり") {
                TextField("メールアドレス", text: $email)
                    .keyboardType(.emailAddress)
                    .textContentType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
            kind("URL（.URL）", "英字側のレイアウトで開き、/ や . が入力しやすい。学習・文脈は通常どおり") {
                TextField("https://", text: $url)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
            kind("数字（.numberPad）", "数字の配列で開く（IsASCIICapable を申告しているため Nagi が使える。H-35）") {
                TextField("数字", text: $number)
                    .keyboardType(.numberPad)
            }
            kind("小数（.decimalPad）", "数字の欄 .numberPad で Nagi が出ない場合の切り分け用。ここで Nagi が出れば .numberPad は iOS の仕様（H-35）") {
                TextField("小数", text: $decimal)
                    .keyboardType(.decimalPad)
            }
            kind("ASCII 数字（.asciiCapableNumberPad）", "同上の切り分け用。ASCII 対応のキーボードが使える数字欄") {
                TextField("ASCII 数字", text: $asciiNumber)
                    .keyboardType(.asciiCapableNumberPad)
            }
            kind("検索（returnKeyType .search）", "改行キーが「検索」表示になり、押すと確定して欄の送信動作が走る") {
                TextField("検索", text: $search)
                    .submitLabel(.search)
            }
            kind("複数行 TextEditor", "改行キーで改行が入り、前後の文脈を使った変換ができる") {
                TextEditor(text: $editor)
                    .frame(minHeight: 100)
            }
        } header: {
            Text("入力欄の種類（テスト用）")
        } footer: {
            Text("各欄で Nagi に切り替えて入力し、期待する動作になるかを確認します。結果は診断ログで確認できます。")
        }
    }

    private func kind<Content: View>(_ title: String, _ expectation: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.subheadline).fontWeight(.semibold)
            content()
            Text("期待する動作: \(expectation)")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .padding(.vertical, 2)
    }
}

// MARK: - ファイル確認

/// 設定ファイル・学習ファイルなど、App Group にあるファイルの状態（テストビルドのみ）
struct NagiFileInspectorView: View {
    private struct Snapshot {
        var userDir: String = "(不明)"
        var configInfo: String = ""
        var configText: String?
        var iosSettingsInfo: String = ""
        var iosSettingsText: String?
        var others: [String] = []
    }

    @State private var snapshot = Snapshot()
    @State private var copied = false

    var body: some View {
        List {
            Section("場所") {
                Text(snapshot.userDir)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                Button("再読み込み") { reload() }
                    .buttonStyle(.borderless)
            }

            Section {
                Text(snapshot.configInfo).font(.footnote)
                if let text = snapshot.configText {
                    ScrollView {
                        Text(text)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)
                    }
                    .frame(maxHeight: 280)
                    Button(copied ? "コピーしました" : "config.toml をコピー") {
                        UIPasteboard.general.string = text
                        copied = true
                    }
                    .buttonStyle(.borderless)
                }
            } header: {
                Text("config.toml")
            }

            Section {
                Text(snapshot.iosSettingsInfo).font(.footnote)
                if let text = snapshot.iosSettingsText {
                    ScrollView {
                        Text(text)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)
                    }
                    .frame(maxHeight: 240)
                }
            } header: {
                Text("ios-settings.json")
            }

            Section("学習・ユーザー辞書") {
                ForEach(snapshot.others, id: \.self) { line in
                    Text(line).font(.footnote)
                }
            }
        }
        .navigationTitle("ファイル確認")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { reload() }
    }

    private func reload() {
        DispatchQueue.global(qos: .userInitiated).async {
            let latest = Self.inspect()
            DispatchQueue.main.async {
                snapshot = latest
                copied = false
            }
        }
    }

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return f
    }()

    private static func attributes(_ url: URL) -> (isDirectory: Bool, size: Int, modified: Date?)? {
        let fileManager = FileManager.default
        var isDir: ObjCBool = false
        guard fileManager.fileExists(atPath: url.path, isDirectory: &isDir),
              let attrs = try? fileManager.attributesOfItem(atPath: url.path) else { return nil }
        return (isDir.boolValue, (attrs[.size] as? NSNumber)?.intValue ?? 0, attrs[.modificationDate] as? Date)
    }

    private static func describe(_ url: URL) -> String {
        guard let a = attributes(url) else { return "なし" }
        let size = a.isDirectory ? "" : "\(a.size) B"
        let date = a.modified.map { dateFormatter.string(from: $0) } ?? "?"
        return "あり \(size) 更新 \(date)"
    }

    private static func inspect() -> Snapshot {
        var s = Snapshot()
        let located = NagiUserDictStore.locate()
        guard let userDir = located.userDir else {
            s.userDir = located.message ?? "(不明)"
            return s
        }
        s.userDir = userDir.path
        let config = userDir.appendingPathComponent("config.toml")
        s.configInfo = describe(config)
        s.configText = try? String(contentsOf: config, encoding: .utf8)

        if let url = NagiIOSSettingsStore.fileURL {
            s.iosSettingsInfo = describe(url)
            s.iosSettingsText = try? String(contentsOf: url, encoding: .utf8)
        } else {
            s.iosSettingsInfo = "保存先なし"
        }

        for name in ["learning.tsv", "learning-next.tsv"] {
            s.others.append("\(name): \(describe(userDir.appendingPathComponent(name)))")
        }
        let dict = userDir.appendingPathComponent("user_dict", isDirectory: true)
        if let a = attributes(dict), a.isDirectory {
            let names = ((try? FileManager.default.contentsOfDirectory(atPath: dict.path)) ?? []).sorted()
            s.others.append("user_dict/: あり（\(names.count) ファイル）")
            for name in names {
                s.others.append("  \(name): \(describe(dict.appendingPathComponent(name)))")
            }
        } else {
            s.others.append("user_dict/: なし")
        }
        return s
    }
}
