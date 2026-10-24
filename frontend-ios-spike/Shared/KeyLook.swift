import Foundation

// レイアウト JSON の見た目の指定の型（docs/keyboard-style-spec.md §2〜§5）。
// UIKit / SwiftUI に依存しない純粋な型だけを置く（XCTest で直接検証する）。
// 解決（重ね合わせ）は Shared/KeyLookResolver.swift。web 側の対応は web/src/lib/keyboard/types.ts・look-resolve.ts。
//
// 読み込みは寛容: 見た目の値の型違い・未知のプロパティ・不正な色は、そのプロパティだけを無視する
// （レイアウト全体は無効にしない。仕様 §1.4）。範囲の検査は `LookSanitizer`（KeyLookResolver.swift）が行う。

/// 動的なキー名で JSON を読むための CodingKey
struct LookKey: CodingKey {
    var stringValue: String
    var intValue: Int? { return nil }
    init?(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { return nil }
    init(_ name: String) { self.stringValue = name }
}

/// 見た目の値として不正（読み込まず、その項目を無視する）
enum LookDecodeError: Error {
    case invalid
}

/// 配列の要素を読み飛ばす（`UnkeyedDecodingContainer` は decode に失敗すると進まないので、これで進める）
struct LookSkip: Decodable {}

/// 外観（ライト / ダーク）。SwiftUI の `colorScheme` に対応する
enum LookScheme: String {
    case light, dark
}

/// 状態の名前（§4.1）
enum LookStateName: String, CaseIterable, Decodable {
    case pressed, released, longPress, flicking, spaceDrag, shift, capsLock, composing

    /// 重ねる順（低 → 高。§4.3）。後のものが先のものを上書きする
    static let precedence: [LookStateName] = [
        .composing, .capsLock, .shift, .released, .pressed, .spaceDrag, .flicking, .longPress,
    ]
}

// MARK: - 色

/// 解決済みの 1 色。`rgba` は 0...255、`token` は `@` を除いたトークン名（label, key, accent …）
enum ResolvedColor: Equatable {
    case rgba(r: Int, g: Int, b: Int, a: Int)
    case token(String)
}

/// 色の指定（§2.2）。1 色か、ライト / ダークで別の色
enum LookColor: Equatable, Decodable {
    case solid(ResolvedColor)
    case pair(light: ResolvedColor?, dark: ResolvedColor?)

    /// 使えるトークン（`@` を除く）
    static let tokens: Set<String> = ["label", "secondaryLabel", "key", "function", "accent", "separator", "clear"]

    /// `#RGB` / `#RRGGBB` / `#RRGGBBAA` / `@token`。不正なら nil
    static func parseSolid(_ text: String) -> ResolvedColor? {
        if text.hasPrefix("@") {
            let name = String(text.dropFirst())
            return tokens.contains(name) ? .token(name) : nil
        }
        guard text.hasPrefix("#") else { return nil }
        var digits: [Int] = []
        for scalar in text.dropFirst().unicodeScalars {
            guard let digit = Int(String(scalar), radix: 16) else { return nil }
            digits.append(digit)
        }
        switch digits.count {
        case 3:
            return .rgba(r: digits[0] * 17, g: digits[1] * 17, b: digits[2] * 17, a: 255)
        case 6:
            return .rgba(
                r: digits[0] * 16 + digits[1], g: digits[2] * 16 + digits[3], b: digits[4] * 16 + digits[5], a: 255
            )
        case 8:
            return .rgba(
                r: digits[0] * 16 + digits[1], g: digits[2] * 16 + digits[3], b: digits[4] * 16 + digits[5],
                a: digits[6] * 16 + digits[7]
            )
        default:
            return nil
        }
    }

    init(from decoder: Decoder) throws {
        if let single = try? decoder.singleValueContainer(), let text = try? single.decode(String.self) {
            guard let color = LookColor.parseSolid(text) else { throw LookDecodeError.invalid }
            self = .solid(color)
            return
        }
        // `{ "light": ..., "dark": ... }`。他のキー・不正な色・空はこのプロパティ全体を無効にする
        let container = try decoder.container(keyedBy: LookKey.self)
        var light: ResolvedColor?
        var dark: ResolvedColor?
        for key in container.allKeys {
            let text = try container.decode(String.self, forKey: key)
            guard let color = LookColor.parseSolid(text) else { throw LookDecodeError.invalid }
            switch key.stringValue {
            case "light": light = color
            case "dark": dark = color
            default: throw LookDecodeError.invalid
            }
        }
        guard light != nil || dark != nil else { throw LookDecodeError.invalid }
        self = .pair(light: light, dark: dark)
    }

    /// 外観に応じた 1 色。片方だけ指定で、書いていない側の外観では nil（未指定扱い）
    func resolved(for scheme: LookScheme) -> ResolvedColor? {
        switch self {
        case .solid(let color):
            return color
        case .pair(let light, let dark):
            return scheme == .light ? light : dark
        }
    }
}

// MARK: - プロパティの表と範囲

enum LookPropKind {
    case number(ClosedRange<Double>)
    case choice([String])
    case color
    /// 画像 id（`assets` のキー）または "none"
    case imageRef
    /// 表示文字（1...labelMax 文字）。states / rules の中だけ
    case label
    /// アイコン（icon の書式）。states / rules の中だけ
    case icon
}

struct LookPropSpec {
    let kind: LookPropKind
    /// `label` / `icon`（states / rules の中だけで有効）か
    let contentOnly: Bool

    init(_ kind: LookPropKind, contentOnly: Bool = false) {
        self.kind = kind
        self.contentOnly = contentOnly
    }
}

/// 範囲などの定数。web の `LOOK_LIMITS` / `LOOK_PROPS`（types.ts）と `config/keyboard/layout.schema.json` と同じ値にする
enum KeyLookLimits {
    static let fontWeights = ["ultraLight", "thin", "light", "regular", "medium", "semibold", "bold", "heavy", "black"]
    static let fontDesigns = ["system", "rounded", "monospaced", "serif"]
    static let fillImageFits = ["fill", "fit", "tile"]

    static let labelMax = 16
    static let iconMax = 64
    static let rulesMax = 32
    static let targetKeysMax = 24
    static let assetsMax = 4
    static let assetBytesMax = 256 * 1024
    static let assetsTotalMax = 768 * 1024
    static let assetIdPattern = "^[a-z0-9][a-z0-9_-]{0,31}\\z"

    /// `released` の保持時間（ms）
    static let holdMsRange = 0.0...2000.0
    static let defaultHoldMs = 200.0
    /// `metrics`
    static let spacingRange = 0.0...8.0
    static let metricsPaddingRange = 0.0...16.0

    private static func number(_ low: Double, _ high: Double) -> LookPropSpec {
        LookPropSpec(.number(low...high))
    }

    /// `look` のプロパティ名 → 型・範囲
    static let specs: [String: LookPropSpec] = [
        "fontSize": number(8, 40),
        "fontWeight": LookPropSpec(.choice(fontWeights)),
        "fontDesign": LookPropSpec(.choice(fontDesigns)),
        "color": LookPropSpec(.color),
        "fill": LookPropSpec(.color),
        "fillOpacity": number(0, 1),
        "fillImage": LookPropSpec(.imageRef),
        "fillImageFit": LookPropSpec(.choice(fillImageFits)),
        "fillImageOpacity": number(0, 1),
        "cornerRadius": number(0, 24),
        "borderWidth": number(0, 4),
        "borderColor": LookPropSpec(.color),
        "shadowColor": LookPropSpec(.color),
        "shadowRadius": number(0, 16),
        "shadowX": number(-8, 8),
        "shadowY": number(-8, 8),
        "shadowOpacity": number(0, 1),
        "opacity": number(0, 1),
        "scale": number(0.5, 1.5),
        "offsetX": number(-20, 20),
        "offsetY": number(-20, 20),
        "inset": number(0, 12),
        "padding": number(0, 16),
        "animationMs": number(0, 1000),
        "label": LookPropSpec(.label, contentOnly: true),
        "icon": LookPropSpec(.icon, contentOnly: true),
    ]

    /// 値がこのプロパティの型・範囲に合うか（クランプはしない。範囲外は合わない）
    static func accepts(_ value: LookValue, spec: LookPropSpec) -> Bool {
        switch (spec.kind, value) {
        case (.number(let range), .number(let n)):
            return n.isFinite && range.contains(n)
        case (.choice(let options), .string(let s)):
            return options.contains(s)
        case (.color, .color):
            return true
        case (.imageRef, .string(let s)):
            return s == "none" || s.range(of: assetIdPattern, options: .regularExpression) != nil
        case (.label, .string(let s)):
            let n = s.unicodeScalars.count
            return n >= 1 && n <= labelMax
        case (.icon, .string(let s)):
            let n = s.unicodeScalars.count
            return n >= 1 && n <= iconMax && KeyboardIconRef.formatError(s) == nil
        default:
            return false
        }
    }
}

// MARK: - look

/// `look` の 1 プロパティの値
enum LookValue: Equatable {
    case number(Double)
    case string(String)
    case color(LookColor)
}

/// プロパティ名 → 値の辞書を持つ型（`KeyLook` と解決結果）
protocol LookValues {
    var values: [String: LookValue] { get }
}

extension LookValues {
    func number(_ name: String) -> Double? {
        if case .number(let n)? = values[name] { return n }
        return nil
    }

    func string(_ name: String) -> String? {
        if case .string(let s)? = values[name] { return s }
        return nil
    }
}

/// 見た目の指定（平らなオブジェクト。§2.1）。指定したプロパティだけを辞書に持つ（無いものは下位の値を継承）。
/// 辞書で持つのは、部分上書き・範囲の表・TypeScript 版との対応を単純にするため。
/// 読み込み時は型が合うものだけを残す（型違い・null・未知の名前は捨てる）。範囲は `LookSanitizer` が見る
struct KeyLook: Equatable, Decodable, LookValues {
    var values: [String: LookValue] = [:]

    init(values: [String: LookValue] = [:]) {
        self.values = values
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: LookKey.self)
        var result: [String: LookValue] = [:]
        for (name, spec) in KeyLookLimits.specs {
            let key = LookKey(name)
            switch spec.kind {
            case .number:
                if let value = try? container.decode(Double.self, forKey: key) { result[name] = .number(value) }
            case .color:
                if let value = try? container.decode(LookColor.self, forKey: key) { result[name] = .color(value) }
            default:
                if let value = try? container.decode(String.self, forKey: key) { result[name] = .string(value) }
            }
        }
        values = result
    }

    var isEmpty: Bool { values.isEmpty }
}

/// 状態ごとの見た目（`states.<状態>`）。`holdMs` は `released` のときだけ使う
struct StateLook: Equatable, Decodable {
    var look: KeyLook
    var holdMs: Double?

    init(look: KeyLook = KeyLook(), holdMs: Double? = nil) {
        self.look = look
        self.holdMs = holdMs
    }

    init(from decoder: Decoder) throws {
        look = try KeyLook(from: decoder)
        let container = try decoder.container(keyedBy: LookKey.self)
        holdMs = try? container.decode(Double.self, forKey: LookKey("holdMs"))
    }
}

/// キーの `states`。未知の状態名・object でない値は捨てる
struct LookStates: Equatable, Decodable {
    var byState: [LookStateName: StateLook] = [:]

    init(byState: [LookStateName: StateLook] = [:]) {
        self.byState = byState
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: LookKey.self)
        for key in container.allKeys {
            guard let name = LookStateName(rawValue: key.stringValue) else { continue }
            if let state = try? container.decode(StateLook.self, forKey: key) { byState[name] = state }
        }
    }
}

/// レイアウトの `roles`（役割ごとの見た目）。未知の役割名・object でない値は捨てる
struct LookRoles: Equatable, Decodable {
    var byRole: [KeyRole: KeyLook] = [:]

    init(byRole: [KeyRole: KeyLook] = [:]) {
        self.byRole = byRole
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: LookKey.self)
        for key in container.allKeys {
            guard let role = KeyRole(rawValue: key.stringValue) else { continue }
            if let look = try? container.decode(KeyLook.self, forKey: key) { byRole[role] = look }
        }
    }
}

// MARK: - rules

/// `rules[].target`（§4.2）
enum LookTarget: Equatable, Decodable {
    case all
    /// 起点以外の全キー（`when: spaceDrag` のときだけ有効。他では `all` と同じ）
    case others
    case keys([String])
    case roles([KeyRole])
    case actions([String])

    init(from decoder: Decoder) throws {
        if let single = try? decoder.singleValueContainer(), let text = try? single.decode(String.self) {
            switch text {
            case "all": self = .all
            case "others": self = .others
            default: throw LookDecodeError.invalid
            }
            return
        }
        let container = try decoder.container(keyedBy: LookKey.self)
        guard container.allKeys.count == 1, let key = container.allKeys.first else { throw LookDecodeError.invalid }
        switch key.stringValue {
        case "keys":
            self = .keys(try container.decode([String].self, forKey: key))
        case "roles":
            let names = try container.decode([String].self, forKey: key)
            self = .roles(names.compactMap { KeyRole(rawValue: $0) })
        case "actions":
            self = .actions(try container.decode([String].self, forKey: key))
        default:
            throw LookDecodeError.invalid
        }
    }

    /// この target がキーに当てはまるか。`spacer` は含めない（描かないため）。
    /// `origin` は `spaceDrag` の起点キーの id
    func matches(key: LayoutKeyDef, when: LookStateName, origin: String?) -> Bool {
        if key.kind == .spacer { return false }
        switch self {
        case .all:
            return true
        case .others:
            guard when == .spaceDrag, let origin = origin else { return true }
            return key.id != origin
        case .keys(let ids):
            return ids.contains(key.id)
        case .roles(let roles):
            return roles.contains(key.role)
        case .actions(let names):
            guard let action = key.action else { return false }
            return names.contains(action)
        }
    }
}

/// `rules[]` の 1 要素。`when` が未知・target の形が不正なものは読み込まない（無視）
struct LookRule: Equatable, Decodable {
    var when: LookStateName
    var target: LookTarget
    var holdMs: Double?
    var look: KeyLook

    init(when: LookStateName, target: LookTarget = .all, holdMs: Double? = nil, look: KeyLook) {
        self.when = when
        self.target = target
        self.holdMs = holdMs
        self.look = look
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: LookKey.self)
        when = try container.decode(LookStateName.self, forKey: LookKey("when"))
        if container.contains(LookKey("target")) {
            target = try container.decode(LookTarget.self, forKey: LookKey("target"))
        } else {
            target = .all
        }
        holdMs = try? container.decode(Double.self, forKey: LookKey("holdMs"))
        look = (try? container.decode(KeyLook.self, forKey: LookKey("look"))) ?? KeyLook()
    }
}

/// `rules` の配列。読めない要素は捨てるが、個数の上限（資源の上限）の検査のために元の個数も持つ
struct LookRuleList: Decodable {
    var rules: [LookRule] = []
    var rawCount = 0

    init(from decoder: Decoder) throws {
        var container = try decoder.unkeyedContainer()
        while !container.isAtEnd {
            rawCount += 1
            if let rule = try? container.decode(LookRule.self) {
                rules.append(rule)
            } else {
                _ = try? container.decode(LookSkip.self)
            }
        }
    }
}

// MARK: - metrics / background

/// キー間・行間・外周の余白（§3.5）。範囲外は無視（nil）
struct LayoutMetrics: Equatable, Decodable {
    var keySpacing: Double?
    var rowSpacing: Double?
    var padding: Double?

    init(keySpacing: Double? = nil, rowSpacing: Double? = nil, padding: Double? = nil) {
        self.keySpacing = keySpacing
        self.rowSpacing = rowSpacing
        self.padding = padding
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: LookKey.self)
        func read(_ name: String, _ range: ClosedRange<Double>) -> Double? {
            guard let value = try? container.decode(Double.self, forKey: LookKey(name)), range.contains(value) else {
                return nil
            }
            return value
        }
        keySpacing = read("keySpacing", KeyLookLimits.spacingRange)
        rowSpacing = read("rowSpacing", KeyLookLimits.spacingRange)
        padding = read("padding", KeyLookLimits.metricsPaddingRange)
    }
}

/// キーボード全体の背景（§6.4）。`fill` / `fillOpacity` / `fillImage` / `fillImageFit` / `fillImageOpacity` だけ
struct LayoutBackground: Equatable, Decodable {
    static let allowed: Set<String> = ["fill", "fillOpacity", "fillImage", "fillImageFit", "fillImageOpacity"]

    var look: KeyLook

    init(look: KeyLook) {
        self.look = look
    }

    init(from decoder: Decoder) throws {
        let all = try KeyLook(from: decoder)
        look = KeyLook(values: all.values.filter { LayoutBackground.allowed.contains($0.key) })
    }
}
