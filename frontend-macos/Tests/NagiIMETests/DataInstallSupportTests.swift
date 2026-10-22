import Foundation
import XCTest

@testable import NagiIME

final class DataInstallSupportTests: XCTestCase {
    func testParsesProgressLines() {
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"dict_fetch","file":"dictionary00.txt","index":1,"count":13}"#),
            .dictFetch(file: "dictionary00.txt", index: 1, count: 13))
        XCTAssertEqual(DataInstallProgress.parse(#"{"stage":"dict_build"}"#), .dictBuild)
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"model_fetch","bytes":10,"total":40}"#),
            .modelFetch(bytes: 10, total: 40))
        XCTAssertEqual(DataInstallProgress.parse(#"{"stage":"done"}"#), .done)
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"error","message":"x"}"#), .error("x"))
        XCTAssertNil(DataInstallProgress.parse("ログの行"))
        XCTAssertNil(DataInstallProgress.parse(#"{"stage":"unknown"}"#))
    }

    func testModelProgressIsThinnedToQuarters() {
        var quarter = -1
        XCTAssertNotNil(DataInstallProgress.modelFetch(bytes: 0, total: 100).message(lastModelQuarter: &quarter))
        XCTAssertNil(DataInstallProgress.modelFetch(bytes: 10, total: 100).message(lastModelQuarter: &quarter))
        XCTAssertEqual(
            DataInstallProgress.modelFetch(bytes: 50, total: 100).message(lastModelQuarter: &quarter),
            "変換モデルをダウンロードしています（50%）")
        XCTAssertNil(DataInstallProgress.dictFetch(file: "b", index: 2, count: 13).message(lastModelQuarter: &quarter))
    }
}
