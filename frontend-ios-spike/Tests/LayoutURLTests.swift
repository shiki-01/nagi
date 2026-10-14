import XCTest

/// nagi://layout?data=<base64url の JSON> の解釈（`Shared/LayoutURL.swift`）
final class LayoutURLTests: XCTestCase {
    private func url(_ data: String) -> URL {
        URL(string: "nagi://layout?data=\(data)")!
    }

    /// JSON → URL → JSON の往復で、元のバイト列に戻る（日本語・記号を含む）
    func testBase64URLRoundTrip() throws {
        let json = #"{"schema":1,"id":"test","name":"テスト???>>>~~","mode":"kana","rows":[]}"#
        let original = Data(json.utf8)
        let built = try XCTUnwrap(NagiLayoutURL.url(forJSON: original))
        let encoded = try XCTUnwrap(URLComponents(url: built, resolvingAgainstBaseURL: false)?.queryItems?.first?.value)
        XCTAssertFalse(encoded.contains("+"))
        XCTAssertFalse(encoded.contains("/"))
        XCTAssertFalse(encoded.contains("="))
        XCTAssertEqual(try NagiLayoutURL.jsonData(fromURL: built), original)
    }

    /// base64 の長さが 4 の倍数にならないデータ（パディングなし）も読める。1〜3 バイト余る場合をすべて試す
    func testRoundTripForEveryPaddingLength() throws {
        for count in 1...12 {
            let original = Data((0..<count).map { UInt8(truncatingIfNeeded: 0xF0 &+ $0 &* 7) })
            let built = try XCTUnwrap(NagiLayoutURL.url(forJSON: original))
            XCTAssertEqual(try NagiLayoutURL.jsonData(fromURL: built), original, "count=\(count)")
        }
    }

    /// `-` `_` は `+` `/` として読む（0xFB 0xFF は標準の base64 で "+/" を含む）
    func testURLSafeAlphabet() throws {
        let original = Data([0xFB, 0xFF, 0xFE])
        XCTAssertEqual(original.base64EncodedString(), "+//+")
        XCTAssertEqual(try NagiLayoutURL.jsonData(fromURL: url("-__-")), original)
    }

    func testRejectsOtherSchemeOrHost() {
        XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: URL(string: "https://layout?data=AAAA")!))
        XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: URL(string: "nagi://other?data=AAAA")!))
    }

    func testRejectsMissingOrEmptyData() {
        XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: URL(string: "nagi://layout")!))
        XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: URL(string: "nagi://layout?x=1")!))
        XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: url("")))
    }

    func testRejectsInvalidBase64() {
        XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: url("!!!!")))
    }

    /// 上限を超える data は、デコードせずに拒否する
    func testRejectsOversizedData() {
        let big = String(repeating: "A", count: NagiLayoutURL.maxDataChars + 4)
        XCTAssertThrowsError(try NagiLayoutURL.jsonData(fromURL: url(big))) { error in
            XCTAssertTrue((error as? NagiLayoutError)?.message.contains("大きすぎ") ?? false)
        }
    }

    /// 上限ちょうどは通る（4 の倍数の長さ）
    func testAcceptsDataAtLimit() throws {
        let atLimit = String(repeating: "A", count: NagiLayoutURL.maxDataChars)
        XCTAssertEqual(try NagiLayoutURL.jsonData(fromURL: url(atLimit)).count, NagiLayoutURL.maxDataChars / 4 * 3)
    }
}
