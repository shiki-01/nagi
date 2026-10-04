import Foundation
import CoreGraphics

// キーボードレイアウトの宣言的な定義（JSON）。描画・タッチ判定はこの定義だけから行い、
// 特定の配列を決め打ちしない（`Layouts/*.json`。書き方は README を参照）。
// このディレクトリのファイルは UIKit / NagiCore に依存させない（単体テストのターゲットにも含める）。

/// キーのタップ・長押しの動作（JSON の `{"type": ..., ...}`）
struct KeyAction: Codable, Equatable {
    /// `char` / `kana` / `key` / `layout` / `action`
    var type: String
    /// `char`・`kana` の文字
    var text: String?
    /// `key` の名前（`SpecialKey`）、`action` のコアの Action 名
    var name: String?
    /// `layout` の切替先レイアウト ID
    var layout: String?
}

/// 特殊キー（`{"type": "key", "name": ...}`）
enum SpecialKey: String, Equatable {
    case space
    case enter
    case backspace
    case left
    case right
    case shift
    case globe
    /// 直前のかなの 濁点・半濁点・小書きの循環（12 キーの「小゛゜」）
    case dakuten
}

/// `KeyAction` を解釈した結果
enum ResolvedAction: Equatable {
    /// ローマ字入力の 1 文字（`process_key`）
    case char(String)
    /// かなの直接挿入（`insert_reading`）
    case kana(String)
    case special(SpecialKey)
    case switchLayout(String)
    /// コアの Action を直接実行する（`perform_action`。例 `ToKatakana`、`MoveCursor:Left`）
    case engineAction(String)
    case unsupported
}

extension KeyAction {
    var resolved: ResolvedAction {
        switch type {
        case "char":
            if let t = text, !t.isEmpty { return .char(t) }
        case "kana":
            if let t = text, !t.isEmpty { return .kana(t) }
        case "key":
            if let n = name, let k = SpecialKey(rawValue: n) { return .special(k) }
        case "layout":
            if let l = layout, !l.isEmpty { return .switchLayout(l) }
        case "action":
            if let n = name, !n.isEmpty { return .engineAction(n) }
        default:
            break
        }
        return .unsupported
    }
}

/// フリックの方向
enum FlickDirection: String, CaseIterable {
    case center
    case left
    case up
    case right
    case down
}

/// 1 つのキー（または空白）の定義
struct KeyDef: Codable, Equatable {
    /// キー ID（レイアウト内で一意。空白は省略可）
    var id: String?
    /// 表示ラベル。省略時は `tap` の文字
    var label: String?
    /// 行内での相対幅（既定 1.0）
    var width: Double?
    /// true なら空白（描画・タッチ判定の対象外）
    var spacer: Bool?
    /// `normal`（既定）/ `function` / `accent`
    var style: String?
    /// タップ時の動作
    var tap: KeyAction?
    /// フリック出力。キーは `left` / `up` / `right` / `down`、値は出力する文字。
    /// 出力の種類（`char` / `kana`）は `tap` と同じ
    var flick: [String: String]?
    /// 長押しの動作
    var longPress: KeyAction?
    /// true なら押し続けると連打する（Backspace 用）
    var repeats: Bool?
}

extension KeyDef {
    var relativeWidth: Double { max(width ?? 1.0, 0.0) }
    var isSpacer: Bool { spacer ?? false }
    var tapAction: ResolvedAction { tap?.resolved ?? .unsupported }

    var isKana: Bool {
        if case .kana = tapAction { return true }
        return false
    }

    /// `tap` が `char` / `kana` のときの出力文字
    var outputText: String? {
        switch tapAction {
        case .char(let t): return t
        case .kana(let t): return t
        default: return nil
        }
    }

    /// 表示するラベル
    var displayLabel: String { label ?? outputText ?? "" }
}

struct KeyRow: Codable, Equatable {
    /// 他の行に対する相対の高さ（既定 1.0）
    var height: Double?
    var keys: [KeyDef]

    var relativeHeight: Double { max(height ?? 1.0, 0.0) }
}

struct KeyboardLayout: Codable, Equatable {
    var id: String
    /// 表示名
    var name: String
    /// true ならコア（変換）を通さず、押したキーをそのまま挿入する（英数入力）
    var direct: Bool?
    var rows: [KeyRow]

    var isDirect: Bool { direct ?? false }
}

extension KeyboardLayout {
    /// 定義の検査結果（空なら妥当）。ユーザー定義レイアウトの取り込み時に使う
    func validationErrors() -> [String] {
        var errors: [String] = []
        if id.isEmpty { errors.append("id が空です") }
        if rows.isEmpty { errors.append("rows が空です") }
        var seen = Set<String>()
        let flickKeys = Set(["left", "up", "right", "down"])
        for (r, row) in rows.enumerated() {
            let total = row.keys.reduce(0.0) { $0 + $1.relativeWidth }
            if total <= 0 { errors.append("行 \(r + 1): 幅の合計が 0 です") }
            if row.relativeHeight <= 0 { errors.append("行 \(r + 1): height が 0 以下です") }
            for key in row.keys where !key.isSpacer {
                let name = key.id ?? "(id なし)"
                guard let keyID = key.id, !keyID.isEmpty else {
                    errors.append("行 \(r + 1): id の無いキーがあります")
                    continue
                }
                if seen.contains(keyID) { errors.append("キー id が重複しています: \(keyID)") }
                seen.insert(keyID)
                if key.tapAction == .unsupported { errors.append("\(name): tap が不正または未指定です") }
                if let flick = key.flick {
                    for (dir, text) in flick {
                        if !flickKeys.contains(dir) { errors.append("\(name): flick の方向が不正です: \(dir)") }
                        if text.isEmpty { errors.append("\(name): flick.\(dir) が空です") }
                    }
                    if key.outputText == nil { errors.append("\(name): flick には tap が char / kana のキーが必要です") }
                }
                if let lp = key.longPress, lp.resolved == .unsupported {
                    errors.append("\(name): longPress が不正です")
                }
            }
        }
        return errors
    }

    /// このレイアウトの `layout` アクションの遷移先 ID
    var referencedLayoutIDs: [String] {
        var ids: [String] = []
        for row in rows {
            for key in row.keys {
                if case .switchLayout(let id) = key.tapAction { ids.append(id) }
                if let lp = key.longPress, case .switchLayout(let id) = lp.resolved { ids.append(id) }
            }
        }
        return ids
    }
}
