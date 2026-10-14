import SwiftUI

// 設定タブ（docs/ios-settings.md）。コアの設定項目（`get_settings` の記述子）と iOS 固有の項目
// （`Shared/IOSSettings.swift`）を、同じ見た目の汎用フォームで出す。項目ごとの知識（範囲・既定値・検証）は
// コアと `NagiIOSSettings.definitions` にあり、ここでは記述子から行を組み立てて描くだけにする。

/// 設定画面の表示モード。切り替えても設定値は変わらない（表示する項目が変わるだけ）
enum NagiUIMode: String {
    case simple
    case detail
}

/// 設定画面の 1 行（コアの項目・iOS 固有の項目・レイアウトの割り当て・診断ログのいずれか）
struct NagiSettingRow: Identifiable {
    enum Origin {
        /// コアの `config.toml`（`update_settings`）
        case core
        /// `ios-settings.json`
        case ios
        /// かな・英字モードのレイアウトの割り当て（`LayoutAssignment`）
        case layout
        /// 診断ログの記録（`DiagLog`）
        case diag
    }

    struct Option {
        let value: String
        let label: String
    }

    enum Control {
        case toggle
        case intStepper(min: Int, max: Int, step: Int)
        case floatStepper(min: Double, max: Double, step: Double)
        case choice([Option])
        case text
    }

    let key: String
    let origin: Origin
    let group: String
    let label: String
    let description: String
    let control: Control
    /// true ならシンプルモードにも出す
    let isSimple: Bool
    /// 反映にキーボードの開き直し（エンジンの再起動）が要る
    let requiresRestart: Bool

    var id: String { key }
}

struct NagiSettingSection: Identifiable {
    let title: String
    let rows: [NagiSettingRow]

    var id: String { title }
}

/// 設定画面の状態。コアの呼び出しはすべて専用の直列キューで行い、結果をメインキューで反映する
/// （`NagiEngine` / `NagiSession` はスレッドを跨いで同時に触れない）
final class NagiSettingsModel: ObservableObject {
    private static let uiModeKey = "ui.mode"
    private static let learningGroup = "学習"
    /// セクションの並び（無い名前は最後に、出てきた順で並ぶ）
    private static let groupOrder = ["入力", "変換", "動作モード", "予測変換", "学習", "キーボード", "診断", "アップデート"]
    /// コアの項目のうち、iOS では出さないもの。`conversion.model` はモデルのファイルのパスで、
    /// キーボードが配置済みのモデルから自動で決める（手で書き換えると読み込めなくなる）
    private static let hiddenCoreKeys: Set<String> = ["conversion.model"]

    private static var defaults: UserDefaults? {
        UserDefaults(suiteName: SharedStore.shared.appGroupId)
    }

    /// 表示モード（App Group の UserDefaults `ui.mode`。既定はシンプル）
    @Published var mode: NagiUIMode {
        didSet { Self.defaults?.set(mode.rawValue, forKey: Self.uiModeKey) }
    }
    @Published private(set) var coreItems: [SettingItem] = []
    @Published private(set) var coreLoaded = false
    /// コアに接続できない・保存できないなどの理由
    @Published private(set) var coreMessage: String?
    /// 設定ファイルが壊れている等で値を読めなかった理由（このとき変更は保存されない）
    @Published private(set) var snapshotError: String?
    @Published private(set) var fieldErrors: [String: String] = [:]
    @Published private(set) var ios = NagiIOSSettings()
    @Published private(set) var layoutRows: [NagiSettingRow] = []
    @Published private(set) var diagEnabled = DiagLog.isEnabled
    @Published private(set) var iosSaveError: String?
    @Published private(set) var learningNotice: String?

    private let coreQueue = DispatchQueue(label: "com.shiki.nagi.settings.core")
    /// 以下 2 つは coreQueue の上でだけ触る
    private var session: NagiSession?
    private var userDirPath: String?
    /// 送信中の update_settings の数（メインキューでだけ触る）。0 になったときだけ最新の状態で上書きする
    private var inFlight = 0

    init() {
        let raw = Self.defaults?.string(forKey: Self.uiModeKey) ?? ""
        mode = NagiUIMode(rawValue: raw) ?? .simple
    }

    // MARK: - 読み込み

    /// 画面を開いたときに呼ぶ。キーボードや他の操作で変わった値を読み直す
    func start() {
        ios = NagiIOSSettingsStore.load()
        diagEnabled = DiagLog.isEnabled
        layoutRows = buildLayoutRows()
        reloadCore()
    }

    func reloadCore() {
        coreQueue.async { [weak self] in
            guard let self = self else { return }
            let opened = self.ensureSession()
            let snapshot = opened.session?.getSettings()
            DispatchQueue.main.async {
                self.coreLoaded = true
                if let snapshot = snapshot {
                    self.coreItems = snapshot.items
                    self.snapshotError = snapshot.error
                    self.coreMessage = nil
                } else {
                    self.coreMessage = opened.message ?? "コアから設定を読めませんでした"
                }
            }
        }
    }

    /// 設定の読み書き用のコア（変換モデルは読み込まない）を用意する。キーボード拡張と同じ `<App Group>/user`
    /// を使うので、書いた `config.toml` を拡張が読む。coreQueue の上で呼ぶ
    private func ensureSession() -> (session: NagiSession?, message: String?) {
        if let session = session { return (session, nil) }
        let store = SharedStore.shared
        let prepared = ProductSession.prepareUserDir(
            store: store,
            modelPath: ProductSession.preferredModelPath(store: store)
        )
        guard let userDir = prepared.path else {
            return (nil, "設定の保存先を作れません（\(prepared.detail)）")
        }
        userDirPath = userDir
        // 辞書が無くても設定は読み書きできる（コアは辞書が無ければ読みそのまま返す縮退で起動する）
        let dataDir = store.dataDir().path ?? (userDir as NSString).appendingPathComponent("data")
        guard let engine = NagiEngine(dataDir: dataDir, userDir: userDir, noLLM: true),
              let created = NagiSession(engine: engine, sessionId: "settings") else {
            return (nil, "設定を扱うエンジンを起動できませんでした")
        }
        session = created
        return (created, nil)
    }

    // MARK: - 表示する行

    var sections: [NagiSettingSection] {
        let all = (coreRows() + iosRows() + layoutRows + [diagRow()])
            .filter { mode == .detail || $0.isSimple }
        var titles: [String] = []
        for row in all where !titles.contains(row.group) {
            titles.append(row.group)
        }
        // 学習データの削除ボタンの置き場として、項目が無くても学習セクションは出す
        if !titles.contains(Self.learningGroup) { titles.append(Self.learningGroup) }
        let ranked = titles.enumerated()
            .map { (offset, title) -> (Int, Int, String) in (Self.groupOrder.firstIndex(of: title) ?? Self.groupOrder.count, offset, title) }
            .sorted { ($0.0, $0.1) < ($1.0, $1.1) }
        return ranked.map { entry in
            NagiSettingSection(title: entry.2, rows: all.filter { $0.group == entry.2 })
        }
    }

    private func coreRows() -> [NagiSettingRow] {
        coreItems.compactMap { item -> NagiSettingRow? in
            if item.platforms == .desktop || Self.hiddenCoreKeys.contains(item.key) { return nil }
            let control: NagiSettingRow.Control
            switch item.kind {
            case .bool:
                control = .toggle
            case .int(let range):
                control = .intStepper(min: Int(range.min), max: Int(range.max), step: max(Int(range.step), 1))
            case .float(let range):
                control = .floatStepper(min: range.min, max: range.max, step: range.step > 0 ? range.step : 0.1)
            case .choice(let choices):
                control = .choice(choices.options.map { NagiSettingRow.Option(value: $0.value, label: $0.label) })
            case .text:
                control = .text
            }
            return NagiSettingRow(
                key: item.key, origin: .core, group: item.group, label: item.label,
                description: item.description, control: control,
                isSimple: item.level == .simple, requiresRestart: item.requires_restart)
        }
    }

    private func iosRows() -> [NagiSettingRow] {
        NagiIOSSettings.definitions.map { def -> NagiSettingRow in
            let control: NagiSettingRow.Control
            switch def.kind {
            case .bool:
                control = .toggle
            case .int(let lo, let hi, let step):
                control = .intStepper(min: lo, max: hi, step: step)
            case .float(let lo, let hi, let step):
                control = .floatStepper(min: lo, max: hi, step: step)
            case .choice(let choices):
                control = .choice(choices.map { NagiSettingRow.Option(value: $0.value, label: $0.label) })
            }
            return NagiSettingRow(
                key: def.key, origin: .ios, group: def.group, label: def.label,
                description: def.description, control: control,
                isSimple: def.isSimple, requiresRestart: false)
        }
    }

    private func diagRow() -> NagiSettingRow {
        NagiSettingRow(
            key: "diag.log_enabled", origin: .diag, group: "診断", label: "診断ログを記録",
            description: "キーボードの動作を記録します。入力した文字を含むので、TestFlight での確認期間だけ使ってください。内容は「入力テスト」タブで見られます。",
            control: .toggle, isSimple: false, requiresRestart: false)
    }

    /// かな・英字モードのレイアウトの選択肢（同梱の既定とユーザーのレイアウトから、そのモードのもの）
    private func buildLayoutRows() -> [NagiSettingRow] {
        func row(key: String, label: String, mode: LayoutMode, fallbackId: String) -> NagiSettingRow {
            var options: [NagiSettingRow.Option] = []
            var seen = Set<String>()
            for id in LayoutLoader.bundledIds() + LayoutLoader.userLayoutIds() where !seen.contains(id) {
                seen.insert(id)
                guard let def = LayoutLoader.load(id: id), def.mode == mode else { continue }
                options.append(NagiSettingRow.Option(value: id, label: def.name == id ? id : "\(def.name)（\(id)）"))
            }
            // 同梱の読み込みに失敗しても、既定と現在の割り当ては選べるようにする
            for id in [fallbackId, LayoutAssignment.id(for: mode)] where !options.contains(where: { $0.value == id }) {
                options.append(NagiSettingRow.Option(value: id, label: id))
            }
            return NagiSettingRow(
                key: key, origin: .layout, group: "キーボード", label: label,
                description: "使うレイアウトです。ローマ字入力にするには、かなに qwerty-kana を選びます。",
                control: .choice(options), isSimple: true, requiresRestart: false)
        }
        return [
            row(key: "keyboard.layout_kana", label: "かなモードのレイアウト", mode: .kana, fallbackId: LayoutLoader.defaultKanaId),
            row(key: "keyboard.layout_alpha", label: "英字モードのレイアウト", mode: .alpha, fallbackId: LayoutLoader.defaultAlphaId),
        ]
    }

    // MARK: - 値の読み出し

    private func coreItem(_ key: String) -> SettingItem? {
        coreItems.first { $0.key == key }
    }

    private func layoutMode(_ row: NagiSettingRow) -> LayoutMode {
        row.key == "keyboard.layout_alpha" ? .alpha : .kana
    }

    func boolValue(_ row: NagiSettingRow) -> Bool {
        switch row.origin {
        case .core:
            if case .bool(let value)? = coreItem(row.key)?.value { return value }
            return false
        case .ios: return ios.bool(row.key)
        case .layout: return false
        case .diag: return diagEnabled
        }
    }

    func intValue(_ row: NagiSettingRow) -> Int {
        guard case .intStepper(let lo, _, _) = row.control else { return 0 }
        switch row.origin {
        case .core:
            if case .int(let value)? = coreItem(row.key)?.value { return Int(value) }
            return lo
        default: return ios.int(row.key)
        }
    }

    func floatValue(_ row: NagiSettingRow) -> Double {
        guard case .floatStepper(let lo, _, _) = row.control else { return 0 }
        switch row.origin {
        case .core:
            if case .float(let value)? = coreItem(row.key)?.value { return value }
            return lo
        default: return ios.double(row.key)
        }
    }

    func choiceValue(_ row: NagiSettingRow) -> String {
        switch row.origin {
        case .core:
            if case .text(let value)? = coreItem(row.key)?.value { return value }
            return ""
        case .ios: return ios.string(row.key)
        case .layout: return LayoutAssignment.id(for: layoutMode(row))
        case .diag: return ""
        }
    }

    func textValue(_ row: NagiSettingRow) -> String {
        if row.origin == .core, case .text(let value)? = coreItem(row.key)?.value { return value }
        return ""
    }

    /// 数値の表示。コアの項目で「未指定でプロファイルが決める」ものは「自動」と出す
    func valueLabel(_ row: NagiSettingRow) -> String {
        if row.origin == .core, let item = coreItem(row.key), item.value == nil { return "自動" }
        switch row.control {
        case .intStepper: return "\(intValue(row))"
        case .floatStepper: return String(format: "%g", floatValue(row))
        default: return ""
        }
    }

    func canReset(_ row: NagiSettingRow) -> Bool {
        switch row.origin {
        case .core: return coreItem(row.key)?.explicit ?? false
        case .ios: return ios.isExplicit(row.key)
        case .layout: return LayoutAssignment.isExplicit(for: layoutMode(row))
        case .diag: return Self.defaults?.object(forKey: DiagLog.enabledKey) != nil
        }
    }

    func error(for row: NagiSettingRow) -> String? {
        row.origin == .core ? fieldErrors[row.key] : nil
    }

    // MARK: - 編集

    func setBool(_ row: NagiSettingRow, _ value: Bool) {
        switch row.origin {
        case .core: editCore(row.key, .bool(value))
        case .ios: editIOS(row.key, .bool(value))
        case .layout: break
        case .diag:
            DiagLog.isEnabled = value
            diagEnabled = value
        }
    }

    func setInt(_ row: NagiSettingRow, _ value: Int) {
        if row.origin == .core {
            editCore(row.key, .int(Int32(clamping: value)))
        } else {
            editIOS(row.key, .number(Double(value)))
        }
    }

    func setFloat(_ row: NagiSettingRow, _ value: Double) {
        if row.origin == .core {
            editCore(row.key, .float(value))
        } else {
            editIOS(row.key, .number(value))
        }
    }

    func setChoice(_ row: NagiSettingRow, _ value: String) {
        switch row.origin {
        case .core: editCore(row.key, .text(value))
        case .ios: editIOS(row.key, .string(value))
        case .layout:
            LayoutAssignment.set(id: value, for: layoutMode(row))
            objectWillChange.send()
        case .diag: break
        }
    }

    func setText(_ row: NagiSettingRow, _ value: String) {
        if row.origin == .core { editCore(row.key, .text(value)) }
    }

    /// 「既定に戻す」
    func reset(_ row: NagiSettingRow) {
        switch row.origin {
        case .core: editCore(row.key, nil)
        case .ios: editIOS(row.key, nil)
        case .layout:
            LayoutAssignment.set(id: nil, for: layoutMode(row))
            objectWillChange.send()
        case .diag:
            Self.defaults?.removeObject(forKey: DiagLog.enabledKey)
            diagEnabled = DiagLog.isEnabled
        }
    }

    /// iOS 固有の設定を保存する。保存の直前に読み直し、知らないキーも含めて書き戻す
    private func editIOS(_ key: String, _ value: NagiJSONValue?) {
        var latest = NagiIOSSettingsStore.load()
        latest.set(key, value)
        do {
            try NagiIOSSettingsStore.save(latest)
            iosSaveError = nil
        } catch {
            iosSaveError = "設定を保存できませんでした: \(error.localizedDescription)"
        }
        ios = latest
    }

    /// コアの設定を書く（`update_settings`）。画面にはすぐ反映し、結果で確定（範囲外などは項目の下に出す）
    private func editCore(_ key: String, _ value: SettingValue?) {
        fieldErrors[key] = nil
        coreItems = coreItems.map { item -> SettingItem in
            guard item.key == key else { return item }
            return SettingItem(
                key: item.key, group: item.group, label: item.label, description: item.description,
                kind: item.kind, value: value ?? item.default, default: item.default,
                explicit: value != nil, requires_restart: item.requires_restart,
                level: item.level, platforms: item.platforms)
        }
        inFlight += 1
        let changes = [SettingChange(key: key, value: value)]
        coreQueue.async { [weak self] in
            guard let self = self else { return }
            let opened = self.ensureSession()
            let result = opened.session?.updateSettings(changes)
            DispatchQueue.main.async {
                self.inFlight -= 1
                guard let result = result else {
                    self.coreMessage = opened.message ?? "設定を保存できませんでした"
                    self.reloadCore()
                    return
                }
                self.coreMessage = nil
                for error in result.errors {
                    self.fieldErrors[error.key] = error.message
                }
                // 続けて操作した場合は、最後の応答のときだけ最新の状態で置き換える
                if self.inFlight == 0 {
                    self.coreItems = result.snapshot.items
                    self.snapshotError = result.snapshot.error
                }
            }
        }
    }

    // MARK: - 学習データの削除

    /// 学習データをすべて削除する。キーボード拡張がメモリ上に持つ学習データも消えるよう、
    /// 合図（`NagiLearningClearSignal`）を出して次の focusIn で `clear_learning` を呼ばせる
    func clearLearning() {
        learningNotice = nil
        coreQueue.async { [weak self] in
            guard let self = self else { return }
            let opened = self.ensureSession()
            var ok = false
            if let session = opened.session, session.clearLearning() != nil {
                ok = true
            } else if let dir = self.userDirPath {
                // コアに繋がらないときは、学習ファイルを直接消す
                let path = (dir as NSString).appendingPathComponent("learning.tsv")
                if FileManager.default.fileExists(atPath: path) {
                    ok = (try? FileManager.default.removeItem(atPath: path)) != nil
                } else {
                    ok = true
                }
            }
            if ok { NagiLearningClearSignal.raise() }
            DispatchQueue.main.async {
                self.learningNotice = ok ? "学習データを削除しました" : "学習データを削除できませんでした"
            }
        }
    }
}

// MARK: - 画面

struct SettingsView: View {
    @StateObject private var model = NagiSettingsModel()
    @State private var confirmingClear = false

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Picker("表示", selection: $model.mode) {
                        Text("シンプル").tag(NagiUIMode.simple)
                        Text("詳細").tag(NagiUIMode.detail)
                    }
                    .pickerStyle(.segmented)
                } footer: {
                    Text("詳細にすると項目が増えます。切り替えても設定値は変わりません。")
                }

                if SharedStore.shared.containerURL == nil {
                    banner("App Group を使えないため、ここで変えた設定はキーボードに届きません。", color: .orange)
                }
                if let message = model.coreMessage {
                    banner(message, color: .red)
                }
                if let error = model.snapshotError {
                    banner("設定ファイルを読み込めませんでした（既定値を表示中・変更は保存されません）: \(error)", color: .orange)
                }
                if let error = model.iosSaveError {
                    banner(error, color: .red)
                }

                ForEach(model.sections) { section in
                    Section(header: Text(section.title)) {
                        ForEach(section.rows) { row in
                            NagiSettingRowView(model: model, row: row)
                        }
                        if section.title == "学習" {
                            Button("学習データをすべて削除", role: .destructive) {
                                confirmingClear = true
                            }
                            .buttonStyle(.borderless)
                            if let notice = model.learningNotice {
                                Text(notice).font(.caption).foregroundColor(.secondary)
                            }
                        }
                    }
                }

                Section {
                    Text("変更はキーボードを次に開いたときに反映されます。「キーボードを開き直すと反映」と出ている項目は、いったんキーボードを閉じて開き直してください。")
                        .font(.footnote)
                        .foregroundColor(.secondary)
                }
            }
            .navigationTitle("設定")
            .onAppear { model.start() }
            .confirmationDialog(
                "学習データをすべて削除しますか？",
                isPresented: $confirmingClear,
                titleVisibility: .visible
            ) {
                Button("削除", role: .destructive) { model.clearLearning() }
                Button("キャンセル", role: .cancel) {}
            } message: {
                Text("確定した変換結果の記録がすべて消えます。元に戻せません。")
            }
        }
    }

    private func banner(_ text: String, color: Color) -> some View {
        Text(text)
            .font(.footnote)
            .foregroundColor(color)
    }
}

/// 設定項目 1 行。型ごとに Toggle / Stepper / Picker / TextField で描く
private struct NagiSettingRowView: View {
    @ObservedObject var model: NagiSettingsModel
    let row: NagiSettingRow

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            control
            Text(row.description)
                .font(.caption)
                .foregroundColor(.secondary)
            if row.requiresRestart {
                Text("キーボードを開き直すと反映されます")
                    .font(.caption)
                    .foregroundColor(.orange)
            }
            if let message = model.error(for: row) {
                Text(message)
                    .font(.caption)
                    .foregroundColor(.red)
            }
            if model.canReset(row) {
                Button("既定に戻す") { model.reset(row) }
                    .buttonStyle(.borderless)
                    .font(.caption)
            }
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var control: some View {
        switch row.control {
        case .toggle:
            Toggle(row.label, isOn: Binding(
                get: { model.boolValue(row) },
                set: { model.setBool(row, $0) }
            ))
        case .intStepper(let lo, let hi, let step):
            HStack {
                Text(row.label)
                Spacer()
                Text(model.valueLabel(row)).foregroundColor(.secondary)
                Stepper("", value: Binding(
                    get: { model.intValue(row) },
                    set: { model.setInt(row, $0) }
                ), in: lo...max(hi, lo), step: step)
                .labelsHidden()
            }
        case .floatStepper(let lo, let hi, let step):
            HStack {
                Text(row.label)
                Spacer()
                Text(model.valueLabel(row)).foregroundColor(.secondary)
                Stepper("", value: Binding(
                    get: { model.floatValue(row) },
                    set: { model.setFloat(row, $0) }
                ), in: lo...max(hi, lo), step: step)
                .labelsHidden()
            }
        case .choice(let options):
            Picker(row.label, selection: Binding(
                get: { model.choiceValue(row) },
                set: { model.setChoice(row, $0) }
            )) {
                ForEach(options, id: \.value) { option in
                    Text(option.label).tag(option.value)
                }
            }
            .pickerStyle(.menu)
        case .text:
            NagiTextSettingRow(model: model, row: row)
        }
    }
}

/// 文字列の項目。入力中は手元の下書きに持ち、確定（Return）で書き込む（1 文字ごとに保存しない）
private struct NagiTextSettingRow: View {
    @ObservedObject var model: NagiSettingsModel
    let row: NagiSettingRow
    @State private var draft = ""

    var body: some View {
        HStack {
            Text(row.label)
            TextField("", text: $draft)
                .multilineTextAlignment(.trailing)
                .onSubmit { model.setText(row, draft) }
        }
        .onAppear { draft = model.textValue(row) }
    }
}

#Preview {
    SettingsView()
}
