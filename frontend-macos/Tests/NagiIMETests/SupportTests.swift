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

    func testModeIdentifiersMatchBundlePlist() {
        let ids = BundleIdentity.modeIdentifiers(bundleID: "com.shiki.inputmethod.Nagi")
        XCTAssertEqual(ids[.hiragana], "com.shiki.inputmethod.Nagi.Japanese")
        XCTAssertEqual(ids[.alphanumeric], "com.shiki.inputmethod.Nagi.Roman")
        XCTAssertEqual(Set(ids.values).count, 2)
    }
}
