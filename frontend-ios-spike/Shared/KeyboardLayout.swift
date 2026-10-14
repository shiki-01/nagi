import Foundation

// キーボードレイアウトのデータモデル（docs/ios-keyboard-layout.md、スキーマ v1）。
// レイアウトは「どのキーが何を送るか」だけを持ち、かな変換・トグル・フリックの解決はコアが行う。
// 段階 2 で toggle / longPress / shortcut も描画・実行する（Keyboard/ 側）。flick キーの longPress は未対応（無視する）。

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
    /// JSON で id が指定されていたか（省略時は補った id。検証では「指定あり」のものだけ形式を見る）
    var idGiven = true

    enum CodingKeys: String, CodingKey {
        case id, width, label, icon, style, kind, text, shift
        case center, up, down, left, right, kana
        case cycle, action, steps, target, longPress, visible
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = try c.decode(LayoutKeyKind.self, forKey: .kind)
        // `spacer` などは id を省略できる（一意になるよう補う）
        let givenId = try c.decodeIfPresent(String.self, forKey: .id)
        id = givenId ?? UUID().uuidString
        idGiven = givenId != nil
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
        // web 版の検証と同じく name は必須
        name = try c.decode(String.self, forKey: .name)
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

    /// 仕様（docs/ios-keyboard-layout.md「検証」）の範囲を確かめる。問題があれば理由を返す。
    /// 規則は `web/src/lib/keyboard/validate.ts` と同じ（変えるときは両方を揃える）。
    /// - targets: `layout.target` として有効な id（組み込み名を除く）。nil なら存在の確認をしない
    ///   （実行時の読み込みでは、参照先が消えていてもレイアウト全体は無効にしない）
    func validationError(targets: [String]? = nil) -> String? {
        typealias V = LayoutValidation
        if schema != 1 { return "未対応のスキーマ \(schema)" }
        if !V.matches(id, V.layoutIdPattern) {
            return "id は英小文字・数字・- _ で始まりが英小文字か数字、64 文字以内にしてください"
        }
        let nameLength = name.unicodeScalars.count
        if nameLength < 1 || nameLength > V.nameMax { return "name は 1〜\(V.nameMax) 文字にしてください" }
        if rows.isEmpty || rows.count > V.rowsMax { return "rows は 1〜\(V.rowsMax) 行（\(rows.count)）" }
        let validTargets: Set<String>? = targets.map { Set(V.builtinLayoutIds + $0 + [id]) }
        var ids = Set<String>()
        var heightSum = 0.0
        for (index, row) in rows.enumerated() {
            let label = "行 \(index + 1)"
            if !(row.height > 0 && row.height <= V.rowHeightMax) { return "\(label) の高さが範囲外（\(row.height)）" }
            heightSum += row.height
            if row.keys.isEmpty { return "\(label) には 1 つ以上のキーが必要です" }
            if row.keys.count > V.keysPerRowMax { return "\(label) のキーは \(V.keysPerRowMax) 個以内にしてください" }
            var total = 0.0
            for key in row.keys {
                if let reason = V.keyError(key, validTargets: validTargets) { return reason }
                if !ids.insert(key.id).inserted { return "キー id の重複: \(key.id)" }
                total += key.width
            }
            if total < V.rowWidthMin || total > V.rowWidthMax { return "\(label) の幅の合計が範囲外（\(total)）" }
        }
        if heightSum > V.rowHeightSumMax {
            return "行の高さの比の合計は \(V.rowHeightSumMax) 以下にしてください（現在 \(heightSum)）"
        }
        return nil
    }
}

/// レイアウトの検証の規則（`web/src/lib/keyboard/types.ts` の LIMITS と同じ値。文字数はコードポイント数）
enum LayoutValidation {
    static let rowsMax = 8
    static let keysPerRowMax = 24
    static let rowWidthMin = 1.0
    static let rowWidthMax = 20.0
    static let rowHeightMax = 4.0
    /// 行の高さの比の合計の上限。キーボード全体の高さは「高さ比 4 = キー 4 行分」を前提に決めており、
    /// これを超えるとはみ出す（同梱のレイアウトは 4）。
    /// **web 側（`web/src/lib/keyboard/validate.ts` と `config/keyboard/layout.schema.json`）には未反映。同じ制約の追加が必要**
    static let rowHeightSumMax = 4.5
    static let widthMax = 20.0
    static let textMax = 16
    static let shortcutTextMax = 256
    static let cycleMax = 16
    static let stepsMax = 32
    static let longPressMax = 12
    static let nameMax = 64
    static let labelMax = 16
    static let iconMax = 64
    static let kanaMax = 4

    static let layoutIdPattern = "^[a-z0-9][a-z0-9_-]{0,63}\\z"
    static let keyIdPattern = "^[A-Za-z0-9][A-Za-z0-9_.-]{0,31}\\z"
    static let actionPattern = "^[a-zA-Z][a-zA-Z0-9]{0,31}\\z"

    /// `layout.target` に指定できる組み込み名（`kana` / `alpha` は割り当て済みのレイアウト）
    static let builtinLayoutIds = [
        "qwerty-kana", "qwerty-alpha", "flick-kana", "flick-alpha", "symbols", "numbers", "emoji", "kana", "alpha",
    ]

    /// `flick` の `kana` に指定できる 12 キー名（先頭の 1 文字で引く。`KanaKeyTable.entry(forName:)`）
    static let kanaKeyNames: Set<Character> = ["あ", "か", "さ", "た", "な", "は", "ま", "や", "ら", "わ", "、"]

    static func matches(_ text: String, _ pattern: String) -> Bool {
        text.range(of: pattern, options: .regularExpression) != nil
    }

    private static func length(_ text: String) -> Int { text.unicodeScalars.count }

    /// 文字列の長さ（コードポイント数）が min...max か
    private static func inRange(_ text: String?, _ min: Int, _ max: Int) -> Bool {
        guard let text = text else { return false }
        let n = length(text)
        return n >= min && n <= max
    }

    /// 1 つのキー（行内のキー）の検証
    static func keyError(_ key: LayoutKey, validTargets: Set<String>?) -> String? {
        if !matches(key.id, keyIdPattern) || !key.idGiven {
            return "キー「\(key.idGiven ? key.id : "(id なし)")」の id は英数字・_ . - で 32 文字以内にしてください"
        }
        let name = "キー \(key.id)"
        if !(key.width > 0 && key.width <= widthMax) { return "\(name) の幅は 0 より大きく \(Int(widthMax)) 以下にしてください" }
        if let label = key.label, length(label) > labelMax { return "\(name) の label は \(labelMax) 文字以内にしてください" }
        if let icon = key.icon, length(icon) > iconMax { return "\(name) の icon は \(iconMax) 文字以内にしてください" }
        switch key.kind {
        case .text:
            if !inRange(key.text, 1, textMax) { return "\(name) の text は 1〜\(textMax) 文字で指定してください" }
            if let shift = key.shift, !inRange(shift, 1, textMax) { return "\(name) の shift は 1〜\(textMax) 文字にしてください" }
        case .flick:
            if key.center == nil && key.kana == nil { return "\(name): フリックには center（中央の文字）か kana が必要です" }
            for (direction, cell) in [("center", key.center), ("up", key.up), ("down", key.down), ("left", key.left), ("right", key.right)] {
                if let cell = cell, !inRange(cell.text, 1, textMax) {
                    return "\(name) の \(direction) の text は 1〜\(textMax) 文字にしてください"
                }
            }
            if let kana = key.kana {
                if !inRange(kana, 1, kanaMax) { return "\(name) の kana は 1〜\(kanaMax) 文字にしてください" }
                if let first = kana.first, !kanaKeyNames.contains(first) { return "\(name) の kana「\(kana)」は未知のキー名です" }
            }
        case .toggle:
            let cycle = key.cycle ?? []
            if cycle.isEmpty || cycle.count > cycleMax { return "\(name) の cycle は 1〜\(cycleMax) 要素" }
            if cycle.contains(where: { !inRange($0, 1, textMax) }) {
                return "\(name) の cycle の要素は 1〜\(textMax) 文字にしてください"
            }
        case .action:
            if !matches(key.action ?? "", actionPattern) { return "\(name) の action は英字で始まる英数字（32 文字以内）にしてください" }
        case .shortcut:
            if let reason = stepsError(key.steps, owner: name) { return reason }
        case .layout:
            let target = key.target ?? ""
            if !matches(target, layoutIdPattern) { return "\(name) の target は切り替え先のレイアウト id にしてください" }
            if let valid = validTargets, !valid.contains(target) {
                return "\(name) の切り替え先「\(target)」は存在するレイアウトでも組み込み名でもありません"
            }
        case .spacer:
            break
        }
        if let popup = key.longPress {
            if popup.isEmpty || popup.count > longPressMax { return "\(name) の longPress は 1〜\(longPressMax) 個（\(popup.count)）" }
            for item in popup {
                if let reason = popupError(item, owner: name) { return reason }
            }
        }
        return nil
    }

    /// 長押しの 1 項目（kind は text / action / shortcut のみ。label 必須）
    private static func popupError(_ item: LayoutKey, owner: String) -> String? {
        if item.idGiven && !matches(item.id, keyIdPattern) { return "\(owner) の longPress の id が不正です" }
        if !inRange(item.label, 1, labelMax) { return "\(owner) の longPress の label は 1〜\(labelMax) 文字で指定してください" }
        if item.longPress != nil { return "\(owner) の longPress の中に longPress は置けません" }
        switch item.kind {
        case .text:
            if !inRange(item.text, 1, textMax) { return "\(owner) の longPress の text は 1〜\(textMax) 文字にしてください" }
        case .action:
            if !matches(item.action ?? "", actionPattern) { return "\(owner) の longPress の action が不正です" }
        case .shortcut:
            return stepsError(item.steps, owner: "\(owner) の longPress")
        default:
            return "\(owner) の longPress の kind は text / action / shortcut のいずれかにしてください"
        }
        return nil
    }

    private static func stepsError(_ steps: [ShortcutStep]?, owner: String) -> String? {
        guard let steps = steps, !steps.isEmpty, steps.count <= stepsMax else {
            return "\(owner) の steps は 1〜\(stepsMax) 個で指定してください"
        }
        for (index, step) in steps.enumerated() {
            if (step.action != nil) == (step.text != nil) {
                return "\(owner) の手順 \(index + 1) は action か text のどちらか一方だけを指定してください"
            }
            if let action = step.action, !matches(action, actionPattern) {
                return "\(owner) の手順 \(index + 1) の action が不正です"
            }
            if let text = step.text, !inRange(text, 1, shortcutTextMax) {
                return "\(owner) の手順 \(index + 1) の text は 1〜\(shortcutTextMax) 文字にしてください"
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

    /// JSON を decode し、仕様の範囲を検証する。
    /// - checkTargets: `layout.target` が存在するレイアウト（同梱・ユーザー・自身）か組み込み名かも確かめる
    ///   （取り込み時のみ true。実行時の読み込みでは、参照先が消えても他のキーは使えるよう確かめない）
    static func decode(_ data: Data, checkTargets: Bool = false) throws -> KeyboardLayoutDef {
        let def: KeyboardLayoutDef
        do {
            def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data)
        } catch {
            throw LoadError(message: describe(error))
        }
        let targets: [String]? = checkTargets ? (userLayoutIds() + bundledIds()) : nil
        if let reason = def.validationError(targets: targets) { throw LoadError(message: reason) }
        return def
    }

    /// decode の失敗を日本語の説明にする（ホストアプリの取り込み画面に出す）
    static func describe(_ error: Error) -> String {
        guard let decoding = error as? DecodingError else {
            return "読み取れません: \(error.localizedDescription)"
        }
        func path(_ keys: [CodingKey]) -> String {
            var result = ""
            for key in keys {
                if let index = key.intValue {
                    result += "[\(index)]"
                } else {
                    result += result.isEmpty ? key.stringValue : ".\(key.stringValue)"
                }
            }
            return result.isEmpty ? "先頭" : result
        }
        switch decoding {
        case .keyNotFound(let key, let context):
            return "必須の項目「\(key.stringValue)」がありません（場所: \(path(context.codingPath))）"
        case .typeMismatch(_, let context):
            return "値の型が違います（場所: \(path(context.codingPath))）"
        case .valueNotFound(_, let context):
            return "値がありません（場所: \(path(context.codingPath))）"
        case .dataCorrupted(let context):
            if context.codingPath.isEmpty { return "JSON の形式が正しくありません" }
            return "値が不正です（場所: \(path(context.codingPath))）"
        @unknown default:
            return "読み取れません"
        }
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

    /// 同梱の既定レイアウトの id 一覧（ホストアプリの設定画面用。バンドルの `presets/` フォルダ）
    static func bundledIds() -> [String] {
        guard let dir = Bundle.main.resourceURL?.appendingPathComponent(bundledFolder, isDirectory: true),
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
    static func isSafeId(_ id: String) -> Bool {
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

    /// 設定画面で明示的に選んだか（「既定に戻す」を出す条件）
    static func isExplicit(for mode: LayoutMode) -> Bool {
        let key = mode == .alpha ? alphaKey : kanaKey
        return defaults?.object(forKey: key) != nil
    }

    static func set(id: String?, for mode: LayoutMode) {
        let key = mode == .alpha ? alphaKey : kanaKey
        defaults?.set(id, forKey: key)
    }
}
