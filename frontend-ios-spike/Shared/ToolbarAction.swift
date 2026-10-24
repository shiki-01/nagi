import Foundation

// 候補バーのツールバー（入力中でなく、候補が無いときに候補バーの場所に出す操作の列）。
// UI に依存しない純粋な定義（Tests/ToolbarActionTests.swift でテストする）。実行は `KeyboardModel.perform(toolbar:)`。
// 項目を足すときは、ここに case と表示（`title` / `systemImage`）を足し、`KeyboardModel.perform(toolbar:)` に処理を足す。
// 表示する項目と並び順は設定 `toolbar.items`（`NagiIOSSettings`）で決まる。

/// ツールバーの操作。`rawValue` が設定ファイルに保存される名前（変えない）
enum ToolbarAction: String, CaseIterable, Equatable {
    /// キーボードを閉じる
    case dismiss
    /// クリップボードの文字列を貼り付ける（クリップボードに文字があるときだけ出す）
    case paste
    /// カーソルを 1 文字左へ
    case cursorLeft
    /// カーソルを 1 文字右へ
    case cursorRight
    /// 改行
    case newline
    /// 半角スペース
    case space
    /// カーソル手前の単語を削除
    case deleteWord
    /// 絵文字のキーボードへ
    case emoji
    /// 次のキーボードへ（地球儀）
    case nextKeyboard

    /// 設定画面での表示名
    var title: String {
        switch self {
        case .dismiss: return "キーボードを閉じる"
        case .paste: return "貼り付け"
        case .cursorLeft: return "カーソルを左へ"
        case .cursorRight: return "カーソルを右へ"
        case .newline: return "改行"
        case .space: return "半角スペース"
        case .deleteWord: return "単語を削除"
        case .emoji: return "絵文字"
        case .nextKeyboard: return "次のキーボード"
        }
    }

    /// ボタンの SF Symbols 名
    var systemImage: String {
        switch self {
        case .dismiss: return "keyboard.chevron.compact.down"
        case .paste: return "doc.on.clipboard"
        case .cursorLeft: return "chevron.left"
        case .cursorRight: return "chevron.right"
        case .newline: return "return"
        case .space: return "space"
        case .deleteWord: return "delete.left"
        case .emoji: return "face.smiling"
        case .nextKeyboard: return "globe"
        }
    }

    /// ボタンにアイコンだけでなく名前も出すか（貼り付けは従来の「貼り付け」ボタンと同じ見た目にする）
    var showsTitle: Bool { self == .paste }

    /// 初期に出す項目と並び順
    static let defaultItems: [ToolbarAction] = [.dismiss, .paste, .cursorLeft, .cursorRight]
}

enum ToolbarLogic {
    /// 保存された名前の列から項目を作る。知らない名前・重複は捨てる（並びは保つ）
    static func actions(from names: [String]) -> [ToolbarAction] {
        var seen = Set<ToolbarAction>()
        var result: [ToolbarAction] = []
        for name in names {
            guard let action = ToolbarAction(rawValue: name), seen.insert(action).inserted else { continue }
            result.append(action)
        }
        return result
    }

    /// ツールバーを候補バーの場所に出すか。入力中でなく、候補が無く、メッセージ（辞書が無いなど）も無いとき
    static func shouldShow(hasPreedit: Bool, candidatesEmpty: Bool, hasMessage: Bool) -> Bool {
        !hasPreedit && candidatesEmpty && !hasMessage
    }

    /// いま実際に出す項目。貼り付けはクリップボードに文字があるときだけ（`clipboardHasText` は設定
    /// `candbar.paste_button` がオフなら常に false。フルアクセスが無い・許可されなければ、押したあとにも隠れる）
    static func visible(_ items: [ToolbarAction], clipboardHasText: Bool) -> [ToolbarAction] {
        items.filter { $0 != .paste || clipboardHasText }
    }
}
