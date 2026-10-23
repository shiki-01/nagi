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

/// トグルキー（ガラケー式）の押下が「直前の文字の置き換え」になるかと、次の文字の位置を決める
enum ToggleCycle {
    /// 同じキーの続けての押下で、直前の文字を次の候補に置き換えるか。
    /// cycle が 2 要素以上で、経過時間（ミリ秒）が 0 以上かつタイムアウト未満のとき true
    static func isContinuation(cycleCount: Int, elapsedMs: Double, timeoutMs: Int) -> Bool {
        cycleCount > 1 && elapsedMs.isFinite && elapsedMs >= 0 && elapsedMs < Double(timeoutMs)
    }

    /// 次の文字の位置。直前の位置が無ければ（新しい押下）先頭、あれば 1 つ進めて末尾の次は先頭に戻る
    static func nextIndex(previous: Int?, cycleCount: Int) -> Int {
        guard cycleCount > 0, let previous = previous else { return 0 }
        return (previous + 1) % cycleCount
    }

    /// `kana` 指定の無い flick キーをトグル入力で回すときの cycle。
    /// center → left → up → right → down の順に並べる（未指定の方向は除く。空文字も入れない）
    static func flickCycle(center: FlickChar?, left: FlickChar?, up: FlickChar?, right: FlickChar?, down: FlickChar?) -> [String] {
        [center, left, up, right, down].compactMap { $0?.text }.filter { !$0.isEmpty }
    }
}

/// フリックの取り消し判定
enum FlickCancel {
    /// 押した位置から、キーの幅（横）・高さ（縦）のこの倍率を超えて動かしたら取り消し
    /// （ポップアップの方向のセルの外側を越えて大きく動かした）
    static let factor = 2.0

    /// 押した位置からの移動量（dx, dy）が取り消しの範囲か。値が有限でない・キーの大きさが 0 以下なら false
    static func isCancelled(dx: Double, dy: Double, keyWidth: Double, keyHeight: Double) -> Bool {
        guard dx.isFinite, dy.isFinite, keyWidth > 0, keyHeight > 0 else { return false }
        return abs(dx) > keyWidth * factor || abs(dy) > keyHeight * factor
    }
}

/// 長押しポップアップの取り消し判定
enum LongPressCancel {
    /// キーの下端より下へ、キーの高さのこの倍率以上離れたら取り消し
    static let heightFactor = 0.3

    /// 指の y（キー内の座標。上端が 0、下端が keyHeight）が取り消しの位置か
    static func isCancelled(y: Double, keyHeight: Double) -> Bool {
        guard y.isFinite, keyHeight > 0 else { return false }
        return y > keyHeight + keyHeight * heightFactor
    }

    /// 指の位置（キー内の座標）が取り消しの位置か。下は `isCancelled(y:keyHeight:)` と同じ。
    /// 横は、ポップアップ（左端 `popupLeft`・幅 `popupWidth`）の外側へ項目 1 つ分（`itemWidth`）を超えて離れたら取り消し
    /// （最下段のキーでも横へずらせば取り消せる）
    static func isCancelled(x: Double, y: Double, keyHeight: Double, popupLeft: Double, popupWidth: Double, itemWidth: Double) -> Bool {
        if isCancelled(y: y, keyHeight: keyHeight) { return true }
        guard x.isFinite, popupLeft.isFinite, popupWidth.isFinite, itemWidth.isFinite else { return false }
        return x < popupLeft - itemWidth || x > popupLeft + popupWidth + itemWidth
    }
}

/// 触覚の互換モード（システムサウンドで振動を鳴らす）の対応表
enum HapticMapping {
    /// 触覚の強さ（`keyboard.haptic`）に対応するシステムサウンド ID。`off` や未知の値は nil。
    /// 1519 = Peek（弱）、1520 = Pop（中）、1521 = 3 連（強）
    static func systemSoundID(for haptic: String) -> UInt32? {
        switch haptic {
        case "light": return 1519
        case "medium": return 1520
        case "heavy": return 1521
        default: return nil
        }
    }
}

/// 数字・記号のレイアウトの文字を、コアの入力中の文字列（Literal）に通すかの判定（設定 `input.compose_numbers`）。
/// 英字のレイアウトは対象外（従来どおり直接入力）
enum NumberCompose {
    static func isNumberOrSymbol(_ mode: LayoutMode) -> Bool {
        mode == .number || mode == .symbol
    }

    /// - setting: 設定 `input.compose_numbers`
    /// - isSecure: パスワード欄など（常に直接入力。学習・候補を避ける）
    /// - japaneseContext: かなから切り替えて来た流れか
    /// - hasPreedit: 入力中の文字列が既にあるか
    static func shouldCompose(mode: LayoutMode, setting: Bool, isSecure: Bool, japaneseContext: Bool, hasPreedit: Bool) -> Bool {
        guard setting, !isSecure, isNumberOrSymbol(mode) else { return false }
        return japaneseContext || hasPreedit
    }

    /// レイアウトを切り替えたあとの「日本語入力の流れ」。かなは true、英字・絵文字・カスタムは false、
    /// 数字・記号は、かなから来たか、数字・記号の間で切り替えた場合に前の流れを引き継ぐ
    static func nextContext(previousMode: LayoutMode, previousContext: Bool, nextMode: LayoutMode) -> Bool {
        switch nextMode {
        case .kana: return true
        case .number, .symbol:
            return previousMode == .kana || (previousContext && isNumberOrSymbol(previousMode))
        case .alpha, .emoji, .custom: return false
        }
    }
}

/// 英字のレイアウトの文字を、コアの入力中の文字列（Literal）に通すかの判定（設定 `input.keep_preedit_on_english`）。
/// 入力中の文字列があるときだけ（無いときは従来どおり直接入力）
enum EnglishCompose {
    static func shouldCompose(mode: LayoutMode, keepSetting: Bool, isSecure: Bool, hasPreedit: Bool) -> Bool {
        keepSetting && !isSecure && hasPreedit && mode == .alpha
    }
}

/// レイアウトを切り替えるとき、入力中の文字列を確定せずに保つかの判定
/// （設定 `input.keep_preedit_on_symbols` / `input.keep_preedit_on_english`）。
/// 保たないときは従来どおり確定してから切り替える
enum LayoutSwitchKeep {
    /// かな・数字・記号の間（かな同士は対象外）は `keepOnSymbols`、英字とそれらの間は `keepOnEnglish`。
    /// 数字・記号が関わる切替は、切替先で入力が続くよう `composeNumbers`（`input.compose_numbers`）がオンのときだけ
    static func shouldKeep(
        from: LayoutMode, to: LayoutMode, hasPreedit: Bool, isSecure: Bool,
        keepOnSymbols: Bool, keepOnEnglish: Bool, composeNumbers: Bool
    ) -> Bool {
        guard hasPreedit, !isSecure else { return false }
        func japanese(_ mode: LayoutMode) -> Bool { mode == .kana || NumberCompose.isNumberOrSymbol(mode) }
        let numeric = NumberCompose.isNumberOrSymbol(from) || NumberCompose.isNumberOrSymbol(to)
        if numeric && !composeNumbers { return false }
        if japanese(from) && japanese(to) { return keepOnSymbols && !(from == .kana && to == .kana) }
        if (from == .alpha && japanese(to)) || (to == .alpha && japanese(from)) { return keepOnEnglish }
        return false
    }
}
