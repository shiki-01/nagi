//! C ABI の疎通テスト。iOS 検証アプリ（Swift）が送る JSON-RPC 行と同じ形で呼ぶ。

use nagi_ffi::*;
use std::ffi::{CStr, CString};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"nagi-ios-spike","client_version":"0.1","os":"ios"}}"#;
const KEY_A: &str = r#"{"jsonrpc":"2.0","id":2,"method":"process_key","params":{"session_id":"s","keysym":97,"keycode":0,"text":"a","modifiers":{"shift":false,"ctrl":false,"alt":false,"meta":false},"kind":"down"}}"#;

fn send(handle: NagiServerHandle, line: &str) -> String {
    let c = CString::new(line).unwrap();
    unsafe {
        let p = nagi_handle_line(handle, c.as_ptr());
        let s = CStr::from_ptr(p).to_str().unwrap().to_string();
        nagi_string_free(p);
        s
    }
}

#[test]
fn handle_roundtrip() {
    let dir = std::env::temp_dir().join("nagi-ffi-smoke");
    let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
    unsafe {
        let handle = nagi_server_new(dir_c.as_ptr(), dir_c.as_ptr(), 1);
        assert!(!handle.is_null());

        let init = send(handle, INIT);
        assert!(init.contains("\"result\""), "initialize の応答: {init}");
        let key = send(handle, KEY_A);
        assert!(key.contains("\"result\""), "process_key の応答: {key}");
        assert!(key.contains("あ"), "ローマ字 a がひらがなになる: {key}");

        let notes = nagi_last_notes(handle);
        assert!(!CStr::from_ptr(notes).to_str().unwrap().is_empty());
        nagi_string_free(notes);
        nagi_server_free(handle);
    }
}

#[test]
fn null_handle_is_safe() {
    let c = CString::new(INIT).unwrap();
    unsafe {
        let p = nagi_handle_line(std::ptr::null_mut(), c.as_ptr());
        assert!(CStr::from_ptr(p).to_bytes().is_empty());
        nagi_string_free(p);
    }
}

#[test]
fn handle_with_memory_budget_roundtrip() {
    let dir = std::env::temp_dir().join("nagi-ffi-smoke-budget");
    let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
    for budget in [0u64, 100, 3072] {
        unsafe {
            let handle = nagi_server_new_with_budget(dir_c.as_ptr(), dir_c.as_ptr(), 0, budget);
            assert!(!handle.is_null(), "budget={budget}");
            send(handle, INIT);
            let key = send(handle, KEY_A);
            assert!(key.contains("\"result\""), "budget={budget}: {key}");
            nagi_server_free(handle);
        }
    }
}

#[test]
fn waker_and_poll_roundtrip() {
    use std::os::raw::c_void;
    unsafe extern "C" fn noop(_user_data: *mut c_void) {}

    let dir = std::env::temp_dir().join("nagi-ffi-smoke-waker");
    let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
    unsafe {
        let handle = nagi_server_new(dir_c.as_ptr(), dir_c.as_ptr(), 1);
        assert!(!handle.is_null());

        // initialize の前は通知を取り出さず、空文字列
        let p = nagi_poll_notifications(handle);
        assert!(CStr::from_ptr(p).to_bytes().is_empty());
        nagi_string_free(p);

        nagi_set_waker(handle, Some(noop), std::ptr::null_mut());
        send(handle, INIT);
        let p = nagi_poll_notifications(handle);
        // 通知が無ければ空（あっても JSON-RPC の通知行であること）
        let text = CStr::from_ptr(p).to_str().unwrap().to_string();
        assert!(text.is_empty() || text.contains("\"jsonrpc\""), "{text}");
        nagi_string_free(p);

        // 登録解除と NULL ハンドルは安全
        nagi_set_waker(handle, None, std::ptr::null_mut());
        nagi_set_waker(std::ptr::null_mut(), Some(noop), std::ptr::null_mut());
        let p = nagi_poll_notifications(std::ptr::null_mut());
        assert!(CStr::from_ptr(p).to_bytes().is_empty());
        nagi_string_free(p);

        // コールバックを登録したまま解放しても安全
        nagi_set_waker(handle, Some(noop), std::ptr::null_mut());
        nagi_server_free(handle);
    }
}
