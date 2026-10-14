import SwiftUI

@main
struct NagiSpikeApp: App {
    @StateObject private var layoutInbox = NagiLayoutURLInbox.shared

    init() { LaunchBaseline.capture() }

    var body: some Scene {
        WindowGroup {
            ContentView()
                // nagi://layout?data=<base64url json>（サイトで作ったレイアウトの 1 タップ取り込み）
                .onOpenURL { url in layoutInbox.handle(url) }
        }
    }
}
