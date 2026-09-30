import Foundation
import NagiClient
import XCTest

@testable import NagiIME

final class KeyNormalizerTests: XCTestCase {
    private func down(
        _ code: UInt16, _ chars: String = "", ignoring: String? = nil, flags: UInt = 0
    ) -> RawKeyInput {
        RawKeyInput(
            keyCode: code, characters: chars, charactersIgnoringModifiers: ignoring ?? chars,
            modifierFlags: flags, isFlagsChanged: false)
    }

    func testPlainLetter() throws {
        let k = try XCTUnwrap(KeyNormalizer.normalize(down(0x00, "a")))
        XCTAssertEqual(k.keysym, 0x61)
        XCTAssertEqual(k.text, "a")
        XCTAssertEqual(k.keycode, 0)
        XCTAssertEqual(k.kind, .down)
        XCTAssertFalse(k.modifiers.shift)
    }

    func testShiftLetterKeepsUppercase() throws {
        let k = try XCTUnwrap(
            KeyNormalizer.normalize(down(0x00, "A", flags: MacModifierFlag.shift)))
        XCTAssertEqual(k.keysym, 0x41)
        XCTAssertEqual(k.text, "A")
        XCTAssertTrue(k.modifiers.shift)
    }

    func testCtrlSuppressesTextButKeepsKeysym() throws {
        // Ctrl+A は characters が制御文字（U+0001）、charactersIgnoringModifiers は "a"
        let k = try XCTUnwrap(
            KeyNormalizer.normalize(
                down(0x00, "\u{01}", ignoring: "a", flags: MacModifierFlag.control)))
        XCTAssertEqual(k.keysym, 0x61)
        XCTAssertEqual(k.text, "")
        XCTAssertTrue(k.modifiers.ctrl)
    }

    func testOptionKeepsOsGeneratedText() throws {
        let k = try XCTUnwrap(
            KeyNormalizer.normalize(down(0x00, "å", ignoring: "a", flags: MacModifierFlag.option)))
        XCTAssertEqual(k.keysym, 0x61)
        XCTAssertEqual(k.text, "å")
        XCTAssertTrue(k.modifiers.alt)
    }

    func testSpecialKeys() throws {
        let space = try XCTUnwrap(KeyNormalizer.normalize(down(0x31, " ")))
        XCTAssertEqual(space.keysym, 0x20)
        XCTAssertEqual(space.text, " ")
        let ret = try XCTUnwrap(KeyNormalizer.normalize(down(0x24, "\r")))
        XCTAssertEqual(ret.keysym, 0xff0d)
        XCTAssertEqual(ret.text, "")
        let bs = try XCTUnwrap(KeyNormalizer.normalize(down(0x33, "\u{7F}")))
        XCTAssertEqual(bs.keysym, 0xff08)
        let esc = try XCTUnwrap(KeyNormalizer.normalize(down(0x35, "\u{1B}")))
        XCTAssertEqual(esc.keysym, 0xff1b)
        // 矢印は characters が私用領域（U+F702 等）
        let left = try XCTUnwrap(KeyNormalizer.normalize(down(0x7B, "\u{F702}")))
        XCTAssertEqual(left.keysym, 0xff51)
        XCTAssertEqual(left.text, "")
    }

    func testShiftArrowAndCtrlShiftArrow() throws {
        let flags = MacModifierFlag.shift | MacModifierFlag.control
        let k = try XCTUnwrap(KeyNormalizer.normalize(down(0x7C, "\u{F703}", flags: flags)))
        XCTAssertEqual(k.keysym, 0xff53)
        XCTAssertTrue(k.modifiers.shift)
        XCTAssertTrue(k.modifiers.ctrl)
    }

    func testJisEisuAndKana() throws {
        XCTAssertEqual(try XCTUnwrap(KeyNormalizer.normalize(down(0x66))).keysym, 0xff2f)
        XCTAssertEqual(try XCTUnwrap(KeyNormalizer.normalize(down(0x68))).keysym, 0xff27)
    }

    func testFunctionKeys() throws {
        XCTAssertEqual(try XCTUnwrap(KeyNormalizer.normalize(down(0x7A, "\u{F704}"))).keysym, 0xffbe)
        XCTAssertEqual(try XCTUnwrap(KeyNormalizer.normalize(down(0x6F, "\u{F70F}"))).keysym, 0xffc9)
    }

    func testNonLatin1UsesUnicodeKeysym() throws {
        let k = try XCTUnwrap(KeyNormalizer.normalize(down(0x5D, "¥")))
        XCTAssertEqual(k.keysym, 0xA5)
        let kana = try XCTUnwrap(KeyNormalizer.normalize(down(0x00, "あ")))
        XCTAssertEqual(kana.keysym, 0x0100_0000 + 0x3042)
    }

    func testFlagsChanged() throws {
        let raw = RawKeyInput(
            keyCode: 0x36, characters: "", charactersIgnoringModifiers: "",
            modifierFlags: MacModifierFlag.command, isFlagsChanged: true)
        let k = try XCTUnwrap(KeyNormalizer.normalize(raw))
        XCTAssertEqual(k.kind, .flagsChanged)
        XCTAssertEqual(k.keysym, 0xffec)  // 右 ⌘ = Super_R
        XCTAssertTrue(k.modifiers.meta)
        XCTAssertEqual(k.text, "")
    }

    func testUnknownFlagsChangedIsNil() {
        let raw = RawKeyInput(
            keyCode: 0x3F, characters: "", charactersIgnoringModifiers: "", modifierFlags: 0,
            isFlagsChanged: true)
        XCTAssertNil(KeyNormalizer.normalize(raw))
    }

    func testCapsLockModifier() throws {
        let k = try XCTUnwrap(KeyNormalizer.normalize(down(0x00, "A", flags: MacModifierFlag.capsLock)))
        XCTAssertTrue(k.modifiers.caps_lock)
    }
}
