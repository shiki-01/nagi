import Foundation

// 候補バーのツールバー（入力中でなく、候補が無いときに候補バーの場所に出す操作の列）。
// UI に依存しない純粋な定義（Tests/ToolbarActionTests.swift でテストする）。
// ツールバーはレイアウトの `toolbar`（キー列。rows のキーと同じ型）で定義でき、無いレイアウトでは設定 `toolbar.items`
// （`NagiIOSSettings`）の並びを `ToolbarKeys` でキー列にして、同じ描画経路で出す。実行は `KeyboardModel.perform(action:)` など。
// 項目を足すときは、ここに case と表示（`title`）を足し、`ToolbarKeys.spec(for:)` と既定アイコンの表（`ToolbarKeys.actionIcons`）に足す。

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
    /// クリップボード履歴のページを開く
    case history

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
        case .history: return "履歴"
        }
    }

    /// ボタンの SF Symbols 名（既定アイコンの表 `ToolbarKeys` から引く）
    var systemImage: String { ToolbarKeys.defaultIcon(for: ToolbarKeys.key(for: self)) ?? "questionmark" }

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
}

/// `ToolbarAction` ↔ ツールバーのキー（レイアウトの `toolbar.keys` と同じ型）の対応と、既定のアイコン。純関数
enum ToolbarKeys {
    /// ToolbarAction に対応するキーの種類と action 名 / layout の target（対応表の唯一の場所）
    static func spec(for action: ToolbarAction) -> (kind: LayoutKeyKind, action: String?, target: String?) {
        switch action {
        case .dismiss: return (.action, "dismiss", nil)
        case .paste: return (.action, "paste", nil)
        case .cursorLeft: return (.action, "moveLeft", nil)
        case .cursorRight: return (.action, "moveRight", nil)
        case .newline: return (.action, "enter", nil)
        case .space: return (.action, "space", nil)
        case .deleteWord: return (.action, "deleteWord", nil)
        case .emoji: return (.layout, nil, "emoji")
        case .nextKeyboard: return (.action, "globe", nil)
        case .history: return (.action, "history", nil)
        }
    }

    /// action 名ごとの既定アイコン（SF Symbols。ツールバーで icon / label が未指定のキーに使う）
    static let actionIcons: [String: String] = [
        "dismiss": "keyboard.chevron.compact.down",
        "paste": "doc.on.clipboard",
        "moveLeft": "chevron.left",
        "moveRight": "chevron.right",
        "enter": "return",
        "space": "space",
        "deleteWord": "delete.left",
        "globe": "globe",
        "history": "clock.arrow.circlepath",
    ]

    /// `layout` キーの target ごとの既定アイコン
    static let layoutIcons: [String: String] = ["emoji": "face.smiling"]

    /// キーの既定アイコン。表に無ければ nil
    static func defaultIcon(for key: LayoutKeyDef) -> String? {
        switch key.kind {
        case .action: return key.action.flatMap { actionIcons[$0] }
        case .layout: return key.target.flatMap { layoutIcons[$0] }
        default: return nil
        }
    }

    /// 生成するキーの id（`tb-<名前>`。レイアウトのキー id と衝突しない前提の接頭辞）
    static func keyId(for action: ToolbarAction) -> String { "tb-\(action.rawValue)" }

    /// ToolbarAction 1 つ分のキー（icon / label は未指定。既定のアイコンで描く）
    static func key(for action: ToolbarAction) -> LayoutKeyDef {
        let entry = spec(for: action)
        var result = LayoutKeyDef(id: keyId(for: action), kind: entry.kind, label: nil, action: entry.action)
        result.target = entry.target
        return result
    }

    /// 設定 `toolbar.items` の並びから作るキー列
    static func keys(for actions: [ToolbarAction]) -> [LayoutKeyDef] {
        actions.map { key(for: $0) }
    }

    /// 出すキー列。レイアウトの `toolbar` があればそれ、無ければ設定の項目から作る。
    /// `paste` のキーは、クリップボードに文字があるときだけ出す（`clipboardHasText` は設定 `candbar.paste_button` が
    /// オフなら常に false。フルアクセスが無い・許可されなければ、押したあとにも隠れる）
    static func visibleKeys(layoutToolbar: LayoutToolbar?, items: [ToolbarAction], clipboardHasText: Bool) -> [LayoutKeyDef] {
        let source = layoutToolbar?.keys ?? keys(for: items)
        return source.filter { !isPaste($0) || clipboardHasText }
    }

    private static func isPaste(_ key: LayoutKeyDef) -> Bool {
        key.kind == .action && key.action == "paste"
    }

    /// ツールバーのキーの表示内容。label も icon も無ければ既定のアイコン。`text` キーは text を文字にする
    static func faceContent(for key: LayoutKeyDef) -> KeyFaceContent {
        let text = key.label ?? (key.kind == .text ? key.text : nil)
        if key.icon == nil && text == nil, let icon = defaultIcon(for: key) {
            return KeyFaceContent(text: nil, icon: icon)
        }
        return KeyFaceContent(text: text ?? key.cycle?.first, icon: key.icon)
    }

    /// VoiceOver 用の名前（label があればそれ、無ければ action ごとの日本語名）
    static func accessibilityLabel(for key: LayoutKeyDef) -> String? {
        if let label = key.label { return label }
        for action in ToolbarAction.allCases {
            let entry = spec(for: action)
            if entry.kind == key.kind && entry.action == key.action && entry.target == key.target { return action.title }
        }
        return key.text
    }
}
