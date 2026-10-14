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
}
