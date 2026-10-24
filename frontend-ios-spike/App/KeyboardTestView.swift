import SwiftUI

/// キーボードの動作確認用の入力欄。Nagi キーボードを選んで、入力・変換・確定を試す
struct KeyboardTestView: View {
    @State private var multiline = ""
    @State private var singleLine = ""
    @FocusState private var focusedField: Field?

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

                // 診断ログ・入力欄の種類のテストは、設定の「開発者モード」をオンにすると出る「開発者」画面にある
            }
            .navigationTitle("入力テスト")
            .toolbar {
                ToolbarItemGroup(placement: .keyboard) {
                    Spacer()
                    Button("閉じる") {
                        focusedField = nil
                        // 入力欄の種類のテスト欄（フォーカス管理の外）も閉じる
                        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
                    }
                }
            }
        }
    }
}
