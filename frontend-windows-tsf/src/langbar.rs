//! 言語バーのボタン（`ITfLangBarItemButton`）。現在の入力モード（あ / A）を表示し、
//! 左クリックでひらがな ⇄ 英数を切り替え、メニュー（右クリック）にモード・状態・設定を出す。
//!
//! 何を出すか・どう切り替えるかは `nagi_windows::status`（`menu_entries` / `toggled` / `button_text`）と
//! `Core` が決める。ここは COM の受け口と、表示用の `HICON`（文字を描いたアイコン）だけ。
//! モードの正はコアで、切り替えは `mode_comp` のコンパートメントへ書く形に統一している
//! （他のクライアントやトレイの操作と同じ経路。`Core::request_mode`）。
//! Win8 以降のデスクトップではトレイの「あ / A」はコンパートメントから OS が描くので、
//! このボタンは従来の言語バー（有効にしている環境・一部のアプリ）向けの補助。

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use nagi_windows::status::{self, MenuEntry};
use windows::Win32::Foundation::{
    COLORREF, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_POINTER, POINT, RECT, TRUE,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FillRect, GetDC, HBITMAP,
    HGDIOBJ, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::TextServices::{
    ITfLangBarItem_Impl, ITfLangBarItemButton, ITfLangBarItemButton_Impl, ITfLangBarItemMgr,
    ITfLangBarItemSink, ITfMenu, ITfSource, ITfSource_Impl, ITfThreadMgr, TF_LANGBARITEMINFO,
    TF_LBI_CLK_LEFT, TF_LBI_ICON, TF_LBI_STYLE_BTN_BUTTON, TF_LBI_STYLE_SHOWNINTRAY, TF_LBI_TEXT,
    TF_LBI_TOOLTIP, TF_LBMENUF_CHECKED, TF_LBMENUF_GRAYED, TF_LBMENUF_SEPARATOR, TfLBIClick,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON, SW_SHOWNORMAL,
};
use windows::core::{BOOL, BSTR, GUID, IUnknown, Interface, PCWSTR, Ref, Result, implement, w};

use crate::guids::{CLSID_NAGI_TEXT_SERVICE, GUID_NAGI_LANGBAR_BUTTON};
use crate::text_service::Core;
use crate::win::{make_font, wide};

/// ボタンの背景（藍。`BGR` 順の `COLORREF`）
const BACKGROUND: COLORREF = COLORREF(0x00B5_513F);
/// 文字の色（白）
const FOREGROUND: COLORREF = COLORREF(0x00FF_FFFF);
/// 言語バー内の並び順（他の項目の後ろ）
const SORT_ORDER: u32 = 10;

type Sinks = Rc<RefCell<Vec<(u32, ITfLangBarItemSink)>>>;

#[implement(ITfLangBarItemButton, ITfSource)]
struct Button {
    core: Weak<Core>,
    sinks: Sinks,
    next_cookie: RefCell<u32>,
}

impl Button {
    fn mode(&self) -> Option<nagi_protocol::InputMode> {
        self.core.upgrade().and_then(|c| c.lang_bar_mode())
    }
}

impl ITfLangBarItem_Impl for Button_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if pinfo.is_null() {
            return Err(E_POINTER.into());
        }
        let mut info = TF_LANGBARITEMINFO {
            clsidService: CLSID_NAGI_TEXT_SERVICE,
            guidItem: GUID_NAGI_LANGBAR_BUTTON,
            dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
            ulSort: SORT_ORDER,
            szDescription: [0; 32],
        };
        // 説明は 31 UTF-16 + NUL まで
        for (slot, unit) in info
            .szDescription
            .iter_mut()
            .zip("Nagi 入力モード".encode_utf16().take(31))
        {
            *slot = unit;
        }
        unsafe { pinfo.write(info) };
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    fn Show(&self, _fshow: BOOL) -> Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::from(status::button_tooltip(self.mode()).as_str()))
    }
}

impl ITfLangBarItemButton_Impl for Button_Impl {
    fn OnClick(&self, click: TfLBIClick, _pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        // 左クリックでモードの切り替え。右クリックのメニューは OS が `InitMenu` で出す
        if click == TF_LBI_CLK_LEFT
            && let Some(core) = self.core.upgrade()
        {
            core.toggle_mode_from_lang_bar();
        }
        Ok(())
    }

    fn InitMenu(&self, pmenu: Ref<ITfMenu>) -> Result<()> {
        let menu = pmenu.ok()?;
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        for entry in core.lang_bar_menu() {
            match entry {
                MenuEntry::Separator => add_item(menu, 0, TF_LBMENUF_SEPARATOR, "")?,
                MenuEntry::Info(text) => add_item(menu, 0, TF_LBMENUF_GRAYED, &text)?,
                MenuEntry::Command {
                    id,
                    label,
                    checked,
                    enabled,
                } => {
                    let mut flags = 0;
                    if checked {
                        flags |= TF_LBMENUF_CHECKED;
                    }
                    if !enabled {
                        flags |= TF_LBMENUF_GRAYED;
                    }
                    add_item(menu, id, flags, &label)?;
                }
            }
        }
        Ok(())
    }

    fn OnMenuSelect(&self, wid: u32) -> Result<()> {
        if let (Some(command), Some(core)) = (status::command_from_id(wid), self.core.upgrade()) {
            core.run_menu_command(command);
        }
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
        make_icon(status::button_text(self.mode()), size)
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(status::button_text(self.mode())))
    }
}

impl ITfSource_Impl for Button_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<IUnknown>) -> Result<u32> {
        if riid.is_null() {
            return Err(E_POINTER.into());
        }
        if unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(E_NOINTERFACE.into());
        }
        let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
        let mut next = self.next_cookie.borrow_mut();
        *next += 1;
        self.sinks.borrow_mut().push((*next, sink));
        Ok(*next)
    }

    fn UnadviseSink(&self, dwcookie: u32) -> Result<()> {
        let mut sinks = self.sinks.borrow_mut();
        let before = sinks.len();
        sinks.retain(|(cookie, _)| *cookie != dwcookie);
        if sinks.len() == before {
            return Err(E_INVALIDARG.into());
        }
        Ok(())
    }
}

fn add_item(menu: &ITfMenu, id: u32, flags: u32, label: &str) -> Result<()> {
    let text: Vec<u16> = label.encode_utf16().collect();
    unsafe {
        menu.AddMenuItem(
            id,
            flags,
            HBITMAP::default(),
            HBITMAP::default(),
            &text,
            std::ptr::null_mut(),
        )
    }
}

/// `text` を白で描いた藍色の正方形のアイコン。呼び出し側（言語バー）が `DestroyIcon` する前提で、
/// 呼ぶたびに新しく作る
fn make_icon(text: &str, size: i32) -> Result<HICON> {
    let mut label: Vec<u16> = text.encode_utf16().collect();
    let font = make_font(size * 3 / 4, 96);
    let screen = unsafe { GetDC(None) };
    let dc = unsafe { CreateCompatibleDC(Some(screen)) };
    let color = unsafe { CreateCompatibleBitmap(screen, size, size) };
    let old_bitmap = unsafe { SelectObject(dc, HGDIOBJ(color.0)) };
    let old_font = unsafe { SelectObject(dc, HGDIOBJ(font.0)) };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: size,
        bottom: size,
    };
    let brush = unsafe { CreateSolidBrush(BACKGROUND) };
    unsafe {
        FillRect(dc, &rect, brush);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, FOREGROUND);
        DrawTextW(
            dc,
            &mut label,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
    }
    // AND マスク: 0 は「色をそのまま使う」（全面を不透明にする）。1bpp で行は 16 bit 境界
    let stride = (size as usize).div_ceil(16) * 2;
    let zeros = vec![0u8; stride * size as usize];
    let mask = unsafe { CreateBitmap(size, size, 1, 1, Some(zeros.as_ptr().cast())) };
    let info = ICONINFO {
        fIcon: TRUE,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color,
    };
    let icon = unsafe { CreateIconIndirect(&info) };
    unsafe {
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(brush.0));
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
    }
    icon
}

/// 設定ファイルをメモ帳で開く（`.toml` に関連付けが無くても開けるようメモ帳を明示する）。
/// 無いファイルはメモ帳が作成を尋ねる
pub fn open_in_notepad(path: &str) {
    let argument = wide(&format!("\"{path}\""));
    // 戻り値が 32 以下ならエラーだが、開けなくても入力には影響しないので無視する
    let _ = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("notepad.exe"),
            PCWSTR(argument.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
}

/// 言語バーに登録したボタン。破棄すると登録を外す
pub struct LangBar {
    manager: ITfLangBarItemMgr,
    item: ITfLangBarItemButton,
    sinks: Sinks,
}

impl LangBar {
    /// ボタンを作って言語バーに追加する
    pub fn new(thread_mgr: &ITfThreadMgr, core: Weak<Core>) -> Result<Self> {
        let manager: ITfLangBarItemMgr = thread_mgr.cast()?;
        let sinks: Sinks = Rc::default();
        let item: ITfLangBarItemButton = Button {
            core,
            sinks: Rc::clone(&sinks),
            next_cookie: RefCell::new(0),
        }
        .into();
        unsafe { manager.AddItem(&item) }?;
        Ok(Self {
            manager,
            item,
            sinks,
        })
    }

    /// モードが変わったので、アイコン・文字・ツールチップを描き直させる
    pub fn update(&self) {
        let sinks: Vec<ITfLangBarItemSink> =
            self.sinks.borrow().iter().map(|(_, s)| s.clone()).collect();
        for sink in sinks {
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_TEXT | TF_LBI_TOOLTIP) };
        }
    }
}

impl Drop for LangBar {
    fn drop(&mut self) {
        let _ = unsafe { self.manager.RemoveItem(&self.item) };
    }
}
