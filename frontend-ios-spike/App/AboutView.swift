import SwiftUI

// About タブ。バージョン、プライバシー方針の要約、ライセンス表示、サイト、フィードバックの案内。
// 検証用の画面（Probes・結果）は「開発者向け」からたどる（呼び出し側が developer で渡す）。

struct NagiAboutView<Developer: View>: View {
    private let developer: Developer

    init(@ViewBuilder developer: () -> Developer) {
        self.developer = developer()
    }

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
                        Text("・フルアクセスは、辞書とモデルの共有領域をキーボードが読むためだけに使います。許可しなくても、ひらがな・カタカナ変換は使えます。")
                        Text("・パスワード欄などでは学習を止めます。入力内容をログに残しません。")
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
                }

                Section("サイト・フィードバック") {
                    Link("nagi.shiki-01.com", destination: Self.siteURL)
                    Text("不具合や要望は、TestFlight のフィードバック機能（スクリーンショットを撮って共有）か、サイトからお知らせください。診断ログには入力内容が含まれるため、共有前に内容を確認してください。")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }

                Section("開発者向け") {
                    NavigationLink("端末情報・App Group・Probes・結果") {
                        developer
                    }
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
                Text("ライセンス全文はリポジトリの NOTICE ファイルにあります（https://github.com/shiki-01/nagi）。")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
        }
        .navigationTitle("辞書・モデルの出典")
        .navigationBarTitleDisplayMode(.inline)
    }
}
