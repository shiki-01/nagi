import Foundation

// 解決済みの見た目（`ResolvedLook`）を、描画に渡す具体的な値にする純粋な変換（docs/keyboard-style-spec.md §7.2）。
// SwiftUI に依存しない（XCTest で直接検証する）。描画そのものは Keyboard/KeyLookRenderer.swift。
// 指定が何も無いプロパティは `LookDefaults`（現行の見た目）で埋める。

/// キーの表示内容（文字か、SF Symbols / Iconify のアイコン）
struct KeyFaceContent: Equatable {
    var text: String?
    var icon: String?

    /// 状態の `label` / `icon` の上書きを重ねる（§2.3）。
    /// - 取得できる `icon` があればそれ。なければ `label` があれば `label`（キー本来のアイコンは出さない）
    /// - どちらも使えなければ本来の表示のまま
    /// - iconAvailable: アイコンを描画できるか（取り込めていない Iconify アイコンは false）
    static func merged(base: KeyFaceContent, override: LookContent, iconAvailable: (String) -> Bool) -> KeyFaceContent {
        if let icon = override.icon, iconAvailable(icon) {
            return KeyFaceContent(text: override.label ?? base.text, icon: icon)
        }
        if let label = override.label {
            return KeyFaceContent(text: label, icon: nil)
        }
        return base
    }
}

/// キー 1 つの描画パラメータ。すべて具体的な値（nil なし）。色は外観で解決済み
struct KeyRenderParams: Equatable {
    var fontSize: Double
    var iconSize: Double
    var fontWeight: String
    var fontDesign: String
    var color: ResolvedColor
    var fill: ResolvedColor
    var fillOpacity: Double
    /// 背景画像の id（`assets` のキー）。無ければ nil
    var fillImage: String?
    var fillImageFit: String
    var fillImageOpacity: Double
    var cornerRadius: Double
    var borderWidth: Double
    var borderColor: ResolvedColor
    var shadowColor: ResolvedColor
    var shadowRadius: Double
    var shadowX: Double
    var shadowY: Double
    var shadowOpacity: Double
    var opacity: Double
    var scale: Double
    var offsetX: Double
    var offsetY: Double
    var inset: Double
    var padding: Double
    var animationMs: Double

    /// 影を描くか（半径が 0 のときは描かない）
    var hasShadow: Bool { shadowRadius > 0 }
    var hasBorder: Bool { borderWidth > 0 }

    /// - active: Caps Lock 中の Shift キー（役割が accent のときと同じ既定の色になる。現行の見た目）
    /// - cornerRadius: 角丸の既定（設定 `keyboard.key_corner_pt`）
    static func make(look: ResolvedLook, key: LayoutKeyDef, active: Bool, cornerRadius: Double) -> KeyRenderParams {
        let role: KeyRole = active ? .accent : key.role
        let fontSize = look.number("fontSize")
        return KeyRenderParams(
            fontSize: fontSize ?? LookDefaults.fontSize(for: key),
            iconSize: fontSize ?? LookDefaults.iconSize,
            fontWeight: look.string("fontWeight") ?? LookDefaults.fontWeight,
            fontDesign: look.string("fontDesign") ?? LookDefaults.fontDesign,
            color: look.color("color") ?? LookDefaults.color(role: role),
            fill: look.color("fill") ?? LookDefaults.fill(role: role),
            fillOpacity: look.number("fillOpacity") ?? LookDefaults.fillOpacity,
            fillImage: look.string("fillImage"),
            fillImageFit: look.string("fillImageFit") ?? LookDefaults.fillImageFit,
            fillImageOpacity: look.number("fillImageOpacity") ?? LookDefaults.fillImageOpacity,
            cornerRadius: look.number("cornerRadius") ?? cornerRadius,
            borderWidth: look.number("borderWidth") ?? LookDefaults.borderWidth,
            borderColor: look.color("borderColor") ?? LookDefaults.borderColor,
            shadowColor: look.color("shadowColor") ?? LookDefaults.shadowColor,
            shadowRadius: look.number("shadowRadius") ?? LookDefaults.shadowRadius,
            shadowX: look.number("shadowX") ?? LookDefaults.shadowX,
            shadowY: look.number("shadowY") ?? LookDefaults.shadowY,
            shadowOpacity: look.number("shadowOpacity") ?? LookDefaults.shadowOpacity,
            opacity: look.number("opacity") ?? LookDefaults.opacity,
            scale: look.number("scale") ?? LookDefaults.scale,
            offsetX: look.number("offsetX") ?? LookDefaults.offsetX,
            offsetY: look.number("offsetY") ?? LookDefaults.offsetY,
            inset: look.number("inset") ?? LookDefaults.inset,
            padding: look.number("padding") ?? LookDefaults.padding,
            animationMs: look.number("animationMs") ?? LookDefaults.animationMs
        )
    }
}

/// キーボード全体の背景の描画パラメータ（§6.4）。色も画像も無ければ `make` は nil（現行どおり塗らない）
struct BackgroundParams: Equatable {
    var fill: ResolvedColor?
    var fillOpacity: Double
    var fillImage: String?
    var fillImageFit: String
    var fillImageOpacity: Double

    static func make(_ background: LayoutBackground?, scheme: LookScheme) -> BackgroundParams? {
        guard background != nil else { return nil }
        let look = LookResolver.resolveBackground(background, scheme: scheme)
        let fill = look.color("fill")
        let image = look.string("fillImage")
        guard fill != nil || image != nil else { return nil }
        return BackgroundParams(
            fill: fill,
            fillOpacity: look.number("fillOpacity") ?? LookDefaults.fillOpacity,
            fillImage: image,
            fillImageFit: look.string("fillImageFit") ?? LookDefaults.fillImageFit,
            fillImageOpacity: look.number("fillImageOpacity") ?? LookDefaults.fillImageOpacity
        )
    }
}

/// 余白の解決結果（`metrics` があれば設定値より優先。§3.5）
struct ResolvedMetrics: Equatable {
    var keySpacing: Double
    var rowSpacing: Double
    var padding: Double

    /// - settingSpacing: 設定 `keyboard.key_spacing_pt`（キー間・行間の既定）
    static func make(_ metrics: LayoutMetrics?, settingSpacing: Double) -> ResolvedMetrics {
        ResolvedMetrics(
            keySpacing: metrics?.keySpacing ?? settingSpacing,
            rowSpacing: metrics?.rowSpacing ?? settingSpacing,
            padding: metrics?.padding ?? LookDefaults.metricsPadding
        )
    }
}
