import XCTest

final class HelperTests: XCTestCase {
    func testKanaTransformCycles() {
        XCTAssertEqual(KanaTransform.next(after: "か"), "が")
        XCTAssertEqual(KanaTransform.next(after: "が"), "か")
        XCTAssertEqual(KanaTransform.next(after: "は"), "ば")
        XCTAssertEqual(KanaTransform.next(after: "ば"), "ぱ")
        XCTAssertEqual(KanaTransform.next(after: "ぱ"), "は")
        XCTAssertEqual(KanaTransform.next(after: "つ"), "っ")
        XCTAssertEqual(KanaTransform.next(after: "っ"), "づ")
        XCTAssertEqual(KanaTransform.next(after: "あ"), "ぁ")
        XCTAssertNil(KanaTransform.next(after: "ん"))
        XCTAssertNil(KanaTransform.next(after: "a"))
    }

    func testDeleteCountByUTF16Length() {
        XCTAssertEqual(TextMath.deleteCount(utf16Length: 0, endingOf: "abc"), 0)
        XCTAssertEqual(TextMath.deleteCount(utf16Length: 2, endingOf: "日本語"), 2)
        // サロゲートペア（UTF-16 で 2 単位）は 1 文字
        XCTAssertEqual(TextMath.deleteCount(utf16Length: 2, endingOf: "a😀"), 1)
        XCTAssertEqual(TextMath.deleteCount(utf16Length: 3, endingOf: "a😀"), 2)
        // 足りない分は 1 単位 = 1 回
        XCTAssertEqual(TextMath.deleteCount(utf16Length: 5, endingOf: "ab"), 5)
    }

    func testKeysym() {
        XCTAssertEqual(Keysym.forText("a"), 0x61)
        XCTAssertEqual(Keysym.forText("A"), 0x41)
        XCTAssertEqual(Keysym.forText("ー"), 0)
        XCTAssertEqual(Keysym.forText("ab"), 0)
    }

    func testWireAlternatives() {
        XCTAssertNil(NagiRpcClient.wireAlternatives([]))
        let wire = NagiRpcClient.wireAlternatives([AltCandidate(text: "s", distance: 0.5)])
        XCTAssertEqual(wire?.count, 1)
        XCTAssertEqual(wire?[0].text, "s")
        XCTAssertEqual(wire?[0].distance ?? 0, 0.5, accuracy: 1e-6)
    }
}
