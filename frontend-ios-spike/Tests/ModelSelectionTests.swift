import XCTest

/// 使うモデルの選び方（`DataPaths.selectModel`）。ホームの表示と、拡張が読むモデルが一致する決め方
final class ModelSelectionTests: XCTestCase {
    private func file(_ name: String, size: Int64 = 1) -> DataPaths.ModelFile {
        DataPaths.ModelFile(name: name, path: "/models/\(name)", sizeBytes: size)
    }

    private let small = "zenz-v3.1-small-Q5_K_M.gguf"
    private let xsmall = "zenz-v3.1-xsmall-Q5_K_M.gguf"

    private let mb: Int64 = 1024 * 1024

    /// 未選択で予算に余裕があれば small（推奨）
    func testUnselectedUsesRecommendedSmall() {
        let files = [file(small, size: 70 * mb), file(xsmall, size: 20 * mb)]
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: nil, budgetMB: 177)?.name, small)
    }

    /// 未選択で予算が小さければ xsmall、どれも載らなければもっとも小さいもの
    func testUnselectedFallsToXSmallOnSmallBudget() {
        let files = [file(small, size: 70 * mb), file(xsmall, size: 20 * mb)]
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: nil, budgetMB: 110)?.name, xsmall)
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: nil, budgetMB: 50)?.name, xsmall)
    }

    /// ユーザーが明示選択していれば、推奨に関係なくそれを使う
    func testUserSelectionWinsOverRecommendation() {
        let files = [file(small, size: 70 * mb), file(xsmall, size: 20 * mb)]
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: xsmall, budgetMB: 177)?.name, xsmall)
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: small, budgetMB: 110)?.name, small)
    }

    /// 選択したファイルが無くなっていたら（選択が残っているだけ）、推奨（予算なしなら既定名）に落ちる
    func testMissingSelectionFallsBackToDefault() {
        let files = [file(small, size: 70 * mb), file(xsmall, size: 20 * mb)]
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: "gone.gguf")?.name, small)
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: "gone.gguf", budgetMB: 110)?.name, xsmall)
    }

    func testDefaultNameThenLargest() {
        XCTAssertEqual(DataPaths.selectModel(from: [file("a.gguf", size: 5), file(small, size: 1)], preferredName: nil)?.name, small)
        XCTAssertEqual(DataPaths.selectModel(from: [file("a.gguf", size: 5), file("b.gguf", size: 9)], preferredName: nil)?.name, "b.gguf")
        XCTAssertNil(DataPaths.selectModel(from: [], preferredName: nil))
    }
}
