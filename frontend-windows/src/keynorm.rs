//! Windows のキーイベント → プロトコルの `KeyEvent`（XKB keysym 正規化）。
//!
//! ロジックは持たず変換のみ（macOS の `KeyNormalizer.swift` と同じ規約）。
//! Win32 型に依存せず、仮想キーは `u16` の定数で自前定義する。

use nagi_protocol::key::keysym;
use nagi_protocol::{KeyEvent, KeyKind, Modifiers};

/// Windows 仮想キーコード（`VK_*`）。値は winuser.h と同じ
pub mod vk {
    /// VK_BACK
    pub const BACK: u16 = 0x08;
    /// VK_TAB
    pub const TAB: u16 = 0x09;
    /// VK_RETURN（拡張キーフラグ付きはテンキー Enter）
    pub const RETURN: u16 = 0x0D;
    /// VK_SHIFT（左右の区別なし。TSF は左右を VK_SHIFT + スキャンコードで渡す）
    pub const SHIFT: u16 = 0x10;
    /// VK_CONTROL（左右の区別なし）
    pub const CONTROL: u16 = 0x11;
    /// VK_MENU = Alt（左右の区別なし）
    pub const MENU: u16 = 0x12;
    /// VK_CAPITAL
    pub const CAPITAL: u16 = 0x14;
    /// VK_KANA（= VK_HANGUL）。カタカナひらがなローマ字キー
    pub const KANA: u16 = 0x15;
    /// VK_KANJI（= VK_HANJA）。半角/全角キー
    pub const KANJI: u16 = 0x19;
    /// VK_IME_ON。IME をオンにする（Win10 以降、かなキーや IME 切替アプリが送る）
    pub const IME_ON: u16 = 0x16;
    /// VK_IME_OFF。IME をオフにする（英数相当）
    pub const IME_OFF: u16 = 0x1A;
    /// VK_ESCAPE
    pub const ESCAPE: u16 = 0x1B;
    /// VK_CONVERT。変換キー
    pub const CONVERT: u16 = 0x1C;
    /// VK_NONCONVERT。無変換キー
    pub const NONCONVERT: u16 = 0x1D;
    /// VK_SPACE
    pub const SPACE: u16 = 0x20;
    /// VK_PRIOR = Page Up
    pub const PRIOR: u16 = 0x21;
    /// VK_NEXT = Page Down
    pub const NEXT: u16 = 0x22;
    /// VK_END
    pub const END: u16 = 0x23;
    /// VK_HOME
    pub const HOME: u16 = 0x24;
    /// VK_LEFT
    pub const LEFT: u16 = 0x25;
    /// VK_UP
    pub const UP: u16 = 0x26;
    /// VK_RIGHT
    pub const RIGHT: u16 = 0x27;
    /// VK_DOWN
    pub const DOWN: u16 = 0x28;
    /// VK_DELETE
    pub const DELETE: u16 = 0x2E;
    /// VK_LWIN
    pub const LWIN: u16 = 0x5B;
    /// VK_RWIN
    pub const RWIN: u16 = 0x5C;
    /// VK_F1（F12 までは連番 0x70..=0x7B）
    pub const F1: u16 = 0x70;
    /// VK_F12
    pub const F12: u16 = 0x7B;
    /// VK_LSHIFT
    pub const LSHIFT: u16 = 0xA0;
    /// VK_RSHIFT
    pub const RSHIFT: u16 = 0xA1;
    /// VK_LCONTROL
    pub const LCONTROL: u16 = 0xA2;
    /// VK_RCONTROL
    pub const RCONTROL: u16 = 0xA3;
    /// VK_LMENU
    pub const LMENU: u16 = 0xA4;
    /// VK_RMENU
    pub const RMENU: u16 = 0xA5;
    /// VK_OEM_ATTN。英数キー
    pub const OEM_ATTN: u16 = 0xF0;
    /// VK_OEM_FINISH。カタカナキー
    pub const OEM_FINISH: u16 = 0xF1;
    /// VK_OEM_COPY。ひらがなキー
    pub const OEM_COPY: u16 = 0xF2;
    /// VK_OEM_AUTO。半角/全角キー（押下側）
    pub const OEM_AUTO: u16 = 0xF3;
    /// VK_OEM_ENLW。半角/全角キー（離上側）
    pub const OEM_ENLW: u16 = 0xF4;
    /// VK_OEM_BACKTAB。ローマ字（ひらがな）キー相当。実機未確認
    pub const OEM_BACKTAB: u16 = 0xF5;
}

/// 右 Shift のスキャンコード（VK_SHIFT の左右判別に使う）
const SCAN_RSHIFT: u16 = 0x36;
/// テンキー Enter の keysym（KP_Enter）
const KP_ENTER: u32 = 0xff8d;
/// Latin-1 を超える Unicode の keysym オフセット
const UNICODE_KEYSYM_BASE: u32 = 0x0100_0000;

/// TSF のキーイベントから取り出した生の値。Win32 型に依存しないので Linux でもテストできる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawKeyInput {
    /// 仮想キーコード（`vk::*`）
    pub vk: u16,
    /// スキャンコード（lParam 16..=23 ビット）。VK_SHIFT の左右判別に使う
    pub scan_code: u16,
    /// 拡張キーフラグ（lParam 24 ビット）。右 Ctrl/Alt・テンキー Enter の判別に使う
    pub extended: bool,
    /// キー離上（WM_KEYUP 相当）か
    pub is_key_up: bool,
    /// この押下で OS が生成したテキスト（`ToUnicodeEx` の結果）。生成されなければ空
    pub text: String,
    /// Ctrl/Alt/Win を無視して Shift だけ反映した文字（macOS の `charactersIgnoringModifiers` 相当）。
    /// 取得できなければ空（英数字は VK から補う）
    pub unmodified_text: String,
    /// 修飾キーの状態（`meta` は Win キー）。flags_changed では変化後の状態
    pub modifiers: Modifiers,
}

/// 修飾キーの VK → 左右を区別した keysym。generic な VK_SHIFT/CONTROL/MENU は拡張フラグ等で判別する
fn modifier_keysym(raw: &RawKeyInput) -> Option<u32> {
    Some(match raw.vk {
        vk::LSHIFT => keysym::SHIFT_L,
        vk::RSHIFT => keysym::SHIFT_R,
        vk::LCONTROL => keysym::CONTROL_L,
        vk::RCONTROL => keysym::CONTROL_R,
        vk::LMENU => keysym::ALT_L,
        vk::RMENU => keysym::ALT_R,
        vk::LWIN => keysym::SUPER_L,
        vk::RWIN => keysym::SUPER_R,
        vk::CAPITAL => keysym::CAPS_LOCK,
        vk::SHIFT if raw.scan_code == SCAN_RSHIFT => keysym::SHIFT_R,
        vk::SHIFT => keysym::SHIFT_L,
        vk::CONTROL if raw.extended => keysym::CONTROL_R,
        vk::CONTROL => keysym::CONTROL_L,
        vk::MENU if raw.extended => keysym::ALT_R,
        vk::MENU => keysym::ALT_L,
        _ => return None,
    })
}

/// 印字文字を持たないキー（テンキー Enter は拡張フラグで別扱い）
fn special_keysym(raw: &RawKeyInput) -> Option<u32> {
    Some(match raw.vk {
        vk::RETURN if raw.extended => KP_ENTER,
        vk::RETURN => keysym::RETURN,
        vk::TAB => keysym::TAB,
        vk::SPACE => keysym::SPACE,
        vk::BACK => keysym::BACKSPACE,
        vk::ESCAPE => keysym::ESCAPE,
        vk::DELETE => keysym::DELETE,
        vk::HOME => keysym::HOME,
        vk::END => keysym::END,
        vk::PRIOR => keysym::PAGE_UP,
        vk::NEXT => keysym::PAGE_DOWN,
        vk::LEFT => keysym::LEFT,
        vk::RIGHT => keysym::RIGHT,
        vk::UP => keysym::UP,
        vk::DOWN => keysym::DOWN,
        v @ vk::F1..=vk::F12 => keysym::F1 + u32::from(v - vk::F1),
        // 日本語キー
        vk::CONVERT => keysym::HENKAN,
        vk::NONCONVERT => keysym::MUHENKAN,
        vk::KANA => keysym::HIRAGANA_KATAKANA,
        vk::KANJI | vk::OEM_AUTO | vk::OEM_ENLW => keysym::ZENKAKU_HANKAKU,
        vk::OEM_ATTN => keysym::EISU_TOGGLE,
        vk::OEM_FINISH => keysym::KATAKANA,
        vk::OEM_COPY => keysym::HIRAGANA,
        // macOS の eisuToggle / かな に倣う。IME_ON は「ひらがなモードへ」（明示。ToggleMode にしない:
        // 状態同期の往復で反転しないように）、IME_OFF は英数モードへ。コアの keymap が
        // EISU_TOGGLE → SetMode(Alphanumeric)、HIRAGANA → SetMode(Hiragana) と解決する
        vk::IME_ON => keysym::HIRAGANA,
        vk::IME_OFF => keysym::EISU_TOGGLE,
        // かなキー（macOS の 0x68 hiraganaKatakana）と同じ扱い
        vk::OEM_BACKTAB => keysym::HIRAGANA_KATAKANA,
        _ => return None,
    })
}

/// 正規化できないイベント（キー離上・未知の flags_changed 等）は `None`。その場合フロントは素通しする
pub fn normalize(raw: &RawKeyInput) -> Option<KeyEvent> {
    let keycode = u32::from(raw.vk);

    if let Some(sym) = modifier_keysym(raw) {
        // 修飾キーは押下・離上の両方を flags_changed として送る（macOS と同じ）
        return Some(KeyEvent {
            keysym: sym,
            keycode,
            text: String::new(),
            modifiers: raw.modifiers,
            kind: KeyKind::FlagsChanged,
        });
    }
    if raw.is_key_up {
        return None;
    }

    let suppress_text = suppresses_text(&raw.modifiers);

    if let Some(sym) = special_keysym(raw) {
        let text = if raw.vk == vk::SPACE && !suppress_text {
            " ".to_string()
        } else {
            String::new()
        };
        return Some(down(sym, keycode, text, raw.modifiers));
    }

    let sym = raw
        .unmodified_text
        .chars()
        .next()
        .and_then(keysym_for_char)
        .or_else(|| keysym_from_vk(raw))
        .unwrap_or(0);
    let text = if suppress_text {
        String::new()
    } else {
        printable_text(&raw.text).to_string()
    };
    Some(down(sym, keycode, text, raw.modifiers))
}

fn down(keysym: u32, keycode: u32, text: String, modifiers: Modifiers) -> KeyEvent {
    KeyEvent {
        keysym,
        keycode,
        text,
        modifiers,
        kind: KeyKind::Down,
    }
}

/// Ctrl / Win を伴う場合は text を空にする（core の `parse_key` と同じ規約）。
/// ただし Windows の AltGr は Ctrl+Alt として届くため、両方が立っているときは抑止しない
/// （AltGr 配列の `@` 等を失わないため。Win が伴えば抑止する）
fn suppresses_text(m: &Modifiers) -> bool {
    m.meta || (m.ctrl && !m.alt)
}

/// Unicode 文字 → keysym。Latin-1 はコードポイントと一致、それ以外は 0x01000000 + コードポイント。
/// 制御文字（C0 / DEL / C1）は `None`
pub fn keysym_for_char(c: char) -> Option<u32> {
    let v = u32::from(c);
    if v < 0x20 || (0x7F..=0x9F).contains(&v) {
        return None;
    }
    Some(if v <= 0xFF {
        v
    } else {
        UNICODE_KEYSYM_BASE + v
    })
}

/// 印字可能な文字で始まる場合だけ text を返す（制御文字は空）
fn printable_text(s: &str) -> &str {
    match s.chars().next() {
        Some(c) if keysym_for_char(c).is_some() => s,
        _ => "",
    }
}

/// `unmodified_text` が空のときの補完。英字は Shift と Caps Lock の排他的論理和で大小を決める。
/// 数字は Shift 無しのときだけ（Shift 時の記号は配列依存なので補えない）
fn keysym_from_vk(raw: &RawKeyInput) -> Option<u32> {
    let m = &raw.modifiers;
    match raw.vk {
        v @ 0x41..=0x5A => {
            let upper = m.shift != m.caps_lock;
            let base = if upper { v } else { v + 0x20 };
            Some(u32::from(base))
        }
        v @ 0x30..=0x39 if !m.shift => Some(u32::from(v)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(vk: u16, text: &str) -> RawKeyInput {
        RawKeyInput {
            vk,
            scan_code: 0,
            extended: false,
            is_key_up: false,
            text: text.to_string(),
            unmodified_text: text.to_string(),
            modifiers: Modifiers::default(),
        }
    }

    fn with_mods(mut r: RawKeyInput, f: impl FnOnce(&mut Modifiers)) -> RawKeyInput {
        f(&mut r.modifiers);
        r
    }

    #[test]
    fn plain_letter() {
        let e = normalize(&raw(0x41, "a")).unwrap();
        assert_eq!(e.keysym, 0x61);
        assert_eq!(e.keycode, 0x41);
        assert_eq!(e.text, "a");
        assert_eq!(e.kind, KeyKind::Down);
        assert_eq!(e.modifiers, Modifiers::default());
    }

    #[test]
    fn shift_letter_keeps_uppercase() {
        let r = with_mods(raw(0x41, "A"), |m| m.shift = true);
        let e = normalize(&r).unwrap();
        assert_eq!(e.keysym, 0x41);
        assert_eq!(e.text, "A");
        assert!(e.modifiers.shift);
    }

    #[test]
    fn ctrl_suppresses_text_but_keeps_keysym() {
        // Ctrl+A は OS が 0x01 を生成するが、unmodified_text は 'a'
        let mut r = with_mods(raw(0x41, "\u{1}"), |m| m.ctrl = true);
        r.unmodified_text = "a".into();
        let e = normalize(&r).unwrap();
        assert_eq!(e.keysym, 0x61);
        assert_eq!(e.text, "");
        assert!(e.modifiers.ctrl);
    }

    #[test]
    fn win_key_suppresses_text() {
        let r = with_mods(raw(0x41, "a"), |m| m.meta = true);
        let e = normalize(&r).unwrap();
        assert_eq!(e.keysym, 0x61);
        assert_eq!(e.text, "");
        assert!(e.modifiers.meta);
    }

    #[test]
    fn alt_keeps_os_generated_text() {
        let r = with_mods(raw(0x41, "a"), |m| m.alt = true);
        let e = normalize(&r).unwrap();
        assert_eq!(e.text, "a");
        assert!(e.modifiers.alt);
    }

    #[test]
    fn altgr_is_ctrl_plus_alt_and_keeps_text() {
        let r = with_mods(raw(0x51, "@"), |m| {
            m.ctrl = true;
            m.alt = true;
        });
        let e = normalize(&r).unwrap();
        assert_eq!(e.keysym, 0x40);
        assert_eq!(e.text, "@");
        // Win を伴えば抑止する
        let r = with_mods(raw(0x51, "@"), |m| {
            m.ctrl = true;
            m.alt = true;
            m.meta = true;
        });
        assert_eq!(normalize(&r).unwrap().text, "");
    }

    #[test]
    fn special_keys() {
        let cases: &[(u16, u32)] = &[
            (vk::RETURN, 0xff0d),
            (vk::TAB, 0xff09),
            (vk::SPACE, 0x20),
            (vk::BACK, 0xff08),
            (vk::ESCAPE, 0xff1b),
            (vk::DELETE, 0xffff),
            (vk::HOME, 0xff50),
            (vk::END, 0xff57),
            (vk::PRIOR, 0xff55),
            (vk::NEXT, 0xff56),
            (vk::LEFT, 0xff51),
            (vk::UP, 0xff52),
            (vk::RIGHT, 0xff53),
            (vk::DOWN, 0xff54),
        ];
        for &(v, sym) in cases {
            let e = normalize(&raw(v, "")).unwrap();
            assert_eq!(e.keysym, sym, "vk {v:#x}");
            assert_eq!(e.kind, KeyKind::Down);
        }
        // 特殊キーは OS 生成テキストを無視する（Enter の "\r" 等）
        assert_eq!(normalize(&raw(vk::RETURN, "\r")).unwrap().text, "");
        assert_eq!(normalize(&raw(vk::TAB, "\t")).unwrap().text, "");
        assert_eq!(normalize(&raw(vk::BACK, "\u{8}")).unwrap().text, "");
    }

    #[test]
    fn space_text_depends_on_ctrl() {
        assert_eq!(normalize(&raw(vk::SPACE, " ")).unwrap().text, " ");
        let r = with_mods(raw(vk::SPACE, " "), |m| m.ctrl = true);
        let e = normalize(&r).unwrap();
        assert_eq!(e.keysym, 0x20);
        assert_eq!(e.text, "");
    }

    #[test]
    fn keypad_enter_uses_extended_flag() {
        let mut r = raw(vk::RETURN, "");
        r.extended = true;
        assert_eq!(normalize(&r).unwrap().keysym, 0xff8d);
    }

    #[test]
    fn shift_arrow_and_ctrl_shift_arrow() {
        let r = with_mods(raw(vk::RIGHT, ""), |m| m.shift = true);
        let e = normalize(&r).unwrap();
        assert_eq!(e.keysym, 0xff53);
        assert!(e.modifiers.shift && !e.modifiers.ctrl);
        let r = with_mods(raw(vk::LEFT, ""), |m| {
            m.shift = true;
            m.ctrl = true;
        });
        let e = normalize(&r).unwrap();
        assert_eq!(e.keysym, 0xff51);
        assert!(e.modifiers.shift && e.modifiers.ctrl);
    }

    #[test]
    fn function_keys() {
        for n in 0..12u16 {
            let e = normalize(&raw(vk::F1 + n, "")).unwrap();
            assert_eq!(e.keysym, 0xffbe + u32::from(n));
        }
        assert_eq!(normalize(&raw(vk::F12, "")).unwrap().keysym, 0xffc9);
        // F13 は特殊キー表に無いので keysym 0
        assert_eq!(normalize(&raw(0x7C, "")).unwrap().keysym, 0);
    }

    #[test]
    fn japanese_keys() {
        let cases: &[(u16, u32)] = &[
            (vk::CONVERT, 0xff23),
            (vk::NONCONVERT, 0xff22),
            (vk::KANA, 0xff27),
            (vk::KANJI, 0xff2a),
            (vk::OEM_AUTO, 0xff2a),
            (vk::OEM_ENLW, 0xff2a),
            (vk::OEM_ATTN, 0xff2f),
            (vk::OEM_FINISH, 0xff26),
            (vk::OEM_COPY, 0xff25),
            (vk::IME_ON, 0xff25),
            (vk::IME_OFF, 0xff2f),
            (vk::OEM_BACKTAB, 0xff27),
        ];
        for &(v, sym) in cases {
            let e = normalize(&raw(v, "")).unwrap();
            assert_eq!(e.keysym, sym, "vk {v:#x}");
            assert_eq!(e.kind, KeyKind::Down);
            assert_eq!(e.text, "");
        }
    }

    #[test]
    fn non_latin1_uses_unicode_keysym() {
        let e = normalize(&raw(0xE7, "あ")).unwrap(); // VK_PACKET
        assert_eq!(e.keysym, 0x0100_0000 + 0x3042);
        assert_eq!(e.text, "あ");
        let e = normalize(&raw(0xDE, "é")).unwrap();
        assert_eq!(e.keysym, 0xE9);
    }

    #[test]
    fn control_characters_have_no_keysym_or_text() {
        let mut r = raw(0x41, "\u{1}");
        r.unmodified_text = String::new();
        r.modifiers = Modifiers::default();
        let e = normalize(&r).unwrap();
        // unmodified_text が無ければ VK から補う。text は制御文字なので空
        assert_eq!(e.keysym, 0x61);
        assert_eq!(e.text, "");
        assert_eq!(keysym_for_char('\u{7f}'), None);
        assert_eq!(keysym_for_char('\u{85}'), None);
    }

    #[test]
    fn vk_fallback_for_letters_and_digits() {
        let mut r = raw(0x5A, "");
        r.unmodified_text = String::new();
        assert_eq!(normalize(&r).unwrap().keysym, 0x7A);
        r.modifiers.shift = true;
        assert_eq!(normalize(&r).unwrap().keysym, 0x5A);
        r.modifiers.caps_lock = true; // Shift + Caps で小文字に戻る
        assert_eq!(normalize(&r).unwrap().keysym, 0x7A);
        let mut d = raw(0x35, "");
        d.unmodified_text = String::new();
        assert_eq!(normalize(&d).unwrap().keysym, 0x35);
        d.modifiers.shift = true;
        assert_eq!(normalize(&d).unwrap().keysym, 0);
    }

    #[test]
    fn flags_changed_distinguishes_left_and_right() {
        let cases: &[(u16, u32)] = &[
            (vk::LSHIFT, 0xffe1),
            (vk::RSHIFT, 0xffe2),
            (vk::LCONTROL, 0xffe3),
            (vk::RCONTROL, 0xffe4),
            (vk::LMENU, 0xffe9),
            (vk::RMENU, 0xffea),
            (vk::LWIN, 0xffeb),
            (vk::RWIN, 0xffec),
            (vk::CAPITAL, 0xffe5),
        ];
        for &(v, sym) in cases {
            let r = with_mods(raw(v, ""), |m| m.shift = true);
            let e = normalize(&r).unwrap();
            assert_eq!(e.keysym, sym, "vk {v:#x}");
            assert_eq!(e.kind, KeyKind::FlagsChanged);
            assert_eq!(e.text, "");
            assert!(e.modifiers.shift);
        }
    }

    #[test]
    fn generic_modifier_vks_use_scan_code_and_extended_flag() {
        let mut r = raw(vk::SHIFT, "");
        r.scan_code = 0x2A;
        assert_eq!(normalize(&r).unwrap().keysym, keysym::SHIFT_L);
        r.scan_code = 0x36;
        assert_eq!(normalize(&r).unwrap().keysym, keysym::SHIFT_R);
        let mut c = raw(vk::CONTROL, "");
        assert_eq!(normalize(&c).unwrap().keysym, keysym::CONTROL_L);
        c.extended = true;
        assert_eq!(normalize(&c).unwrap().keysym, keysym::CONTROL_R);
        let mut a = raw(vk::MENU, "");
        assert_eq!(normalize(&a).unwrap().keysym, keysym::ALT_L);
        a.extended = true;
        assert_eq!(normalize(&a).unwrap().keysym, keysym::ALT_R);
    }

    #[test]
    fn key_up_is_ignored_except_modifiers() {
        let mut r = raw(0x41, "a");
        r.is_key_up = true;
        assert_eq!(normalize(&r), None);
        let mut s = raw(vk::LSHIFT, "");
        s.is_key_up = true;
        let e = normalize(&s).unwrap();
        assert_eq!(e.kind, KeyKind::FlagsChanged);
        assert!(!e.modifiers.shift);
    }

    #[test]
    fn caps_lock_modifier_state_is_passed_through() {
        let r = with_mods(raw(0x41, "A"), |m| m.caps_lock = true);
        assert!(normalize(&r).unwrap().modifiers.caps_lock);
    }
}
