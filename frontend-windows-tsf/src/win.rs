//! Win32 の共通部品: 文字列・矩形の変換、モニターの作業領域、GDI のフォントと文字幅の計測、
//! ウィンドウクラスの登録管理。判断は持たない（配置・寸法の計算は `nagi_windows`）。

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::Mutex;

use nagi_windows::candidates::{self, FontRole, Point, Rect, TextMeasure, scale_px};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateFontW, DEFAULT_CHARSET,
    DEFAULT_PITCH, DeleteDC, DeleteObject, EnumDisplayMonitors, FF_DONTCARE, FW_NORMAL, GetDC,
    GetMonitorInfoW, GetTextExtentPoint32W, HDC, HFONT, HGDIOBJ, HMONITOR, MONITORINFO,
    OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetCursorPos, HCURSOR, HICON, MONITORINFOF_PRIMARY, RegisterClassExW,
    UnregisterClassW, WNDCLASS_STYLES, WNDCLASSEXW, WNDPROC,
};
use windows::core::{BOOL, PCWSTR};

use crate::dll;

/// UTF-16 に変換して NUL で終端する
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `RECT` → 論理矩形
pub fn to_rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// 論理矩形 → `RECT`
pub fn from_rect(r: Rect) -> RECT {
    RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// マウスカーソルの位置（スクリーン座標）。取れなければ原点
pub fn cursor_position() -> Point {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_ok() {
        Point { x: p.x, y: p.y }
    } else {
        Point::default()
    }
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<(Rect, bool)>) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        out.push((
            to_rect(info.rcWork),
            info.dwFlags & MONITORINFOF_PRIMARY != 0,
        ));
    }
    BOOL(1)
}

/// 全モニターの作業領域（タスクバーを除く）。先頭がプライマリ
pub fn work_areas() -> Vec<Rect> {
    let mut monitors: Vec<(Rect, bool)> = Vec::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut monitors as *mut Vec<(Rect, bool)> as isize),
        )
    };
    if !ok.as_bool() {
        return Vec::new();
    }
    candidates::primary_first(&monitors)
}

/// この DLL の `HINSTANCE`
pub fn instance() -> windows::Win32::Foundation::HINSTANCE {
    let module = dll::module_handle();
    if module.is_invalid() {
        // DllMain を経ていない場合（ありえないが）、呼び出しプロセスのハンドルで代用
        unsafe { GetModuleHandleW(PCWSTR::null()) }
            .map(|m| windows::Win32::Foundation::HINSTANCE(m.0))
            .unwrap_or_default()
    } else {
        windows::Win32::Foundation::HINSTANCE(module.0)
    }
}

/// ウィンドウクラスの登録管理。DLL がアンロードされるとウィンドウプロシージャが消えるため、
/// 最後のウィンドウを壊したらクラスも登録解除する（再ロード時に古いプロシージャを指さないように）
pub struct WindowClass {
    name: &'static str,
    count: Mutex<u32>,
}

impl WindowClass {
    /// 名前を指定して作る（登録はしない）
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            count: Mutex::new(0),
        }
    }

    /// クラス名（NUL 終端の UTF-16）
    pub fn name_wide(&self) -> Vec<u16> {
        wide(self.name)
    }

    /// ウィンドウを 1 つ作る前に呼ぶ。最初の 1 つで登録する
    pub fn acquire(&self, wndproc: WNDPROC) -> bool {
        let mut n = self.count.lock().unwrap_or_else(|e| e.into_inner());
        if *n == 0 {
            let name = wide(self.name);
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: WNDCLASS_STYLES(0),
                lpfnWndProc: wndproc,
                hInstance: instance(),
                hIcon: HICON::default(),
                hCursor: HCURSOR::default(),
                lpszClassName: PCWSTR(name.as_ptr()),
                ..Default::default()
            };
            if unsafe { RegisterClassExW(&class) } == 0 {
                return false;
            }
        }
        *n += 1;
        true
    }

    /// ウィンドウを 1 つ壊した後に呼ぶ。最後の 1 つで登録解除する
    pub fn release(&self) {
        let mut n = self.count.lock().unwrap_or_else(|e| e.into_inner());
        *n = n.saturating_sub(1);
        if *n == 0 {
            let name = wide(self.name);
            let _ = unsafe { UnregisterClassW(PCWSTR(name.as_ptr()), Some(instance())) };
        }
    }
}

/// 既定のウィンドウプロシージャへ
///
/// # Safety
/// ウィンドウプロシージャ内から、受け取った引数をそのまま渡すこと
pub unsafe fn default_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// 候補ウィンドウの文字の大きさ（96 DPI 基準の px。macOS 版 `CandidatePanel` と同じ）
const FONT_SURFACE: i32 = 15;
const FONT_NUMBER: i32 = 13;
const FONT_LABEL: i32 = 11;

/// 用途別のフォントと文字幅の計測（GDI）。`dpi` を掛けた大きさで作る
pub struct Fonts {
    dc: HDC,
    surface: HFONT,
    number: HFONT,
    label: HFONT,
    dpi: u32,
}

fn make_font(px: i32, dpi: u32) -> HFONT {
    let face = wide("Yu Gothic UI");
    unsafe {
        CreateFontW(
            -scale_px(px, dpi),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            u32::from(DEFAULT_PITCH.0) | u32::from(FF_DONTCARE.0),
            PCWSTR(face.as_ptr()),
        )
    }
}

impl Fonts {
    /// `dpi` 用のフォントを作る
    pub fn new(dpi: u32) -> Self {
        let screen = unsafe { GetDC(None) };
        let dc = unsafe { CreateCompatibleDC(Some(screen)) };
        unsafe { ReleaseDC(None, screen) };
        Self {
            dc,
            surface: make_font(FONT_SURFACE, dpi),
            number: make_font(FONT_NUMBER, dpi),
            label: make_font(FONT_LABEL, dpi),
            dpi,
        }
    }

    /// この DPI 用か
    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    /// 用途に対応するフォント
    pub fn font(&self, role: FontRole) -> HFONT {
        match role {
            FontRole::Number => self.number,
            FontRole::Surface => self.surface,
            FontRole::Label => self.label,
        }
    }
}

impl TextMeasure for Fonts {
    fn width(&self, text: &str, role: FontRole) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut size = windows::Win32::Foundation::SIZE::default();
        unsafe {
            let old = SelectObject(self.dc, HGDIOBJ(self.font(role).0));
            let _ = GetTextExtentPoint32W(self.dc, &utf16, &mut size);
            SelectObject(self.dc, old);
        }
        size.cx
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.surface.0));
            let _ = DeleteObject(HGDIOBJ(self.number.0));
            let _ = DeleteObject(HGDIOBJ(self.label.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

/// `*mut c_void` を介さずにユーザーデータを `Box<T>` として保持するための補助（`GWLP_USERDATA` 用）
pub fn into_user_data<T>(value: Box<T>) -> isize {
    Box::into_raw(value) as *mut c_void as isize
}

/// `into_user_data` で渡した値を参照に戻す。0 なら `None`
///
/// # Safety
/// `ptr` は `into_user_data::<T>` が返した値で、`from_user_data` で解放されていないこと。
/// 返す参照はウィンドウプロシージャの呼び出しの間だけ有効
pub unsafe fn user_data_ref<'a, T>(ptr: isize) -> Option<&'a T> {
    unsafe { (ptr as *const T).as_ref() }
}

/// `into_user_data` で渡した値を取り戻して破棄する
///
/// # Safety
/// `ptr` は `into_user_data::<T>` が返した値で、まだ解放されていないこと
pub unsafe fn drop_user_data<T>(ptr: isize) {
    if ptr != 0 {
        drop(unsafe { Box::from_raw(ptr as *mut T) });
    }
}
