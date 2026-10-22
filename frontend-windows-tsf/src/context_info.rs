//! ホストアプリの文脈の取得（TSF）: キャレット直前のテキスト、入力欄の種別（`InputScope`）、アプリ ID。
//! 取得だけを行い、`set_context` の params への整形・secure の規約は `nagi_windows::context`。

#![allow(unsafe_code)]

use std::sync::OnceLock;

use nagi_windows::context::{CONTEXT_UTF16, INPUT_SCOPE_PASSWORD, SurroundingRead};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::UI::TextServices::{
    GUID_PROP_INPUTSCOPE, IS_PASSWORD, ITfContext, ITfInputScope, ITfRange, InputScope,
    TF_ANCHOR_START,
};
use windows::core::{IUnknown, Interface};

use crate::edit::selection_range;

// `nagi_windows::context::INPUT_SCOPE_PASSWORD`（記憶に基づく値）が windows クレートの `IS_PASSWORD` と
// 一致することをコンパイル時に検査する
const _: () = assert!(IS_PASSWORD.0 == INPUT_SCOPE_PASSWORD);

/// 編集セッション内で得た文脈
#[derive(Debug)]
pub struct Surroundings {
    /// 入力欄が機密（パスワード等）か
    pub secure: bool,
    /// キャレット直前のテキスト（secure のときは読まない）
    pub text: SurroundingRead,
}

/// 範囲の `InputScope` の一覧
fn input_scopes(context: &ITfContext, ec: u32, range: &ITfRange) -> Vec<i32> {
    let read = || -> windows::core::Result<Vec<i32>> {
        let property = unsafe { context.GetProperty(&GUID_PROP_INPUTSCOPE) }?;
        let value = unsafe { property.GetValue(ec, range) }?;
        let unknown = IUnknown::try_from(&value)?;
        let scope: ITfInputScope = unknown.cast()?;
        let mut list: *mut InputScope = std::ptr::null_mut();
        let mut count = 0u32;
        unsafe { scope.GetInputScopes(&mut list, &mut count) }?;
        if list.is_null() {
            return Ok(Vec::new());
        }
        let scopes = unsafe { std::slice::from_raw_parts(list, count as usize) }
            .iter()
            .map(|s| s.0)
            .collect();
        unsafe { CoTaskMemFree(Some(list.cast_const().cast())) };
        Ok(scopes)
    };
    read().unwrap_or_default()
}

/// キャレット直前の最大 [`CONTEXT_UTF16`] コード単位
fn text_before_caret(ec: u32, caret: &ITfRange) -> windows::core::Result<String> {
    // キャレット（選択の先頭）に畳み、始点だけを手前へ広げる（終端はキャレットのまま）
    let before = unsafe { caret.Clone() }?;
    unsafe { before.Collapse(ec, TF_ANCHOR_START) }?;
    let mut moved = 0i32;
    let back = -i32::try_from(CONTEXT_UTF16).unwrap_or(64);
    unsafe { before.ShiftStart(ec, back, &mut moved, std::ptr::null()) }?;
    let mut buf = [0u16; CONTEXT_UTF16];
    let mut fetched = 0u32;
    unsafe { before.GetText(ec, 0, &mut buf, &mut fetched) }?;
    let mut units = &buf[..(fetched as usize).min(CONTEXT_UTF16)];
    // 先頭がサロゲートペアの後半なら落とす（途中から読んだため）
    if let Some(first) = units.first()
        && (0xDC00..0xE000).contains(first)
    {
        units = &units[1..];
    }
    Ok(String::from_utf16_lossy(units))
}

/// 現在の選択位置の文脈を読む。**編集セッション（読み取り）の内側で呼ぶ**。
/// secure なら周辺テキストは読まない
pub fn read_surroundings(context: &ITfContext, ec: u32) -> Surroundings {
    let Ok(selection) = selection_range(context, ec) else {
        return Surroundings {
            secure: false,
            text: SurroundingRead::Unavailable,
        };
    };
    let scopes = input_scopes(context, ec, &selection);
    let secure = nagi_windows::context::is_secure_input_scope(&scopes);
    let text = if secure {
        SurroundingRead::Unavailable
    } else {
        match text_before_caret(ec, &selection) {
            Ok(t) => SurroundingRead::Text(t),
            Err(_) => SurroundingRead::Unavailable,
        }
    };
    Surroundings { secure, text }
}

/// このプロセスの実行ファイル名（例: `chrome.exe`）。アプリ ID として `set_context` に送る
pub fn app_id() -> Option<&'static str> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        let mut buf = vec![0u16; 1024];
        let n = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
        if n == 0 || n >= buf.len() {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..n]);
        let name = path.rsplit(['\\', '/']).next()?.to_string();
        (!name.is_empty()).then_some(name)
    })
    .as_deref()
}
