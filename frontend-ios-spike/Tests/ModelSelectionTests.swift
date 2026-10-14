import XCTest

/// 使うモデルの選び方（`DataPaths.selectModel`）。ホームの表示と、拡張が読むモデルが一致する決め方
final class ModelSelectionTests: XCTestCase {
    private func file(_ name: String, size: Int64 = 1) -> DataPaths.ModelFile {
        DataPaths.ModelFile(name: name, path: "/models/\(name)", sizeBytes: size)
    }

    private let small = "zenz-v3.1-small-Q5_K_M.gguf"
    private let xsmall = "zenz-v3.1-xsmall-Q5_K_M.gguf"

    /// 未選択のときは xsmall を優先する
    func testUnselectedPrefersXSmall() {
        let files = [file(small, size: 70), file(xsmall, size: 30)]
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: nil)?.name, xsmall)
    }

    /// ユーザーが small を選んでいれば、xsmall があっても small
    func testUserSelectionWinsOverXSmall() {
        let files = [file(small, size: 70), file(xsmall, size: 30)]
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: small)?.name, small)
    }

    /// 選択したファイルが無くなっていたら（選択が残っているだけ）、既定の選び方に落ちる
    func testMissingSelectionFallsBackToDefault() {
        let files = [file(small, size: 70), file(xsmall, size: 30)]
        XCTAssertEqual(DataPaths.selectModel(from: files, preferredName: "gone.gguf")?.name, small)
    }

    func testDefaultNameThenLargest() {
        XCTAssertEqual(DataPaths.selectModel(from: [file("a.gguf", size: 5), file(small, size: 1)], preferredName: nil)?.name, small)
        XCTAssertEqual(DataPaths.selectModel(from: [file("a.gguf", size: 5), file("b.gguf", size: 9)], preferredName: nil)?.name, "b.gguf")
        XCTAssertNil(DataPaths.selectModel(from: [], preferredName: nil))
    }
}
