import Foundation
import NagiClient
import XCTest

@testable import NagiIME

final class SettingsFormStateTests: XCTestCase {
    private func item(
        _ key: String, group: String = "変換", kind: SettingKind, value: SettingValue?,
        def: SettingValue?, explicit: Bool = false, restart: Bool = false
    ) -> SettingItem {
        SettingItem(
            key: key, group: group, label: key, description: "説明", kind: kind, value: value,
            default: def, explicit: explicit, requires_restart: restart)
    }

    private func snapshot(error: String? = nil) -> SettingsSnapshot {
        SettingsSnapshot(
            items: [
                item(
                    "conversion.sync_budget_ms", kind: .int(IntRange(min: 0, max: 5000, step: 10)),
                    value: .int(150), def: .int(150)),
                item("conversion.live_conversion", kind: .bool, value: .bool(true), def: .bool(true), explicit: true),
                item(
                    "conversion.beam_width", kind: .int(IntRange(min: 1, max: 8, step: 1)), value: nil,
                    def: nil),
                item(
                    "profile.mode", group: "プロファイル",
                    kind: .choice(
                        ChoiceOptions(options: [
                            SettingChoice(value: "auto", label: "自動"),
                            SettingChoice(value: "light", label: "軽量"),
                        ])),
                    value: .text("auto"), def: .text("auto")),
                item(
                    "ranking.weight", group: "プロファイル",
                    kind: .float(FloatRange(min: 0, max: 1, step: 0.1)), value: .float(0.5),
                    def: .float(0.5)),
                item("update.channel", group: "更新", kind: .text, value: .text("stable"), def: .text("stable"), restart: true),
            ],
            error: error)
    }

    func testGroupsKeepOrderAndMergeConsecutive() {
        let s = SettingsFormState(snapshot: snapshot())
        XCTAssertEqual(s.groups.map(\.title), ["変換", "プロファイル", "更新"])
        XCTAssertEqual(s.groups[0].items.count, 3)
        XCTAssertEqual(s.groups[1].items.count, 2)
    }

    func testClampInt() {
        let r = IntRange(min: 0, max: 5000, step: 10)
        XCTAssertEqual(SettingsFormState.clamp(-5, to: r), 0)
        XCTAssertEqual(SettingsFormState.clamp(9999, to: r), 5000)
        XCTAssertEqual(SettingsFormState.clamp(123, to: r), 120)
        XCTAssertEqual(SettingsFormState.clamp(3, to: IntRange(min: 1, max: 8, step: 1)), 3)
        // min 起点の刻み。max を超えない
        XCTAssertEqual(SettingsFormState.clamp(100, to: IntRange(min: 1, max: 100, step: 10)), 100)
    }

    func testClampFloat() {
        let r = FloatRange(min: 0, max: 1, step: 0.1)
        XCTAssertEqual(SettingsFormState.clamp(0.30000000000000004, to: r), 0.3, accuracy: 1e-12)
        XCTAssertEqual(SettingsFormState.clamp(2, to: r), 1, accuracy: 1e-12)
        XCTAssertEqual(SettingsFormState.clamp(-1, to: r), 0, accuracy: 1e-12)
    }

    func testEditClampsAndStagesChange() {
        var s = SettingsFormState(snapshot: snapshot())
        s.edit(key: "conversion.sync_budget_ms", int: 99999)
        XCTAssertEqual(s.intValue(forKey: "conversion.sync_budget_ms"), 5000)
        XCTAssertTrue(s.canReset(forKey: "conversion.sync_budget_ms"))
        let changes = s.takeStagedChanges()
        XCTAssertEqual(changes.count, 1)
        guard case .int(let v)? = changes[0].value else { return XCTFail("value") }
        XCTAssertEqual(v, 5000)
        XCTAssertFalse(s.hasStagedChanges)
    }

    func testEditToSameValueStagesNothing() {
        var s = SettingsFormState(snapshot: snapshot())
        s.edit(key: "conversion.sync_budget_ms", int: 150)
        s.edit(key: "profile.mode", text: "auto")
        XCTAssertFalse(s.hasStagedChanges)
    }

    func testLaterEditOfSameKeyWins() {
        var s = SettingsFormState(snapshot: snapshot())
        s.edit(key: "conversion.live_conversion", bool: false)
        s.edit(key: "conversion.sync_budget_ms", int: 200)
        s.edit(key: "conversion.live_conversion", bool: true)
        let changes = s.takeStagedChanges()
        XCTAssertEqual(changes.map(\.key), ["conversion.sync_budget_ms", "conversion.live_conversion"])
    }

    func testResetSendsNullAndShowsDefault() {
        var s = SettingsFormState(snapshot: snapshot())
        XCTAssertTrue(s.canReset(forKey: "conversion.live_conversion"))
        XCTAssertFalse(s.canReset(forKey: "profile.mode"))
        s.edit(key: "conversion.beam_width", int: 5)
        s.reset(key: "conversion.beam_width")
        XCTAssertTrue(s.isUnspecified(forKey: "conversion.beam_width"))
        let changes = s.takeStagedChanges()
        XCTAssertEqual(changes.count, 1)
        XCTAssertNil(changes[0].value)
    }

    func testUnspecifiedIntShowsMinimum() {
        let s = SettingsFormState(snapshot: snapshot())
        XCTAssertTrue(s.isUnspecified(forKey: "conversion.beam_width"))
        XCTAssertEqual(s.intValue(forKey: "conversion.beam_width"), 1)
    }

    func testChoiceOptionsAndTextValue() {
        let s = SettingsFormState(snapshot: snapshot())
        XCTAssertEqual(s.choiceOptions(forKey: "profile.mode").map(\.value), ["auto", "light"])
        XCTAssertEqual(s.textValue(forKey: "profile.mode"), "auto")
        XCTAssertEqual(s.item(forKey: "update.channel")?.requires_restart, true)
    }

    func testSnapshotErrorIsKept() {
        var s = SettingsFormState(snapshot: snapshot(error: "設定ファイルが壊れています"))
        XCTAssertEqual(s.snapshotError, "設定ファイルが壊れています")
        s.apply(snapshot: snapshot())
        XCTAssertNil(s.snapshotError)
    }

    func testResultErrorsAreShownPerKeyAndRevertDraft() {
        var s = SettingsFormState(snapshot: snapshot())
        s.edit(key: "conversion.sync_budget_ms", int: 300)
        _ = s.takeStagedChanges()
        let result = UpdateSettingsResult(
            applied: [],
            errors: [SettingError(key: "conversion.sync_budget_ms", message: "範囲外です")],
            snapshot: snapshot())
        s.apply(result: result)
        XCTAssertEqual(s.error(forKey: "conversion.sync_budget_ms"), "範囲外です")
        XCTAssertEqual(s.intValue(forKey: "conversion.sync_budget_ms"), 150)
        // 次の編集でエラー表示は消える
        s.edit(key: "conversion.sync_budget_ms", int: 200)
        XCTAssertNil(s.error(forKey: "conversion.sync_budget_ms"))
    }

    func testEditDuringFlightSurvivesResult() {
        var s = SettingsFormState(snapshot: snapshot())
        s.edit(key: "conversion.sync_budget_ms", int: 300)
        _ = s.takeStagedChanges()
        s.edit(key: "conversion.sync_budget_ms", int: 400)  // 送信中の再編集
        s.apply(result: UpdateSettingsResult(applied: ["conversion.sync_budget_ms"], errors: [], snapshot: snapshot()))
        XCTAssertEqual(s.intValue(forKey: "conversion.sync_budget_ms"), 400)
        XCTAssertTrue(s.hasStagedChanges)
    }
}
