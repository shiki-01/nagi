import XCTest

final class IOSSettingsTests: XCTestCase {
    typealias K = NagiIOSSettings.Key

    private func decode(_ json: String) throws -> NagiIOSSettings {
        try JSONDecoder().decode(NagiIOSSettings.self, from: Data(json.utf8))
    }

    private func encodeToObject(_ settings: NagiIOSSettings) throws -> [String: Any] {
        let data = try JSONEncoder().encode(settings)
        return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    // MARK: 既定値

    func testDefaultsForEmptySettings() {
        let s = NagiIOSSettings()
        XCTAssertFalse(s.toggleInput)
        XCTAssertTrue(s.flickPopup)
        XCTAssertEqual(s.flickLongPressMs, 350)
        XCTAssertEqual(s.keyRepeatDelayMs, 450)
        XCTAssertEqual(s.keyRepeatMs, 110)
        XCTAssertEqual(s.keySpacingPt, 6)
        XCTAssertEqual(s.keyCornerPt, 6)
        XCTAssertEqual(s.oneHanded, "off")
        XCTAssertEqual(s.oneHandedWidth, 0.8, accuracy: 1e-9)
        XCTAssertEqual(s.haptic, "light")
        XCTAssertTrue(s.soundEnabled)
        XCTAssertEqual(s.soundMode, "compat")
        XCTAssertTrue(s.layoutByField)
        XCTAssertEqual(s.commitMethod, "insert")
        XCTAssertEqual(s.hapticEngine, "standard")
        XCTAssertEqual(s.toggleTimeoutMs, 1000)
        XCTAssertEqual(s.flickThresholdPoints, 14, accuracy: 1e-9)
        XCTAssertEqual(s.heightScale, 1.0, accuracy: 1e-9)
        XCTAssertEqual(s.schema, NagiIOSSettings.currentSchema)
    }

    func testEveryDefinitionDefaultIsAlreadyNormalized() {
        for def in NagiIOSSettings.definitions {
            XCTAssertEqual(def.normalize(def.defaultValue), def.defaultValue, def.key)
            XCTAssertEqual(def.normalize(nil), def.defaultValue, def.key)
        }
    }

    func testDefinitionKeysAreUnique() {
        let keys = NagiIOSSettings.definitions.map { $0.key }
        XCTAssertEqual(Set(keys).count, keys.count)
    }

    // MARK: 丸め

    func testIntIsClampedAndSnappedToStep() {
        // flick.long_press_ms: 150...1000 刻み 50
        var s = NagiIOSSettings()
        s.set(K.flickLongPressMs, .number(100))
        XCTAssertEqual(s.flickLongPressMs, 150)
        s.set(K.flickLongPressMs, .number(5000))
        XCTAssertEqual(s.flickLongPressMs, 1000)
        s.set(K.flickLongPressMs, .number(351))
        XCTAssertEqual(s.flickLongPressMs, 350)
        s.set(K.flickLongPressMs, .number(380))
        XCTAssertEqual(s.flickLongPressMs, 400)
        s.set(K.flickLongPressMs, .number(990))
        XCTAssertEqual(s.flickLongPressMs, 1000)
        // touch.key_repeat_ms: 30...300 刻み 10
        s.set(K.keyRepeatMs, .number(113))
        XCTAssertEqual(s.keyRepeatMs, 110)
        s.set(K.keyRepeatMs, .number(0))
        XCTAssertEqual(s.keyRepeatMs, 30)
    }

    func testFloatIsClampedAndSnappedToStep() {
        var s = NagiIOSSettings()
        // flick.sensitivity: 0.5...2.0 刻み 0.1
        s.set(K.flickSensitivity, .number(0.34))
        XCTAssertEqual(s.double(K.flickSensitivity), 0.5, accuracy: 1e-9)
        s.set(K.flickSensitivity, .number(5))
        XCTAssertEqual(s.double(K.flickSensitivity), 2.0, accuracy: 1e-9)
        s.set(K.flickSensitivity, .number(1.26))
        XCTAssertEqual(s.double(K.flickSensitivity), 1.3, accuracy: 1e-9)
        // keyboard.key_spacing_pt: 0...8 刻み 0.5
        s.set(K.keyboardKeySpacingPt, .number(0.3))
        XCTAssertEqual(s.keySpacingPt, 0.5, accuracy: 1e-9)
        s.set(K.keyboardKeySpacingPt, .number(100))
        XCTAssertEqual(s.keySpacingPt, 8, accuracy: 1e-9)
        s.set(K.keyboardKeySpacingPt, .number(-3))
        XCTAssertEqual(s.keySpacingPt, 0, accuracy: 1e-9)
    }

    func testTypeMismatchFallsBackToDefault() {
        var s = NagiIOSSettings()
        s.set(K.flickLongPressMs, .string("abc"))
        XCTAssertEqual(s.flickLongPressMs, 350)
        s.set(K.flickSensitivity, .bool(true))
        XCTAssertEqual(s.double(K.flickSensitivity), 1.0, accuracy: 1e-9)
        s.set(K.toggleInput, .number(1))
        XCTAssertFalse(s.toggleInput)
        s.set(K.keyboardHaptic, .number(3))
        XCTAssertEqual(s.haptic, "light")
    }

    func testNonFiniteNumberFallsBackToDefault() {
        var s = NagiIOSSettings()
        s.set(K.flickLongPressMs, .number(.nan))
        XCTAssertEqual(s.flickLongPressMs, 350)
        s.set(K.flickSensitivity, .number(.infinity))
        XCTAssertEqual(s.double(K.flickSensitivity), 1.0, accuracy: 1e-9)
    }

    func testChoiceRejectsUnknownValue() {
        var s = NagiIOSSettings()
        s.set(K.keyboardHeight, .string("tall"))
        XCTAssertEqual(s.string(K.keyboardHeight), "tall")
        s.set(K.keyboardHeight, .string("huge"))
        XCTAssertEqual(s.string(K.keyboardHeight), "standard")
        s.set(K.keyboardSound, .string("system"))
        XCTAssertTrue(s.soundEnabled)
        XCTAssertEqual(s.soundMode, "compat")
    }

    func testLegacySystemSoundIsReadAsCompat() throws {
        // 以前の版が保存した `system`（廃止）は、読むときに既定の `compat` に丸める
        let s = try decode(#"{"schema":1,"keyboard.sound":"system"}"#)
        XCTAssertEqual(s.soundMode, "compat")
        XCTAssertTrue(s.soundEnabled)
    }

    func testLayoutByFieldCanBeTurnedOff() {
        var s = NagiIOSSettings()
        s.set(K.keyboardLayoutByField, .bool(false))
        XCTAssertFalse(s.layoutByField)
        s.set(K.keyboardLayoutByField, .number(1))
        XCTAssertTrue(s.layoutByField)
    }

    func testBoolKeepsBoolValue() {
        var s = NagiIOSSettings()
        s.set(K.toggleInput, .bool(true))
        XCTAssertTrue(s.toggleInput)
        s.set(K.flickPopup, .bool(false))
        XCTAssertFalse(s.flickPopup)
    }

    /// 壊れた値がファイルに入っていても、読むときに既定へ丸める
    func testInvalidStoredValuesAreNormalizedOnRead() throws {
        let s = try decode(#"{"schema":1,"flick.long_press_ms":"abc","keyboard.haptic":"strong","flick.popup":"yes","touch.key_repeat_ms":99999}"#)
        XCTAssertEqual(s.flickLongPressMs, 350)
        XCTAssertEqual(s.haptic, "light")
        XCTAssertTrue(s.flickPopup)
        XCTAssertEqual(s.keyRepeatMs, 300)
        XCTAssertTrue(s.isExplicit(K.flickLongPressMs))
    }

    // MARK: 明示指定

    func testExplicitAndReset() {
        var s = NagiIOSSettings()
        XCTAssertFalse(s.isExplicit(K.toggleInput))
        s.set(K.toggleInput, .bool(true))
        XCTAssertTrue(s.isExplicit(K.toggleInput))
        s.set(K.toggleInput, nil)
        XCTAssertFalse(s.isExplicit(K.toggleInput))
        XCTAssertFalse(s.toggleInput)
    }

    func testOnlyExplicitKeysAreWritten() throws {
        var s = NagiIOSSettings()
        s.set(K.flickPopup, .bool(false))
        let object = try encodeToObject(s)
        XCTAssertEqual(Set(object.keys), ["schema", K.flickPopup])
        XCTAssertEqual(object["schema"] as? Int, NagiIOSSettings.currentSchema)
        XCTAssertEqual(object[K.flickPopup] as? Bool, false)
    }

    // MARK: 未知のキー・schema

    func testUnknownKeysArePreservedThroughRoundTrip() throws {
        let json = #"{"schema":1,"future.x":{"a":[1,"b",null,true]},"future.y":"s","flick.popup":false}"#
        var s = try decode(json)
        XCTAssertEqual(s.value("future.y"), .string("s"))
        XCTAssertEqual(s.value("not.exist"), .null)
        // 既知の項目を変えても未知のキーは残る
        s.set(K.toggleInput, .bool(true))
        let decoded = try JSONDecoder().decode(NagiIOSSettings.self, from: JSONEncoder().encode(s))
        XCTAssertEqual(decoded, s)
        XCTAssertEqual(decoded.value("future.x"),
                       .object(["a": .array([.number(1), .string("b"), .null, .bool(true)])]))
        XCTAssertEqual(decoded.value("future.y"), .string("s"))
        XCTAssertFalse(decoded.flickPopup)
        XCTAssertTrue(decoded.toggleInput)
    }

    func testUnknownKeyCanBeSetAndCleared() {
        var s = NagiIOSSettings()
        s.set("custom.key", .string("v"))
        XCTAssertEqual(s.value("custom.key"), .string("v"))
        s.set("custom.key", nil)
        XCTAssertEqual(s.value("custom.key"), .null)
    }

    func testSchemaHandling() throws {
        XCTAssertEqual(try decode(#"{}"#).schema, 1)
        XCTAssertEqual(try decode(#"{"schema":"x"}"#).schema, 1)
        XCTAssertEqual(try decode(#"{"schema":-3}"#).schema, 1)
        XCTAssertEqual(try decode(#"{"schema":2}"#).schema, 2)
        // 新しい版が書いたファイルは、版を下げずに書き戻す
        let newer = try decode(#"{"schema":3,"future":1}"#)
        XCTAssertEqual(try encodeToObject(newer)["schema"] as? Int, 3)
        // schema は stored に混ざらない
        XCTAssertEqual(try encodeToObject(try decode(#"{}"#))["schema"] as? Int, 1)
    }

    func testDecodeRejectsNonObject() {
        XCTAssertThrowsError(try decode("[1,2]"))
        XCTAssertThrowsError(try decode("\"text\""))
    }

    // MARK: toggleTimeoutMs

    func testToggleTimeoutMs() throws {
        XCTAssertEqual(try decode(#"{"input.toggle_timeout_ms":100}"#).toggleTimeoutMs, 200)
        XCTAssertEqual(try decode(#"{"input.toggle_timeout_ms":5000}"#).toggleTimeoutMs, 3000)
        XCTAssertEqual(try decode(#"{"input.toggle_timeout_ms":1500}"#).toggleTimeoutMs, 1500)
        XCTAssertEqual(try decode(#"{"input.toggle_timeout_ms":"x"}"#).toggleTimeoutMs, 1000)
    }

    // MARK: 派生値

    func testFlickThresholdDividesBySensitivity() {
        var s = NagiIOSSettings()
        s.set(K.flickSensitivity, .number(2.0))
        XCTAssertEqual(s.flickThresholdPoints, 7, accuracy: 1e-9)
        s.set(K.flickThresholdPt, .number(20))
        XCTAssertEqual(s.flickThresholdPoints, 10, accuracy: 1e-9)
        s.set(K.flickSensitivity, .number(0.5))
        XCTAssertEqual(s.flickThresholdPoints, 40, accuracy: 1e-9)
    }

    func testHeightScaleCombinesPresetAndMultiplier() {
        var s = NagiIOSSettings()
        s.set(K.keyboardHeight, .string("compact"))
        XCTAssertEqual(s.heightScale, 0.9, accuracy: 1e-9)
        s.set(K.keyboardHeight, .string("tall"))
        s.set(K.keyboardHeightScale, .number(1.3))
        XCTAssertEqual(s.heightScale, 1.1 * 1.3, accuracy: 1e-9)
    }

    // MARK: NagiJSONValue

    func testJSONValueRoundTrip() throws {
        let value: NagiJSONValue = .object([
            "n": .null, "b": .bool(true), "d": .number(1.5), "s": .string("あ"),
            "a": .array([.number(1), .string("x")]),
        ])
        let data = try JSONEncoder().encode(value)
        XCTAssertEqual(try JSONDecoder().decode(NagiJSONValue.self, from: data), value)
    }
}
