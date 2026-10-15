import XCTest

/// 端末のメモリ予算に応じたモデルの推奨（`NagiModelRecommendation`。decisions.md H-22）
final class ModelRecommendationTests: XCTestCase {
    private let mb: Int64 = 1024 * 1024

    private func file(_ name: String, mb size: Int64) -> DataPaths.ModelFile {
        DataPaths.ModelFile(name: name, path: "/models/\(name)", sizeBytes: size * mb)
    }

    private var small: DataPaths.ModelFile { file("zenz-v3.1-small-Q5_K_M.gguf", mb: 70) }
    private var xsmall: DataPaths.ModelFile { file("zenz-v3.1-xsmall-Q5_K_M.gguf", mb: 20) }

    /// 実機の拡張の上限（約 177MB）なら small
    func testRecommendsSmallOnMeasuredExtensionBudget() {
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 177, available: [xsmall, small])?.name, small.name)
    }

    /// 境界: small は 70 + 20 + 40 = 130MB 以上で余裕あり
    func testSmallBoundary() {
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 130, available: [small, xsmall])?.name, small.name)
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 129, available: [small, xsmall])?.name, xsmall.name)
    }

    /// xsmall は 20 + 20 + 40 = 80MB 以上。それ未満はどれも推奨できない
    func testNothingFitsOnTinyBudget() {
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 80, available: [small, xsmall])?.name, xsmall.name)
        XCTAssertNil(NagiModelRecommendation.recommend(budgetMB: 79, available: [small, xsmall]))
        XCTAssertNil(NagiModelRecommendation.recommend(budgetMB: 0, available: [small, xsmall]))
    }

    /// 1 つしかなければ、載るときだけそれを返す。空なら nil
    func testSingleAndEmpty() {
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 177, available: [small])?.name, small.name)
        XCTAssertNil(NagiModelRecommendation.recommend(budgetMB: 100, available: [small]))
        XCTAssertNil(NagiModelRecommendation.recommend(budgetMB: 177, available: []))
    }

    /// 配置順に依存しない。余裕のある最大のものを選ぶ
    func testIndependentOfOrderAndPicksLargestFitting() {
        let big = file("big-Q8.gguf", mb: 300)
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 177, available: [big, xsmall, small])?.name, small.name)
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 177, available: [small, xsmall, big])?.name, small.name)
        XCTAssertEqual(NagiModelRecommendation.recommend(budgetMB: 500, available: [small, big, xsmall])?.name, big.name)
    }

    func testSummaryText() {
        XCTAssertEqual(
            NagiModelRecommendation.summary(budgetMB: 177, available: [small, xsmall]),
            "推奨: small（この端末の予算 177MB に対して余裕あり）"
        )
        XCTAssertTrue(NagiModelRecommendation.summary(budgetMB: 60, available: [small, xsmall]).hasPrefix("推奨なし"))
        XCTAssertEqual(NagiModelRecommendation.summary(budgetMB: 177, available: []), "")
    }

    // MARK: LLM を読み込んでよいか（H-26）

    /// 境界: small は 70 + 40 = 110MB 以上の空きで読み込む（辞書・拡張本体は使用量に含まれるので、オーバーヘッドは足さない）
    func testCanLoadNowBoundary() {
        XCTAssertTrue(NagiModelRecommendation.canLoadNow(small, availableMB: 110))
        XCTAssertFalse(NagiModelRecommendation.canLoadNow(small, availableMB: 109.9))
        XCTAssertTrue(NagiModelRecommendation.canLoadNow(xsmall, availableMB: 60))
        XCTAssertFalse(NagiModelRecommendation.canLoadNow(xsmall, availableMB: 59))
    }

    /// 旧インスタンスがメモリを持っている間（空きが少ない）は待ち、解放されて空きが戻れば読み込む
    func testCanLoadNowWaitsWhileOldInstanceHoldsMemory() {
        XCTAssertFalse(NagiModelRecommendation.canLoadNow(small, availableMB: 60))
        XCTAssertTrue(NagiModelRecommendation.canLoadNow(small, availableMB: 140))
    }

    /// 起動直後の実測（拡張の上限 177MB で、辞書込みの使用が 30MB 前後 = 空き約 147MB）なら small も読み込める
    func testCanLoadNowOnTypicalFreshExtension() {
        XCTAssertTrue(NagiModelRecommendation.canLoadNow(small, availableMB: 147))
    }

    /// モデルが無いなら待つものがない
    func testCanLoadNowWithoutModel() {
        XCTAssertTrue(NagiModelRecommendation.canLoadNow(nil, availableMB: 0))
    }

    /// 読み込み判定は推奨の安全マージンと整合する: 予算全体で余裕あり（fits）なら、使用量が
    /// オーバーヘッド分以下の新規起動の空きで読み込める
    func testCanLoadNowConsistentWithFits() {
        let budget: UInt64 = 177
        XCTAssertTrue(NagiModelRecommendation.fits(small, budgetMB: budget))
        let available = Double(budget) - NagiModelRecommendation.runtimeOverheadMB
        XCTAssertTrue(NagiModelRecommendation.canLoadNow(small, availableMB: available))
    }

    // MARK: 簡易モードの注意の猶予

    func testNoticeHiddenWithinDelay() {
        let start = Date(timeIntervalSince1970: 1000)
        XCTAssertFalse(NagiModelRecommendation.shouldShowDictionaryOnlyNotice(since: start, now: start))
        XCTAssertFalse(NagiModelRecommendation.shouldShowDictionaryOnlyNotice(since: start, now: start.addingTimeInterval(10)))
    }

    func testNoticeShownAfterDelay() {
        let start = Date(timeIntervalSince1970: 1000)
        XCTAssertTrue(NagiModelRecommendation.shouldShowDictionaryOnlyNotice(since: start, now: start.addingTimeInterval(10.1)))
    }

    func testNoticeHiddenWhenNotDictionaryOnly() {
        XCTAssertFalse(NagiModelRecommendation.shouldShowDictionaryOnlyNotice(since: nil, now: Date()))
    }
}
