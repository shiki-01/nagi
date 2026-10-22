//! UI スレッドを起こすための隠しメッセージウィンドウ（`HWND_MESSAGE`）。
//!
//! - 別スレッドで届くエンジンの非同期通知（ライブ変換・LLM 結果の差し替え等）を `PostMessage` で
//!   UI スレッドに渡し、次のキー入力を待たずに反映する
//! - 定期タイマー（環境の報告・スリープ復帰の検出）
//! - 電源通知（スリープ復帰・AC/電池・バッテリー節約の切り替わり）
//!
//! ウィンドウプロシージャは受けた出来事を [`MessageEvent`] にしてハンドラへ渡すだけ。

#![allow(unsafe_code)]

use std::sync::Arc;

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Power::{
    HPOWERNOTIFY, RegisterPowerSettingNotification, RegisterSuspendResumeNotification,
    UnregisterPowerSettingNotification, UnregisterSuspendResumeNotification,
};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_POWER_SAVING_STATUS};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW,
    HWND_MESSAGE, KillTimer, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_POWERSETTINGCHANGE,
    PostMessageW, SetTimer, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_NCDESTROY,
    WM_POWERBROADCAST, WM_TIMER,
};
use windows::core::PCWSTR;

use crate::engine::Waker;
use crate::win::{self, WindowClass, drop_user_data, into_user_data, user_data_ref, wide};

static CLASS: WindowClass = WindowClass::new("NagiMessageWindow");

/// エンジンの非同期通知がキューに積まれた
const WM_NAGI_EVENTS: u32 = WM_APP + 1;
const TIMER_ID: usize = 1;

/// 定期タイマーの間隔（ミリ秒）。環境の変化とスリープ復帰の検出に使う
pub const TICK_INTERVAL_MS: u32 = 30_000;

/// ウィンドウで起きた出来事
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageEvent {
    /// エンジンの非同期通知が溜まった
    EngineEvents,
    /// 定期タイマー
    Tick,
    /// OS からのスリープ復帰の通知
    Resume,
    /// 電源の状態（AC/電池・バッテリー節約）が変わった
    PowerChanged,
}

type Handler = Box<dyn Fn(MessageEvent)>;

/// 隠しメッセージウィンドウ。作ったスレッドでのみ使う
pub struct MessageWindow {
    hwnd: HWND,
    notifications: Vec<(HPOWERNOTIFY, bool)>,
}

impl MessageWindow {
    /// 作る。`handler` は UI スレッド（ウィンドウを作ったスレッド）で呼ばれる
    pub fn create(handler: Handler) -> Option<Self> {
        if !CLASS.acquire(Some(wndproc)) {
            return None;
        }
        let class = CLASS.name_wide();
        let title = wide("Nagi message window");
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(win::instance()),
                None,
            )
        };
        let Ok(hwnd) = created else {
            CLASS.release();
            return None;
        };
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, into_user_data(Box::new(handler))) };
        unsafe { SetTimer(Some(hwnd), TIMER_ID, TICK_INTERVAL_MS, None) };

        let mut notifications = Vec::new();
        let recipient = HANDLE(hwnd.0);
        if let Ok(h) =
            unsafe { RegisterSuspendResumeNotification(recipient, DEVICE_NOTIFY_WINDOW_HANDLE) }
        {
            notifications.push((h, false));
        }
        for guid in [GUID_ACDC_POWER_SOURCE, GUID_POWER_SAVING_STATUS] {
            if let Ok(h) = unsafe {
                RegisterPowerSettingNotification(recipient, &guid, DEVICE_NOTIFY_WINDOW_HANDLE)
            } {
                notifications.push((h, true));
            }
        }
        Some(Self {
            hwnd,
            notifications,
        })
    }

    /// 別スレッドから UI スレッドを起こす関数（`EngineEvents` が届く）
    pub fn waker(&self) -> Waker {
        let raw = self.hwnd.0 as isize;
        Arc::new(move || {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            let _ = unsafe { PostMessageW(Some(hwnd), WM_NAGI_EVENTS, WPARAM(0), LPARAM(0)) };
        })
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        for (handle, is_setting) in self.notifications.drain(..) {
            let _ = if is_setting {
                unsafe { UnregisterPowerSettingNotification(handle) }
            } else {
                unsafe { UnregisterSuspendResumeNotification(handle) }
            };
        }
        let _ = unsafe { KillTimer(Some(self.hwnd), TIMER_ID) };
        let _ = unsafe { DestroyWindow(self.hwnd) };
        CLASS.release();
    }
}

fn dispatch(hwnd: HWND, event: MessageEvent) {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if let Some(handler) = unsafe { user_data_ref::<Handler>(ptr) } {
        // パニックをウィンドウプロシージャ（FFI 境界）から出さない
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(event)));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NAGI_EVENTS => {
            dispatch(hwnd, MessageEvent::EngineEvents);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_ID => {
            dispatch(hwnd, MessageEvent::Tick);
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            match wparam.0 as u32 {
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                    dispatch(hwnd, MessageEvent::Resume);
                }
                PBT_POWERSETTINGCHANGE => dispatch(hwnd, MessageEvent::PowerChanged),
                _ => {}
            }
            LRESULT(1)
        }
        WM_NCDESTROY => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop_user_data::<Handler>(ptr);
                win::default_proc(hwnd, msg, wparam, lparam)
            }
        }
        _ => unsafe { win::default_proc(hwnd, msg, wparam, lparam) },
    }
}
