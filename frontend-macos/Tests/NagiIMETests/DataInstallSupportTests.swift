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
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"english_fetch","bytes":10,"total":40}"#),
            .englishFetch(bytes: 10, total: 40, skipped: false))
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"english_fetch","skipped":true}"#),
            .englishFetch(bytes: 0, total: 0, skipped: true))
        XCTAssertEqual(DataInstallProgress.parse(#"{"stage":"english_build"}"#), .englishBuild)
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"loanword_fetch","bytes":0}"#),
            .loanwordFetch(bytes: 0, total: 0, skipped: false))
        XCTAssertEqual(DataInstallProgress.parse(#"{"stage":"loanword_build"}"#), .loanwordBuild)
        XCTAssertEqual(
            DataInstallProgress.parse(#"{"stage":"warning","message":"x"}"#), .warning("x"))
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
            "辞書・モデル・語彙は最新です（取得済みのため、ダウンロードはしませんでした）")
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

    func testOptionalDataSteps() {
        var quarter = -1
        XCTAssertNil(
            DataInstallProgress.englishFetch(bytes: 0, total: 0, skipped: true)
                .message(lastModelQuarter: &quarter))
        XCTAssertNil(
            DataInstallProgress.loanwordFetch(bytes: 0, total: 0, skipped: true)
                .message(lastModelQuarter: &quarter))
        XCTAssertEqual(
            DataInstallProgress.englishFetch(bytes: 0, total: 100, skipped: false)
                .message(lastModelQuarter: &quarter),
            "英語の語彙をダウンロードしています")
        XCTAssertNil(
            DataInstallProgress.englishFetch(bytes: 50, total: 100, skipped: false)
                .message(lastModelQuarter: &quarter))
        XCTAssertEqual(
            DataInstallProgress.loanwordFetch(bytes: 0, total: 0, skipped: false)
                .message(lastModelQuarter: &quarter),
            "カタカナ語の原語データをダウンロードしています")
        XCTAssertEqual(
            DataInstallProgress.loanwordBuild.message(lastModelQuarter: &quarter),
            "カタカナ語の原語データを組み立てています")
        XCTAssertEqual(
            DataInstallProgress.warning("x").message(lastModelQuarter: &quarter),
            "辞書とモデルは入れましたが、一部のデータを入れられませんでした: x")
        // 任意のデータの進捗は、モデルの進捗の間引きに影響しない
        XCTAssertEqual(quarter, -1)
    }

    func testDataLinesGuideTheInstallWhenMissing() {
        XCTAssertTrue(StatusMessages.dataLines(hasEnglish: true, hasLoanword: true).isEmpty)
        XCTAssertEqual(StatusMessages.dataLines(hasEnglish: false, hasLoanword: true).count, 1)
        let both = StatusMessages.dataLines(hasEnglish: false, hasLoanword: false)
        XCTAssertEqual(both.count, 2)
        XCTAssertTrue(both[1].contains("カタカナ語の原語データ: 未導入"))
    }
}
