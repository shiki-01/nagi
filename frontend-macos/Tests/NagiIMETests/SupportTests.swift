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
}
