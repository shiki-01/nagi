import Foundation

// スペースキーのスワイプ・⌫のスワイプの判定ロジック（UI に依存しない純粋な処理。Tests/TouchLogicTests.swift でテストする）

/// スペースキーのスワイプによるカーソル移動の量を決める。
/// 押下位置からの横方向の移動量を渡すと、前回から何文字ぶん動かすか（右が正、左が負）を返す。
/// 移動量が 1 ステップ（設定 `touch.space_swipe_step_pt`）に届くまでは何もせず（= 単純タップのまま）、
/// 届いたらカーソル移動モードに入る。モードに入ったあとは、ドラッグ距離に比例して動かし、指を戻せば戻る
struct SpaceSwipeTracker {
    let step: Double
    /// カーソル移動モードに入ったか（入ったら、離したときにスペースを入力しない）
    private(set) var isActive = false
    private var movedSteps = 0

    init(step: Double) {
        // 0 や負の値で割らない（設定は 4pt 以上に丸められているが、念のため）
        self.step = max(step, 1)
    }

    /// - Returns: 今回動かす文字数（右が正）。モードに入る前・変化なしは 0
    mutating func update(translationWidth: Double) -> Int {
        if !isActive {
            guard translationWidth.isFinite, abs(translationWidth) >= step else { return 0 }
            isActive = true
        }
        guard translationWidth.isFinite else { return 0 }
        let total = Int((translationWidth / step).rounded(.towardZero))
        let delta = total - movedSteps
        movedSteps = total
        return delta
    }
}

/// ⌫の左スワイプで消す「単語」の長さ（文字数）を決める
enum WordBoundary {
    /// 1 回で消す最大の文字数（暴走防止）
    static let maxCount = 50

    private enum Kind: Equatable {
        case space, newline, kanji, hiragana, katakana, word, other
    }

    private static func kind(of char: Character) -> Kind {
        if char == "\n" || char == "\r\n" || char == "\r" { return .newline }
        if char.isWhitespace { return .space }
        guard let value = char.unicodeScalars.first?.value else { return .other }
        switch value {
        case 0x3041...0x309F: return .hiragana
        case 0x30A0...0x30FF, 0x31F0...0x31FF, 0xFF66...0xFF9F: return .katakana
        case 0x3400...0x4DBF, 0x4E00...0x9FFF, 0xF900...0xFAFF, 0x3005: return .kanji
        default: break
        }
        if char.isLetter || char.isNumber { return .word }
        return .other
    }

    /// カーソル手前のテキストから、削除する文字数を返す（0 は何もしない）。
    /// 末尾の空白をまとめて消し、その手前の同じ種類の文字（漢字・ひらがな・カタカナ・英数字）の連なりを 1 単語として消す。
    /// 末尾が改行なら改行 1 つだけ。記号は 1 文字だけ
    static func deleteCount(before text: String) -> Int {
        let chars = Array(text)
        guard !chars.isEmpty else { return 0 }
        var index = chars.count - 1
        if kind(of: chars[index]) == .newline { return 1 }
        var count = 0
        while index >= 0, count < maxCount, kind(of: chars[index]) == .space {
            count += 1
            index -= 1
        }
        guard index >= 0, count < maxCount else { return count }
        let wordKind = kind(of: chars[index])
        guard wordKind != .newline else { return count }
        if wordKind == .other { return count + 1 }
        while index >= 0, count < maxCount, kind(of: chars[index]) == wordKind {
            count += 1
            index -= 1
        }
        return count
    }
}
