import AudioToolbox
import UIKit

/// 画面の寸法と操作の判定に使う設定の現在値。`KeyboardModel` が focusIn のたびに `ios-settings.json` を読んで
/// 更新し、描画のたびにここを参照する（値を変えてから `@Published` を更新するので、再描画は新しい値を見る）
enum NagiKeyMetrics {
    /// メインスレッドからのみ読み書きする（Swift の並行性検査を抑制）
    nonisolated(unsafe) static var current = NagiIOSSettings()

    /// キー 1 行（高さ比 1）の高さ
    static var keyHeight: Double { 46 * current.heightScale }
    static var keySpacing: Double { current.keySpacingPt }

    /// キーボード全体の高さ。候補バー 44 + 余白 12 = 56 は固定で、残りをキーの高さの倍率で伸縮する
    /// （倍率 1.0 で従来の 270）
    static var totalHeight: Double { 56 + 214 * current.heightScale }
}

/// キー入力のフィードバック（触覚・クリック音）。いずれも失敗しても何も起きない。
/// 触覚はフルアクセスが無いと出ない（`fullAccess`。`KeyboardViewController` が画面表示のたびに更新する）。
/// 診断ログ（DiagLog）に、発火のたびに状況を残す（先頭の数十回のみ。focusIn のたびに数え直す）
enum NagiKeyFeedback {
    nonisolated(unsafe) private static var generator: UIImpactFeedbackGenerator?
    nonisolated(unsafe) private static var generatorStyle = ""
    nonisolated(unsafe) private static var logCount = 0
    private static let logLimit = 30

    /// `hasFullAccess` の現在値
    nonisolated(unsafe) static var fullAccess = false
    /// 触覚ジェネレータを結びつけるビュー（iOS 17.5 以降の `init(style:view:)`）。キーボードのビュー
    nonisolated(unsafe) static weak var anchorView: UIView?

    private static func makeGenerator(_ style: UIImpactFeedbackGenerator.FeedbackStyle) -> UIImpactFeedbackGenerator {
        if #available(iOS 17.5, *), let view = anchorView {
            return UIImpactFeedbackGenerator(style: style, view: view)
        }
        return UIImpactFeedbackGenerator(style: style)
    }

    private static func feedbackStyle(for haptic: String) -> UIImpactFeedbackGenerator.FeedbackStyle? {
        switch haptic {
        case "light": return .light
        case "medium": return .medium
        case "heavy": return .heavy
        default: return nil
        }
    }

    /// 画面の表示時（focusIn）に呼ぶ。ジェネレータを作り直して `prepare()` し、ログの回数を数え直す
    static func prepare() {
        logCount = 0
        generator = nil
        generatorStyle = ""
        let settings = NagiKeyMetrics.current
        if settings.hapticEngine == "standard", let style = feedbackStyle(for: settings.haptic) {
            let created = makeGenerator(style)
            generator = created
            generatorStyle = settings.haptic
            created.prepare()
        }
        DiagLog.log("feedback prepare haptic=\(settings.haptic) engine=\(settings.hapticEngine) sound=\(settings.soundMode) fullAccess=\(fullAccess) generator=\(generator != nil)")
    }

    /// キーを押したときに呼ぶ（メインスレッド）
    static func keyDown() {
        let settings = NagiKeyMetrics.current
        let haptic = settings.haptic
        let shouldLog = logCount < logLimit
        if shouldLog { logCount += 1 }
        if let style = feedbackStyle(for: haptic) {
            if settings.hapticEngine == "compat" {
                if let id = HapticMapping.systemSoundID(for: haptic) {
                    AudioServicesPlaySystemSound(id)
                }
                if shouldLog {
                    DiagLog.log("haptic fire kind=\(haptic) engine=compat fullAccess=\(fullAccess) generator=none")
                }
            } else {
                // 方式を切り替えた・設定が変わった場合はここで作り直す。押下のたびには作らない
                if generator == nil || generatorStyle != haptic {
                    generator = makeGenerator(style)
                    generatorStyle = haptic
                }
                generator?.impactOccurred()
                generator?.prepare()
                if shouldLog {
                    DiagLog.log("haptic fire kind=\(haptic) engine=standard fullAccess=\(fullAccess) generator=\(generator != nil)")
                }
            }
        }
        if settings.soundEnabled {
            let mode = settings.soundMode
            if shouldLog {
                DiagLog.log("click play mode=\(mode) anchorView=\(anchorView != nil)")
            }
            if mode == "compat" {
                // システムのキーボードのクリック音（1104）を直接鳴らす。端末の「キーボードのクリック」設定には
                // 従わず、消音スイッチに従う
                AudioServicesPlaySystemSound(1104)
            } else {
                // 端末の「キーボードのクリック」がオンのときだけ鳴る。入力ビュー階層に UIInputViewAudioFeedback に
                // 準拠したビューが要る（下の拡張。`UIInputView` は `UIView` の子孫なので準拠を引き継ぐ。
                // それでも鳴らない端末では、`loadView` で `UIInputView` のサブクラスを `inputView` に据える案が次の手）
                UIDevice.current.playInputClick()
            }
        }
    }
}

/// `UIDevice.playInputClick()` を有効にするための準拠（キーボード拡張のビューは UIInputView の子孫）
extension UIView: @retroactive UIInputViewAudioFeedback {
    public var enableInputClicksWhenVisible: Bool { true }
}
