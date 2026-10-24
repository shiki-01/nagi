import XCTest

/// 同梱のレイアウト（config/keyboard/presets/*.json）が、アプリ内の検証（LayoutLoader）を全て通ること
final class PresetLayoutTests: XCTestCase {
    private let expectedModes: [String: LayoutMode] = [
        "emoji": .emoji,
        "flick-alpha": .alpha,
        "flick-kana": .kana,
        "numbers": .number,
        "qwerty-alpha": .alpha,
        "qwerty-kana": .kana,
        "symbols": .symbol,
        "toggle-alpha": .alpha,
    ]

    private func presetIds() -> [String] {
        TestSupport.presetURLs().map { $0.deletingPathExtension().lastPathComponent }
    }

    func testBundledPresetsAreAllPresent() {
        let ids = Set(presetIds())
        XCTAssertFalse(ids.isEmpty, "presets がテストバンドルに入っていない")
        for expected in expectedModes.keys {
            XCTAssertTrue(ids.contains(expected), "\(expected).json がバンドルにない")
        }
    }

    func testEveryPresetPassesLayoutLoaderValidation() throws {
        let urls = TestSupport.presetURLs()
        XCTAssertFalse(urls.isEmpty)
        let allIds = presetIds()
        for url in urls {
            let name = url.lastPathComponent
            let data = try Data(contentsOf: url)
            // 実行時の読み込みと同じ経路（decode + 検証）
            XCTAssertNoThrow(try LayoutLoader.decode(data), "\(name)")
            // 取り込み時と同じく layout.target の存在も確かめる（同梱の全 id + 組み込み名が参照先）
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data)
            XCTAssertNil(def.validationError(targets: allIds), "\(name)")
        }
    }

    func testPresetIdMatchesFileNameAndMode() throws {
        for url in TestSupport.presetURLs() {
            let fileId = url.deletingPathExtension().lastPathComponent
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(contentsOf: url))
            XCTAssertEqual(def.id, fileId, "id がファイル名と違う")
            XCTAssertTrue(LayoutLoader.isSafeId(def.id), fileId)
            if let mode = expectedModes[fileId] {
                XCTAssertEqual(def.mode, mode, fileId)
            }
        }
    }

    func testPresetHeightsFitTheKeyboard() throws {
        for url in TestSupport.presetURLs() {
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(contentsOf: url))
            let sum = def.rows.reduce(0.0) { $0 + $1.height }
            XCTAssertLessThanOrEqual(sum, LayoutValidation.rowHeightSumMax, def.id)
        }
    }

    func testDefaultAssignmentsExistAndHaveRightMode() throws {
        let ids = presetIds()
        XCTAssertTrue(ids.contains(LayoutLoader.defaultKanaId))
        XCTAssertTrue(ids.contains(LayoutLoader.defaultAlphaId))
        let kana = try loadPreset(LayoutLoader.defaultKanaId)
        let alpha = try loadPreset(LayoutLoader.defaultAlphaId)
        XCTAssertEqual(kana.mode, .kana)
        XCTAssertEqual(alpha.mode, .alpha)
    }

    func testFlickKanaCoversAll11KanaKeys() throws {
        let def = try loadPreset("flick-kana")
        var names = Set<Character>()
        for key in def.rows.flatMap({ $0.keys }) where key.kind == .flick {
            if let first = key.kana?.first { names.insert(first) }
        }
        XCTAssertEqual(names, LayoutValidation.kanaKeyNames)
    }

    /// プリセットの flick の `kana` は、すべて表示用の表（KanaKeyTable）で引ける
    func testFlickKanaNamesResolveInKanaKeyTable() throws {
        for url in TestSupport.presetURLs() {
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(contentsOf: url))
            for key in def.rows.flatMap({ $0.keys }) {
                guard let kana = key.kana else { continue }
                let entry = KanaKeyTable.entry(forName: kana)
                XCTAssertNotNil(entry, "\(def.id)/\(key.id) の kana「\(kana)」")
                XCTAssertEqual(entry?.chars.count, 5, "\(def.id)/\(key.id)")
            }
        }
    }

    /// 移行後の同梱レイアウトは schema 2 で、見た目の指定を持たない（`LookDefaults` だけで描かれる）
    func testPresetsAreSchema2WithoutLookStyling() throws {
        for url in TestSupport.presetURLs() {
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(contentsOf: url))
            XCTAssertEqual(def.schema, 2, def.id)
            XCTAssertNil(def.look, def.id)
            XCTAssertNil(def.roles, def.id)
            XCTAssertTrue(def.rules.isEmpty, def.id)
            XCTAssertNil(def.metrics, def.id)
            XCTAssertNil(def.background, def.id)
            XCTAssertTrue(def.assets.isEmpty, def.id)
            for row in def.rows {
                XCTAssertNil(row.look, def.id)
                for key in row.keys {
                    XCTAssertNil(key.look, "\(def.id)/\(key.id)")
                    XCTAssertNil(key.states, "\(def.id)/\(key.id)")
                }
            }
        }
    }

    /// 同梱のファイルに旧名 `style` が残っていない
    func testPresetsDoNotUseTheOldStyleName() throws {
        for url in TestSupport.presetURLs() {
            let text = try String(contentsOf: url, encoding: .utf8)
            XCTAssertFalse(text.contains("\"style\""), url.lastPathComponent)
        }
    }

    func testBuiltinFallbacksAreValid() {
        let kana = LayoutLoader.builtin(mode: .kana)
        let alpha = LayoutLoader.builtin(mode: .alpha)
        XCTAssertEqual(kana.id, "builtin-kana")
        XCTAssertEqual(alpha.id, "builtin-alpha")
        XCTAssertNil(kana.validationError())
        XCTAssertNil(alpha.validationError())
        XCTAssertEqual(kana.mode, .kana)
        XCTAssertEqual(alpha.mode, .alpha)
    }

    private func loadPreset(_ id: String) throws -> KeyboardLayoutDef {
        guard let url = TestSupport.presetURLs().first(where: { $0.deletingPathExtension().lastPathComponent == id }) else {
            throw XCTSkip("\(id).json がない")
        }
        return try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(contentsOf: url))
    }
}
