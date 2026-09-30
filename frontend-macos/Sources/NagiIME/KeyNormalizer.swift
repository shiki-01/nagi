import Foundation
import NagiClient

/// NSEvent から取り出した生の値。AppKit 型に依存しないので Linux でもテストできる。
public struct RawKeyInput: Equatable {
    public var keyCode: UInt16
    /// `NSEvent.characters`。flagsChanged では NSEvent から読めない（例外になる）ので空にする
    public var characters: String
    /// `NSEvent.charactersIgnoringModifiers`（Shift 以外の修飾を無視した文字）
    public var charactersIgnoringModifiers: String
    /// `NSEvent.modifierFlags.rawValue`
    public var modifierFlags: UInt
    public var isFlagsChanged: Bool

    public init(
        keyCode: UInt16, characters: String, charactersIgnoringModifiers: String,
        modifierFlags: UInt, isFlagsChanged: Bool
    ) {
        self.keyCode = keyCode
        self.characters = characters
        self.charactersIgnoringModifiers = charactersIgnoringModifiers
        self.modifierFlags = modifierFlags
        self.isFlagsChanged = isFlagsChanged
    }
}

/// `NSEvent.ModifierFlags` の生ビット（デバイス非依存部分）
public enum MacModifierFlag {
    public static let capsLock: UInt = 1 << 16
    public static let shift: UInt = 1 << 17
    public static let control: UInt = 1 << 18
    public static let option: UInt = 1 << 19
    public static let command: UInt = 1 << 20
}

/// macOS のキーイベント → プロトコルの `KeyEvent`（XKB keysym 正規化）。ロジックは持たず変換のみ。
public enum KeyNormalizer {
    // XKB keysym（core/protocol/src/key.rs の `keysym` と同値）
    enum Sym {
        static let space: UInt32 = 0x0020
        static let backspace: UInt32 = 0xff08
        static let tab: UInt32 = 0xff09
        static let `return`: UInt32 = 0xff0d
        static let escape: UInt32 = 0xff1b
        static let delete: UInt32 = 0xffff
        static let home: UInt32 = 0xff50
        static let left: UInt32 = 0xff51
        static let up: UInt32 = 0xff52
        static let right: UInt32 = 0xff53
        static let down: UInt32 = 0xff54
        static let pageUp: UInt32 = 0xff55
        static let pageDown: UInt32 = 0xff56
        static let end: UInt32 = 0xff57
        static let keypadEnter: UInt32 = 0xff8d
        static let f1: UInt32 = 0xffbe
        static let eisuToggle: UInt32 = 0xff2f
        static let hiraganaKatakana: UInt32 = 0xff27
        static let shiftL: UInt32 = 0xffe1
        static let shiftR: UInt32 = 0xffe2
        static let controlL: UInt32 = 0xffe3
        static let controlR: UInt32 = 0xffe4
        static let capsLock: UInt32 = 0xffe5
        static let altL: UInt32 = 0xffe9
        static let altR: UInt32 = 0xffea
        static let superL: UInt32 = 0xffeb
        static let superR: UInt32 = 0xffec
    }

    /// 印字文字を持たないキー（macOS の仮想キーコード → keysym）
    static let specialKeys: [UInt16: UInt32] = [
        0x24: Sym.return, 0x4C: Sym.keypadEnter, 0x30: Sym.tab, 0x31: Sym.space,
        0x33: Sym.backspace, 0x35: Sym.escape, 0x75: Sym.delete,
        0x73: Sym.home, 0x77: Sym.end, 0x74: Sym.pageUp, 0x79: Sym.pageDown,
        0x7B: Sym.left, 0x7C: Sym.right, 0x7D: Sym.down, 0x7E: Sym.up,
        0x66: Sym.eisuToggle, 0x68: Sym.hiraganaKatakana,
        // F1..F12（キーコードは連番ではない）
        0x7A: Sym.f1, 0x78: Sym.f1 + 1, 0x63: Sym.f1 + 2, 0x76: Sym.f1 + 3,
        0x60: Sym.f1 + 4, 0x61: Sym.f1 + 5, 0x62: Sym.f1 + 6, 0x64: Sym.f1 + 7,
        0x65: Sym.f1 + 8, 0x6D: Sym.f1 + 9, 0x67: Sym.f1 + 10, 0x6F: Sym.f1 + 11,
    ]

    /// flagsChanged で届く修飾キー（左右を区別）
    static let modifierKeys: [UInt16: UInt32] = [
        0x38: Sym.shiftL, 0x3C: Sym.shiftR,
        0x3B: Sym.controlL, 0x3E: Sym.controlR,
        0x3A: Sym.altL, 0x3D: Sym.altR,
        0x37: Sym.superL, 0x36: Sym.superR,
        0x39: Sym.capsLock,
    ]

    public static func modifiers(from flags: UInt) -> Modifiers {
        Modifiers(
            shift: flags & MacModifierFlag.shift != 0,
            ctrl: flags & MacModifierFlag.control != 0,
            alt: flags & MacModifierFlag.option != 0,
            meta: flags & MacModifierFlag.command != 0,
            caps_lock: flags & MacModifierFlag.capsLock != 0)
    }

    /// 正規化できないイベント（未知の flagsChanged 等）は nil。その場合フロントは素通しする
    public static func normalize(_ raw: RawKeyInput) -> KeyEvent? {
        let mods = modifiers(from: raw.modifierFlags)
        let keycode = UInt32(raw.keyCode)

        if raw.isFlagsChanged {
            guard let sym = modifierKeys[raw.keyCode] else { return nil }
            return KeyEvent(
                keysym: sym, keycode: keycode, text: "", modifiers: mods, kind: .flagsChanged)
        }

        // Ctrl / ⌘ を伴う場合は text を空にする（core の parse_key と同じ規約）。Option は OS が生成した文字を渡す
        let suppressText = mods.ctrl || mods.meta

        if let sym = specialKeys[raw.keyCode] {
            let text = (raw.keyCode == 0x31 && !suppressText) ? " " : ""
            return KeyEvent(keysym: sym, keycode: keycode, text: text, modifiers: mods, kind: .down)
        }

        let sym = raw.charactersIgnoringModifiers.unicodeScalars.first.flatMap(keysym(forScalar:)) ?? 0
        let text = suppressText ? "" : printableText(raw.characters)
        return KeyEvent(keysym: sym, keycode: keycode, text: text, modifiers: mods, kind: .down)
    }

    /// Unicode スカラー → keysym。Latin-1 はコードポイントと一致、それ以外は 0x01000000 + コードポイント。
    /// 制御文字と NSEvent の機能キー用私用領域（U+F700..U+F8FF）は nil
    static func keysym(forScalar scalar: Unicode.Scalar) -> UInt32? {
        let v = scalar.value
        if v < 0x20 || (0x7F...0x9F).contains(v) || (0xF700...0xF8FF).contains(v) { return nil }
        return v <= 0xFF ? v : 0x0100_0000 + v
    }

    /// 印字可能な文字だけを text にする（制御文字・機能キーの私用領域文字は空）
    static func printableText(_ s: String) -> String {
        guard let first = s.unicodeScalars.first else { return "" }
        return keysym(forScalar: first) == nil ? "" : s
    }
}
