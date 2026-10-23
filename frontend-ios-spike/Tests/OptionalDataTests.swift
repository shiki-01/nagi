import XCTest

/// 任意データ（英語の語彙・カタカナ語の原語）の取得元の解釈と、取得するかの判断（`OptionalData`）
final class OptionalDataTests: XCTestCase {
    private let json = """
        {"english":{"dir":"english","file":"scowl.tar.gz","url":"https://example.com/s.tar.gz",
        "sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","index_file":"words.fst"},
        "loanword":{"dir":"loanword","file":"JMdict_e.gz","url":"https://example.com/j.gz",
        "sha256":null,"index_file":"loanword.fst"}}
        """

    private func tempDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("nagi-optional-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testParsesSourcesIncludingUnpinnedHash() {
        let sources = OptionalData.parseSources(json)
        XCTAssertEqual(sources.count, 2)
        XCTAssertEqual(sources[.english]?.indexFile, "words.fst")
        XCTAssertEqual(sources[.english]?.isDownloadable, true)
        XCTAssertNil(sources[.loanword]?.sha256)
        XCTAssertEqual(sources[.loanword]?.isDownloadable, false)
    }

    func testBrokenJsonGivesNoSources() {
        XCTAssertTrue(OptionalData.parseSources("not json").isEmpty)
        XCTAssertTrue(OptionalData.parseSources(#"{"english":{"dir":"english"}}"#).isEmpty)
    }

    func testSha256Hex() throws {
        let dir = try tempDir()
        let file = dir.appendingPathComponent("abc")
        try Data("abc".utf8).write(to: file)
        XCTAssertEqual(
            OptionalData.sha256Hex(of: file),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        XCTAssertNil(OptionalData.sha256Hex(of: dir.appendingPathComponent("none")))
    }

    func testStepDownloadsSkipsAndRebuilds() throws {
        let dataDir = try tempDir()
        let source = try XCTUnwrap(OptionalData.parseSources(json)[.english])
        // 何も無ければ取得する
        XCTAssertEqual(OptionalData.step(for: source, in: dataDir), .download)
        // アーカイブ（SHA-256 一致）だけあれば索引の組み立てだけ
        let dir = OptionalData.directory(source, in: dataDir)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try Data("abc".utf8).write(to: OptionalData.archiveURL(source, in: dataDir))
        XCTAssertEqual(OptionalData.step(for: source, in: dataDir), .buildOnly)
        // 索引もあれば飛ばす
        try Data([0]).write(to: OptionalData.indexURL(source, in: dataDir))
        XCTAssertEqual(OptionalData.step(for: source, in: dataDir), .skip)
        XCTAssertTrue(OptionalData.isInstalled(source, in: dataDir))
        // アーカイブが壊れていれば取り直す
        try Data("xyz".utf8).write(to: OptionalData.archiveURL(source, in: dataDir))
        XCTAssertEqual(OptionalData.step(for: source, in: dataDir), .download)
    }

    func testUnpinnedSourceIsNeverFetched() throws {
        let dataDir = try tempDir()
        let source = try XCTUnwrap(OptionalData.parseSources(json)[.loanword])
        XCTAssertEqual(OptionalData.step(for: source, in: dataDir), .unavailable)
        XCTAssertTrue(OptionalData.statusText(source, in: dataDir).contains("未固定"))
        XCTAssertEqual(OptionalData.statusText(nil, in: dataDir), "確認できません")
    }
}
