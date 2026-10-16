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

/// 互換モード（システムサウンド）の触覚・クリック音の再生。
/// `AudioServicesPlaySystemSound` をメインスレッドで呼ぶと、速く打ったときに鳴り終わるまで次の入力が遅れたので、
/// 専用の直列キューで鳴らす。種類（触覚・音）ごとに再生中なら次の要求は捨てる（重ねない・待たせない）。
/// 再生中のフラグはキューの上だけで読み書きする
private enum CompatFeedbackPlayer {
    enum Kind {
        case haptic, sound
    }

    private static let queue = DispatchQueue(label: "com.shiki.nagi.feedback", qos: .userInteractive)
    /// キューの上だけで読み書きする（Swift の並行性検査を抑制）
    nonisolated(unsafe) private static var hapticPlaying = false
    nonisolated(unsafe) private static var soundPlaying = false

    /// `id` を鳴らす（メインスレッドから呼ぶ。すぐ戻る）。
    /// `onDropped` は再生中で捨てたときにメインスレッドで呼ぶ（ログ用。ログはメインスレッドで出す）
    static func play(_ id: SystemSoundID, kind: Kind, onDropped: (() -> Void)? = nil) {
        queue.async {
            if isPlaying(kind) {
                if let onDropped = onDropped { DispatchQueue.main.async(execute: onDropped) }
                return
            }
            setPlaying(kind, true)
            AudioServicesPlaySystemSoundWithCompletion(id) {
                // 完了ハンドラは任意のスレッドで呼ばれるので、フラグはキューへ戻してから戻す
                queue.async { setPlaying(kind, false) }
            }
        }
    }

    private static func isPlaying(_ kind: Kind) -> Bool {
        switch kind {
        case .haptic: return hapticPlaying
        case .sound: return soundPlaying
        }
    }

    private static func setPlaying(_ kind: Kind, _ value: Bool) {
        switch kind {
        case .haptic: hapticPlaying = value
        case .sound: soundPlaying = value
        }
    }
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
                // 鳴らすのは専用のキュー。ここでは要求するだけで、再生中で捨てたかは後からメインスレッドで分かる
                if let id = HapticMapping.systemSoundID(for: haptic) {
                    CompatFeedbackPlayer.play(id, kind: .haptic, onDropped: shouldLog ? {
                        DiagLog.log("haptic fire kind=\(haptic) engine=compat dropped=true")
                    } : nil)
                }
                if shouldLog {
                    DiagLog.log("haptic fire kind=\(haptic) engine=compat fullAccess=\(fullAccess) generator=none requested=true")
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
            if shouldLog {
                DiagLog.log("click play mode=\(settings.soundMode)")
            }
            // システムのキーボードのクリック音（1104）を直接鳴らす。端末の「キーボードのクリック」設定には
            // 従わず、消音スイッチに従う。鳴らすのは専用のキュー（再生中なら捨てる）。
            // 端末の設定に従う `UIDevice.playInputClick()` は実機で鳴らなかったので使わない
            CompatFeedbackPlayer.play(1104, kind: .sound, onDropped: shouldLog ? {
                DiagLog.log("click play mode=compat dropped=true")
            } : nil)
        }
    }
}
