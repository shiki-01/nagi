import Foundation
import NagiClient
import XCTest

@testable import NagiIME

final class SupportTests: XCTestCase {
    func testHostOptionsArgumentsOverrideEnvironment() {
        let o = HostOptions.parse(
            arguments: ["--engine", "/a/imserver", "--user-dir", "/u", "--no-llm"],
            environment: ["NAGI_ENGINE_PATH": "/env/imserver", "NAGI_DATA_DIR": "/env/data"])
        XCTAssertEqual(o.enginePath, "/a/imserver")
        XCTAssertEqual(o.dataDir, "/env/data")
        XCTAssertEqual(
            o.engineArguments(defaultDataDir: "/d"),
            ["--data-dir", "/env/data", "--user-dir", "/u", "--no-llm"])
    }

    func testHostOptionsDefaultDataDir() {
        let o = HostOptions.parse(arguments: [], environment: [:])
        XCTAssertEqual(o.engineArguments(defaultDataDir: "/d"), ["--data-dir", "/d"])
    }

    func testHostOptionsSettingsFlag() {
        XCTAssertFalse(HostOptions.parse(arguments: [], environment: [:]).settings)
        let o = HostOptions.parse(arguments: ["--settings", "--engine", "/e"], environment: [:])
        XCTAssertTrue(o.settings)
        XCTAssertEqual(o.enginePath, "/e")
    }

    func testHostOptionsRegisterFlag() {
        XCTAssertFalse(HostOptions.parse(arguments: [], environment: [:]).register)
        let o = HostOptions.parse(arguments: ["--register", "--no-llm"], environment: [:])
        XCTAssertTrue(o.register)
        XCTAssertTrue(o.noLLM)
    }

    func testRegistrationReportSuccess() {
        let r = RegistrationReport(
            registerStatus: 0, foundSourceCount: 2, enableStatuses: [0, 0])
        XCTAssertTrue(r.succeeded)
        XCTAssertTrue(r.text.contains("2"))
    }

    func testRegistrationReportFailures() {
        XCTAssertFalse(
            RegistrationReport(registerStatus: -50, foundSourceCount: 1, enableStatuses: [0]).succeeded)
        XCTAssertFalse(
            RegistrationReport(registerStatus: 0, foundSourceCount: 0, enableStatuses: []).succeeded)
        let bad = RegistrationReport(registerStatus: 0, foundSourceCount: 1, enableStatuses: [-50])
        XCTAssertFalse(bad.succeeded)
        XCTAssertTrue(bad.text.contains("-50"))
    }

    func testSyncBridgeReturnsValueAndTimesOut() {
        let ok = SyncBridge.wait(timeout: 2) { 42 }
        XCTAssertEqual(try? ok.get(), 42)

        let slow: Result<Int, Error> = SyncBridge.wait(timeout: 0.05) {
            try await Task.sleep(nanoseconds: 2_000_000_000)
            return 1
        }
        if case .failure(let e) = slow {
            XCTAssertEqual(e as? NagiClientError, .timeout)
        } else {
            XCTFail("timeout expected")
        }
    }

    func testSessionStoreBuildsRestoreParams() {
        let store = SessionStore()
        let p = PreeditView(
            text: "かな",
            segments: [SegmentView(surface: "かな", reading: "かな", kind: .japanese,
                range: RangeU16(start: 0, end: 2))],
            cursor: 2, pending: nil, focus: nil, selection: nil)
        store.update(sessionId: "s1", preedit: p)
        let params = store.restoreParams()
        XCTAssertEqual(params.count, 1)
        XCTAssertEqual(params[0].session_id, "s1")
        XCTAssertEqual(params[0].cursor, 2)
        XCTAssertFalse(params[0].converting)

        store.update(sessionId: "s1", preedit: PreeditView(
            text: "", segments: [], cursor: 0, pending: nil, focus: nil, selection: nil))
        XCTAssertNil(store.preedit(for: "s1"))
        XCTAssertTrue(store.restoreParams().isEmpty)
    }

    func testBundleIdentityDerivesIDs() {
        XCTAssertEqual(BundleIdentity.resolve(nil), "com.shiki.inputmethod.Nagi")
        XCTAssertEqual(BundleIdentity.resolve(""), "com.shiki.inputmethod.Nagi")
        XCTAssertEqual(
            BundleIdentity.resolve("com.shiki.inputmethod.Nagi"), "com.shiki.inputmethod.Nagi")
        XCTAssertEqual(
            BundleIdentity.inputSourceID(bundleID: "com.shiki.inputmethod.Nagi"),
            "com.shiki.inputmethod.Nagi.Japanese")
        XCTAssertEqual(
            BundleIdentity.connectionName(bundleID: "com.shiki.inputmethod.Nagi"),
            "com.shiki.inputmethod.Nagi_Connection")
    }

    func testEnvironmentMapping() {
        XCTAssertEqual(EnvironmentMapping.onBattery(providingPowerSourceType: "Battery Power"), true)
        XCTAssertEqual(EnvironmentMapping.onBattery(providingPowerSourceType: "AC Power"), false)
        XCTAssertNil(EnvironmentMapping.onBattery(providingPowerSourceType: nil))
        XCTAssertEqual(EnvironmentMapping.thermal(.serious), .serious)
        XCTAssertEqual(EnvironmentMapping.memoryPressure([.warning]), .warning)
        XCTAssertEqual(EnvironmentMapping.memoryPressure([.warning, .critical]), .critical)
        XCTAssertEqual(EnvironmentMapping.memoryPressure([.normal]), .normal)
    }

    func testStatusToastMessages() {
        let info = NotifyParams(level: .info, message: "i", code: nil)
        let warn = NotifyParams(level: .warning, message: "w", code: nil)
        XCTAssertNil(StatusMessages.toast(for: .notify(info)))
        XCTAssertEqual(StatusMessages.toast(for: .notify(warn)), "w")
        XCTAssertNotNil(StatusMessages.toast(for: .crashed(attempt: 1, delay: 0.2)))
        XCTAssertNil(StatusMessages.toast(for: .crashed(attempt: 2, delay: 0.4)))
        XCTAssertNotNil(StatusMessages.toast(for: .enteredMinimalMode(restarts: 5)))
    }

    func testStatusMenuLines() {
        let status = StatusResult(
            profile: "balanced", profile_reason: "既定", backend: "x", llm: false, learning: true,
            learned_entries: 3, user_dict_entries: 0, config_path: nil, config_error: true)
        let lines = StatusMessages.menuLines(for: status)
        XCTAssertEqual(lines.first, "プロファイル: balanced（既定）")
        XCTAssertTrue(lines.contains("変換: 辞書のみ"))
        XCTAssertTrue(lines.contains("学習: 有効（3 件）"))
        XCTAssertEqual(lines.count, 5)
    }

    func testToastThrottle() {
        var t = ToastThrottle(interval: 30)
        let now = Date()
        XCTAssertTrue(t.shouldShow("a", now: now))
        XCTAssertFalse(t.shouldShow("a", now: now.addingTimeInterval(10)))
        XCTAssertTrue(t.shouldShow("b", now: now.addingTimeInterval(10)))
        XCTAssertTrue(t.shouldShow("a", now: now.addingTimeInterval(31)))
    }

    func testModeIdentifiersMatchBundlePlist() {
        let ids = BundleIdentity.modeIdentifiers(bundleID: "com.shiki.inputmethod.Nagi")
        XCTAssertEqual(ids[.hiragana], "com.shiki.inputmethod.Nagi.Japanese")
        XCTAssertEqual(ids[.alphanumeric], "com.shiki.inputmethod.Nagi.Roman")
        XCTAssertEqual(Set(ids.values).count, 2)
    }
}
