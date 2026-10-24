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
        }
        .padding(CGFloat(params.inset))
        .opacity(params.opacity)
        .scaleEffect(CGFloat(params.scale))
        .offset(x: CGFloat(params.offsetX), y: CGFloat(params.offsetY))
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
    let rowIndex: Int
    let key: LayoutKeyDef
    let content: KeyFaceContent
    /// Caps Lock 中の Shift キー（現行の見た目: accent の色）
    let active: Bool
}

/// 見た目を解決して `StyledKeyFace` に渡す薄いラッパ。指定が無ければ既定値（現行の見た目）で描く
struct KeyFaceHost: View {
    let input: KeyFaceInput
    let local: Set<LookStateName>
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        let scheme: LookScheme = colorScheme == .dark ? .dark : .light
        let resolved = LookResolver.resolve(
            layout: input.layout, rowIndex: input.rowIndex, key: input.key,
            context: LookContext(states: local, origin: nil, scheme: scheme)
        )
        let params = KeyRenderParams.make(
            look: resolved, key: input.key, active: input.active,
            cornerRadius: NagiKeyMetrics.current.keyCornerPt
        )
        let content = KeyFaceContent.merged(
            base: input.content, override: LookResolver.content(of: resolved),
            iconAvailable: { KeyIconView.isAvailable($0) }
        )
        StyledKeyFace(params: params, content: content)
            .equatable()
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
