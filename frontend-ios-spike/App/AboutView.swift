import SwiftUI

// About タブ。バージョン、プライバシー方針の要約、ライセンス表示、サイト、フィードバックの案内。
// 検証用の画面（Probes・結果・診断ログ）は、設定の「開発者モード」をオンにすると出る「開発者」画面にある。

struct NagiAboutView: View {
    private static var siteURL: URL { URL(string: "https://nagi.shiki-01.com")! }

    private var versionText: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "\(short) (\(build))"
    }

    var body: some View {
        NavigationStack {
            List {
                Section("バージョン") {
                    HStack {
                        Text("Nagi")
                        Spacer()
                        Text(versionText)
                            .foregroundColor(.secondary)
                            .textSelection(.enabled)
                    }
                }

                Section("プライバシー") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("・変換・学習などの処理はすべて端末の中で行います。")
                        Text("・入力した内容を外部に送信しません。ネットワーク通信もしません。")
                        Text("・フルアクセスは、辞書とモデルの共有領域をキーボードが読むためだけに使います。許可しない場合の機能は端末により異なる場合があり、辞書とモデルの利用に許可が必要になる場合があります。")
                        Text("・パスワード欄などでは学習を止め、ログにも入力内容を残しません。")
                        if DiagLog.isTestBuild {
                            Text("・このビルド（TestFlight・開発用）では、不具合の調査用に診断ログを端末内にだけ記録します。既定で有効で、入力した文字を含む場合があります。外部には送信されません。設定の詳細モードで「開発者モード」をオンにすると出る「開発者」画面で、記録の無効化と消去ができます。")
                        } else {
                            Text("・診断ログは既定で無効です。")
                        }
                    }
                    .font(.callout)
                }

                Section("ライセンス") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Nagi のコード: MIT OR Apache-2.0")
                            .font(.callout)
                        Text("辞書・モデルは Nagi に含まれず、別途取得したものを使います。それぞれのライセンスが適用されます。")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                    NavigationLink("辞書・モデルの出典") {
                        NagiThirdPartyNoticeView()
                    }
                    NavigationLink("レイアウトのアイコンの出典") {
                        NagiIconLicensesView()
                    }
                    NavigationLink("ライセンス全文（NOTICE）") {
                        NagiBundledTextView(
                            title: "NOTICE",
                            resourceName: "NOTICE",
                            resourceExtension: nil
                        )
                    }
                    NavigationLink("オープンソースライセンス") {
                        NagiBundledTextView(
                            title: "オープンソースライセンス",
                            resourceName: "THIRD_PARTY_LICENSES",
                            resourceExtension: "md"
                        )
                    }
                }

                Section("サイト・フィードバック") {
                    Link("nagi.shiki-01.com", destination: Self.siteURL)
                    Text("不具合や要望は、TestFlight のフィードバック機能（スクリーンショットを撮って共有）か、サイトからお知らせください。診断ログには入力内容が含まれるため、共有前に内容を確認してください。")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
            }
            .navigationTitle("About")
        }
    }
}

/// 辞書・モデルの出典とライセンスの要約（全文はリポジトリの NOTICE）
struct NagiThirdPartyNoticeView: View {
    var body: some View {
        List {
            Section("システム辞書") {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Mozc OSS 辞書（dictionary_oss）")
                        .font(.callout)
                        .fontWeight(.semibold)
                    Text("https://github.com/google/mozc")
                        .font(.caption)
                        .foregroundColor(.secondary)
                    Text("IPAdic（Nara Institute of Science and Technology 他）と沖縄辞書を含みます。IPAdic は、著作権表示と免責条項をすべての複製に含めることを条件に、利用・複製・配布が許可されています。")
                        .font(.caption)
                }
            }

            Section("変換モデル") {
                VStack(alignment: .leading, spacing: 6) {
                    Text("zenz-v3.1（small / xsmall、GGUF）")
                        .font(.callout)
                        .fontWeight(.semibold)
                    Text("作者: Miwa Keita")
                        .font(.caption)
                    Text("https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf")
                        .font(.caption)
                        .foregroundColor(.secondary)
                    Text("ライセンス: CC BY-SA 4.0。モデルを再配布する場合は、表示（クレジット）と同一条件での共有を満たす必要があります。")
                        .font(.caption)
                }
            }

            Section {
                Text("ライセンス全文は、前の画面の「ライセンス全文（NOTICE）」で読めます（リポジトリの NOTICE ファイルと同じ内容です）。")
                    .font(.caption)
                    .foregroundColor(.secondary)
                if let url = URL(string: "https://github.com/shiki-01/nagi") {
                    Link("github.com/shiki-01/nagi", destination: url)
                        .font(.caption)
                }
            }
        }
        .navigationTitle("辞書・モデルの出典")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// アプリバンドルに同梱したテキストファイル（NOTICE など）をスクロール表示する。
/// 同梱されていない・読めないときはフォールバックの案内文を出す。
struct NagiBundledTextView: View {
    let title: String
    let resourceName: String
    let resourceExtension: String?

    private var text: String? {
        guard let url = Bundle.main.url(forResource: resourceName, withExtension: resourceExtension),
              let body = try? String(contentsOf: url, encoding: .utf8),
              !body.isEmpty
        else { return nil }
        return body
    }

    var body: some View {
        ScrollView {
            if let text = text {
                Text(text)
                    .font(.system(.caption, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
                    .textSelection(.enabled)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    Text("ライセンス文書を読み込めませんでした。")
                        .font(.callout)
                    Text("全文は https://github.com/shiki-01/nagi の \(resourceName) ファイルで確認できます。")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding()
            }
        }
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
    }
}
