import SwiftUI

@main
struct NagiSpikeApp: App {
    @StateObject private var layoutInbox = NagiLayoutURLInbox.shared
    @Environment(\.scenePhase) private var scenePhase

    init() { LaunchBaseline.capture() }

    private static func captureClipboard() {
        ClipboardCapture.captureIfChanged(
            config: NagiIOSSettingsStore.load().clipboardHistory, hasFullAccess: true, isSecure: false,
            store: ClipboardHistoryStore())
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                // nagi://layout?data=<base64url json>（サイトで作ったレイアウトの 1 タップ取り込み）
                .onOpenURL { url in layoutInbox.handle(url) }
                // クリップボード履歴: アプリが前面になったとき、クリップボードが変わっていれば取り込む（履歴がオンのときだけ）
                .onAppear { Self.captureClipboard() }
                .onChange(of: scenePhase) { phase in
                    if phase == .active { Self.captureClipboard() }
                }
        }
    }
}
