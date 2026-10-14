import UIKit

/// キーボードのテーマ（設定 `keyboard.theme` / `keyboard.accent`）。値の対応だけを持つ純粋な処理
enum NagiTheme {
    /// `keyboard.theme` に対応する外観。system は端末に従う（上書きしない）
    static func interfaceStyle(_ theme: String) -> UIUserInterfaceStyle {
        switch theme {
        case "light": return .light
        case "dark": return .dark
        default: return .unspecified
        }
    }

    /// `keyboard.accent` に対応する色（ライト/ダークで自動的に調整されるシステムカラー）。知らない名前は青
    static func accentUIColor(_ name: String) -> UIColor {
        switch name {
        case "green": return .systemGreen
        case "orange": return .systemOrange
        case "pink": return .systemPink
        case "gray": return .systemGray
        default: return .systemBlue
        }
    }
}
