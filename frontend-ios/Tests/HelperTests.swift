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

    func testLoggableKanaAcceptsOnlyOneHiraganaOrProlongedSoundMark() {
        XCTAssertTrue(KanaText.isLoggableKana("あ"))
        XCTAssertTrue(KanaText.isLoggableKana("ん"))
        XCTAssertTrue(KanaText.isLoggableKana("ぁ"))
        XCTAssertTrue(KanaText.isLoggableKana("ー"))
        XCTAssertFalse(KanaText.isLoggableKana("("))
        XCTAssertFalse(KanaText.isLoggableKana("、"))
        XCTAssertFalse(KanaText.isLoggableKana("。"))
        XCTAssertFalse(KanaText.isLoggableKana("ア"))
        XCTAssertFalse(KanaText.isLoggableKana("a"))
        XCTAssertFalse(KanaText.isLoggableKana("あい"))
        XCTAssertFalse(KanaText.isLoggableKana(""))
    }

    func testLastFlickTransformMapsAlternativesWithTheSameCycle() {
        // か（代替 き 0.3・く 0.5、確定したゾーン自身 0.1）→ が（代替 ぎ・ぐ。距離はそのまま）
        let ka = LastFlick(reading: "か", alternatives: [
            AltCandidate(text: "き", distance: 0.3),
            AltCandidate(text: "く", distance: 0.5),
            AltCandidate(text: "か", distance: 0.1),
        ])
        XCTAssertEqual(ka.transformed(), LastFlick(reading: "が", alternatives: [
            AltCandidate(text: "ぎ", distance: 0.3),
            AltCandidate(text: "ぐ", distance: 0.5),
            AltCandidate(text: "が", distance: 0.1),
        ]))
    }

    func testLastFlickTransformDropsAlternativesWithoutTransform() {
        // つ → っ。て → で は写る。ん・ー は循環に無いので落ちる
        let tsu = LastFlick(reading: "つ", alternatives: [
            AltCandidate(text: "て", distance: 0.3),
            AltCandidate(text: "ん", distance: 0.5),
            AltCandidate(text: "ー", distance: 0.6),
            AltCandidate(text: "つ", distance: 0.1),
        ])
        XCTAssertEqual(tsu.transformed(), LastFlick(reading: "っ", alternatives: [
            AltCandidate(text: "で", distance: 0.3),
            AltCandidate(text: "っ", distance: 0.1),
        ]))
    }

    func testLastFlickTransformCyclesLikeDakutenKey() {
        // は → ば → ぱ → は
        var flick = LastFlick(reading: "は", alternatives: [
            AltCandidate(text: "ひ", distance: 0.5),
            AltCandidate(text: "は", distance: 0.1),
        ])
        var readings: [String] = []
        var firstAlternatives: [String] = []
        for _ in 0..<3 {
            guard let next = flick.transformed() else {
                XCTFail("変換できるはず")
                return
            }
            readings.append(next.reading)
            firstAlternatives.append(next.alternatives[0].text)
            flick = next
        }
        XCTAssertEqual(readings, ["ば", "ぱ", "は"])
        XCTAssertEqual(firstAlternatives, ["び", "ぴ", "ひ"])
        XCTAssertEqual(flick.alternatives.map { $0.distance }, [0.5, 0.1])
    }

    func testLastFlickTransformIsNilWhenReadingHasNoTransform() {
        XCTAssertNil(LastFlick(reading: "ん", alternatives: []).transformed())
        XCTAssertNil(LastFlick(reading: "かき", alternatives: []).transformed())
        XCTAssertNil(LastFlick(reading: "", alternatives: []).transformed())
        XCTAssertEqual(LastFlick(reading: "か", alternatives: []).transformed(),
                       LastFlick(reading: "が", alternatives: []))
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
