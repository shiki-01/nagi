import Foundation

// iOS 固有の設定（docs/ios-settings.md §3）。コアの `config.toml` に無い、キーボード UI 側で完結する項目を
// App Group の `ios-settings.json` に持つ。ホストアプリ（設定画面）だけが書き、キーボード拡張は focusIn のたびに読む。
//
// - 形式は平らな JSON: `{"schema":1,"flick.threshold_pt":14,...}`。キーは docs/ios-settings.md の ID と同じ
// - 範囲外・型違いの値は読むときに既定へ丸める（入力を失わない）
// - 知らないキーは捨てずに保持して書き戻す（新しい版のアプリが書いた項目を壊さない）
// - 明示指定された項目だけを書く。無い項目は既定値（既定を後から変えても、触っていない人には追随する）

/// JSON の値。`ios-settings.json` の未知のキーを、中身を解釈せずに保持するための型
enum NagiJSONValue: Codable, Equatable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([NagiJSONValue])
    case object([String: NagiJSONValue])

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([NagiJSONValue].self) {
            self = .array(value)
        } else if let value = try? container.decode([String: NagiJSONValue].self) {
            self = .object(value)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "対応していない JSON の値")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .string(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        }
    }
}

/// iOS 固有の設定項目 1 件の定義。既定値・範囲・設定画面での表示を 1 か所で持つ
struct NagiIOSSettingDef {
    struct Choice {
        let value: String
        let label: String
    }

    enum Kind {
        case bool
        case int(min: Int, max: Int, step: Int)
        case float(min: Double, max: Double, step: Double)
        case choice([Choice])
    }

    let key: String
    /// 設定画面のセクション名（コアの記述子の `group` と同じ名前なら同じセクションに並ぶ）
    let group: String
    let label: String
    let description: String
    let kind: Kind
    /// true ならシンプルモードにも出す（false は詳細モードのみ）
    let isSimple: Bool
    let defaultValue: NagiJSONValue

    /// 保存された値を検証して返す。型違い・範囲外は既定に丸める（数値は範囲内に収め、刻みに揃える）
    func normalize(_ raw: NagiJSONValue?) -> NagiJSONValue {
        switch kind {
        case .bool:
            if case .bool(let value)? = raw { return .bool(value) }
            return defaultValue
        case .int(let lo, let hi, let step):
            guard case .number(let number)? = raw, number.isFinite else { return defaultValue }
            let clamped = Int(min(max(number, Double(lo)), Double(hi)).rounded())
            var value = clamped
            if step > 1 {
                let snapped = lo + Int((Double(clamped - lo) / Double(step)).rounded()) * step
                value = min(snapped, hi)
            }
            return .number(Double(value))
        case .float(let lo, let hi, let step):
            guard case .number(let number)? = raw, number.isFinite else { return defaultValue }
            var value = min(max(number, lo), hi)
            if step > 0 {
                let snapped = lo + ((value - lo) / step).rounded() * step
                // 浮動小数点の誤差（0.30000000000000004 など）を落とす
                value = min((snapped * 1_000_000).rounded() / 1_000_000, hi)
            }
            return .number(value)
        case .choice(let choices):
            if case .string(let text)? = raw, choices.contains(where: { $0.value == text }) {
                return .string(text)
            }
            return defaultValue
        }
    }
}

/// `ios-settings.json` の内容。値の読み出しは常に検証済み（不正な値は既定）
struct NagiIOSSettings: Codable, Equatable {
    static let currentSchema = 1

    /// 書き込まれていたキーと値（未知のキーを含む。`schema` は含まない）
    private(set) var stored: [String: NagiJSONValue] = [:]
    private(set) var schema = NagiIOSSettings.currentSchema

    init() {}

    init(from decoder: Decoder) throws {
        var all = try decoder.singleValueContainer().decode([String: NagiJSONValue].self)
        if case .number(let number)? = all.removeValue(forKey: "schema"), number.isFinite,
           number >= 0, number < 1_000_000 {
            schema = Int(number)
        }
        stored = all
    }

    func encode(to encoder: Encoder) throws {
        var all = stored
        // 読んだ版が新しければそのまま書き戻す（新しい版の項目の意味を変えない）
        all["schema"] = .number(Double(max(schema, NagiIOSSettings.currentSchema)))
        var container = encoder.singleValueContainer()
        try container.encode(all)
    }

    // MARK: 定義

    private static func choice(_ pairs: [(String, String)]) -> NagiIOSSettingDef.Kind {
        .choice(pairs.map { NagiIOSSettingDef.Choice(value: $0.0, label: $0.1) })
    }

    /// 項目の定義（設定画面の表示順）。docs/ios-settings.md の ID と同じキー
    static let definitions: [NagiIOSSettingDef] = [
        NagiIOSSettingDef(
            key: Key.toggleInput, group: "入力", label: "トグル入力",
            description: "同じキーを続けて押すと次の文字（あ→い→う）になります。オフのときはフリックで入力し、タップは常に先頭の文字です。",
            kind: .bool, isSimple: true, defaultValue: .bool(false)),
        NagiIOSSettingDef(
            key: Key.flickSensitivity, group: "入力", label: "フリック感度",
            description: "値が大きいほど、短い移動でフリックと判定します。",
            kind: .float(min: 0.5, max: 2.0, step: 0.1), isSimple: true, defaultValue: .number(1.0)),
        NagiIOSSettingDef(
            key: Key.flickThresholdPt, group: "入力", label: "フリックの判定距離（pt）",
            description: "指をこの距離より動かすとフリックと判定します。感度の倍率で割った値が実際の距離です。",
            kind: .float(min: 4, max: 40, step: 1), isSimple: false, defaultValue: .number(14)),
        NagiIOSSettingDef(
            key: Key.flickPopup, group: "入力", label: "フリックの案内を表示",
            description: "キーを押している間、上下左右の文字をポップアップで見せます。オフにすると描画が軽くなります。",
            kind: .bool, isSimple: true, defaultValue: .bool(true)),
        NagiIOSSettingDef(
            key: Key.flickLongPressMs, group: "入力", label: "長押しの判定時間（ミリ秒）",
            description: "長押しと判定するまでの時間です。レイアウトの longPress を持つキーを、この時間押し続けると候補のポップアップが出ます。",
            kind: .int(min: 150, max: 1000, step: 50), isSimple: false, defaultValue: .number(350)),
        NagiIOSSettingDef(
            key: Key.keyRepeatDelayMs, group: "入力", label: "⌫の連打を始めるまで（ミリ秒）",
            description: "⌫を押し続けてから、連続して消し始めるまでの時間です。",
            kind: .int(min: 200, max: 1000, step: 50), isSimple: false, defaultValue: .number(450)),
        NagiIOSSettingDef(
            key: Key.keyRepeatMs, group: "入力", label: "⌫の連打の間隔（ミリ秒）",
            description: "連打の最初の間隔です。押し続けると少しずつ短くなります。",
            kind: .int(min: 30, max: 300, step: 10), isSimple: false, defaultValue: .number(110)),
        NagiIOSSettingDef(
            key: Key.spaceSwipe, group: "入力", label: "スペースキーのスワイプでカーソル移動",
            description: "スペースキーを押したまま左右に動かすと、カーソルを動かします（離してもスペースは入りません）。入力中は文節・カーソルの移動になります。タップは通常のスペースです。",
            kind: .bool, isSimple: false, defaultValue: .bool(true)),
        NagiIOSSettingDef(
            key: Key.spaceSwipeStepPt, group: "入力", label: "カーソル移動 1 文字あたりの距離（pt）",
            description: "スペースキーをスワイプしたとき、この距離ごとにカーソルが 1 つ動きます。小さいほど速く動きます。",
            kind: .float(min: 4, max: 40, step: 1), isSimple: false, defaultValue: .number(14)),
        NagiIOSSettingDef(
            key: Key.backspaceSwipe, group: "入力", label: "⌫の左スワイプで単語を削除",
            description: "⌫を押したまま左へ大きく動かすと、カーソル手前の単語（同じ種類の文字の連なり）をまとめて消します。入力中は働きません。",
            kind: .bool, isSimple: false, defaultValue: .bool(false)),

        NagiIOSSettingDef(
            key: Key.keyboardHeight, group: "キーボード", label: "キーボードの高さ",
            description: "キー全体の高さです。細かく調整するときは「高さの倍率」を使います。",
            kind: NagiIOSSettings.choice([("compact", "低め"), ("standard", "標準"), ("tall", "高め")]),
            isSimple: true, defaultValue: .string("standard")),
        NagiIOSSettingDef(
            key: Key.keyboardHeightScale, group: "キーボード", label: "高さの倍率",
            description: "「キーボードの高さ」にさらに掛ける倍率です。",
            kind: .float(min: 0.8, max: 1.3, step: 0.05), isSimple: false, defaultValue: .number(1.0)),
        NagiIOSSettingDef(
            key: Key.keyboardOneHanded, group: "キーボード", label: "片手モード",
            description: "キー部分を画面の左または右に寄せます。",
            kind: NagiIOSSettings.choice([("off", "オフ"), ("left", "左に寄せる"), ("right", "右に寄せる")]),
            isSimple: true, defaultValue: .string("off")),
        NagiIOSSettingDef(
            key: Key.keyboardOneHandedWidth, group: "キーボード", label: "片手モードの幅",
            description: "片手モードのとき、キー部分が画面の幅に占める割合です。",
            kind: .float(min: 0.6, max: 0.9, step: 0.05), isSimple: false, defaultValue: .number(0.8)),
        NagiIOSSettingDef(
            key: Key.keyboardKeySpacingPt, group: "キーボード", label: "キーの間隔（pt）",
            description: "キーとキーのあいだの余白です。",
            kind: .float(min: 0, max: 8, step: 0.5), isSimple: false, defaultValue: .number(6)),
        NagiIOSSettingDef(
            key: Key.keyboardKeyCornerPt, group: "キーボード", label: "キーの角丸（pt）",
            description: "キーの角の丸さです。",
            kind: .float(min: 0, max: 16, step: 1), isSimple: false, defaultValue: .number(6)),
        NagiIOSSettingDef(
            key: Key.keyboardHaptic, group: "キーボード", label: "キー入力の触覚",
            description: "キーを押したときの振動です。フルアクセスを許可していないと働かない場合があります。",
            kind: NagiIOSSettings.choice([("off", "オフ"), ("light", "弱"), ("medium", "中"), ("heavy", "強")]),
            isSimple: true, defaultValue: .string("light")),
        NagiIOSSettingDef(
            key: Key.keyboardSound, group: "キーボード", label: "キー入力の音",
            description: "端末の「キーボードのクリック」設定に従ってクリック音を鳴らします。",
            kind: NagiIOSSettings.choice([("off", "オフ"), ("system", "システムのクリック音")]),
            isSimple: true, defaultValue: .string("off")),
        NagiIOSSettingDef(
            key: Key.keyboardTheme, group: "キーボード", label: "テーマ",
            description: "キーボードの明るさです。「システム」は端末の外観（ライト/ダーク）に従います。",
            kind: NagiIOSSettings.choice([("system", "システムに合わせる"), ("light", "ライト"), ("dark", "ダーク")]),
            isSimple: true, defaultValue: .string("system")),
        NagiIOSSettingDef(
            key: Key.keyboardAccent, group: "キーボード", label: "アクセントカラー",
            description: "確定キーや選択中の候補などの色です。",
            kind: NagiIOSSettings.choice([
                ("blue", "ブルー"), ("green", "グリーン"), ("orange", "オレンジ"), ("pink", "ピンク"), ("gray", "グレー"),
            ]),
            isSimple: true, defaultValue: .string("blue")),
        NagiIOSSettingDef(
            key: Key.candbarPasteButton, group: "キーボード", label: "候補バーに「貼り付け」を出す",
            description: "入力中でなく、クリップボードに文字があるときだけ、候補バーの左端に出します。押したときだけクリップボードを読みます（iOS の貼り付け許可の確認が出ることがあります）。",
            kind: .bool, isSimple: false, defaultValue: .bool(true)),
    ]

    private static let definitionsByKey: [String: NagiIOSSettingDef] =
        Dictionary(definitions.map { ($0.key, $0) }, uniquingKeysWith: { first, _ in first })

    static func definition(for key: String) -> NagiIOSSettingDef? {
        definitionsByKey[key]
    }

    // MARK: キー

    enum Key {
        static let toggleInput = "input.toggle_input"
        static let toggleTimeoutMs = "input.toggle_timeout_ms"
        static let flickSensitivity = "flick.sensitivity"
        static let flickThresholdPt = "flick.threshold_pt"
        static let flickPopup = "flick.popup"
        static let flickLongPressMs = "flick.long_press_ms"
        static let keyRepeatDelayMs = "touch.key_repeat_delay_ms"
        static let keyRepeatMs = "touch.key_repeat_ms"
        static let spaceSwipe = "touch.space_swipe"
        static let spaceSwipeStepPt = "touch.space_swipe_step_pt"
        static let backspaceSwipe = "touch.backspace_swipe"
        static let keyboardTheme = "keyboard.theme"
        static let keyboardAccent = "keyboard.accent"
        static let candbarPasteButton = "candbar.paste_button"
        static let keyboardHeight = "keyboard.height"
        static let keyboardHeightScale = "keyboard.height_scale"
        static let keyboardOneHanded = "keyboard.one_handed"
        static let keyboardOneHandedWidth = "keyboard.one_handed_width"
        static let keyboardKeySpacingPt = "keyboard.key_spacing_pt"
        static let keyboardKeyCornerPt = "keyboard.key_corner_pt"
        static let keyboardHaptic = "keyboard.haptic"
        static let keyboardSound = "keyboard.sound"
    }

    // MARK: 読み書き

    /// 検証済みの値（未指定・不正は既定）。未知のキーは `.null`
    func value(_ key: String) -> NagiJSONValue {
        guard let def = Self.definition(for: key) else { return stored[key] ?? .null }
        return def.normalize(stored[key])
    }

    func bool(_ key: String) -> Bool {
        if case .bool(let value) = self.value(key) { return value }
        return false
    }

    func double(_ key: String) -> Double {
        if case .number(let value) = self.value(key) { return value }
        return 0
    }

    func int(_ key: String) -> Int {
        Int(double(key).rounded())
    }

    func string(_ key: String) -> String {
        if case .string(let value) = self.value(key) { return value }
        return ""
    }

    /// 明示指定があるか（「既定に戻す」を出す条件）
    func isExplicit(_ key: String) -> Bool {
        stored[key] != nil
    }

    /// 値を設定する（検証して丸める）。nil は明示指定を消して既定に戻す
    mutating func set(_ key: String, _ newValue: NagiJSONValue?) {
        guard let newValue = newValue else {
            stored.removeValue(forKey: key)
            return
        }
        if let def = Self.definition(for: key) {
            stored[key] = def.normalize(newValue)
        } else {
            stored[key] = newValue
        }
    }

    // MARK: キーボードが使う値

    var toggleInput: Bool { bool(Key.toggleInput) }
    var flickPopup: Bool { bool(Key.flickPopup) }
    var flickLongPressMs: Int { int(Key.flickLongPressMs) }
    /// トグルキー（レイアウトの `toggle`）が次の押下を「置き換え」とみなす時間。
    /// 設定項目 `input.toggle_timeout_ms` は設定画面には未登録（コア側の概念待ち）で、ファイルに値があれば使い、無ければ 1000ms
    var toggleTimeoutMs: Int {
        if case .number(let value)? = stored[Key.toggleTimeoutMs], value.isFinite {
            return Int(min(max(value, 200), 3000))
        }
        return 1000
    }
    var keyRepeatDelayMs: Int { int(Key.keyRepeatDelayMs) }
    var keyRepeatMs: Int { int(Key.keyRepeatMs) }
    var keySpacingPt: Double { double(Key.keyboardKeySpacingPt) }
    var keyCornerPt: Double { double(Key.keyboardKeyCornerPt) }
    var oneHanded: String { string(Key.keyboardOneHanded) }
    var oneHandedWidth: Double { double(Key.keyboardOneHandedWidth) }
    var haptic: String { string(Key.keyboardHaptic) }
    var soundEnabled: Bool { string(Key.keyboardSound) == "system" }
    var spaceSwipe: Bool { bool(Key.spaceSwipe) }
    var spaceSwipeStepPt: Double { double(Key.spaceSwipeStepPt) }
    var backspaceSwipe: Bool { bool(Key.backspaceSwipe) }
    var theme: String { string(Key.keyboardTheme) }
    var accent: String { string(Key.keyboardAccent) }
    var pasteButton: Bool { bool(Key.candbarPasteButton) }

    /// フリックと判定する移動量（pt）。感度が高いほど短い
    var flickThresholdPoints: Double {
        let sensitivity = max(double(Key.flickSensitivity), 0.1)
        return double(Key.flickThresholdPt) / sensitivity
    }

    /// キー全体の高さの倍率（プリセット × 倍率）
    var heightScale: Double {
        let preset: Double
        switch string(Key.keyboardHeight) {
        case "compact": preset = 0.9
        case "tall": preset = 1.1
        default: preset = 1.0
        }
        return preset * double(Key.keyboardHeightScale)
    }
}

// MARK: - ファイル

/// `ios-settings.json` の読み書き。書くのはホストアプリだけ（原子的に置き換える）
enum NagiIOSSettingsStore {
    static let fileName = "ios-settings.json"

    /// App Group コンテナ直下。無ければこのプロセス専用の Application Support（ホストと拡張で別になる）
    static var fileURL: URL? {
        if let container = SharedStore.shared.containerURL {
            return container.appendingPathComponent(fileName)
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("com.shiki.nagi", isDirectory: true)
            .appendingPathComponent(fileName)
    }

    /// 読む。ファイルが無い・壊れている場合は既定（空）
    static func load() -> NagiIOSSettings {
        guard let url = fileURL, let data = try? Data(contentsOf: url) else { return NagiIOSSettings() }
        return (try? JSONDecoder().decode(NagiIOSSettings.self, from: data)) ?? NagiIOSSettings()
    }

    static func save(_ settings: NagiIOSSettings) throws {
        guard let url = fileURL else {
            throw NSError(domain: "NagiIOSSettingsStore", code: 1, userInfo: [NSLocalizedDescriptionKey: "保存先がありません"])
        }
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(settings).write(to: url, options: .atomic)
    }
}

/// 学習データの全削除をホストアプリからキーボード拡張へ伝える合図。
/// 拡張は自分のメモリ上の学習データを、あとで書き戻して復活させてしまわないよう、
/// 合図が変わっていたら focusIn で `clear_learning` を呼ぶ
enum NagiLearningClearSignal {
    static let fileName = "ios-learning-cleared"

    private static var fileURL: URL? {
        SharedStore.shared.containerURL?.appendingPathComponent(fileName)
    }

    /// 現在の合図（無ければ nil）
    static func current() -> String? {
        guard let url = fileURL else { return nil }
        return try? String(contentsOf: url, encoding: .utf8)
    }

    /// 合図を更新する（ホストアプリが学習データを削除したあとに呼ぶ）
    static func raise() {
        guard let url = fileURL else { return }
        let stamp = String(Date().timeIntervalSince1970)
        try? stamp.write(to: url, atomically: true, encoding: .utf8)
    }
}
