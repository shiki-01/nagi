import UIKit

/// クリップボードから履歴への取り込み（キーボード拡張とホストアプリで共有）。
///
/// iOS のキーボード拡張はバックグラウンドでクリップボードを監視できないため、動いている間の次のタイミングでだけ取り込む:
/// キーボードの表示時・`UIPasteboard.changedNotification`・ホストアプリが前面になったとき・「貼り付け」ボタンで読んだとき。
/// 読むかどうかは `UIPasteboard.changeCount` が前回記録した値と違うかで決める（値は App Group の UserDefaults で
/// ホストと拡張が共有する）。`hasStrings` は読まない確認なので先に使う。
/// 内容はログ（DiagLog）に出さない。
enum ClipboardCapture {
    private static let changeCountKey = "clipboardHistoryChangeCount"

    private static var defaults: UserDefaults? {
        UserDefaults(suiteName: SharedStore.shared.appGroupId)
    }

    /// 前回記録した changeCount
    private static var lastChangeCount: Int? {
        defaults?.object(forKey: changeCountKey) as? Int
    }

    /// 履歴がオンで、クリップボードが変わっていれば読んで記録する。記録したら true
    /// - hasFullAccess: フルアクセス（`RequestsOpenAccess`）が有効か。ホストアプリは常に true
    /// - isSecure: パスワード欄などにフォーカスがあるか
    @discardableResult
    static func captureIfChanged(
        config: ClipboardHistoryConfig, hasFullAccess: Bool, isSecure: Bool,
        store: ClipboardHistoryStore, now: Date = Date()
    ) -> Bool {
        let pasteboard = UIPasteboard.general
        let count = pasteboard.changeCount
        guard ClipboardCaptureLogic.shouldRead(
            enabled: config.enabled, hasFullAccess: hasFullAccess, isSecure: isSecure,
            currentChangeCount: count, lastChangeCount: lastChangeCount
        ) else { return false }
        // 許可の確認が出て拒否された場合などに、毎回読み直して確認を繰り返さないよう、読む前に記録する
        defaults?.set(count, forKey: changeCountKey)
        guard pasteboard.hasStrings, let text = pasteboard.string else { return false }
        return store.record(text: text, now: now, settings: config)
    }

    /// 「貼り付け」ボタンで読んだ文字列を記録する（取り込みと同じ条件）。同じ内容を次の取り込みで読み直さないよう
    /// changeCount も覚える
    @discardableResult
    static func recordPasted(
        _ text: String, config: ClipboardHistoryConfig, hasFullAccess: Bool, isSecure: Bool,
        store: ClipboardHistoryStore, now: Date = Date()
    ) -> Bool {
        guard config.enabled, hasFullAccess, !isSecure else { return false }
        defaults?.set(UIPasteboard.general.changeCount, forKey: changeCountKey)
        return store.record(text: text, now: now, settings: config)
    }
}
