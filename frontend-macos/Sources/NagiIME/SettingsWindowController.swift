#if os(macOS)
import AppKit
import NagiClient
import SwiftUI

/// 「Nagi 設定」ウィンドウ。閉じても破棄せず再利用し、開いていれば前面化する。メインスレッドからのみ使う。
@MainActor
final class SettingsWindowController {
    private let model: SettingsModel
    private lazy var window: NSWindow = {
        let controller = NSHostingController(rootView: SettingsView(model: model))
        let w = NSWindow(contentViewController: controller)
        w.title = "Nagi 設定"
        w.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        w.isReleasedWhenClosed = false
        w.setContentSize(NSSize(width: 560, height: 520))
        w.center()
        return w
    }()

    init(supervisor: EngineSupervisor) {
        model = SettingsModel(supervisor: supervisor)
    }

    /// 表示して前面化し、最新の設定を読み込み直す
    func show() {
        model.load()
        window.makeKeyAndOrderFront(nil)
        // LSUIElement アプリなので明示的にアクティブ化しないと背面に出る
        NSApp.activate(ignoringOtherApps: true)
    }
}
#endif
