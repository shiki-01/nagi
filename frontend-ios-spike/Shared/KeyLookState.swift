import Foundation

// 見た目の「状態」の判定と、解決結果のメモ化（docs/keyboard-style-spec.md §4、§7.3）。
// UIKit / SwiftUI に依存しない純粋な処理（XCTest で直接検証する）。状態の通知元（各キー View）は Keyboard/NagiKeyboardView.swift。

enum LookStateLogic {
    /// 大域の状態（キーボード全体に同時に入る。§4.1）。`spaceDrag` はスペースのドラッグ移動中
    static func globalStates(shiftOnce: Bool, capsLock: Bool, composing: Bool, spaceDrag: Bool) -> Set<LookStateName> {
        var result: Set<LookStateName> = []
        if shiftOnce { result.insert(.shift) }
        if capsLock { result.insert(.capsLock) }
        if composing { result.insert(.composing) }
        if spaceDrag { result.insert(.spaceDrag) }
        return result
    }

    /// 局所の状態（そのキー自身にだけ入る）。`released` は `pressed` の直後にしか入らず、再び押されたら消えるので、
    /// 押している間は入れない。`longPress` / `flicking` は押している間のもの（`pressed` も真のまま）
    static func localStates(pressed: Bool, released: Bool, longPress: Bool, flicking: Bool) -> Set<LookStateName> {
        var result: Set<LookStateName> = []
        if pressed { result.insert(.pressed) }
        if released && !pressed { result.insert(.released) }
        if longPress { result.insert(.longPress) }
        if flicking { result.insert(.flicking) }
        return result
    }
}

/// どの状態が見た目に効くか。効かない状態は通知・再描画を省くのに使う（§7.3）
enum LookUsage {
    /// そのキーの見た目が影響を受け得る状態。`pressed` は組み込みの既定（背景 0.6）があるので常に含む。
    /// `target: others` は起点を知らずに「当てはまる」とみなす（多めに見積もる）
    static func states(for key: LayoutKeyDef, in layout: KeyboardLayoutDef) -> Set<LookStateName> {
        if key.kind == .spacer { return [] }
        var result: Set<LookStateName> = [.pressed]
        for rule in layout.rules where rule.target.matches(key: key, when: rule.when, origin: nil) {
            result.insert(rule.when)
        }
        if let states = key.states {
            for name in states.byState.keys { result.insert(name) }
        }
        return result
    }

    /// レイアウトのどこかの `rules` / `states` がその状態を使っているか
    static func layoutUses(_ state: LookStateName, in layout: KeyboardLayoutDef) -> Bool {
        if layout.rules.contains(where: { $0.when == state }) { return true }
        if layout.rows.contains(where: { row in row.keys.contains { $0.states?.byState[state] != nil } }) { return true }
        return layout.toolbar?.keys.contains { $0.states?.byState[state] != nil } ?? false
    }
}

/// 解決結果のメモ化。キー × 効く状態の集合 × 外観 → `ResolvedLook`。レイアウトが変わったら `reset()` で捨てる。
/// メインスレッドからだけ使う。状態が変わっても、そのキーに関係しない状態は無視するので、未関係のキーは同じ結果を返す
final class LookCache {
    private struct Entry: Hashable {
        let keyId: String
        let states: Set<LookStateName>
        /// `target: others` の判定に効く: このキーが `spaceDrag` の起点か
        let isOrigin: Bool
        let dark: Bool
    }

    private var storage: [Entry: ResolvedLook] = [:]
    private var relevant: [String: Set<LookStateName>] = [:]
    private var layoutUse: [LookStateName: Bool] = [:]
    private(set) var hits = 0
    private(set) var misses = 0

    var entryCount: Int { storage.count }

    func reset() {
        storage.removeAll()
        relevant.removeAll()
        layoutUse.removeAll()
    }

    func relevantStates(layout: KeyboardLayoutDef, key: LayoutKeyDef) -> Set<LookStateName> {
        if let cached = relevant[key.id] { return cached }
        let states = LookUsage.states(for: key, in: layout)
        relevant[key.id] = states
        return states
    }

    func layoutUses(_ state: LookStateName, layout: KeyboardLayoutDef) -> Bool {
        if let cached = layoutUse[state] { return cached }
        let used = LookUsage.layoutUses(state, in: layout)
        layoutUse[state] = used
        return used
    }

    func resolve(layout: KeyboardLayoutDef, rowIndex: Int, key: LayoutKeyDef, context: LookContext) -> ResolvedLook {
        let states = context.states.intersection(relevantStates(layout: layout, key: key))
        let entry = Entry(
            keyId: key.id, states: states,
            isOrigin: states.contains(.spaceDrag) && context.origin == key.id,
            dark: context.scheme == .dark
        )
        if let hit = storage[entry] {
            hits += 1
            return hit
        }
        misses += 1
        let resolved = LookResolver.resolve(
            layout: layout, rowIndex: rowIndex, key: key,
            context: LookContext(states: states, origin: context.origin, scheme: context.scheme)
        )
        storage[entry] = resolved
        return resolved
    }
}
