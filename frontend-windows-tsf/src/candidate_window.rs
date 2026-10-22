//! 候補ウィンドウ（自前の Win32 ウィンドウ）。描画と位置合わせだけを行い、何を出すか・
//! どこに置くか・寸法は `nagi_windows::candidates` が決める。
//!
//! フォーカスを奪わない（`WS_EX_NOACTIVATE`）最前面のポップアップ。マウス操作は受けない
//! （v1 の非目標。クリックは下のウィンドウに通す）。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use nagi_windows::candidates::{
    CandidatePage, Metrics, Point, Rect, Size, WindowLayout, anchor_rect, layout, place_window,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_3DSHADOW, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW,
    COLOR_WINDOWTEXT, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DrawTextW,
    EndPaint, FillRect, FrameRect, GetSysColor, GetSysColorBrush, HGDIOBJ, InvalidateRect,
    PAINTSTRUCT, SYS_COLOR_INDEX, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOPMOST,
    MA_NOACTIVATE, SW_HIDE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCDESTROY,
    WM_NCHITTEST, WM_PAINT, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::PCWSTR;

use nagi_windows::candidates::FontRole;

use crate::win::{
    self, Fonts, WindowClass, cursor_position, drop_user_data, into_user_data, user_data_ref, wide,
    work_areas,
};

static CLASS: WindowClass = WindowClass::new("NagiCandidateWindow");

/// 描画に必要なもの（ウィンドウプロシージャから参照する）
struct Paint {
    page: CandidatePage,
    layout: WindowLayout,
    fonts: Fonts,
}

type PaintCell = RefCell<Option<Paint>>;

/// 候補ウィンドウ。UI スレッドでのみ使う
pub struct CandidateWindow {
    hwnd: Cell<HWND>,
    /// ウィンドウのユーザーデータとして渡したポインタ（`WM_NCDESTROY` で解放される）
    paint: Cell<isize>,
    visible: Cell<bool>,
}

impl Default for CandidateWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl CandidateWindow {
    /// 作る（ウィンドウは最初に表示するときに作る）
    pub fn new() -> Self {
        Self {
            hwnd: Cell::new(HWND::default()),
            paint: Cell::new(0),
            visible: Cell::new(false),
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
        let created = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                PCWSTR(class.as_ptr()),
                PCWSTR(wide("Nagi candidates").as_ptr()),
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
        let cell: Box<PaintCell> = Box::new(RefCell::new(None));
        let ptr = into_user_data(cell);
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr) };
        self.paint.set(ptr);
        self.hwnd.set(hwnd);
        Some(hwnd)
    }

    /// 候補を表示（または更新）する。`caret` はキャレット（composition）の矩形（スクリーン座標）。
    /// 取れないときはマウス位置を使う
    pub fn show(&self, page: &CandidatePage, caret: Option<Rect>) {
        let Some(hwnd) = self.ensure_window() else {
            return;
        };
        let anchor = anchor_rect(caret, cursor_position());
        // 先に基準位置へ動かして、そこのモニターの DPI を得る（Per-Monitor 対応のホスト向け）
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                anchor.left,
                anchor.bottom,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            )
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);

        let cell = unsafe { user_data_ref::<PaintCell>(self.paint.get()) };
        let Some(cell) = cell else {
            return;
        };
        // DPI が変わったときだけフォントを作り直す
        let previous = cell.borrow_mut().take();
        let fonts = match previous {
            Some(p) if p.fonts.dpi() == dpi => p.fonts,
            _ => Fonts::new(dpi),
        };
        let metrics = Metrics::default().scaled(dpi);
        let computed = layout(page, &fonts, &metrics);
        let placement = place_window(anchor, computed.size, &work_areas());
        let Size { width, height } = computed.size;
        *cell.borrow_mut() = Some(Paint {
            page: page.clone(),
            layout: computed,
            fonts,
        });
        let Point { x, y } = placement.origin;
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        };
        let _ = unsafe { InvalidateRect(Some(hwnd), None, true) };
        self.visible.set(true);
    }

    /// 隠す
    pub fn hide(&self) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_invalid() && self.visible.replace(false) {
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }

    /// ウィンドウを壊す（`Deactivate` 時）
    pub fn destroy(&self) {
        let hwnd = self.hwnd.replace(HWND::default());
        self.visible.set(false);
        if !hwnd.is_invalid() {
            // ユーザーデータは WM_NCDESTROY で解放する
            let _ = unsafe { DestroyWindow(hwnd) };
            CLASS.release();
        }
        self.paint.set(0);
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn color(index: SYS_COLOR_INDEX) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(unsafe { GetSysColor(index) })
}

fn rect_of(r: Rect) -> RECT {
    win::from_rect(r)
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
        let size = paint.layout.size;
        let whole = RECT {
            left: 0,
            top: 0,
            right: size.width,
            bottom: size.height,
        };
        unsafe {
            FillRect(hdc, &whole, GetSysColorBrush(COLOR_WINDOW));
            FrameRect(hdc, &whole, GetSysColorBrush(COLOR_3DSHADOW));
            SetBkMode(hdc, TRANSPARENT);
        }
        let text_flags = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX;
        let draw_text = |text: &str, role: FontRole, mut rect: RECT, align, col| {
            let mut buf: Vec<u16> = text.encode_utf16().collect();
            unsafe {
                let old = SelectObject(hdc, HGDIOBJ(paint.fonts.font(role).0));
                SetTextColor(hdc, col);
                DrawTextW(hdc, &mut buf, &mut rect, text_flags | align);
                SelectObject(hdc, old);
            }
        };
        let lay = &paint.layout;
        for (row, rect) in paint.page.rows.iter().zip(&lay.row_rects) {
            let r = rect_of(*rect);
            let (main, sub) = if row.is_selected {
                unsafe { FillRect(hdc, &r, GetSysColorBrush(COLOR_HIGHLIGHT)) };
                (color(COLOR_HIGHLIGHTTEXT), color(COLOR_HIGHLIGHTTEXT))
            } else {
                (color(COLOR_WINDOWTEXT), color(COLOR_GRAYTEXT))
            };
            let from_x = |x: i32| RECT { left: x, ..r };
            draw_text(
                &row.number.to_string(),
                FontRole::Number,
                from_x(lay.number_x),
                DT_LEFT,
                sub,
            );
            draw_text(
                &row.surface,
                FontRole::Surface,
                from_x(lay.surface_x),
                DT_LEFT,
                main,
            );
            if let Some(label_x) = lay.label_x
                && !row.label.is_empty()
            {
                draw_text(&row.label, FontRole::Label, from_x(label_x), DT_LEFT, sub);
            }
        }
        if let (Some(footer), Some(text)) = (lay.footer_rect, paint.page.footer_text()) {
            let mut r = rect_of(footer);
            r.right -= Metrics::default().padding;
            draw_text(&text, FontRole::Label, r, DT_RIGHT, color(COLOR_GRAYTEXT));
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
        // フォーカスもクリックも奪わない
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
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
