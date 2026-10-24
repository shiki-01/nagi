import Foundation

// 見た目の解決（docs/keyboard-style-spec.md §3〜§4）。UIKit / SwiftUI に依存しない純粋関数。
// TypeScript 版は web/src/lib/keyboard/look-resolve.ts。二つは `config/keyboard/test-looks/*.json` の
// 同じテストベクタで同じ結果を出すこと（変えるときは両方と、ベクタを揃える）。

/// 解決の入力となる文脈（今の状態）
struct LookContext: Equatable {
    /// 有効な状態。局所の状態（pressed など）は、そのキー自身について有効なものを渡す
    var states: Set<LookStateName> = []
    /// `spaceDrag` の起点キーの id（`target: "others"` の判定に使う）
    var origin: String?
    var scheme: LookScheme = .light
}

/// 解決結果。指定された（または状態の既定で決まった）プロパティだけを持ち、無いものは「既定のまま」（`LookDefaults`）。
/// 色はすべて外観で解決済みの `.color(.solid(...))`。`fillImage` が無いときは画像なし（"none" は取り除く）
struct ResolvedLook: Equatable, LookValues {
    var values: [String: LookValue] = [:]

    init(values: [String: LookValue] = [:]) {
        self.values = values
    }

    func color(_ name: String) -> ResolvedColor? {
        if case .color(.solid(let color))? = values[name] { return color }
        return nil
    }
}

/// 表示する文字・アイコンの上書き（§2.3）
struct LookContent: Equatable {
    var label: String?
    var icon: String?
    /// `label` だけ指定され `icon` が無いとき true（キー本来のアイコンを出さず、`label` を出す）
    var hidesKeyIcon: Bool
}

/// 範囲外・不正なプロパティを取り除く（クランプはしない。仕様 §5.5）
enum LookSanitizer {
    /// - allowsContent: `label` / `icon` を受理するか（`states` / `rules` の中だけ true）
    static func sanitize(_ look: KeyLook, allowsContent: Bool) -> KeyLook {
        var result: [String: LookValue] = [:]
        for (name, value) in look.values {
            guard let spec = KeyLookLimits.specs[name] else { continue }
            if spec.contentOnly && !allowsContent { continue }
            if KeyLookLimits.accepts(value, spec: spec) { result[name] = value }
        }
        return KeyLook(values: result)
    }
}

/// 現行の見た目（指定が何も無いときの既定値。仕様 §2.1）。描画（フェーズ 2）がここを参照する
enum LookDefaults {
    /// 押下中の背景の不透明度（組み込みの `pressed` の既定）
    static let pressedFillOpacity = 0.6
    /// 組み込みの `pressed` の指定（最下位に重ねる。§3.3）
    static let pressedLook = KeyLook(values: ["fillOpacity": .number(pressedFillOpacity)])
    /// 角丸の既定（設定 `keyboard.key_corner_pt` の既定値）
    static let cornerRadius = 6.0
    /// アイコンの大きさの既定（pt）。`fontSize` を指定したときは、アイコンも同じ大きさになる
    static let iconSize = 17.0
    static let fontWeight = "regular"
    static let fontDesign = "system"
    static let fillOpacity = 1.0
    static let fillImageFit = "fill"
    static let fillImageOpacity = 1.0
    static let borderWidth = 0.0
    static let borderColor: ResolvedColor = .token("separator")
    static let shadowColor: ResolvedColor = .rgba(r: 0, g: 0, b: 0, a: 255)
    static let shadowRadius = 0.0
    static let shadowX = 0.0
    static let shadowY = 0.0
    static let shadowOpacity = 0.3
    static let opacity = 1.0
    static let scale = 1.0
    static let offsetX = 0.0
    static let offsetY = 0.0
    static let inset = 0.0
    static let padding = 0.0
    static let animationMs = 0.0
    /// 外周の余白の既定（pt。現行の `padding(6)`）
    static let metricsPadding = 6.0

    /// 背景色の既定（役割ごと）
    static func fill(role: KeyRole) -> ResolvedColor {
        switch role {
        case .normal: return .token("key")
        case .function: return .token("function")
        case .accent: return .token("accent")
        }
    }

    /// 文字・アイコンの色の既定（accent だけ白）
    static func color(role: KeyRole) -> ResolvedColor {
        switch role {
        case .accent: return .rgba(r: 255, g: 255, b: 255, a: 255)
        default: return .token("label")
        }
    }

    /// 文字の大きさの既定（現行の `fontSize(key)`。フリックは 22、`label` などが 3 文字以上なら 16、text キーは 20、他は 17）
    static func fontSize(for key: LayoutKeyDef) -> Double {
        if key.kind == .flick {
            let count = (key.label ?? key.kana ?? key.center?.text ?? "").count
            return count > 2 ? 16 : 22
        }
        let count = (key.label ?? key.text ?? key.cycle?.first ?? "").count
        if count > 2 { return 16 }
        return key.kind == .text ? 20 : 17
    }
}

enum LookResolver {
    /// キー 1 つの見た目を解決する。重ねる順（低 → 高。後が先を上書き）:
    /// layout.look → layout.roles[role] → 行の look → キーの look →
    /// 有効な状態を優先順位（`LookStateName.precedence`）の低い順に、それぞれ
    /// 〔組み込みの既定（pressed の fillOpacity 0.6）→ 一致する rules（配列順）→ キーの states[状態]〕
    /// 指定が無いプロパティは結果に含まれない（既定のまま）。`spacer` は何も持たない
    static func resolve(layout: KeyboardLayoutDef, rowIndex: Int, key: LayoutKeyDef, context: LookContext) -> ResolvedLook {
        if key.kind == .spacer { return ResolvedLook() }
        var result: [String: LookValue] = [:]

        func apply(_ look: KeyLook?, allowsContent: Bool) {
            merge(look, allowsContent: allowsContent, scheme: context.scheme, into: &result)
        }

        apply(layout.look, allowsContent: false)
        apply(layout.roles?.byRole[key.role], allowsContent: false)
        if rowIndex >= 0 && rowIndex < layout.rows.count {
            apply(layout.rows[rowIndex].look, allowsContent: false)
        }
        apply(key.look, allowsContent: false)
        for state in LookStateName.precedence where context.states.contains(state) {
            if state == .pressed { apply(LookDefaults.pressedLook, allowsContent: false) }
            for rule in layout.rules where rule.when == state {
                if rule.target.matches(key: key, when: state, origin: context.origin) {
                    apply(rule.look, allowsContent: true)
                }
            }
            apply(key.states?.byState[state]?.look, allowsContent: true)
        }
        // "none" は下位で指定された画像を外す指定。結果には画像なしとして残さない
        if let image = result["fillImage"], image == LookValue.string("none") {
            result.removeValue(forKey: "fillImage")
        }
        return ResolvedLook(values: result)
    }

    /// 見た目 1 つを範囲検査して `result` に重ねる。色は外観で解決し、片方の外観だけ指定した色は、
    /// 書いていない側では未指定扱い（下位の値を残す）
    private static func merge(
        _ look: KeyLook?, allowsContent: Bool, scheme: LookScheme, into result: inout [String: LookValue]
    ) {
        guard let look = look else { return }
        let clean = LookSanitizer.sanitize(look, allowsContent: allowsContent)
        for (name, value) in clean.values {
            if case .color(let color) = value {
                if let solid = color.resolved(for: scheme) { result[name] = .color(.solid(solid)) }
            } else {
                result[name] = value
            }
        }
    }

    /// キーボード全体の背景（`background`）の解決。`fill` / `fillOpacity` / `fillImage` / `fillImageFit` /
    /// `fillImageOpacity` だけを持つ。`fillImage: "none"` は画像なし
    static func resolveBackground(_ background: LayoutBackground?, scheme: LookScheme) -> ResolvedLook {
        var result: [String: LookValue] = [:]
        merge(background?.look, allowsContent: false, scheme: scheme, into: &result)
        if let image = result["fillImage"], image == LookValue.string("none") {
            result.removeValue(forKey: "fillImage")
        }
        return ResolvedLook(values: result)
    }

    /// `released` の保持時間（ms。0...2000、既定 200）。一致する rules（配列順）→ キーの `states.released` の順で、
    /// 範囲内の `holdMs` を指定した最後のものが勝つ
    static func releasedHoldMs(layout: KeyboardLayoutDef, key: LayoutKeyDef) -> Double {
        var hold = KeyLookLimits.defaultHoldMs
        for rule in layout.rules where rule.when == .released {
            if rule.target.matches(key: key, when: .released, origin: nil),
               let ms = rule.holdMs, KeyLookLimits.holdMsRange.contains(ms) {
                hold = ms
            }
        }
        if let ms = key.states?.byState[.released]?.holdMs, KeyLookLimits.holdMsRange.contains(ms) {
            hold = ms
        }
        return hold
    }

    /// 状態が変わったときの補間の長さ（ms）。前後の解決結果の `animationMs` の大きいほう（§3.4）。
    /// 「視差効果を減らす」のときは 0
    static func transitionMs(from: ResolvedLook, to: ResolvedLook, reduceMotion: Bool) -> Double {
        if reduceMotion { return 0 }
        return max(from.number("animationMs") ?? 0, to.number("animationMs") ?? 0)
    }

    /// 表示する文字・アイコンの上書き（§2.3）。`label` だけのときは `hidesKeyIcon`
    static func content(of look: ResolvedLook) -> LookContent {
        let label = look.string("label")
        let icon = look.string("icon")
        return LookContent(label: label, icon: icon, hidesKeyIcon: label != nil && icon == nil)
    }
}
