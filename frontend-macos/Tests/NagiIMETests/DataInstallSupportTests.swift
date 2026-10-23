import Foundation
import XCTest

@testable import NagiIME

final class DataInstallSupportTests: XCTestCase {
    func testParsesProgressLines() {
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"dict_fetch","file":"dictionary00.txt","index":1,"count":14}"#),
            .dictFetch(file: "dictionary00.txt", index: 1, count: 14, skipped: false))
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"count":14,"file":"a","index":2,"skipped":true,"stage":"dict_fetch"}"#),
            .dictFetch(file: "a", index: 2, count: 14, skipped: true))
        XCTAssertEqual(DataInstallProgress.parse(#"{"stage":"dict_build"}"#), .dictBuild)
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"model_fetch","bytes":10,"total":40}"#),
            .modelFetch(bytes: 10, total: 40, skipped: false))
        XCTAssertEqual(DataInstallProgress.parse(#"{"stage":"done","changed":false}"#), .done(changed: false))
        XCTAssertEqual(DataInstallProgress.parse(#"{"stage":"done"}"#), .done(changed: true))
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"error","message":"x"}"#), .error("x"))
        XCTAssertNil(DataInstallProgress.parse("ログの行"))
        XCTAssertNil(DataInstallProgress.parse(#"{"stage":"unknown"}"#))
        // 英語の語彙・外来語の索引の段階は、アプリ側の表示対象外（未知の種別として無視する）
        XCTAssertNil(DataInstallProgress.parse(#"{"stage":"english_fetch","bytes":10,"total":40}"#))
        XCTAssertNil(DataInstallProgress.parse(#"{"stage":"english_build"}"#))
        XCTAssertNil(DataInstallProgress.parse(#"{"stage":"loanword_fetch","skipped":true}"#))
    }

    func testSkippedStepsAreSummarizedAtTheEnd() {
        var quarter = -1
        XCTAssertNil(
            DataInstallProgress.dictFetch(file: "a", index: 1, count: 14, skipped: true)
                .message(lastModelQuarter: &quarter))
        XCTAssertNil(
            DataInstallProgress.modelFetch(bytes: 40, total: 40, skipped: true)
                .message(lastModelQuarter: &quarter))
        XCTAssertEqual(
            DataInstallProgress.done(changed: false).message(lastModelQuarter: &quarter),
            "辞書とモデルは最新です（取得済みのため、ダウンロードはしませんでした）")
    }

    func testModelProgressIsThinnedToQuarters() {
        var quarter = -1
        XCTAssertNotNil(
            DataInstallProgress.modelFetch(bytes: 0, total: 100, skipped: false)
                .message(lastModelQuarter: &quarter))
        XCTAssertNil(
            DataInstallProgress.modelFetch(bytes: 10, total: 100, skipped: false)
                .message(lastModelQuarter: &quarter))
        XCTAssertEqual(
            DataInstallProgress.modelFetch(bytes: 50, total: 100, skipped: false)
                .message(lastModelQuarter: &quarter),
            "変換モデルをダウンロードしています（50%）")
        XCTAssertEqual(
            DataInstallProgress.dictFetch(file: "b", index: 2, count: 14, skipped: false)
                .message(lastModelQuarter: &quarter),
            "辞書をダウンロードしています（2/14: b）")
    }
}
