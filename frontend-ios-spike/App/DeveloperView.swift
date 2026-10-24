import SwiftUI
import UIKit

// 開発者モード（設定 `developer.mode`。docs/ios-settings.md、docs/decisions.md H-64）。
// 診断ログ・実機計測・動作テスト用の画面など、コアの機能に関係しないものは、開発者モードをオンにしたときだけ
// 設定の「開発者」から開ける。診断ログの既定の ON/OFF は変えない（TestFlight・開発用ビルドは既定で有効。H-22）。

/// 開発者モードの状態。ホストアプリの画面が共有して参照する（値は App Group の `ios-settings.json`。
/// 設定画面が書いたら `update` で知らせる）。キーボード拡張は `NagiIOSSettings.developerMode` を直接読む
final class NagiDeveloperMode: ObservableObject {
    static let shared = NagiDeveloperMode()

    @Published private(set) var isEnabled: Bool

    private init() {
        isEnabled = NagiIOSSettingsStore.load().developerMode
    }

    /// 保存済みの値を読み直す
    func reload() {
        update(NagiIOSSettingsStore.load().developerMode)
    }

    func update(_ enabled: Bool) {
        if enabled != isEnabled { isEnabled = enabled }
    }

    /// 動作テスト用の部品（実機確認用のテストデータ・レイアウトなど）を出すか。
    /// TestFlight・開発用ビルドで、開発者モードがオンのときだけ（App Store 版では出さない。H-22）
    var showsTestTools: Bool { DiagLog.isTestBuild && isEnabled }
}

/// 「開発者」画面: 診断ログ、入力欄の種類のテスト、端末情報・Probes・結果、ファイル確認
struct NagiDeveloperView<Probes: View>: View {
    private let probes: Probes

    init(@ViewBuilder probes: () -> Probes) {
        self.probes = probes()
    }

    var body: some View {
        List {
            NagiDiagLogSection()

            // 入力欄の種類を試す欄・計測の画面は TestFlight・開発用ビルドだけ（App Store 版では出さない）
            if DiagLog.isTestBuild {
                NagiInputKindsSection()
                Section("計測・確認") {
                    NavigationLink("端末情報・App Group・Probes・結果") { probes }
                    NavigationLink("ファイル確認（config.toml・学習・辞書）") { NagiFileInspectorView() }
                }
            }

            Section {
                Text("開発者向けの機能です。設定の詳細モードにある「開発者モード」をオフにすると、この画面は出なくなります（診断ログを記録するかどうかは変わりません）。")
                    .font(.footnote)
                    .foregroundColor(.secondary)
            }
        }
        .navigationTitle("開発者")
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button("閉じる") {
                    UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
                }
            }
        }
    }
}
