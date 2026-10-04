import Foundation

/// 曖昧入力の代替（近傍のキー・フリックゾーン）。コアへは `InputAlternative` として送る
struct AltCandidate: Equatable {
    /// 代替の文字
    let text: String
    /// タッチ位置からそのキー（ゾーン）中心までの距離をキー幅で正規化した値
    let distance: Double
}

/// レイアウト（描画層）からコントローラへ渡す入力イベント
enum KeyboardInput: Equatable {
    /// ローマ字入力の 1 文字
    case char(text: String, alternatives: [AltCandidate])
    /// かなの直接挿入（フリック等）
    case kana(reading: String, alternatives: [AltCandidate])
    case special(SpecialKey)
    case switchLayout(String)
    /// コアの Action 名を直接実行
    case engineAction(String)
    /// 候補バーの候補を選択（0 始まり）
    case selectCandidate(Int)
}

/// XKB keysym（コアへ送るキー）
enum Keysym {
    static let space: UInt32 = 0x20
    static let backspace: UInt32 = 0xff08
    static let enter: UInt32 = 0xff0d
    static let left: UInt32 = 0xff51
    static let right: UInt32 = 0xff53

    /// 文字に対する keysym。ASCII は文字コードそのもの、それ以外は 0（コアは `text` を優先する）
    static func forText(_ text: String) -> UInt32 {
        guard let scalar = text.unicodeScalars.first, text.unicodeScalars.count == 1 else { return 0 }
        return scalar.value < 0x7f ? scalar.value : 0
    }
}
