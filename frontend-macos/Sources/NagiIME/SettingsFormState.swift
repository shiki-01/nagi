import Foundation
import NagiClient

/// 設定フォームの状態（AppKit / SwiftUI 非依存）。コアが返す `SettingItem` の記述子から汎用フォームを組み立てる。
/// 項目ごとの知識（範囲・既定値・検証）はコアにあり、ここでは表示用の整理・クランプ・差分の生成だけを行う。
public struct SettingsFormState {
    /// 同じ `group` の項目のまとまり（コアが返す表示順を保つ）
    public struct Group {
        public let title: String
        public let items: [SettingItem]
    }

    /// 未送信・未確定の編集値
    private enum Draft {
        case value(SettingValue)
        /// 明示指定を削除して既定に戻す（値は既定値の表示に切り替わる）
        case reset
    }

    public private(set) var items: [SettingItem] = []
    /// 設定ファイルの構文エラー等（`SettingsSnapshot.error`）。警告バナー用
    public private(set) var snapshotError: String?
    /// 項目別のエラー（`UpdateSettingsResult.errors`）
    public private(set) var fieldErrors: [String: String] = [:]

    private var drafts: [String: Draft] = [:]
    /// 送信待ちの変更（同じキーは後勝ち）
    private var staged: [SettingChange] = []

    public init() {}

    public init(snapshot: SettingsSnapshot) {
        apply(snapshot: snapshot)
    }

    // MARK: 反映

    public mutating func apply(snapshot: SettingsSnapshot) {
        items = snapshot.items
        snapshotError = snapshot.error
        let stagedKeys = Set(staged.map(\.key))
        drafts = drafts.filter { stagedKeys.contains($0.key) }
    }

    /// `update_settings` の結果を反映する。エラーになった項目は現在値へ戻し、メッセージを表示する
    public mutating func apply(result: UpdateSettingsResult) {
        apply(snapshot: result.snapshot)
        var errs: [String: String] = [:]
        for e in result.errors { errs[e.key] = e.message }
        fieldErrors = errs
        // エラーになった項目は入力値を残さずコアの現在値へ戻る（保存されたと誤解させないため）。メッセージだけ残る
    }

    // MARK: 表示用の整理

    public var groups: [Group] {
        var result: [Group] = []
        for item in items {
            if let last = result.last, last.title == item.group {
                result[result.count - 1] = Group(title: last.title, items: last.items + [item])
            } else {
                result.append(Group(title: item.group, items: [item]))
            }
        }
        return result
    }

    public func item(forKey key: String) -> SettingItem? {
        items.first { $0.key == key }
    }

    /// 表示する値（編集中ならその値、なければコアの現在値）。`nil` は「未指定でプロファイルが決める」
    public func displayValue(forKey key: String) -> SettingValue? {
        guard let item = item(forKey: key) else { return nil }
        switch drafts[key] {
        case .value(let v): return v
        case .reset: return item.default
        case nil: return item.value
        }
    }

    /// 「既定に戻す」を出す項目か（`explicit` か、未送信の編集がある項目）
    public func canReset(forKey key: String) -> Bool {
        guard let item = item(forKey: key) else { return false }
        switch drafts[key] {
        case .value: return true
        case .reset: return false
        case nil: return item.explicit
        }
    }

    public func isUnspecified(forKey key: String) -> Bool { sameSettingValue(displayValue(forKey: key), nil) }

    public func error(forKey key: String) -> String? { fieldErrors[key] }

    // MARK: UI 値への変換

    public func boolValue(forKey key: String) -> Bool {
        if case .bool(let b)? = displayValue(forKey: key) { return b }
        return false
    }

    /// 整数項目の表示値。未指定なら最小値
    public func intValue(forKey key: String) -> Int {
        if case .int(let i)? = displayValue(forKey: key) { return Int(i) }
        if let item = item(forKey: key), case .int(let r) = item.kind { return Int(r.min) }
        return 0
    }

    public func floatValue(forKey key: String) -> Double {
        if case .float(let d)? = displayValue(forKey: key) { return d }
        if let item = item(forKey: key), case .float(let r) = item.kind { return r.min }
        return 0
    }

    /// Choice / Text の表示値。Choice が未指定なら空文字（Picker は該当なしになる）
    public func textValue(forKey key: String) -> String {
        if case .text(let s)? = displayValue(forKey: key) { return s }
        return ""
    }

    public func choiceOptions(forKey key: String) -> [SettingChoice] {
        if let item = item(forKey: key), case .choice(let c) = item.kind { return c.options }
        return []
    }

    // MARK: クランプ

    /// 範囲内に収め、`min` 起点の刻みに揃える
    public static func clamp(_ value: Int, to range: IntRange) -> Int {
        let lo = Int(range.min), hi = Int(range.max)
        var v = min(max(value, lo), hi)
        let step = Int(range.step)
        if step > 1 {
            let snapped = lo + Int((Double(v - lo) / Double(step)).rounded()) * step
            v = min(snapped, hi)
        }
        return v
    }

    public static func clamp(_ value: Double, to range: FloatRange) -> Double {
        var v = min(max(value, range.min), range.max)
        if range.step > 0 {
            let snapped = range.min + ((v - range.min) / range.step).rounded() * range.step
            // 浮動小数点の誤差（0.30000000000000004 等）を落とす
            v = min((snapped * 1e9).rounded() / 1e9, range.max)
        }
        return v
    }

    // MARK: 編集と差分の生成

    /// UI からの編集。範囲外の数値はクランプする。現在値と同じなら何もしない
    public mutating func edit(key: String, bool value: Bool) { stage(key: key, .bool(value)) }

    public mutating func edit(key: String, int value: Int) {
        guard let item = item(forKey: key), case .int(let r) = item.kind else { return }
        stage(key: key, .int(Int32(clamping: Self.clamp(value, to: r))))
    }

    public mutating func edit(key: String, float value: Double) {
        guard let item = item(forKey: key), case .float(let r) = item.kind else { return }
        stage(key: key, .float(Self.clamp(value, to: r)))
    }

    /// Choice / Text 用
    public mutating func edit(key: String, text value: String) { stage(key: key, .text(value)) }

    /// 「既定に戻す」（`null` を送って明示指定を削除する）
    public mutating func reset(key: String) {
        guard item(forKey: key) != nil else { return }
        drafts[key] = .reset
        fieldErrors[key] = nil
        replaceStaged(SettingChange(key: key, value: nil))
    }

    private mutating func stage(key: String, _ value: SettingValue) {
        guard let item = item(forKey: key) else { return }
        // 現在値と同じ編集は送らない（ただし未送信の別の編集があればそれを取り消す）
        if drafts[key] == nil, sameSettingValue(item.value, value) { return }
        drafts[key] = .value(value)
        fieldErrors[key] = nil
        replaceStaged(SettingChange(key: key, value: value))
    }

    private mutating func replaceStaged(_ change: SettingChange) {
        staged.removeAll { $0.key == change.key }
        staged.append(change)
    }

    public var hasStagedChanges: Bool { !staged.isEmpty }

    /// 送信待ちの変更を取り出す（`update_settings` の `changes`）
    public mutating func takeStagedChanges() -> [SettingChange] {
        defer { staged = [] }
        return staged
    }
}

/// `SettingValue` は Equatable でない（生成物）ため、比較はここで行う
func sameSettingValue(_ a: SettingValue?, _ b: SettingValue?) -> Bool {
    switch (a, b) {
    case (nil, nil): return true
    case (.bool(let x)?, .bool(let y)?): return x == y
    case (.int(let x)?, .int(let y)?): return x == y
    case (.float(let x)?, .float(let y)?): return x == y
    case (.text(let x)?, .text(let y)?): return x == y
    default: return false
    }
}
