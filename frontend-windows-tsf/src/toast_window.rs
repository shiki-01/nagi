//! 通知パネル（トースト）。フォーカスを奪わない最前面の小さなポップアップを、作業領域の右下に
//! 数秒だけ出す。文言は `nagi_windows::status`、配置・表示時間は `nagi_windows::toast` が決める。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use nagi_windows::candidates::{Size, scale_px};
use nagi_windows::toast::{TOAST_DURATION, TOAST_MARGIN, TOAST_MAX_WIDTH, place_toast};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateRoundRectRgn, CreateSolidBrush, DT_CALCRECT, DT_NOPREFIX, DT_WORDBREAK,
    DeleteObject, DrawTextW, EndPaint, FillRect, HGDIOBJ, InvalidateRect, PAINTSTRUCT,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOPMOST,
    KillTimer, MA_NOACTIVATE, SW_HIDE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_MOUSEACTIVATE,
    WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_TIMER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::PCWSTR;

use crate::win::{
    self, Fonts, WindowClass, drop_user_data, into_user_data, user_data_ref, wide, work_areas,
};
use nagi_windows::candidates::FontRole;

static CLASS: WindowClass = WindowClass::new("NagiToastWindow");
const TIMER_ID: usize = 1;

/// 角丸の直径（96 DPI 基準）
const CORNER: i32 = 12;
/// 文字の周りの余白（96 DPI 基準）
const PAD_X: i32 = 14;
const PAD_Y: i32 = 10;

struct Paint {
    text: Vec<u16>,
    fonts: Fonts,
    pad_x: i32,
    pad_y: i32,
    size: Size,
}

type PaintCell = RefCell<Option<Paint>>;

/// 通知パネル。UI スレッドでのみ使う
pub struct ToastWindow {
    hwnd: Cell<HWND>,
    paint: Cell<isize>,
}

impl Default for ToastWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastWindow {
    /// 作る（ウィンドウは最初に表示するときに作る）
    pub fn new() -> Self {
        Self {
            hwnd: Cell::new(HWND::default()),
            paint: Cell::new(0),
        }
    }

    fn ensure_window(&self) -> Option<HWND> {
        let current = self.hwnd.get();
        if !current.is_invalid() {
            return Some(current);
        }
        if !CLASS.acquire(Some(wndproc)) {
            return None;
        }
        let class = CLASS.name_wide();
        let title = wide("Nagi notification");
        let created = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(win::instance()),
                None,
            )
        };
        let Ok(hwnd) = created else {
            CLASS.release();
            return None;
        };
        let ptr = into_user_data(Box::new(RefCell::new(None::<Paint>)));
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr) };
        self.paint.set(ptr);
        self.hwnd.set(hwnd);
        Some(hwnd)
    }

    /// `message` を表示する。表示中なら置き換えて、表示時間をやり直す
    pub fn show(&self, message: &str) {
        let Some(hwnd) = self.ensure_window() else {
            return;
        };
        let areas = work_areas();
        let Some(work) = areas.first().copied() else {
            return;
        };
        // プライマリの作業領域の右下へ先に動かし、そこの DPI で寸法を決める
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                work.right - 1,
                work.bottom - 1,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            )
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let Some(cell) = (unsafe { user_data_ref::<PaintCell>(self.paint.get()) }) else {
            return;
        };
        let fonts = Fonts::new(dpi);
        let text: Vec<u16> = format!("Nagi: {message}").encode_utf16().collect();
        let (pad_x, pad_y) = (scale_px(PAD_X, dpi), scale_px(PAD_Y, dpi));
        let max_text_width = scale_px(TOAST_MAX_WIDTH, dpi) - pad_x * 2;

        // 折り返したときの文字領域の大きさを計る
        let mut measure = RECT {
            left: 0,
            top: 0,
            right: max_text_width,
            bottom: 0,
        };
        let mut buf = text.clone();
        unsafe {
            let dc = windows::Win32::Graphics::Gdi::GetDC(Some(hwnd));
            let old = SelectObject(dc, HGDIOBJ(fonts.font(FontRole::Surface).0));
            DrawTextW(
                dc,
                &mut buf,
                &mut measure,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(dc, old);
            windows::Win32::Graphics::Gdi::ReleaseDC(Some(hwnd), dc);
        }
        let size = Size {
            width: (measure.right - measure.left) + pad_x * 2,
            height: (measure.bottom - measure.top) + pad_y * 2,
        };
        let origin = place_toast(work, size, scale_px(TOAST_MARGIN, dpi));
        *cell.borrow_mut() = Some(Paint {
            text,
            fonts,
            pad_x,
            pad_y,
            size,
        });
        unsafe {
            let corner = scale_px(CORNER, dpi);
            let region = CreateRoundRectRgn(0, 0, size.width + 1, size.height + 1, corner, corner);
            // 領域の所有権はウィンドウに移る
            SetWindowRgn(hwnd, Some(region), true);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                origin.x,
                origin.y,
                size.width,
                size.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(hwnd), None, true);
            let ms = u32::try_from(TOAST_DURATION.as_millis()).unwrap_or(4000);
            SetTimer(Some(hwnd), TIMER_ID, ms, None);
        }
    }

    /// ウィンドウを壊す
    pub fn destroy(&self) {
        let hwnd = self.hwnd.replace(HWND::default());
        if !hwnd.is_invalid() {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ID) };
            let _ = unsafe { DestroyWindow(hwnd) };
            CLASS.release();
        }
        self.paint.set(0);
    }
}

impl Drop for ToastWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn draw(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.is_invalid() {
        return;
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if let Some(cell) = unsafe { user_data_ref::<PaintCell>(ptr) }
        && let Some(paint) = cell.borrow().as_ref()
    {
        let whole = RECT {
            left: 0,
            top: 0,
            right: paint.size.width,
            bottom: paint.size.height,
        };
        unsafe {
            // 暗い半透明風の HUD（色は固定。システム色に依存しない）
            let brush = CreateSolidBrush(COLORREF(0x0030_2B2B));
            FillRect(hdc, &whole, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FF_FFFF));
            let old = SelectObject(hdc, HGDIOBJ(paint.fonts.font(FontRole::Surface).0));
            let mut text_rect = RECT {
                left: paint.pad_x,
                top: paint.pad_y,
                right: paint.size.width - paint.pad_x,
                bottom: paint.size.height - paint.pad_y,
            };
            let mut buf = paint.text.clone();
            DrawTextW(hdc, &mut buf, &mut text_rect, DT_WORDBREAK | DT_NOPREFIX);
            SelectObject(hdc, old);
        }
    }
    let _ = unsafe { EndPaint(hwnd, &ps) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let _ = std::panic::catch_unwind(|| draw(hwnd));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_TIMER if wparam.0 == TIMER_ID => {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_ID) };
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop_user_data::<PaintCell>(ptr);
                win::default_proc(hwnd, msg, wparam, lparam)
            }
        }
        _ => unsafe { win::default_proc(hwnd, msg, wparam, lparam) },
    }
}
