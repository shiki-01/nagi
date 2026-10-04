import SwiftUI
import UIKit

/// 本体アプリの状態。キーボード拡張とは別プロセスなので、拡張の状態そのものではなく、
/// 同じ規則で解決したパス（辞書・App Group・user_dir）と、エンジンの自己テスト結果を表示する
final class AppModel: ObservableObject {
    @Published var statusText = ""
    @Published var selfTestText = ""
    @Published var selfTestRunning = false
    @Published var layoutMessage = ""
    @Published var customLayoutText = ""
    @Published var keyboardLogText = ""

    private var paths = NagiPaths.resolve()

    func refresh() {
        paths = NagiPaths.resolve()
        let outcome = ConfigBootstrap.ensureConfig(userDir: paths.userDir)
        var lines: [String] = []
        lines.append("エンジン: \(NagiEngine.version)")
        lines.append("辞書: \(paths.dictPath == nil ? "なし" : "あり（\(paths.dataSource)）")")
        if let dict = paths.dictPath,
           let attrs = try? FileManager.default.attributesOfItem(atPath: dict),
           let size = attrs[.size] as? NSNumber {
            lines.append(String(format: "辞書サイズ: %.1f MB", size.doubleValue / 1024.0 / 1024.0))
        }
        if let id = paths.appGroup.resolvedID {
            lines.append("App Group: 利用可（\(id)）")
        } else {
            lines.append("App Group: 利用不可（拡張ごとに別のデータ領域で動作します）")
        }
        lines.append("ユーザーデータ: \(paths.userDirSource)")
        lines.append("config.toml: \(outcome.rawValue)")
        lines.append("LLM: 無効（辞書のみ）")
        statusText = lines.joined(separator: "\n")
        loadKeyboardLog()
        customLayoutText = customLayoutSummary()
    }

    func runSelfTest() {
        guard !selfTestRunning else { return }
        selfTestRunning = true
        selfTestText = ""
        let paths = self.paths
        DispatchQueue.global(qos: .userInitiated).async {
            let text = SelfTest.run(paths: paths)
            DispatchQueue.main.async {
                self.selfTestText = text
                self.selfTestRunning = false
            }
        }
    }

    /// 診断情報（状態・自己テスト・キーボードのログ）を 1 つのテキストにする
    func diagnosticsText() -> String {
        var lines: [String] = ["Nagi 診断（本体アプリ）", statusText, paths.summary]
        if !selfTestText.isEmpty {
            lines.append("--- 自己テスト ---")
            lines.append(selfTestText)
        }
        if !keyboardLogText.isEmpty {
            lines.append("--- キーボード拡張の診断 ---")
            lines.append(keyboardLogText)
        }
        return lines.joined(separator: "\n")
    }

    func copyDiagnostics() {
        UIPasteboard.general.string = diagnosticsText()
    }

    /// クリップボードのレイアウト JSON を検査して、ユーザー定義のレイアウトとして保存する
    func importLayoutFromClipboard() {
        guard let userDir = paths.userDir else {
            layoutMessage = "ユーザーデータの置き場がありません"
            return
        }
        guard let text = UIPasteboard.general.string, !text.isEmpty else {
            layoutMessage = "クリップボードが空です"
            return
        }
        let problems = LayoutStore.saveUserLayout(jsonText: text, userDir: userDir)
        if problems.isEmpty {
            if paths.appGroup.containerURL == nil {
                layoutMessage = "保存しました。ただし App Group が使えないため、キーボード拡張からは読めません"
            } else {
                layoutMessage = "保存しました。キーボードを開き直すと反映されます"
            }
        } else {
            layoutMessage = "取り込めません: " + problems.joined(separator: " / ")
        }
        customLayoutText = customLayoutSummary()
    }

    private func customLayoutSummary() -> String {
        guard let userDir = paths.userDir else { return "" }
        let dir = URL(fileURLWithPath: userDir).appendingPathComponent(LayoutStore.userFolder)
        guard let urls = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else {
            return "ユーザー定義レイアウト: なし"
        }
        let names = urls.map { $0.lastPathComponent }.filter { $0.hasSuffix(".json") }.sorted()
        return names.isEmpty ? "ユーザー定義レイアウト: なし" : "ユーザー定義レイアウト: " + names.joined(separator: ", ")
    }

    /// App Group があれば、キーボード拡張が書き出した診断を読む
    private func loadKeyboardLog() {
        guard paths.appGroup.containerURL != nil, let userDir = paths.userDir else {
            keyboardLogText = ""
            return
        }
        let url = URL(fileURLWithPath: userDir).appendingPathComponent("diagnostics-keyboard.log")
        keyboardLogText = (try? String(contentsOf: url, encoding: .utf8)) ?? ""
    }
}

struct ContentView: View {
    @StateObject private var model = AppModel()
    @State private var testText = ""
    @State private var showCopied = false

    var body: some View {
        NavigationStack {
            List {
                Section("キーボードの有効化") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("1. 設定 > 一般 > キーボード > キーボード > 新しいキーボードを追加")
                        Text("2. 「Nagi」を選んで追加")
                        Text("3. 追加した「Nagi」をタップし、「フルアクセスを許可」は **オフのまま** にします（不要です）")
                        Text("4. 入力欄で 🌐 を長押しして「Nagi」を選びます")
                    }
                    .font(.callout)
                }

                Section("状態") {
                    Text(model.statusText)
                        .font(.system(.footnote, design: .monospaced))
                        .textSelection(.enabled)
                    Button("再読み込み") { model.refresh() }
                }

                Section("テスト入力") {
                    TextField("ここで Nagi キーボードを試します", text: $testText, axis: .vertical)
                        .lineLimit(3...6)
                    Button("入力欄を消去") { testText = "" }
                }

                Section("エンジン自己テスト") {
                    Button(model.selfTestRunning ? "実行中…" : "自己テストを実行") { model.runSelfTest() }
                        .disabled(model.selfTestRunning)
                    if !model.selfTestText.isEmpty {
                        Text(model.selfTestText)
                            .font(.system(.footnote, design: .monospaced))
                            .textSelection(.enabled)
                    }
                }

                Section("レイアウト") {
                    Text(model.customLayoutText).font(.footnote)
                    Button("クリップボードのレイアウト JSON を取り込む") { model.importLayoutFromClipboard() }
                    if !model.layoutMessage.isEmpty {
                        Text(model.layoutMessage).font(.footnote)
                    }
                }

                Section("診断") {
                    Button("診断情報をコピー") {
                        model.copyDiagnostics()
                        showCopied = true
                    }
                    if showCopied {
                        Text("コピーしました").font(.footnote).foregroundColor(.green)
                    }
                    if !model.keyboardLogText.isEmpty {
                        Text(model.keyboardLogText)
                            .font(.system(.caption2, design: .monospaced))
                            .textSelection(.enabled)
                    }
                }
            }
            .navigationTitle("Nagi")
            .onAppear { model.refresh() }
        }
    }
}
