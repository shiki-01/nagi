import Combine
import XCTest

// 見た目の状態（docs/keyboard-style-spec.md §4）: 状態の組み立て、効く状態の判定、解決結果のメモ化、
// 仕様の例（§5.3・§5.4）の解決、状態の保持（released）と起点の通知。

private func decode(_ json: String) throws -> KeyboardLayoutDef {
    try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(json.utf8))
}

private func key(_ def: KeyboardLayoutDef, _ id: String) -> (Int, LayoutKeyDef) {
    for (index, row) in def.rows.enumerated() {
        if let found = row.keys.first(where: { $0.id == id }) { return (index, found) }
    }
    fatalError("key \(id) がない")
}

private func resolve(
    _ def: KeyboardLayoutDef, _ id: String, _ states: Set<LookStateName>, origin: String? = nil, scheme: LookScheme = .light
) -> ResolvedLook {
    let (row, k) = key(def, id)
    return LookResolver.resolve(layout: def, rowIndex: row, key: k, context: LookContext(states: states, origin: origin, scheme: scheme))
}

/// 仕様 §5.4: スペースが移動状態のとき、他のキーを薄くし、スペースのラベルを「移動」にする
private let spaceDragLayout = ##"""
{"schema":2,"id":"cursor","name":"n","mode":"alpha",
 "rules":[{"when":"spaceDrag","target":"others","look":{"opacity":0.35,"animationMs":120}}],
 "rows":[{"keys":[
  {"id":"mode","width":1.2,"label":"あ","kind":"action","action":"modeSwitch","role":"function"},
  {"id":"sp","width":4.4,"label":"空白","kind":"action","action":"space",
   "states":{"spaceDrag":{"label":"移動","opacity":0.7,"fontWeight":"semibold","animationMs":120}}},
  {"id":"en","width":1.6,"label":"改行","kind":"action","action":"enter","role":"function"}]}]}
"""##

/// 仕様 §5.3: 押下で縮小、長押しで光る、離した後に戻る
private let glowLayout = ##"""
{"schema":2,"id":"glow","name":"n","mode":"alpha",
 "rules":[
  {"when":"pressed","target":"all","look":{"scale":0.94,"fillOpacity":1,"animationMs":80}},
  {"when":"released","target":"all","holdMs":150,"look":{"animationMs":150}}],
 "rows":[{"keys":[
  {"id":"q","kind":"text","text":"q"},
  {"id":"w","kind":"text","text":"w","look":{"borderWidth":1},
   "states":{"pressed":{"fill":"#E0B341"},"longPress":{"scale":1.08,"shadowRadius":8}},
   "longPress":[{"label":"x","kind":"text","text":"x"}]}]}]}
"""##

final class LookStateLogicTests: XCTestCase {
    func testGlobalStates() {
        XCTAssertEqual(LookStateLogic.globalStates(shiftOnce: false, capsLock: false, composing: false, spaceDrag: false), [])
        XCTAssertEqual(
            LookStateLogic.globalStates(shiftOnce: true, capsLock: false, composing: true, spaceDrag: false), [.shift, .composing]
        )
        XCTAssertEqual(
            LookStateLogic.globalStates(shiftOnce: false, capsLock: true, composing: false, spaceDrag: true), [.capsLock, .spaceDrag]
        )
    }

    func testLocalStatesStayPressedDuringLongPressAndFlick() {
        XCTAssertEqual(LookStateLogic.localStates(pressed: true, released: false, longPress: true, flicking: false), [.pressed, .longPress])
        XCTAssertEqual(LookStateLogic.localStates(pressed: true, released: false, longPress: false, flicking: true), [.pressed, .flicking])
        XCTAssertEqual(LookStateLogic.localStates(pressed: false, released: false, longPress: false, flicking: false), [])
    }

    func testReleasedNeverOverlapsPressed() {
        XCTAssertEqual(LookStateLogic.localStates(pressed: false, released: true, longPress: false, flicking: false), [.released])
        XCTAssertEqual(LookStateLogic.localStates(pressed: true, released: true, longPress: false, flicking: false), [.pressed])
    }
}

final class FlickStateTests: XCTestCase {
    func testSmallMovementIsATapNotAFlick() {
        XCTAssertFalse(FlickState.isFlicking(dx: 5, dy: -5, threshold: 12, keyWidth: 60, keyHeight: 46))
    }

    func testMovementOverTheThresholdIsFlicking() {
        XCTAssertTrue(FlickState.isFlicking(dx: 0, dy: -20, threshold: 12, keyWidth: 60, keyHeight: 46))
        XCTAssertTrue(FlickState.isFlicking(dx: -12, dy: 0, threshold: 12, keyWidth: 60, keyHeight: 46))
    }

    func testCancelRangeIsNotFlicking() {
        // 幅の 2 倍を超えて動かした（取り消し）
        XCTAssertFalse(FlickState.isFlicking(dx: 130, dy: 0, threshold: 12, keyWidth: 60, keyHeight: 46))
        XCTAssertFalse(FlickState.isFlicking(dx: 0, dy: -100, threshold: 12, keyWidth: 60, keyHeight: 46))
    }

    func testNonFiniteIsNotFlicking() {
        XCTAssertFalse(FlickState.isFlicking(dx: .nan, dy: 30, threshold: 12, keyWidth: 60, keyHeight: 46))
    }
}

final class LookUsageTests: XCTestCase {
    func testPressedIsAlwaysRelevantOthersOnlyWhenUsed() throws {
        let def = try decode(spaceDragLayout)
        XCTAssertEqual(LookUsage.states(for: key(def, "mode").1, in: def), [.pressed, .spaceDrag])
        XCTAssertEqual(LookUsage.states(for: key(def, "sp").1, in: def), [.pressed, .spaceDrag])
    }

    func testLayoutWithoutStatesOnlyCaresAboutPressed() throws {
        let def = try decode(##"{"schema":2,"id":"x","name":"n","mode":"kana","rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]}]}"##)
        XCTAssertEqual(LookUsage.states(for: key(def, "a").1, in: def), [.pressed])
        for state in LookStateName.allCases { XCTAssertFalse(LookUsage.layoutUses(state, in: def)) }
    }

    func testRuleTargetsAndKeyStatesDecideRelevance() throws {
        let def = try decode(##"""
        {"schema":2,"id":"x","name":"n","mode":"kana",
         "rules":[{"when":"shift","target":{"roles":["function"]},"look":{"opacity":0.5}},
                  {"when":"composing","target":{"keys":["b"]},"look":{"opacity":0.5}}],
         "rows":[{"keys":[
          {"id":"a","kind":"text","text":"a","states":{"longPress":{"scale":1.1}}},
          {"id":"b","kind":"action","action":"enter","role":"function"}]}]}
        """##)
        XCTAssertEqual(LookUsage.states(for: key(def, "a").1, in: def), [.pressed, .longPress])
        XCTAssertEqual(LookUsage.states(for: key(def, "b").1, in: def), [.pressed, .shift, .composing])
        XCTAssertTrue(LookUsage.layoutUses(.shift, in: def))
        XCTAssertTrue(LookUsage.layoutUses(.longPress, in: def))
        XCTAssertFalse(LookUsage.layoutUses(.spaceDrag, in: def))
    }

    func testSpacerHasNoRelevantStates() throws {
        let def = try decode(##"""
        {"schema":2,"id":"x","name":"n","mode":"kana","rules":[{"when":"pressed","look":{"scale":0.9}}],
         "rows":[{"keys":[{"id":"s","kind":"spacer"},{"id":"a","kind":"text","text":"a"}]}]}
        """##)
        XCTAssertEqual(LookUsage.states(for: key(def, "s").1, in: def), [])
    }
}

final class LookCacheTests: XCTestCase {
    func testCachedResultEqualsDirectResolution() throws {
        let def = try decode(glowLayout)
        let cache = LookCache()
        let (row, w) = key(def, "w")
        for states in [Set<LookStateName>(), [.pressed], [.pressed, .longPress], [.released], [.shift, .composing]] {
            let context = LookContext(states: states, origin: nil, scheme: .dark)
            XCTAssertEqual(
                cache.resolve(layout: def, rowIndex: row, key: w, context: context),
                LookResolver.resolve(layout: def, rowIndex: row, key: w, context: context),
                "\(states)"
            )
        }
    }

    func testRepeatedResolutionHitsTheCache() throws {
        let def = try decode(glowLayout)
        let cache = LookCache()
        let (row, w) = key(def, "w")
        let context = LookContext(states: [.pressed], origin: nil, scheme: .light)
        _ = cache.resolve(layout: def, rowIndex: row, key: w, context: context)
        _ = cache.resolve(layout: def, rowIndex: row, key: w, context: context)
        XCTAssertEqual(cache.misses, 1)
        XCTAssertEqual(cache.hits, 1)
        XCTAssertEqual(cache.entryCount, 1)
    }

    func testUnrelatedStatesShareOneEntry() throws {
        let def = try decode(glowLayout)
        let cache = LookCache()
        let (row, q) = key(def, "q")
        for states in [Set<LookStateName>(), [.shift], [.composing, .capsLock], [.spaceDrag]] {
            _ = cache.resolve(layout: def, rowIndex: row, key: q, context: LookContext(states: states, origin: "sp", scheme: .light))
        }
        XCTAssertEqual(cache.entryCount, 1)
        XCTAssertEqual(cache.misses, 1)
    }

    func testSchemeAndStatesSplitEntries() throws {
        let def = try decode(glowLayout)
        let cache = LookCache()
        let (row, q) = key(def, "q")
        _ = cache.resolve(layout: def, rowIndex: row, key: q, context: LookContext(states: [], origin: nil, scheme: .light))
        _ = cache.resolve(layout: def, rowIndex: row, key: q, context: LookContext(states: [], origin: nil, scheme: .dark))
        _ = cache.resolve(layout: def, rowIndex: row, key: q, context: LookContext(states: [.pressed], origin: nil, scheme: .light))
        XCTAssertEqual(cache.entryCount, 3)
    }

    func testOriginDecidesTheOthersTarget() throws {
        let def = try decode(spaceDragLayout)
        let cache = LookCache()
        let context = LookContext(states: [.spaceDrag], origin: "sp", scheme: .light)
        let (modeRow, mode) = key(def, "mode")
        let (spRow, sp) = key(def, "sp")
        XCTAssertEqual(cache.resolve(layout: def, rowIndex: modeRow, key: mode, context: context).number("opacity"), 0.35)
        XCTAssertEqual(cache.resolve(layout: def, rowIndex: spRow, key: sp, context: context).number("opacity"), 0.7)
        // 起点が変わった（別のキーが起点）なら結果も変わる
        let other = LookContext(states: [.spaceDrag], origin: "mode", scheme: .light)
        XCTAssertEqual(cache.resolve(layout: def, rowIndex: modeRow, key: mode, context: other).number("opacity"), nil)
    }

    func testResetDropsEverything() throws {
        let def = try decode(glowLayout)
        let cache = LookCache()
        let (row, q) = key(def, "q")
        _ = cache.resolve(layout: def, rowIndex: row, key: q, context: LookContext(states: [.pressed], origin: nil, scheme: .light))
        XCTAssertTrue(cache.layoutUses(.pressed, layout: def))
        cache.reset()
        XCTAssertEqual(cache.entryCount, 0)
        let plain = try decode(##"{"schema":2,"id":"x","name":"n","mode":"kana","rows":[{"keys":[{"id":"q","kind":"text","text":"q"}]}]}"##)
        XCTAssertFalse(cache.layoutUses(.pressed, layout: plain))
    }
}

final class SpecExampleStateTests: XCTestCase {
    func testSpaceDragDimsOthersAndRelabelsSpace() throws {
        let def = try decode(spaceDragLayout)
        let global: Set<LookStateName> = [.spaceDrag]
        let mode = resolve(def, "mode", global.union([]), origin: "sp")
        XCTAssertEqual(mode.number("opacity"), 0.35)
        XCTAssertEqual(mode.number("animationMs"), 120)
        let sp = resolve(def, "sp", global.union([.pressed]), origin: "sp")
        XCTAssertEqual(sp.number("opacity"), 0.7)
        XCTAssertEqual(sp.string("fontWeight"), "semibold")
        let content = LookResolver.content(of: sp)
        XCTAssertEqual(content.label, "移動")
        XCTAssertTrue(content.hidesKeyIcon)
        // 移動していないときは何も変わらない
        XCTAssertNil(resolve(def, "mode", [], origin: nil).number("opacity"))
        XCTAssertNil(LookResolver.content(of: resolve(def, "sp", [.pressed])).label)
    }

    func testPressedShrinksAndLongPressGlows() throws {
        let def = try decode(glowLayout)
        let q = resolve(def, "q", [.pressed])
        XCTAssertEqual(q.number("scale"), 0.94)
        XCTAssertEqual(q.number("fillOpacity"), 1)
        let w = resolve(def, "w", [.pressed, .longPress])
        XCTAssertEqual(w.number("scale"), 1.08)
        XCTAssertEqual(w.number("shadowRadius"), 8)
        XCTAssertEqual(w.color("fill"), .rgba(r: 0xE0, g: 0xB3, b: 0x41, a: 255))
    }

    func testReleasedHoldComesFromTheRule() throws {
        let def = try decode(glowLayout)
        XCTAssertEqual(LookResolver.releasedHoldMs(layout: def, key: key(def, "q").1), 150)
        XCTAssertEqual(resolve(def, "q", [.released]).number("animationMs"), 150)
    }

    func testTransitionUsesTheLongerOfBeforeAndAfter() throws {
        let def = try decode(glowLayout)
        let pressed = resolve(def, "q", [.pressed])
        let released = resolve(def, "q", [.released])
        XCTAssertEqual(LookResolver.transitionMs(from: released, to: pressed, reduceMotion: false), 150)
        XCTAssertEqual(LookResolver.transitionMs(from: pressed, to: released, reduceMotion: false), 150)
        XCTAssertEqual(LookResolver.transitionMs(from: pressed, to: released, reduceMotion: true), 0)
    }

    func testShiftCapsAndComposingLayerBelowPressed() throws {
        let def = try decode(##"""
        {"schema":2,"id":"x","name":"n","mode":"alpha",
         "rules":[{"when":"composing","look":{"opacity":0.9}},{"when":"capsLock","look":{"opacity":0.8}},
                  {"when":"shift","look":{"opacity":0.7}},{"when":"pressed","look":{"opacity":0.6}}],
         "rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]}]}
        """##)
        XCTAssertEqual(resolve(def, "a", [.composing]).number("opacity"), 0.9)
        XCTAssertEqual(resolve(def, "a", [.composing, .capsLock]).number("opacity"), 0.8)
        XCTAssertEqual(resolve(def, "a", [.composing, .capsLock, .shift]).number("opacity"), 0.7)
        XCTAssertEqual(resolve(def, "a", [.composing, .shift, .pressed]).number("opacity"), 0.6)
    }
}

final class IconifyUsesInStatesTests: XCTestCase {
    func testStateAndRuleIconsAreCollected() throws {
        let def = try decode(##"""
        {"schema":2,"id":"x","name":"n","mode":"alpha",
         "rules":[{"when":"pressed","look":{"icon":"iconify:tabler:star"}},{"when":"shift","look":{"icon":"sf:shift"}}],
         "rows":[{"keys":[
          {"id":"a","kind":"text","text":"a","icon":"iconify:mdi:home","label":"家",
           "look":{"icon":"iconify:mdi:key"},
           "states":{"spaceDrag":{"icon":"iconify:mdi:arrow-left-right"},"pressed":{"icon":"iconify:mdi:home"},"nope":{"icon":"iconify:mdi:x"}}}]}]}
        """##)
        let uses = def.iconifyUses()
        XCTAssertEqual(
            uses.map { $0.id },
            ["mdi:home", "mdi:key", "mdi:home", "mdi:arrow-left-right", "tabler:star"]
        )
        XCTAssertEqual(uses.map { $0.keyId }, ["a", "a", "a", "a", "rules[0]"])
        // 状態の icon は取得できなくても本来の表示に戻れる
        XCTAssertEqual(uses.map { $0.hasLabel }, [true, true, true, true, true])
    }

    func testInvalidIconFormatsAreNotCollected() throws {
        let def = try decode(##"""
        {"schema":2,"id":"x","name":"n","mode":"alpha",
         "rows":[{"keys":[{"id":"a","kind":"text","text":"a","states":{"pressed":{"icon":"iconify:bad"}}}]}]}
        """##)
        XCTAssertTrue(def.iconifyUses().isEmpty)
    }
}

final class VisualStateTests: XCTestCase {
    func testSpaceDragOriginPublishesOnlyOnChange() {
        let visual = KeyboardVisualState()
        var seen: [String?] = []
        let token = visual.$spaceDragOrigin.sink { seen.append($0) }
        visual.setSpaceDragOrigin("sp")
        visual.setSpaceDragOrigin("sp")
        visual.setSpaceDragOrigin(nil)
        visual.setSpaceDragOrigin(nil)
        token.cancel()
        XCTAssertEqual(seen.count, 3)
        XCTAssertNil(seen[0])
        XCTAssertEqual(seen[1], "sp")
        XCTAssertNil(seen[2])
    }

    func testReleaseHoldCancelAndZero() {
        let hold = ReleaseHold()
        hold.begin(ms: 60_000)
        XCTAssertTrue(hold.active)
        hold.cancel()
        XCTAssertFalse(hold.active)
        hold.begin(ms: 0)
        XCTAssertFalse(hold.active)
    }

    func testReleaseHoldEndsAfterTheHoldTime() {
        let hold = ReleaseHold()
        let ended = expectation(description: "released ended")
        let token = hold.$active.dropFirst().sink { active in
            if !active { ended.fulfill() }
        }
        hold.begin(ms: 30)
        XCTAssertTrue(hold.active)
        wait(for: [ended], timeout: 3)
        token.cancel()
        XCTAssertFalse(hold.active)
    }
}
