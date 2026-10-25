import Foundation

// クリップボード履歴（docs/ios-settings.md §クリップボード履歴、docs/decisions.md H-69）。
// UI・UIKit に依存しない純粋な部分（種類の判定・刈り込み・表示用の整形・取り込みの判定）と、
// App Group の `clipboard-history.json` への保存（`ClipboardHistoryStore`）。ホストアプリとキーボード拡張で共有する。
//
// プライバシー: 既定はオフ。内容は端末内の App Group にだけ置き（バックアップ対象から外す）、ログ（DiagLog）には出さない。

// MARK: - 種類

/// 履歴に記録する文字列の種類
enum ClipboardKind: String, Codable, CaseIterable, Equatable {
    /// その他の文字列
    case text
    case url
    /// 数字・記号のみ（電話番号・コードなど）
    case number
    case email

    /// 設定画面での表示名
    var title: String {
        switch self {
        case .text: return "文字列"
        case .url: return "URL"
        case .number: return "数字・記号"
        case .email: return "メールアドレス"
        }
    }

    /// カードに出す SF Symbols 名
    var systemImage: String {
        switch self {
        case .text: return "text.alignleft"
        case .url: return "link"
        case .number: return "number"
        case .email: return "envelope"
        }
    }

    // 知らない種類（新しい版が書いた値）は text として読む
    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = ClipboardKind(rawValue: raw) ?? .text
    }

    /// 前後の空白・改行を除いた全体で判定する（純関数）。
    /// URL は http / https で始まり空白を含まない、email は簡易パターン、number は数字と電話番号などの記号のみで数字を含む
    static func classify(_ text: String) -> ClipboardKind {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return .text }
        let hasWhitespace = trimmed.unicodeScalars.contains { CharacterSet.whitespacesAndNewlines.contains($0) }
        let lower = trimmed.lowercased()
        if !hasWhitespace {
            for scheme in ["http://", "https://"] where lower.hasPrefix(scheme) && lower.count > scheme.count {
                return .url
            }
            if trimmed.range(of: emailPattern, options: .regularExpression) == trimmed.startIndex..<trimmed.endIndex {
                return .email
            }
        }
        if isNumberLike(trimmed) { return .number }
        return .text
    }

    private static let emailPattern = "[A-Za-z0-9._%+\\-]+@[A-Za-z0-9\\-]+(\\.[A-Za-z0-9\\-]+)+"
    /// 数字と並べてよい記号（電話番号・郵便番号・金額・コードなど）
    private static let numberSymbols = Set("+-−－ー()（）.,．，/／:#*%％$¥￥ \u{3000}")

    private static func isNumberLike(_ text: String) -> Bool {
        var hasDigit = false
        for scalar in text.unicodeScalars {
            if scalar.properties.numericType == .decimal {
                hasDigit = true
            } else if !numberSymbols.contains(Character(scalar)) {
                return false
            }
        }
        return hasDigit
    }
}

// MARK: - 項目

struct ClipboardItem: Codable, Equatable, Identifiable {
    /// UUID 文字列
    var id: String
    var text: String
    var kind: ClipboardKind
    var copiedAt: Date
    var pinned: Bool

    init(id: String = UUID().uuidString, text: String, kind: ClipboardKind, copiedAt: Date, pinned: Bool = false) {
        self.id = id
        self.text = text
        self.kind = kind
        self.copiedAt = copiedAt
        self.pinned = pinned
    }
}

// MARK: - 設定値

/// 履歴の設定を、秒・件数・種類の集合にしたもの（`NagiIOSSettings.clipboardHistory`）
struct ClipboardHistoryConfig: Equatable {
    var enabled: Bool
    /// 保存期間（秒）。nil は無期限
    var retention: TimeInterval?
    var maxItems: Int
    var kinds: Set<ClipboardKind>
    /// カードに出す最大文字数
    var previewChars: Int

    /// 設定値（選択肢の value 文字列）から作る。知らない値は既定
    init(enabled: Bool, retentionValue: String, maxItemsValue: String, kindValues: [String], previewCharsValue: String) {
        self.enabled = enabled
        self.retention = ClipboardHistoryLogic.retentionSeconds(for: retentionValue)
        self.maxItems = Int(maxItemsValue) ?? 50
        self.kinds = Set(kindValues.compactMap { ClipboardKind(rawValue: $0) })
        self.previewChars = Int(previewCharsValue) ?? 40
    }

    init(enabled: Bool, retention: TimeInterval?, maxItems: Int, kinds: Set<ClipboardKind>, previewChars: Int) {
        self.enabled = enabled
        self.retention = retention
        self.maxItems = maxItems
        self.kinds = kinds
        self.previewChars = previewChars
    }
}

extension NagiIOSSettings {
    var clipboardHistory: ClipboardHistoryConfig {
        ClipboardHistoryConfig(
            enabled: bool(Key.clipboardHistoryEnabled),
            retentionValue: string(Key.clipboardHistoryRetention),
            maxItemsValue: string(Key.clipboardHistoryMaxItems),
            kindValues: stringList(Key.clipboardHistoryTypes),
            previewCharsValue: string(Key.clipboardHistoryPreviewChars))
    }
}

// MARK: - 純粋なロジック

/// 履歴の刈り込み・並び・表示用の整形（時刻は引数で受ける）
enum ClipboardHistoryLogic {
    /// 1 件の最大文字数（超えたものは記録しない）
    static let maxTextLength = 10_000
    /// ピン留めの上限
    static let maxPinned = 20

    /// 保存期間の選択肢の値 → 秒。`forever` は nil（無期限）。知らない値は 24 時間
    static func retentionSeconds(for value: String) -> TimeInterval? {
        switch value {
        case "1h": return 3600
        case "24h": return 24 * 3600
        case "7d": return 7 * 24 * 3600
        case "30d": return 30 * 24 * 3600
        case "forever": return nil
        default: return 24 * 3600
        }
    }

    /// 期限切れ（ピン留め以外）と、保存件数を超えた分（ピン留め以外の古い順）を除く。残りの並びは保つ
    static func prune(_ items: [ClipboardItem], now: Date, config: ClipboardHistoryConfig) -> [ClipboardItem] {
        var alive = items.filter { item in
            guard !item.pinned, let retention = config.retention else { return true }
            return now.timeIntervalSince(item.copiedAt) <= retention
        }
        let limit = max(config.maxItems, 0)
        let unpinned = alive.filter { !$0.pinned }
        if unpinned.count > limit {
            let keep = Set(unpinned.sorted { $0.copiedAt > $1.copiedAt }.prefix(limit).map { $0.id })
            alive = alive.filter { $0.pinned || keep.contains($0.id) }
        }
        return alive
    }

    /// 記録できる文字列か（空白のみ・長すぎるもの・許可されていない種類は不可）。可能なら種類を返す
    static func acceptedKind(for text: String, config: ClipboardHistoryConfig) -> ClipboardKind? {
        guard config.enabled, text.count <= maxTextLength,
              !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        let kind = ClipboardKind.classify(text)
        return config.kinds.contains(kind) ? kind : nil
    }

    /// 1 件記録した結果。同一文字列は先頭へ移して日時を更新する（ID・ピン留めは保つ）。そのあと刈り込む
    static func recorded(
        _ items: [ClipboardItem], text: String, now: Date, config: ClipboardHistoryConfig
    ) -> [ClipboardItem] {
        guard let kind = acceptedKind(for: text, config: config) else { return items }
        var result = items
        var item = ClipboardItem(text: text, kind: kind, copiedAt: now)
        if let index = result.firstIndex(where: { $0.text == text }) {
            item.id = result[index].id
            item.pinned = result[index].pinned
            result.remove(at: index)
        }
        result.insert(item, at: 0)
        return prune(result, now: now, config: config)
    }

    /// 表示順: ピン留めが先頭、続けて新しい順（同時刻は元の並びを保つ）
    static func ordered(_ items: [ClipboardItem]) -> [ClipboardItem] {
        let indexed = items.enumerated().sorted { lhs, rhs in
            if lhs.element.pinned != rhs.element.pinned { return lhs.element.pinned }
            if lhs.element.copiedAt != rhs.element.copiedAt { return lhs.element.copiedAt > rhs.element.copiedAt }
            return lhs.offset < rhs.offset
        }
        return indexed.map { $0.element }
    }

    /// カードの本文。`limit` 文字を超えた分は「…」で省略する
    static func preview(_ text: String, limit: Int) -> String {
        let limit = max(limit, 1)
        guard text.count > limit else { return text }
        return String(text.prefix(limit)) + "…"
    }

    /// 相対時刻（「3 分前」など）。未来の時刻（時計のずれ）は「たった今」
    static func relativeTime(from date: Date, now: Date) -> String {
        let seconds = Int(now.timeIntervalSince(date))
        if seconds < 60 { return "たった今" }
        let minutes = seconds / 60
        if minutes < 60 { return "\(minutes) 分前" }
        let hours = minutes / 60
        if hours < 24 { return "\(hours) 時間前" }
        return "\(hours / 24) 日前"
    }
}

/// 履歴ページの表示内容の種別
enum ClipboardPageState: Equatable {
    /// 設定でオフ
    case disabled
    /// パスワード欄（内容は出さない）
    case secure
    case empty
    case items

    static func state(enabled: Bool, secure: Bool, itemCount: Int) -> ClipboardPageState {
        if !enabled { return .disabled }
        if secure { return .secure }
        return itemCount == 0 ? .empty : .items
    }

    var message: String? {
        switch self {
        case .disabled: return "設定でクリップボード履歴をオンにしてください"
        case .secure: return "パスワード欄では履歴は使えません"
        case .empty: return "履歴はありません"
        case .items: return nil
        }
    }
}

/// クリップボードを読むかの判定（読む前の確認だけで決める。中身は読まない）
enum ClipboardCaptureLogic {
    /// - lastChangeCount: 前回記録した `UIPasteboard.general.changeCount`（ホストとキーボードで共有。無ければ nil）
    static func shouldRead(
        enabled: Bool, hasFullAccess: Bool, isSecure: Bool, currentChangeCount: Int, lastChangeCount: Int?
    ) -> Bool {
        guard enabled, hasFullAccess, !isSecure else { return false }
        return currentChangeCount != lastChangeCount
    }
}

// MARK: - 保存

/// `clipboard-history.json` の中身
private struct ClipboardHistoryFile: Codable {
    static let currentSchema = 1
    var schema = ClipboardHistoryFile.currentSchema
    var items: [ClipboardItem] = []
}

/// App Group の `clipboard-history.json` への保存。すべての更新は「ロックを取る → 読み直す → 変更 → 原子的に書く」で、
/// ホストアプリとキーボード拡張が同時に書いても、他方の更新を消さない。
/// 保存先（App Group）が無ければ何もしない（App Group 以外には置かない）
final class ClipboardHistoryStore {
    static let fileName = "clipboard-history.json"

    private let fileURL: URL?

    /// - fileURL: 保存先。既定は App Group コンテナ直下（無ければ nil で、何も保存しない）
    init(fileURL: URL? = ClipboardHistoryStore.defaultFileURL) {
        self.fileURL = fileURL
    }

    static var defaultFileURL: URL? {
        SharedStore.shared.containerURL?.appendingPathComponent(fileName)
    }

    /// 保存先があるか
    var isAvailable: Bool { fileURL != nil }

    // MARK: 読み書き

    private static func load(_ url: URL) -> ClipboardHistoryFile {
        guard let data = try? Data(contentsOf: url) else { return ClipboardHistoryFile() }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .secondsSince1970
        // 壊れたファイルは空として扱う
        return (try? decoder.decode(ClipboardHistoryFile.self, from: data)) ?? ClipboardHistoryFile()
    }

    /// 読み直して `change` を適用し、変わっていれば書く。`change` が nil を返したときは書かない。戻り値は書いた（変わった）か
    @discardableResult
    private func mutate(_ change: (inout [ClipboardItem]) -> Bool) -> Bool {
        guard let url = fileURL else { return false }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        var changed = false
        func apply() -> Bool {
            var file = Self.load(url)
            var items = file.items
            guard change(&items) else { return true }
            file.items = items
            file.schema = max(file.schema, ClipboardHistoryFile.currentSchema)
            changed = Self.write(file, to: url)
            return true
        }
        var ran = false
        let lockPath = url.path + ".lock"
        _ = NagiFileLock.withExclusive(path: lockPath) {
            ran = true
            return apply()
        }
        // ロックを取れなかったときも、データを失わないよう読み直して書く（最悪でも直前の更新との競合だけ）
        if !ran { _ = apply() }
        return changed
    }

    private static func write(_ file: ClipboardHistoryFile, to url: URL) -> Bool {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .secondsSince1970
        guard let data = try? encoder.encode(file) else { return false }
        do {
            try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        } catch {
            return false
        }
        // iCloud などのバックアップに出さない（置き換えのたびに付け直す）
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        var target = url
        try? target.setResourceValues(values)
        return true
    }

    // MARK: 操作

    /// 記録する。記録できない文字列（空白のみ・許可されていない種類・長すぎる）は無視して false
    @discardableResult
    func record(text: String, now: Date, settings: ClipboardHistoryConfig) -> Bool {
        guard ClipboardHistoryLogic.acceptedKind(for: text, config: settings) != nil else { return false }
        return mutate { items in
            items = ClipboardHistoryLogic.recorded(items, text: text, now: now, config: settings)
            return true
        }
    }

    /// 期限切れと保存件数の超過を消す（変わったときだけ書く）
    func prune(now: Date, settings: ClipboardHistoryConfig) {
        mutate { items in
            let pruned = ClipboardHistoryLogic.prune(items, now: now, config: settings)
            guard pruned != items else { return false }
            items = pruned
            return true
        }
    }

    /// ピン留めの切り替え。ピン留めは `maxPinned` 件まで（超えるときは何もせず false）
    @discardableResult
    func togglePin(id: String) -> Bool {
        var result = false
        mutate { items in
            guard let index = items.firstIndex(where: { $0.id == id }) else { return false }
            if !items[index].pinned && items.filter({ $0.pinned }).count >= ClipboardHistoryLogic.maxPinned {
                return false
            }
            items[index].pinned.toggle()
            result = true
            return true
        }
        return result
    }

    func delete(id: String) {
        mutate { items in
            let before = items.count
            items.removeAll { $0.id == id }
            return items.count != before
        }
    }

    /// 履歴をすべて消す（ピン留めも）
    func clear() {
        mutate { items in
            guard !items.isEmpty else { return false }
            items = []
            return true
        }
    }

    /// 表示順の項目（ピン留めが先頭、続けて新しい順）。`settings` を渡すと期限切れを除く（ファイルは書き換えない）
    func items(now: Date, settings: ClipboardHistoryConfig? = nil) -> [ClipboardItem] {
        guard let url = fileURL else { return [] }
        var items = Self.load(url).items
        if let settings = settings { items = ClipboardHistoryLogic.prune(items, now: now, config: settings) }
        return ClipboardHistoryLogic.ordered(items)
    }
}
