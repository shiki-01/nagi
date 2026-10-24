import XCTest

/// 候補バーのツールバー: 項目の既定、設定の保存/読込、出す判定
final class ToolbarActionTests: XCTestCase {
    typealias K = NagiIOSSettings.Key

    func testDefaultItemsStartWithDismissAndPaste() {
        XCTAssertEqual(Array(ToolbarAction.defaultItems.prefix(2)), [.dismiss, .paste])
        XCTAssertEqual(NagiIOSSettings().toolbarItems, ToolbarAction.defaultItems)
        XCTAssertEqual(Set(ToolbarAction.defaultItems).count, ToolbarAction.defaultItems.count)
    }

    func testEveryActionHasTitleAndSymbolAndUniqueName() {
        let names = ToolbarAction.allCases.map { $0.rawValue }
        XCTAssertEqual(Set(names).count, names.count)
        for action in ToolbarAction.allCases {
            XCTAssertFalse(action.title.isEmpty, action.rawValue)
            XCTAssertFalse(action.systemImage.isEmpty, action.rawValue)
        }
    }

    func testActionsFromNamesDropsUnknownAndDuplicatesKeepingOrder() {
        XCTAssertEqual(
            ToolbarLogic.actions(from: ["space", "bogus", "dismiss", "space", "paste"]),
            [.space, .dismiss, .paste]
        )
        XCTAssertEqual(ToolbarLogic.actions(from: []), [])
    }

    func testSaveAndLoadKeepsOrder() throws {
        var s = NagiIOSSettings()
        s.set(K.toolbarItems, .array(["newline", "dismiss", "emoji"].map { .string($0) }))
        XCTAssertEqual(s.toolbarItems, [.newline, .dismiss, .emoji])
        XCTAssertTrue(s.isExplicit(K.toolbarItems))
        let data = try JSONEncoder().encode(s)
        let loaded = try JSONDecoder().decode(NagiIOSSettings.self, from: data)
        XCTAssertEqual(loaded.toolbarItems, [.newline, .dismiss, .emoji])
    }

    func testInvalidValuesFallBackOrAreFiltered() {
        var s = NagiIOSSettings()
        // 型違いは既定
        s.set(K.toolbarItems, .string("dismiss"))
        XCTAssertEqual(s.toolbarItems, ToolbarAction.defaultItems)
        // 配列の中の知らない名前・文字列でない値・重複は捨てる
        s.set(K.toolbarItems, .array([.string("paste"), .number(1), .string("nope"), .string("paste"), .string("space")]))
        XCTAssertEqual(s.toolbarItems, [.paste, .space])
    }

    func testEmptyListIsAnExplicitChoice() {
        var s = NagiIOSSettings()
        s.set(K.toolbarItems, .array([]))
        XCTAssertEqual(s.toolbarItems, [])
        // 既定に戻す
        s.set(K.toolbarItems, nil)
        XCTAssertEqual(s.toolbarItems, ToolbarAction.defaultItems)
        XCTAssertFalse(s.isExplicit(K.toolbarItems))
    }

    func testDefinitionListsEveryAction() {
        let def = NagiIOSSettings.definition(for: K.toolbarItems)
        guard case .list(let choices)? = def?.kind else { return XCTFail("list ではない") }
        XCTAssertEqual(choices.map { $0.value }, ToolbarAction.allCases.map { $0.rawValue })
    }

    func testShouldShowOnlyWhenIdleWithoutCandidatesOrMessage() {
        XCTAssertTrue(ToolbarLogic.shouldShow(hasPreedit: false, candidatesEmpty: true, hasMessage: false))
        XCTAssertFalse(ToolbarLogic.shouldShow(hasPreedit: true, candidatesEmpty: true, hasMessage: false))
        XCTAssertFalse(ToolbarLogic.shouldShow(hasPreedit: false, candidatesEmpty: false, hasMessage: false))
        XCTAssertFalse(ToolbarLogic.shouldShow(hasPreedit: false, candidatesEmpty: true, hasMessage: true))
    }

    func testPasteIsHiddenWithoutClipboardText() {
        let items: [ToolbarAction] = [.dismiss, .paste, .space]
        XCTAssertEqual(ToolbarLogic.visible(items, clipboardHasText: true), items)
        XCTAssertEqual(ToolbarLogic.visible(items, clipboardHasText: false), [.dismiss, .space])
    }
}
