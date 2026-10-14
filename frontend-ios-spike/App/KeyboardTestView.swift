import SwiftUI

/// キーボードの動作確認用の入力欄。Nagi キーボードを選んで、入力・変換・確定を試す
struct KeyboardTestView: View {
    @State private var multiline = ""
    @State private var singleLine = ""
    @FocusState private var focusedField: Field?
    /// 診断ログ（D-1）の記録の有無と、表示中の内容
    @State private var diagEnabled = DiagLog.isEnabled
    @State private var diagLog = ""
    @State private var diagCopied = false

    private enum Field {
        case multiline, singleLine
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("使い方") {
                    Text("入力欄をタップしてキーボードを出し、地球儀キーで Nagi に切り替えてください。"
                        + "切り替えの一覧に Nagi が無いときは、設定 > 一般 > キーボード > 新しいキーボードを追加 で追加します。")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                }

                Section("複数行") {
                    TextEditor(text: $multiline)
                        .focused($focusedField, equals: .multiline)
                        .frame(minHeight: 140)
                    HStack {
                        Text("\(multiline.count) 文字")
                            .font(.footnote)
                            .foregroundColor(.secondary)
                        Spacer()
                        Button("クリア") { multiline = "" }
                    }
                }

                Section("1 行") {
                    TextField("ここに入力", text: $singleLine)
                        .focused($focusedField, equals: .singleLine)
                    HStack {
                        Text("\(singleLine.count) 文字")
                            .font(.footnote)
                            .foregroundColor(.secondary)
                        Spacer()
                        Button("クリア") { singleLine = "" }
                    }
                }

                Section("診断ログ（D-1）") {
                    Toggle("キーボードの動作を記録する", isOn: Binding(
                        get: { diagEnabled },
                        set: { newValue in
                            diagEnabled = newValue
                            DiagLog.isEnabled = newValue
                        }
                    ))
                    Text("入力した文字を含みます。TestFlight での確認期間だけ使い、配布前に無効にします。直近 \(DiagLog.maxLines) 行だけ残ります。")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                    HStack {
                        Button("表示を更新") { diagLog = DiagLog.read() }
                        Spacer()
                        Button(diagCopied ? "コピーしました" : "コピー") {
                            diagLog = DiagLog.read()
                            UIPasteboard.general.string = diagLog
                            diagCopied = true
                        }
                        Spacer()
                        Button("消去", role: .destructive) {
                            DiagLog.clear()
                            diagLog = ""
                            diagCopied = false
                        }
                    }
                    .buttonStyle(.borderless)
                    if diagLog.isEmpty {
                        Text("（空）").font(.footnote).foregroundColor(.secondary)
                    } else {
                        Text(diagLog)
                            .font(.system(size: 10, design: .monospaced))
                            .textSelection(.enabled)
                    }
                }
            }
            .onAppear { diagLog = DiagLog.read() }
            .navigationTitle("入力テスト")
            .toolbar {
                ToolbarItemGroup(placement: .keyboard) {
                    Spacer()
                    Button("閉じる") { focusedField = nil }
                }
            }
        }
    }
}
