//! キーイベント型と XKB keysym 定数。

use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// キーの押下・離上の種別
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    /// キー押下
    Down,
    /// 修飾キーの状態変化（押下・離上）
    FlagsChanged,
}

/// 修飾キーの状態
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Modifiers {
    /// Shift キー
    pub shift: bool,
    /// Ctrl / Control キー
    pub ctrl: bool,
    /// Alt / Option キー
    pub alt: bool,
    /// ⌘ (macOS) / Win キー
    pub meta: bool,
    /// Caps Lock の状態
    pub caps_lock: bool,
}

/// キーイベント
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEvent {
    /// XKB keysym（数値）
    pub keysym: u32,
    /// OS キーコード（参考情報）
    pub keycode: u32,
    /// この押下で生成されたテキスト（Dvorak 等の配列対応用）。生成されなければ空文字列
    pub text: String,
    /// 修飾キーの状態
    pub modifiers: Modifiers,
    /// キーの種別（押下 or 修飾キー状態変化）
    pub kind: KeyKind,
}

pub mod keysym {
    //! XKB keysym 定数
    //!
    //! <https://www.x.org/releases/current/doc/kbproto/keysymdefs.html>
    //!
    //! 数値が小さいものは通常 Unicode コードポイントと一致する。

    /// Space キー
    pub const SPACE: u32 = 0x0020;

    /// Backspace キー
    pub const BACKSPACE: u32 = 0xff08;

    /// Tab キー
    pub const TAB: u32 = 0xff09;

    /// Return / Enter キー
    pub const RETURN: u32 = 0xff0d;

    /// Escape キー
    pub const ESCAPE: u32 = 0xff1b;

    /// Delete キー
    pub const DELETE: u32 = 0xffff;

    /// Home キー
    pub const HOME: u32 = 0xff50;

    /// ← キー
    pub const LEFT: u32 = 0xff51;

    /// ↑ キー
    pub const UP: u32 = 0xff52;

    /// → キー
    pub const RIGHT: u32 = 0xff53;

    /// ↓ キー
    pub const DOWN: u32 = 0xff54;

    /// Page Up キー
    pub const PAGE_UP: u32 = 0xff55;

    /// Page Down キー
    pub const PAGE_DOWN: u32 = 0xff56;

    /// End キー
    pub const END: u32 = 0xff57;

    /// F1 キー
    pub const F1: u32 = 0xffbe;

    /// F2 キー
    pub const F2: u32 = 0xffbf;

    /// F3 キー
    pub const F3: u32 = 0xffc0;

    /// F4 キー
    pub const F4: u32 = 0xffc1;

    /// F5 キー
    pub const F5: u32 = 0xffc2;

    /// F6 キー
    pub const F6: u32 = 0xffc3;

    /// F7 キー
    pub const F7: u32 = 0xffc4;

    /// F8 キー
    pub const F8: u32 = 0xffc5;

    /// F9 キー
    pub const F9: u32 = 0xffc6;

    /// F10 キー
    pub const F10: u32 = 0xffc7;

    /// F11 キー
    pub const F11: u32 = 0xffc8;

    /// F12 キー
    pub const F12: u32 = 0xffc9;

    /// 英数 / Eisu_toggle キー（JIS キーボード）
    pub const EISU_TOGGLE: u32 = 0xff2f;

    /// カタカナ / ひらがな キー（JIS キーボード）
    pub const HIRAGANA_KATAKANA: u32 = 0xff27;

    /// ひらがな キー（JIS キーボード）
    pub const HIRAGANA: u32 = 0xff25;

    /// カタカナ キー（JIS キーボード）
    pub const KATAKANA: u32 = 0xff26;

    /// 全角 / 半角 キー（JIS キーボード）
    pub const ZENKAKU_HANKAKU: u32 = 0xff2a;

    /// 無変換 キー（JIS キーボード）
    pub const MUHENKAN: u32 = 0xff22;

    /// 変換 キー（JIS キーボード）
    pub const HENKAN: u32 = 0xff23;

    /// Shift_L キー
    pub const SHIFT_L: u32 = 0xffe1;

    /// Shift_R キー
    pub const SHIFT_R: u32 = 0xffe2;

    /// Control_L キー
    pub const CONTROL_L: u32 = 0xffe3;

    /// Control_R キー
    pub const CONTROL_R: u32 = 0xffe4;

    /// Caps_Lock キー
    pub const CAPS_LOCK: u32 = 0xffe5;

    /// Alt_L キー
    pub const ALT_L: u32 = 0xffe9;

    /// Alt_R キー
    pub const ALT_R: u32 = 0xffea;

    /// Super_L / Windows キー
    pub const SUPER_L: u32 = 0xffeb;

    /// Super_R / Windows キー
    pub const SUPER_R: u32 = 0xffec;

    /// Meta_L キー
    pub const META_L: u32 = 0xffe7;

    /// Meta_R キー
    pub const META_R: u32 = 0xffe8;
}
