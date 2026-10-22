//! TSF のキーイベント（`WPARAM` / `LPARAM`）→ `nagi_windows::keynorm::RawKeyInput`。
//!
//! Win32 の API で状態を読むだけで、判断はしない（正規化は `keynorm` の仕事）。

#![allow(unsafe_code)]

use nagi_protocol::Modifiers;
use nagi_windows::keynorm::{RawKeyInput, vk};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, GetKeyboardState, ToUnicodeEx, VK_CAPITAL, VK_CONTROL,
    VK_LCONTROL, VK_LMENU, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RWIN, VK_SHIFT,
};

/// `ToUnicodeEx` のフラグ bit2: キーボード状態（デッドキー）を変更しない（Windows 10 1607 以降）
const TO_UNICODE_NO_STATE_CHANGE: u32 = 0x4;

fn key_down(vkey: i32) -> bool {
    // 上位ビットが押下中
    (unsafe { GetKeyState(vkey) } as u16 & 0x8000) != 0
}

fn current_modifiers() -> Modifiers {
    Modifiers {
        shift: key_down(i32::from(VK_SHIFT.0)),
        ctrl: key_down(i32::from(VK_CONTROL.0)),
        alt: key_down(i32::from(VK_MENU.0)),
        meta: key_down(i32::from(VK_LWIN.0)) || key_down(i32::from(VK_RWIN.0)),
        // 下位ビットがトグル状態
        caps_lock: (unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1) != 0,
    }
}

/// 現在のキーボード状態で `vkey` が生成する文字列。`clear_ctrl_alt_win` なら Ctrl / Alt / Win を無いものとして扱う
fn translate(vkey: u16, scan: u16, clear_ctrl_alt_win: bool) -> String {
    let mut state = [0u8; 256];
    if unsafe { GetKeyboardState(&mut state) }.is_err() {
        return String::new();
    }
    if clear_ctrl_alt_win {
        for k in [
            VK_CONTROL,
            VK_LCONTROL,
            VK_RCONTROL,
            VK_MENU,
            VK_LMENU,
            VK_RMENU,
            VK_LWIN,
            VK_RWIN,
        ] {
            state[usize::from(k.0)] = 0;
        }
    }
    let mut buf = [0u16; 8];
    let layout = unsafe { GetKeyboardLayout(0) };
    let n = unsafe {
        ToUnicodeEx(
            u32::from(vkey),
            u32::from(scan),
            &state,
            &mut buf,
            TO_UNICODE_NO_STATE_CHANGE,
            Some(layout),
        )
    };
    // 0 以下は文字なし（負はデッドキー）
    match usize::try_from(n) {
        Ok(len) if len > 0 => String::from_utf16_lossy(&buf[..len.min(buf.len())]),
        _ => String::new(),
    }
}

fn is_modifier_vk(v: u16) -> bool {
    matches!(
        v,
        vk::SHIFT
            | vk::CONTROL
            | vk::MENU
            | vk::LSHIFT
            | vk::RSHIFT
            | vk::LCONTROL
            | vk::RCONTROL
            | vk::LMENU
            | vk::RMENU
            | vk::LWIN
            | vk::RWIN
            | vk::CAPITAL
    )
}

/// `OnKeyDown` 等の引数から生のキー入力を作る
pub fn raw_key_input(wparam: WPARAM, lparam: LPARAM, is_key_up: bool) -> RawKeyInput {
    let vkey = (wparam.0 & 0xFFFF) as u16;
    let bits = lparam.0 as usize;
    let scan = ((bits >> 16) & 0xFF) as u16;
    let extended = (bits >> 24) & 1 != 0;
    let (text, unmodified_text) = if is_key_up || is_modifier_vk(vkey) {
        (String::new(), String::new())
    } else {
        (translate(vkey, scan, false), translate(vkey, scan, true))
    };
    RawKeyInput {
        vk: vkey,
        scan_code: scan,
        extended,
        is_key_up,
        text,
        unmodified_text,
        modifiers: current_modifiers(),
    }
}
