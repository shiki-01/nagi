import UIKit

/// 入力欄の種類（`UITextInputTraits`）から、キーボードの表示を決める純粋な対応表。
/// `KeyboardModel.applyInputTraits()` がフォーカスのたび・入力欄の切替のたびに引く
enum InputTraitsMapping {
    /// 入力欄の種類に合わせて自動で選ぶレイアウト
    enum PreferredLayout: Equatable {
        case kana, alpha, numbers

        /// `KeyboardModel.switchLayout(target:)` に渡す切替先（かな・英字は各モードの割り当て、数字は組み込みの id）
        var target: String {
            switch self {
            case .kana: return "kana"
            case .alpha: return "alpha"
            case .numbers: return "numbers"
            }
        }

        /// このレイアウトに当たる `LayoutMode`（既にそのモードを表示中かの判定に使う）
        var mode: LayoutMode {
            switch self {
            case .kana: return .kana
            case .alpha: return .alpha
            case .numbers: return .number
            }
        }
    }

    /// `keyboardType` に合うレイアウト。指定なし（通常の文章の欄など）は nil で、ユーザーが選んだレイアウトのままにする
    static func preferredLayout(for type: UIKeyboardType) -> PreferredLayout? {
        switch type {
        case .emailAddress, .URL, .asciiCapable:
            return .alpha
        case .numberPad, .decimalPad, .numbersAndPunctuation, .asciiCapableNumberPad, .phonePad, .namePhonePad:
            return .numbers
        case .default, .twitter, .webSearch:
            return nil
        @unknown default:
            return nil
        }
    }

    /// 改行キーに出す文字。既定（改行）とその他は nil で、レイアウトのラベルのままにする
    static func returnLabel(for type: UIReturnKeyType) -> String? {
        switch type {
        case .search, .google, .yahoo: return "検索"
        case .go: return "開く"
        case .send: return "送信"
        case .done: return "完了"
        case .next: return "次へ"
        case .join: return "参加"
        case .route: return "経路"
        case .`continue`: return "続ける"
        case .emergencyCall: return "緊急"
        case .default: return nil
        @unknown default: return nil
        }
    }
}
