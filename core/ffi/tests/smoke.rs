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
fn null_line_is_safe() {
    let dir = std::env::temp_dir().join("nagi-ffi-smoke-null-line");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let c = CString::new(dir.to_str().unwrap()).unwrap();
    unsafe {
        let handle = nagi_server_new(c.as_ptr(), c.as_ptr(), 1);
        assert!(!handle.is_null());
        let p = nagi_handle_line(handle, std::ptr::null());
        assert!(!p.is_null());
        assert!(CStr::from_ptr(p).to_bytes().is_empty());
        nagi_string_free(p);
        nagi_server_free(handle);
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

#[test]
fn settings_roundtrip_via_user_dir() {
    let dir = std::env::temp_dir().join(format!("nagi-ffi-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
    unsafe {
        let handle = nagi_server_new(dir_c.as_ptr(), dir_c.as_ptr(), 1);
        assert!(!handle.is_null());
        send(handle, INIT);
        let got = send(
            handle,
            r#"{"jsonrpc":"2.0","id":3,"method":"get_settings"}"#,
        );
        assert!(got.contains("\"conversion.n_threads\""), "{got}");
        assert!(got.contains("\"level\":\"simple\""), "{got}");
        let upd = send(
            handle,
            r#"{"jsonrpc":"2.0","id":4,"method":"update_settings","params":{"changes":[{"key":"conversion.live_conversion","value":{"type":"bool","value":false}},{"key":"conversion.n_threads","value":{"type":"int","value":2}}]}}"#,
        );
        assert!(upd.contains("\"applied\""), "{upd}");
        assert!(!upd.contains("\"errors\":[{"), "{upd}");
        // user_dir 直下の config.toml に書かれる
        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(text.contains("live_conversion = false"), "{text}");
        assert!(text.contains("n_threads = 2"), "{text}");
        nagi_server_free(handle);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_table_and_model_unload_via_handle_line() {
    let dir = std::env::temp_dir().join("nagi-ffi-smoke-key-table");
    let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
    unsafe {
        let handle = nagi_server_new_with_budget(dir_c.as_ptr(), dir_c.as_ptr(), 1, 3072);
        assert!(!handle.is_null());
        // capabilities つきの initialize（任意項目。省略したフロントも従来どおり動く）
        let init = send(
            handle,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":1,"client_name":"t","client_version":"0","os":"ios","capabilities":{"input_method":"touch","environment":{"memory_budget_mb":3072}}}}"#,
        );
        assert!(init.contains("\"result\""), "{init}");

        let table = send(
            handle,
            r#"{"jsonrpc":"2.0","id":2,"method":"get_key_table"}"#,
        );
        assert!(
            table.contains("\"keys\"") && table.contains("\"variants\""),
            "{table}"
        );
        assert!(
            table.contains("\"toggle\":[\"か\",\"き\",\"く\",\"け\",\"こ\"]"),
            "{table}"
        );

        // モデルを持たない構成でも、解放 / 再読み込みの要求は安全に受け付ける
        for (id, method) in [(3, "unload_model"), (4, "reload_model")] {
            let r = send(
                handle,
                &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}"}}"#),
            );
            assert!(r.contains("\"result\""), "{method}: {r}");
        }
        let key = send(handle, KEY_A);
        assert!(key.contains("あ"), "{key}");
        nagi_server_free(handle);
    }
}

#[test]
fn optional_data_sources_and_index_build() {
    use std::ffi::CString;

    let p = nagi_ffi::nagi_optional_data_sources();
    let json = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
    unsafe { nagi_ffi::nagi_string_free(p) };
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["english"]["index_file"], "words.fst");
    assert_eq!(v["english"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(v["loanword"]["index_file"], "loanword.fst");

    // 組み立ての失敗は理由を返し、成功は NULL
    let dir = std::env::temp_dir().join(format!("nagi-ffi-opt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let missing = CString::new(dir.join("none.zip").to_str().unwrap()).unwrap();
    let out = CString::new(dir.join("out.fst").to_str().unwrap()).unwrap();
    let kind = CString::new("loanword").unwrap();
    let err = unsafe {
        nagi_ffi::nagi_build_optional_index(kind.as_ptr(), missing.as_ptr(), out.as_ptr())
    };
    assert!(!err.is_null());
    unsafe { nagi_ffi::nagi_string_free(err) };
    let bad = CString::new("other").unwrap();
    let err = unsafe {
        nagi_ffi::nagi_build_optional_index(bad.as_ptr(), missing.as_ptr(), out.as_ptr())
    };
    assert!(!err.is_null());
    unsafe { nagi_ffi::nagi_string_free(err) };
    let err = unsafe {
        nagi_ffi::nagi_build_optional_index(std::ptr::null(), missing.as_ptr(), out.as_ptr())
    };
    assert!(!err.is_null());
    unsafe { nagi_ffi::nagi_string_free(err) };

    // 最小の JMdict（zip の JSON）から組み立てる
    assert_eq!(v["loanword"]["sha256"].as_str().unwrap().len(), 64);
    let archive = dir.join("jmdict.json.zip");
    std::fs::write(
        &archive,
        nagi_loanword::archive::zip_of_json(
            r#"{"words":[{"kana":[{"text":"アルバイト"}],"sense":[{"languageSource":[{"lang":"ger","text":"Arbeit"}]}]}]}"#,
        ),
    )
    .unwrap();
    let archive_c = CString::new(archive.to_str().unwrap()).unwrap();
    let err = unsafe {
        nagi_ffi::nagi_build_optional_index(kind.as_ptr(), archive_c.as_ptr(), out.as_ptr())
    };
    assert!(err.is_null());
    let index = nagi_loanword::LoanwordIndex::load(&dir.join("out.fst")).unwrap();
    assert_eq!(index.lookup("アルバイト", true)[0].spelling, "Arbeit");
    let _ = std::fs::remove_dir_all(&dir);
}
