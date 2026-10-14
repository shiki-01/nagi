import Foundation

// キーボードレイアウトのデータモデル（docs/ios-keyboard-layout.md、スキーマ v1）。
// レイアウトは「どのキーが何を送るか」だけを持ち、かな変換・トグル・フリックの解決はコアが行う。
// 段階 1 は text / flick / action / layout / spacer。toggle / longPress は decode だけ通し、描画側では無視する。

enum LayoutMode: String, Codable {
    case kana, alpha, number, symbol, emoji, custom
}

enum LayoutKeyKind: String, Codable {
    case text, flick, toggle, action, shortcut, layout, spacer
}

enum KeyStyle: String, Codable {
    case normal, function, accent
}

/// フリックの 1 方向（と中央）の文字
struct FlickChar: Codable {
    let text: String
}

/// `shortcut` の 1 手順（`action` か `text` のどちらか）
struct ShortcutStep: Codable {
    let action: String?
    let text: String?
}

/// `visible` の条件（v1 では描画に使わない）
struct VisibleCondition: Codable {
    let field: String
    let values: [String]

    enum CodingKeys: String, CodingKey {
        case field
        case values = "in"
    }
}

struct LayoutKey: Codable {
    var id: String
    /// 行内の幅の比（既定 1）
    var width: Double
    var label: String?
    /// SF Symbols 名
    var icon: String?
    var style: KeyStyle
    var kind: LayoutKeyKind
    // text
    var text: String?
    var shift: String?
    // flick
    var center: FlickChar?
    var up: FlickChar?
    var down: FlickChar?
    var left: FlickChar?
    var right: FlickChar?
    /// 12 キー名（「あ」など）。指定するとコアのトグル・フリック・濁点循環に任せる
    var kana: String?
    // toggle
    var cycle: [String]?
    // action
    var action: String?
    // shortcut
    var steps: [ShortcutStep]?
    // layout
    var target: String?
    var longPress: [LayoutKey]?
    var visible: VisibleCondition?

    enum CodingKeys: String, CodingKey {
        case id, width, label, icon, style, kind, text, shift
        case center, up, down, left, right, kana
        case cycle, action, steps, target, longPress, visible
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = try c.decode(LayoutKeyKind.self, forKey: .kind)
        // `spacer` などは id を省略できる（一意になるよう補う）
        id = try c.decodeIfPresent(String.self, forKey: .id) ?? UUID().uuidString
        width = try c.decodeIfPresent(Double.self, forKey: .width) ?? 1
        label = try c.decodeIfPresent(String.self, forKey: .label)
        icon = try c.decodeIfPresent(String.self, forKey: .icon)
        style = try c.decodeIfPresent(KeyStyle.self, forKey: .style) ?? .normal
        text = try c.decodeIfPresent(String.self, forKey: .text)
        shift = try c.decodeIfPresent(String.self, forKey: .shift)
        center = try c.decodeIfPresent(FlickChar.self, forKey: .center)
        up = try c.decodeIfPresent(FlickChar.self, forKey: .up)
        down = try c.decodeIfPresent(FlickChar.self, forKey: .down)
        left = try c.decodeIfPresent(FlickChar.self, forKey: .left)
        right = try c.decodeIfPresent(FlickChar.self, forKey: .right)
        kana = try c.decodeIfPresent(String.self, forKey: .kana)
        cycle = try c.decodeIfPresent([String].self, forKey: .cycle)
        action = try c.decodeIfPresent(String.self, forKey: .action)
        steps = try c.decodeIfPresent([ShortcutStep].self, forKey: .steps)
        target = try c.decodeIfPresent(String.self, forKey: .target)
        longPress = try c.decodeIfPresent([LayoutKey].self, forKey: .longPress)
        // 条件の形が想定と違っても、レイアウト全体は無効にしない（v1 では使わない）
        visible = (try? c.decodeIfPresent(VisibleCondition.self, forKey: .visible)) ?? nil
    }

    /// コードから作る最小のキー（最後の砦のレイアウト用）
    init(id: String, kind: LayoutKeyKind, label: String?, action: String?, width: Double = 1, style: KeyStyle = .normal) {
        self.id = id
        self.width = width
        self.label = label
        self.icon = nil
        self.style = style
        self.kind = kind
        self.text = nil
        self.shift = nil
        self.center = nil
        self.up = nil
        self.down = nil
        self.left = nil
        self.right = nil
        self.kana = nil
        self.cycle = nil
        self.action = action
        self.steps = nil
        self.target = nil
        self.longPress = nil
        self.visible = nil
    }
}

struct LayoutRow: Codable {
    /// 行の高さの比（既定 1）
    var height: Double
    var keys: [LayoutKey]

    enum CodingKeys: String, CodingKey {
        case height, keys
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        height = try c.decodeIfPresent(Double.self, forKey: .height) ?? 1
        keys = try c.decode([LayoutKey].self, forKey: .keys)
    }

    init(height: Double, keys: [LayoutKey]) {
        self.height = height
        self.keys = keys
    }
}

struct KeyboardLayoutDef: Codable {
    var schema: Int
    var id: String
    var name: String
    var mode: LayoutMode
    var rows: [LayoutRow]

    enum CodingKeys: String, CodingKey {
        case schema, id, name, mode, rows
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        schema = try c.decode(Int.self, forKey: .schema)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? id
        mode = try c.decode(LayoutMode.self, forKey: .mode)
        rows = try c.decode([LayoutRow].self, forKey: .rows)
    }

    init(id: String, name: String, mode: LayoutMode, rows: [LayoutRow]) {
        self.schema = 1
        self.id = id
        self.name = name
        self.mode = mode
        self.rows = rows
    }

    /// 仕様（docs/ios-keyboard-layout.md「検証」）の範囲を確かめる。問題があれば理由を返す
    func validationError() -> String? {
        if schema != 1 { return "未対応のスキーマ \(schema)" }
        if rows.isEmpty || rows.count > 8 { return "rows は 1〜8 行（\(rows.count)）" }
        var ids = Set<String>()
        for (index, row) in rows.enumerated() {
            let total = row.keys.reduce(0.0) { $0 + $1.width }
            if total < 1 || total > 20 { return "行 \(index + 1) の幅の合計が範囲外（\(total)）" }
            if row.height <= 0 || row.height > 4 { return "行 \(index + 1) の高さが範囲外（\(row.height)）" }
            for key in row.keys {
                if key.width <= 0 { return "キー \(key.id) の幅が不正" }
                if !ids.insert(key.id).inserted { return "キー id の重複: \(key.id)" }
                if let text = key.text, text.count > 16 { return "キー \(key.id) の text が長すぎる" }
                if key.kind == .toggle {
                    let count = key.cycle?.count ?? 0
                    if count < 1 || count > 16 { return "キー \(key.id) の cycle は 1〜16 要素" }
                }
                for step in key.steps ?? [] {
                    if let text = step.text, text.count > 256 { return "キー \(key.id) の shortcut の text が長すぎる" }
                }
            }
        }
        return nil
    }
}

// MARK: - 読み込み

/// レイアウトの読み込み。優先順位は「ユーザーのレイアウト（App Group の keyboard/layouts/<id>.json）→
/// アプリに同梱の既定（presets/<id>.json）→ 組み込みのフォールバック」。
/// 不正なレイアウト（decode 失敗・検証違反）は読み込まず、次の候補に進む（入力を失わない）
enum LayoutLoader {
    /// 同梱の既定レイアウトのフォルダ名（project.yml が `config/keyboard/presets` を置く）
    static let bundledFolder = "presets"

    static let defaultKanaId = "flick-kana"
    static let defaultAlphaId = "qwerty-alpha"

    /// ユーザーのレイアウトのフォルダ（App Group の `keyboard/layouts/`）
    static var userLayoutsDirectory: URL? {
        SharedStore.shared.containerURL?
            .appendingPathComponent("keyboard", isDirectory: true)
            .appendingPathComponent("layouts", isDirectory: true)
    }

    struct LoadError: Error {
        let message: String
    }

    /// JSON を decode し、仕様の範囲を検証する
    static func decode(_ data: Data) throws -> KeyboardLayoutDef {
        let def: KeyboardLayoutDef
        do {
            def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data)
        } catch {
            throw LoadError(message: "decode 失敗: \(error)")
        }
        if let reason = def.validationError() { throw LoadError(message: reason) }
        return def
    }

    /// id のレイアウトを読む。ユーザー → 同梱の順。どちらも無い・不正なら nil
    static func load(id: String) -> KeyboardLayoutDef? {
        guard isSafeId(id) else { return nil }
        if let dir = userLayoutsDirectory {
            let url = dir.appendingPathComponent("\(id).json")
            if let def = read(url: url, label: "user") { return def }
        }
        if let url = bundledURL(id: id), let def = read(url: url, label: "bundled") { return def }
        return nil
    }

    /// モード（かな / 英字）に割り当てたレイアウトを読む。読めなければ既定、組み込みの順に戻す
    static func loadAssigned(mode: LayoutMode) -> KeyboardLayoutDef {
        let assigned = LayoutAssignment.id(for: mode)
        let fallbackId = mode == .alpha ? defaultAlphaId : defaultKanaId
        if let def = load(id: assigned) { return def }
        if assigned != fallbackId, let def = load(id: fallbackId) { return def }
        DiagLog.log("layout fallback to builtin mode=\(mode.rawValue) assigned=\(assigned)")
        return builtin(mode: mode)
    }

    /// ユーザーのレイアウトの id 一覧（ホストアプリの管理画面用）
    static func userLayoutIds() -> [String] {
        guard let dir = userLayoutsDirectory,
              let names = try? FileManager.default.contentsOfDirectory(atPath: dir.path) else { return [] }
        return names.filter { $0.hasSuffix(".json") }.map { String($0.dropLast(5)) }.sorted()
    }

    private static func read(url: URL, label: String) -> KeyboardLayoutDef? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        do {
            return try decode(data)
        } catch let error as LoadError {
            DiagLog.log("layout invalid (\(label)) \(url.lastPathComponent): \(error.message)")
            return nil
        } catch {
            return nil
        }
    }

    private static func bundledURL(id: String) -> URL? {
        Bundle.main.url(forResource: id, withExtension: "json", subdirectory: bundledFolder)
            ?? Bundle.main.url(forResource: id, withExtension: "json")
    }

    /// パスの組み立てに使うので、英数字・ハイフン・アンダースコアだけを許す
    private static func isSafeId(_ id: String) -> Bool {
        !id.isEmpty && id.count <= 64 && id.unicodeScalars.allSatisfy {
            ($0.value < 128) && (CharacterSet.alphanumerics.contains($0) || $0 == "-" || $0 == "_")
        }
    }

    // MARK: - 組み込みのフォールバック

    /// 同梱の JSON が読めないときの最小のレイアウト。
    /// `config/keyboard/presets/flick-kana.json` / `qwerty-alpha.json` の簡略版（重複。ここは最後の砦なので
    /// 読み込みに依存しない文字列で持つ）
    static func builtin(mode: LayoutMode) -> KeyboardLayoutDef {
        let json = mode == .alpha ? builtinAlphaJSON : builtinKanaJSON
        if let data = json.data(using: .utf8), let def = try? decode(data) { return def }
        // JSON も壊れている場合の最後の砦: 地球儀キーだけ
        return KeyboardLayoutDef(
            id: "builtin-minimal", name: "minimal", mode: .custom,
            rows: [LayoutRow(height: 1, keys: [
                LayoutKey(id: "globe", kind: .action, label: "🌐", action: "globe"),
                LayoutKey(id: "bs", kind: .action, label: "⌫", action: "backspace"),
            ])]
        )
    }

    private static let builtinKanaJSON = #"""
    {"schema":1,"id":"builtin-kana","name":"builtin","mode":"kana","rows":[
    {"keys":[{"id":"l","width":0.7,"label":"←","kind":"action","action":"moveLeft","style":"function"},
    {"id":"a","kind":"flick","kana":"あ","label":"あ"},{"id":"ka","kind":"flick","kana":"か","label":"か"},{"id":"sa","kind":"flick","kana":"さ","label":"さ"},
    {"id":"bs","width":0.85,"label":"⌫","kind":"action","action":"backspace","style":"function"}]},
    {"keys":[{"id":"r","width":0.7,"label":"→","kind":"action","action":"moveRight","style":"function"},
    {"id":"ta","kind":"flick","kana":"た","label":"た"},{"id":"na","kind":"flick","kana":"な","label":"な"},{"id":"ha","kind":"flick","kana":"は","label":"は"},
    {"id":"sp","width":0.85,"label":"空白","kind":"action","action":"space","style":"function"}]},
    {"keys":[{"id":"mode","width":0.7,"label":"A","kind":"action","action":"modeSwitch","style":"function"},
    {"id":"ma","kind":"flick","kana":"ま","label":"ま"},{"id":"ya","kind":"flick","kana":"や","label":"や"},{"id":"ra","kind":"flick","kana":"ら","label":"ら"},
    {"id":"en","width":0.85,"label":"改行","kind":"action","action":"enter","style":"function"}]},
    {"keys":[{"id":"gl","width":0.7,"label":"🌐","kind":"action","action":"globe","style":"function"},
    {"id":"dk","label":"゛゜小","kind":"action","action":"dakuten"},{"id":"wa","kind":"flick","kana":"わ","label":"わ"},{"id":"pu","kind":"flick","kana":"、","label":"、。？！"},
    {"id":"sp2","width":0.85,"kind":"spacer"}]}]}
    """#

    private static let builtinAlphaJSON = #"""
    {"schema":1,"id":"builtin-alpha","name":"builtin","mode":"alpha","rows":[
    {"keys":[{"id":"q","kind":"text","text":"q","shift":"Q"},{"id":"w","kind":"text","text":"w","shift":"W"},{"id":"e","kind":"text","text":"e","shift":"E"},{"id":"r","kind":"text","text":"r","shift":"R"},{"id":"t","kind":"text","text":"t","shift":"T"},{"id":"y","kind":"text","text":"y","shift":"Y"},{"id":"u","kind":"text","text":"u","shift":"U"},{"id":"i","kind":"text","text":"i","shift":"I"},{"id":"o","kind":"text","text":"o","shift":"O"},{"id":"p","kind":"text","text":"p","shift":"P"}]},
    {"keys":[{"id":"sl","width":0.5,"kind":"spacer"},{"id":"a","kind":"text","text":"a","shift":"A"},{"id":"s","kind":"text","text":"s","shift":"S"},{"id":"d","kind":"text","text":"d","shift":"D"},{"id":"f","kind":"text","text":"f","shift":"F"},{"id":"g","kind":"text","text":"g","shift":"G"},{"id":"h","kind":"text","text":"h","shift":"H"},{"id":"j","kind":"text","text":"j","shift":"J"},{"id":"k","kind":"text","text":"k","shift":"K"},{"id":"l","kind":"text","text":"l","shift":"L"},{"id":"sr","width":0.5,"kind":"spacer"}]},
    {"keys":[{"id":"sh","width":1.4,"icon":"shift","kind":"action","action":"shift","style":"function"},{"id":"z","kind":"text","text":"z","shift":"Z"},{"id":"x","kind":"text","text":"x","shift":"X"},{"id":"c","kind":"text","text":"c","shift":"C"},{"id":"v","kind":"text","text":"v","shift":"V"},{"id":"b","kind":"text","text":"b","shift":"B"},{"id":"n","kind":"text","text":"n","shift":"N"},{"id":"m","kind":"text","text":"m","shift":"M"},{"id":"bs","width":1.4,"label":"⌫","kind":"action","action":"backspace","style":"function"}]},
    {"keys":[{"id":"mode","width":1.2,"label":"あ","kind":"action","action":"modeSwitch","style":"function"},{"id":"gl","width":1,"label":"🌐","kind":"action","action":"globe","style":"function"},{"id":"sp","width":4.4,"label":"空白","kind":"action","action":"space"},{"id":"en","width":1.6,"label":"改行","kind":"action","action":"enter","style":"function"}]}]}
    """#
}

/// 各モード（かな / 英字）にどのレイアウトを割り当てるか。App Group の UserDefaults に保存し、
/// ホストアプリ（設定）と拡張で共有する。未設定は既定（かな: flick-kana、英字: qwerty-alpha。
/// ローマ字入力の QWERTY かなにするなら、かなに `qwerty-kana` を割り当てる）
enum LayoutAssignment {
    private static let kanaKey = "keyboardLayoutKana"
    private static let alphaKey = "keyboardLayoutAlpha"

    private static var defaults: UserDefaults? {
        UserDefaults(suiteName: SharedStore.shared.appGroupId)
    }

    static func id(for mode: LayoutMode) -> String {
        switch mode {
        case .alpha:
            return defaults?.string(forKey: alphaKey) ?? LayoutLoader.defaultAlphaId
        default:
            return defaults?.string(forKey: kanaKey) ?? LayoutLoader.defaultKanaId
        }
    }

    static func set(id: String?, for mode: LayoutMode) {
        let key = mode == .alpha ? alphaKey : kanaKey
        defaults?.set(id, forKey: key)
    }
}
