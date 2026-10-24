import SwiftUI
import UIKit

// キーの見た目の描画（docs/keyboard-style-spec.md §7.2）。
// 見た目の解決は Shared/KeyLookResolver.swift、描画パラメータへの変換は Shared/KeyRenderParams.swift（どちらも純粋関数で
// XCTest の対象）。ここは SwiftUI の薄い描画だけを持つ。

/// `ResolvedColor` → SwiftUI の `Color`。トークンは現行の UIColor に対応する（§2.2）
enum LookPalette {
    static func color(_ color: ResolvedColor) -> Color {
        switch color {
        case .rgba(let r, let g, let b, let a):
            return Color(.sRGB, red: Double(r) / 255, green: Double(g) / 255, blue: Double(b) / 255, opacity: Double(a) / 255)
        case .token(let name):
            return Color(tokenColor(name))
        }
    }

    /// トークン（`@` を除く）の色。ライト / ダークとアクセント（設定 `keyboard.accent`）に追従する
    static func tokenColor(_ name: String) -> UIColor {
        switch name {
        case "label": return UIColor.label
        case "secondaryLabel": return UIColor.secondaryLabel
        case "key": return UIColor.systemBackground
        case "function": return UIColor.systemGray4
        case "accent": return NagiTheme.accentUIColor(NagiKeyMetrics.current.accent)
        case "separator": return UIColor.separator
        default: return UIColor.clear
        }
    }

    static func weight(_ name: String) -> Font.Weight {
        switch name {
        case "ultraLight": return .ultraLight
        case "thin": return .thin
        case "light": return .light
        case "medium": return .medium
        case "semibold": return .semibold
        case "bold": return .bold
        case "heavy": return .heavy
        case "black": return .black
        default: return .regular
        }
    }

    static func design(_ name: String) -> Font.Design {
        switch name {
        case "rounded": return .rounded
        case "monospaced": return .monospaced
        case "serif": return .serif
        default: return .default
        }
    }
}

/// 影（半径が 0 のときは何も付けない）
private struct KeyShadow: ViewModifier {
    let params: KeyRenderParams

    func body(content: Content) -> some View {
        if params.hasShadow {
            content.shadow(
                color: LookPalette.color(params.shadowColor).opacity(params.shadowOpacity),
                radius: CGFloat(params.shadowRadius), x: CGFloat(params.shadowX), y: CGFloat(params.shadowY)
            )
        } else {
            content
        }
    }
}

/// キーの見た目。`KeyRenderParams` と表示内容だけで決まる（同じ内容なら再描画しない。`.equatable()` で使う）。
/// 描画順: 影つきの背景色 → 枠線 → 文字 / アイコン。`inset` で縮めた領域に描き、全体に `opacity` / `scale` / `offset` を掛ける。
/// 当たり判定は元のセル全体のまま: 見た目の層は当たり判定の対象にせず、透明な層がセル全体の当たり判定を持つ
struct StyledKeyFace: View, Equatable {
    let params: KeyRenderParams
    let content: KeyFaceContent
    /// 状態が変わったときの補間の長さ（ms。0 なら即時。前後の指定の大きいほう。`LookResolver.transitionMs`）
    let animationMs: Double

    var body: some View {
        Color.clear
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle())
            .overlay(visual.allowsHitTesting(false))
    }

    private var shape: RoundedRectangle {
        RoundedRectangle(cornerRadius: CGFloat(params.cornerRadius))
    }

    private var visual: some View {
        ZStack {
            shape
                .fill(LookPalette.color(params.fill).opacity(params.fillOpacity))
                .modifier(KeyShadow(params: params))
            if params.hasBorder {
                shape.strokeBorder(LookPalette.color(params.borderColor), lineWidth: CGFloat(params.borderWidth))
            }
            label
                .padding(CGFloat(params.padding))
                // 文字・アイコンの切り替えは補間せず即座に行う（色や大きさの変化は補間する）
                .animation(nil, value: content)
        }
        .padding(CGFloat(params.inset))
        .opacity(params.opacity)
        .scaleEffect(CGFloat(params.scale))
        .offset(x: CGFloat(params.offsetX), y: CGFloat(params.offsetY))
        .animation(animationMs > 0 ? Animation.easeInOut(duration: animationMs / 1000) : nil, value: params)
    }

    @ViewBuilder
    private var label: some View {
        if let icon = content.icon, KeyIconView.isAvailable(icon) {
            KeyIconView(raw: icon, size: CGFloat(params.iconSize))
                .foregroundColor(LookPalette.color(params.color))
        } else {
            Text(content.text ?? "")
                .font(.system(
                    size: CGFloat(params.fontSize),
                    weight: LookPalette.weight(params.fontWeight),
                    design: LookPalette.design(params.fontDesign)
                ))
                .foregroundColor(LookPalette.color(params.color))
                .lineLimit(1)
                .minimumScaleFactor(0.6)
        }
    }
}

/// キー 1 つの描画に必要な入力（解決の材料と表示内容）
struct KeyFaceInput {
    let layout: KeyboardLayoutDef
    let cache: LookCache
    let rowIndex: Int
    let key: LayoutKeyDef
    let content: KeyFaceContent
    /// Caps Lock 中の Shift キー（現行の見た目: accent の色）
    let active: Bool
    /// 大域の状態（shift / capsLock / composing / spaceDrag。全キーに同時に入る）
    let global: Set<LookStateName>
    /// `spaceDrag` の起点キーの id
    let origin: String?

    /// そのキーに効く状態か（効かない状態は通知・再描画を省く）
    func uses(_ state: LookStateName) -> Bool {
        cache.relevantStates(layout: layout, key: key).contains(state)
    }

    /// `released` の保持時間（ms）
    var releasedHoldMs: Double {
        LookResolver.releasedHoldMs(layout: layout, key: key)
    }

    func resolve(local: Set<LookStateName>, scheme: LookScheme) -> ResolvedLook {
        cache.resolve(
            layout: layout, rowIndex: rowIndex, key: key,
            context: LookContext(states: global.union(local), origin: origin, scheme: scheme)
        )
    }
}

/// 直前の解決結果の記録（補間の長さ = 前後の `animationMs` の大きいほう、を求めるため）。描画には使わない参照型
final class LookTransitionTracker {
    var previous = ResolvedLook()
}

/// 見た目を解決して `StyledKeyFace` に渡す薄いラッパ。指定が無ければ既定値（現行の見た目）で描く
struct KeyFaceHost: View {
    let input: KeyFaceInput
    /// このキー自身の局所の状態（pressed / released / longPress / flicking）
    let local: Set<LookStateName>
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var tracker = LookTransitionTracker()

    var body: some View {
        let scheme: LookScheme = colorScheme == .dark ? .dark : .light
        let resolved = input.resolve(local: local, scheme: scheme)
        let params = KeyRenderParams.make(
            look: resolved, key: input.key, active: input.active,
            cornerRadius: NagiKeyMetrics.current.keyCornerPt
        )
        let content = KeyFaceContent.merged(
            base: input.content, override: LookResolver.content(of: resolved),
            iconAvailable: { KeyIconView.isAvailable($0) }
        )
        let ms = LookResolver.transitionMs(from: tracker.previous, to: resolved, reduceMotion: reduceMotion)
        StyledKeyFace(params: params, content: content, animationMs: ms)
            .equatable()
            .onChange(of: resolved) { tracker.previous = $0 }
    }
}

/// キーボード全体の背景（`background`）。指定が無ければ何も塗らない（システムの背景が見える。H-61）
struct KeyboardBackgroundView: View {
    let background: LayoutBackground?
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        if let params = BackgroundParams.make(background, scheme: colorScheme == .dark ? .dark : .light),
           let fill = params.fill {
            Rectangle().fill(LookPalette.color(fill).opacity(params.fillOpacity))
        }
    }
}
