import UIKit

/// 画面の寸法と操作の判定に使う設定の現在値。`KeyboardModel` が focusIn のたびに `ios-settings.json` を読んで
/// 更新し、描画のたびにここを参照する（値を変えてから `@Published` を更新するので、再描画は新しい値を見る）
enum NagiKeyMetrics {
    static var current = NagiIOSSettings()

    /// キー 1 行（高さ比 1）の高さ
    static var keyHeight: Double { 46 * current.heightScale }
    static var keySpacing: Double { current.keySpacingPt }

    /// キーボード全体の高さ。候補バー 44 + 余白 12 = 56 は固定で、残りをキーの高さの倍率で伸縮する
    /// （倍率 1.0 で従来の 270）
    static var totalHeight: Double { 56 + 214 * current.heightScale }
}

/// キー入力のフィードバック（触覚・クリック音）。いずれも失敗しても何も起きない
/// （フルアクセスが無いと触覚が出ない端末・OS があるため。要実機確認）
enum NagiKeyFeedback {
    private static var generator: UIImpactFeedbackGenerator?
    private static var generatorStyle = ""

    /// キーを押したときに呼ぶ（メインスレッド）
    static func keyDown() {
        let settings = NagiKeyMetrics.current
        let haptic = settings.haptic
        if haptic != "off" {
            let style: UIImpactFeedbackGenerator.FeedbackStyle
            switch haptic {
            case "medium": style = .medium
            case "heavy": style = .heavy
            default: style = .light
            }
            if generator == nil || generatorStyle != haptic {
                generator = UIImpactFeedbackGenerator(style: style)
                generatorStyle = haptic
            }
            generator?.impactOccurred()
            generator?.prepare()
        }
        if settings.soundEnabled {
            // 端末の「キーボードのクリック」がオンのときだけ鳴る。ビューが UIInputViewAudioFeedback に
            // 準拠している必要があるので、下の拡張で全 UIView を準拠させている
            UIDevice.current.playInputClick()
        }
    }
}

/// `UIDevice.playInputClick()` を有効にするための準拠（キーボード拡張のビューは UIInputView の子孫）
extension UIView: UIInputViewAudioFeedback {
    public var enableInputClicksWhenVisible: Bool { true }
}
