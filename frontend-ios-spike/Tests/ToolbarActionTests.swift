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
        let all = ToolbarKeys.visibleKeys(layoutToolbar: nil, items: items, clipboardHasText: true)
        XCTAssertEqual(all.map { $0.action }, ["dismiss", "paste", "space"])
        let none = ToolbarKeys.visibleKeys(layoutToolbar: nil, items: items, clipboardHasText: false)
        XCTAssertEqual(none.map { $0.action }, ["dismiss", "space"])
    }

    // MARK: ToolbarAction ↔ キー（対応表）

    /// 全ケースの対応（kind / action 名 / target / 既定アイコン）。表を変えたらここも変える
    func testEveryToolbarActionMapsToExpectedKey() {
        let expected: [ToolbarAction: (LayoutKeyKind, String?, String?, String)] = [
            .dismiss: (.action, "dismiss", nil, "keyboard.chevron.compact.down"),
            .paste: (.action, "paste", nil, "doc.on.clipboard"),
            .cursorLeft: (.action, "moveLeft", nil, "chevron.left"),
            .cursorRight: (.action, "moveRight", nil, "chevron.right"),
            .newline: (.action, "enter", nil, "return"),
            .space: (.action, "space", nil, "space"),
            .deleteWord: (.action, "deleteWord", nil, "delete.left"),
            .emoji: (.layout, nil, "emoji", "face.smiling"),
            .nextKeyboard: (.action, "globe", nil, "globe"),
            .history: (.action, "history", nil, "clock.arrow.circlepath"),
        ]
        XCTAssertEqual(Set(expected.keys), Set(ToolbarAction.allCases), "対応表に無いケースがある")
        for action in ToolbarAction.allCases {
            guard let want = expected[action] else { continue }
            let key = ToolbarKeys.key(for: action)
            XCTAssertEqual(key.kind, want.0, action.rawValue)
            XCTAssertEqual(key.action, want.1, action.rawValue)
            XCTAssertEqual(key.target, want.2, action.rawValue)
            XCTAssertEqual(key.id, "tb-\(action.rawValue)")
            XCTAssertNil(key.icon)
            XCTAssertNil(key.label)
            XCTAssertEqual(ToolbarKeys.defaultIcon(for: key), want.3, action.rawValue)
            XCTAssertEqual(action.systemImage, want.3, action.rawValue)
            XCTAssertEqual(ToolbarKeys.faceContent(for: key), KeyFaceContent(text: nil, icon: want.3), action.rawValue)
            XCTAssertEqual(ToolbarKeys.accessibilityLabel(for: key), action.title, action.rawValue)
            // 生成したキーは検証を通る
            XCTAssertNil(LayoutValidation.keyError(key, validTargets: nil), action.rawValue)
        }
    }

    func testGeneratedKeysKeepOrderAndHaveUniqueIds() {
        let keys = ToolbarKeys.keys(for: ToolbarAction.allCases)
        XCTAssertEqual(keys.count, ToolbarAction.allCases.count)
        XCTAssertEqual(Set(keys.map { $0.id }).count, keys.count)
        XCTAssertEqual(ToolbarKeys.keys(for: []).count, 0)
    }

    func testFaceContentPrefersExplicitIconAndLabel() {
        var key = LayoutKeyDef(id: "a", kind: .action, label: nil, action: "paste")
        XCTAssertEqual(ToolbarKeys.faceContent(for: key), KeyFaceContent(text: nil, icon: "doc.on.clipboard"))
        key.label = "貼付"
        XCTAssertEqual(ToolbarKeys.faceContent(for: key), KeyFaceContent(text: "貼付", icon: nil))
        key.icon = "sf:star"
        XCTAssertEqual(ToolbarKeys.faceContent(for: key), KeyFaceContent(text: "貼付", icon: "sf:star"))
        // 既定のアイコンが無い action は何も出さない（label を付けるのは作者）
        let unknown = LayoutKeyDef(id: "u", kind: .action, label: nil, action: "convert")
        XCTAssertEqual(ToolbarKeys.faceContent(for: unknown), KeyFaceContent(text: nil, icon: nil))
        // layout キーは target ごと
        var layout = LayoutKeyDef(id: "l", kind: .layout, label: nil, action: nil)
        layout.target = "symbols"
        XCTAssertEqual(ToolbarKeys.faceContent(for: layout), KeyFaceContent(text: nil, icon: nil))
    }

    // MARK: レイアウトの toolbar

    private func toolbarLayout(_ keys: [JSON], look: JSON? = nil) -> JSON {
        var toolbar: JSON = ["keys": keys]
        if let look = look { toolbar["look"] = look }
        return LB.layout([LB.row([LB.text()])], overrides: ["toolbar": toolbar])
    }

    func testLayoutToolbarOverridesSettingsItems() throws {
        let obj = toolbarLayout([
            LB.key("t1", "action", ["action": "paste"]),
            LB.key("t2", "action", ["action": "deleteWord", "icon": "sf:star"]),
        ], look: ["cornerRadius": 3])
        let data = try JSONSerialization.data(withJSONObject: obj)
        let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data)
        XCTAssertNil(def.validationError())
        XCTAssertEqual(def.toolbar?.keys.map { $0.id }, ["t1", "t2"])
        // レイアウトの toolbar があれば設定の項目は使わない
        let shown = ToolbarKeys.visibleKeys(layoutToolbar: def.toolbar, items: [.space, .emoji], clipboardHasText: true)
        XCTAssertEqual(shown.map { $0.id }, ["t1", "t2"])
        // paste はクリップボードが空なら出さない
        let hidden = ToolbarKeys.visibleKeys(layoutToolbar: def.toolbar, items: [], clipboardHasText: false)
        XCTAssertEqual(hidden.map { $0.id }, ["t2"])
        // toolbar が無いレイアウトは設定の項目
        let plain = try JSONDecoder().decode(
            KeyboardLayoutDef.self, from: JSONSerialization.data(withJSONObject: LB.single(LB.text()))
        )
        XCTAssertNil(plain.toolbar)
        XCTAssertEqual(
            ToolbarKeys.visibleKeys(layoutToolbar: plain.toolbar, items: [.space, .emoji], clipboardHasText: true).map { $0.id },
            ["tb-space", "tb-emoji"]
        )
    }

    func testToolbarValidation() {
        assertValid(toolbarLayout([LB.key("t1", "action", ["action": "dismiss"])]))
        // 空・13 個は範囲外
        assertInvalid(toolbarLayout([]), contains: "toolbar")
        let thirteen = (0..<13).map { LB.key("t\($0)", "action", ["action": "dismiss", "width": 1]) }
        assertInvalid(toolbarLayout(thirteen), contains: "toolbar")
        let twelve = (0..<12).map { LB.key("t\($0)", "action", ["action": "dismiss"]) }
        assertValid(toolbarLayout(twelve))
        // キー id は rows と合わせて一意
        assertInvalid(toolbarLayout([LB.key("a", "action", ["action": "dismiss"])]), contains: "重複")
        // キーの検証は rows と同じ（action 名の形・幅）
        assertInvalid(toolbarLayout([LB.key("t1", "action", ["action": "1bad"])]), contains: "action")
        assertInvalid(toolbarLayout([LB.key("t1", "action", ["action": "dismiss", "width": 0])]), contains: "幅")
        // 幅の合計は 1〜20
        assertInvalid(toolbarLayout([LB.key("t1", "action", ["action": "dismiss", "width": 0.5])]), contains: "toolbar")
        // layout キーの target も rows と同じ
        assertInvalid(toolbarLayout([LB.key("t1", "layout", ["target": "nope"])]), contains: "切り替え先", targets: [])
        assertValid(toolbarLayout([LB.key("t1", "layout", ["target": "emoji"])]), targets: [])
        // toolbar.keys が無いのは構造エラー
        assertInvalid(LB.layout([LB.row([LB.text()])], overrides: ["toolbar": ["look": ["opacity": 1]]]), contains: "keys")
    }

    func testToolbarActionsAreKnownToPerform() {
        // 追加した action 名が KeyboardModel の knownActions と対応表に揃っている（目視用の一覧）
        let names = ToolbarAction.allCases.compactMap { ToolbarKeys.spec(for: $0).action }
        XCTAssertTrue(names.contains("paste"))
        XCTAssertTrue(names.contains("deleteWord"))
        XCTAssertTrue(names.contains("history"))
    }
}
