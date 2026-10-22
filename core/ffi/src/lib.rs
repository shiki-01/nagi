//! Nagi コアエンジンの C ABI ラッパー（iOS キーボード拡張等での利用）

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use nagi_server::{Server, build, parse_args};

/// 通知待ちコールバックの呼び出し先（`nagi_set_waker` で登録したもの）
struct WakeTarget {
    callback: unsafe extern "C" fn(*mut c_void),
    user_data: *mut c_void,
}

// SAFETY: `user_data` は呼び出し側（C / Swift）が、任意のスレッドから `callback` に渡されても
// よいものとして登録する責務を負う（`nagi_set_waker` の契約）。このクレートは中身を参照しない
unsafe impl Send for WakeTarget {}

/// 登録されたコールバック。`nagi_server_free` が空にしてから `Server` を捨てるので、解放後に
/// バックグラウンドスレッドから `user_data` が使われることはない
type WakeSlot = Arc<Mutex<Option<WakeTarget>>>;

/// Server と初期化時の診断情報を保持する不透明なハンドル
pub struct ServerHandle {
    /// JSON-RPC サーバー
    pub server: Server,
    /// 初期化時の診断情報（notes + profile 情報）
    pub notes: String,
    /// 通知待ちコールバック
    wake: WakeSlot,
}

/// `nagi_server_new` から返されるハンドルのポインタ型。
pub type NagiServerHandle = *mut ServerHandle;

// ---- 初期化 ----

/// サーバーを初期化して返す。プロセスのメモリ上限を指定できる版。
///
/// # Arguments
/// - `data_dir`: データディレクトリのパス（UTF-8 C 文字列、NULL で未指定）
/// - `user_dir`: ユーザーデータディレクトリのパス（UTF-8 C 文字列、NULL で未指定）
/// - `no_llm`: 0 以外なら LLM を無効化（--no-llm オプション相当）
/// - `memory_budget_mb`: このプロセスが OS から課されているメモリ上限（MB）。0 は指定なし。
///   iOS のキーボード拡張では `phys_footprint + os_proc_available_memory()` を渡す。
///   コアが LLM に必要な量（`MIN_MEMORY_BUDGET_FOR_LLM_MB`）未満なら、LLM を読み込まず辞書のみで動く
///
/// # Returns
/// 成功時は不透明なハンドル（`nagi_server_free` で解放必須）、
/// 失敗時（UTF-8 デコード失敗等）は NULL。
///
/// # Safety
/// - `data_dir` と `user_dir` は NULL-terminated UTF-8 文字列であること。
/// - 返されたハンドルは `nagi_server_free` で解放すること。
/// - 返されたハンドルを複数のスレッドから同時にアクセスしないこと（Server 内部は Mutex でロックしている）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_server_new_with_budget(
    data_dir: *const c_char,
    user_dir: *const c_char,
    no_llm: i32,
    memory_budget_mb: u64,
) -> NagiServerHandle {
    // SAFETY: C 呼び出し側の責務：data_dir / user_dir は NULL-terminated UTF-8 文字列
    let result = catch_unwind(|| {
        let mut args = Vec::new();

        // data_dir を引数に追加
        if !data_dir.is_null() {
            // SAFETY: C 呼び出し側の責務で data_dir は NULL-terminated UTF-8 文字列
            let data_dir_str = unsafe { CStr::from_ptr(data_dir) }
                .to_str()
                .map_err(|_| "data_dir が有効な UTF-8 ではありません")?;
            args.push("--data-dir".to_string());
            args.push(data_dir_str.to_string());
        }

        // user_dir を引数に追加
        if !user_dir.is_null() {
            // SAFETY: C 呼び出し側の責務で user_dir は NULL-terminated UTF-8 文字列
            let user_dir_str = unsafe { CStr::from_ptr(user_dir) }
                .to_str()
                .map_err(|_| "user_dir が有効な UTF-8 ではありません")?;
            args.push("--user-dir".to_string());
            args.push(user_dir_str.to_string());
        }

        // no_llm を引数に追加
        if no_llm != 0 {
            args.push("--no-llm".to_string());
        }

        // メモリ上限（0 は指定なし）
        if memory_budget_mb != 0 {
            args.push("--memory-budget-mb".to_string());
            args.push(memory_budget_mb.to_string());
        }

        // parse_args を呼ぶ
        let mut options =
            parse_args(args.into_iter()).map_err(|e| format!("引数解析エラー: {e}"))?;

        // 同期ロード（実機検証ではロード時間を測りたいため）
        options.async_model_load = false;

        // build する
        let built = build(&options);

        // 診断情報を集める（notes + profile + reason）
        let mut notes_str = built.notes.join("\n");
        notes_str.push_str(&format!(
            "\nプロファイル {name} ({reason})",
            name = built.runtime.profile().as_str(),
            reason = built.runtime.profile_reason()
        ));

        // Server を作る
        let server = Server::from_built(built);

        Ok::<ServerHandle, String>(ServerHandle {
            server,
            notes: notes_str,
            wake: Arc::new(Mutex::new(None)),
        })
    });

    match result {
        Ok(Ok(handle)) => Box::into_raw(Box::new(handle)),
        _ => std::ptr::null_mut(),
    }
}

/// サーバーを初期化して返す（メモリ上限の指定なし。`nagi_server_new_with_budget` の `memory_budget_mb` = 0）。
///
/// # Safety
/// `nagi_server_new_with_budget` と同じ。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_server_new(
    data_dir: *const c_char,
    user_dir: *const c_char,
    no_llm: i32,
) -> NagiServerHandle {
    // SAFETY: 引数の条件は `nagi_server_new_with_budget` と同じで、呼び出し側が満たす
    unsafe { nagi_server_new_with_budget(data_dir, user_dir, no_llm, 0) }
}

// ---- JSON-RPC 処理 ----

/// JSON-RPC リクエスト行を処理し、応答行を返す。
///
/// # Arguments
/// - `handle`: `nagi_server_new` から返されたハンドル
/// - `line`: JSON-RPC リクエストの1行（UTF-8 C 文字列）
///
/// # Returns
/// 応答行（改行区切り JSON-RPC 行を連結）を UTF-8 C 文字列で返す。
/// 呼び出し側で `nagi_string_free` で解放すること。
/// handle が NULL またはエラーの場合は、空文字列を返す。
///
/// # Safety
/// - `handle` は `nagi_server_new` から返された有効なハンドルであること。
/// - `line` は NULL-terminated UTF-8 文字列であること。
/// - 返された文字列は `nagi_string_free` で解放すること。
/// - 同一の handle に複数スレッドから同時に呼び出さないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_handle_line(
    handle: NagiServerHandle,
    line: *const c_char,
) -> *mut c_char {
    // SAFETY: C 呼び出し側の責務：handle は有効なポインタ、line は NULL-terminated UTF-8 文字列
    let result = catch_unwind(|| {
        if handle.is_null() {
            return Err("ハンドルが NULL です".to_string());
        }
        if line.is_null() {
            return Err("line が NULL です".to_string());
        }

        // SAFETY: line は NULL でないことを確認済み。C 呼び出し側の責務で NULL-terminated UTF-8 文字列
        let line_str = unsafe { CStr::from_ptr(line) }
            .to_str()
            .map_err(|_| "line が有効な UTF-8 ではありません".to_string())?;

        // SAFETY: C 呼び出し側の責務で handle は有効なポインタ
        let handle_ref = unsafe { &mut *handle };
        let response_lines = handle_ref.server.handle_line(line_str);
        let response = response_lines.join("\n");

        Ok(response)
    });

    match result {
        Ok(Ok(response)) => {
            if let Ok(cstring) = CString::new(response) {
                cstring.into_raw()
            } else {
                CString::new("").unwrap().into_raw()
            }
        }
        _ => CString::new("").unwrap().into_raw(),
    }
}

// ---- 非同期通知 ----

/// 通知（`update_preedit`・`update_candidates`・`notify`）が取り出せるようになったときに呼ばれる
/// コールバックを登録する。LLM が同期の待ち時間（`conversion.sync_budget_ms`）を超えた分の完了や、
/// ライブ変換の結果など、次のリクエストを待たずに届けるべき通知のために使う。
///
/// # Arguments
/// - `handle`: `nagi_server_new` から返されたハンドル
/// - `callback`: コールバック。NULL なら登録を解除する
/// - `user_data`: コールバックにそのまま渡す値
///
/// # Safety
/// - `callback` は **任意のスレッドから、`handle` のロック内で** 呼ばれる。中で `nagi_*` を呼ばず、
///   呼び出し側のキューに `nagi_poll_notifications` の呼び出しを積んですぐに戻ること。
/// - `user_data` は `nagi_server_free` が返るまで有効であること（解放後は呼ばれない）。
/// - `nagi_handle_line` / `nagi_poll_notifications` と同時に呼ばないこと。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_set_waker(
    handle: NagiServerHandle,
    callback: Option<unsafe extern "C" fn(*mut c_void)>,
    user_data: *mut c_void,
) {
    let _ = catch_unwind(|| {
        if handle.is_null() {
            return;
        }
        // SAFETY: C 呼び出し側の責務で handle は有効なポインタ
        let handle_ref = unsafe { &mut *handle };
        if let Ok(mut slot) = handle_ref.wake.lock() {
            *slot = callback.map(|callback| WakeTarget {
                callback,
                user_data,
            });
        }
        // 登録の有無にかかわらず、Server 側のフックは常に同じ 1 つ（スロットを見て呼ぶ）
        let slot = Arc::clone(&handle_ref.wake);
        handle_ref.server.set_wake_callback(Arc::new(move || {
            if let Ok(slot) = slot.lock()
                && let Some(target) = slot.as_ref()
            {
                // SAFETY: 登録した呼び出し側が、user_data を有効に保つ契約（nagi_set_waker）
                unsafe { (target.callback)(target.user_data) };
            }
        }));
    });
}

/// 次のリクエストを待たずに配信すべき通知の行を取り出す。`nagi_set_waker` のコールバックで
/// 起こされたあとに呼ぶ。
///
/// # Returns
/// 通知行（改行区切りの JSON-RPC 通知）を UTF-8 C 文字列で返す。なければ空文字列。
/// `nagi_string_free` で解放必須。handle が NULL の場合も空文字列。
/// `initialize` の前は、通知を取り出さずに（積んだまま）空文字列を返す。
///
/// # Safety
/// - `handle` は `nagi_server_new` から返された有効なハンドルであること。
/// - `nagi_handle_line` と同時に呼ばないこと（同一のキューで直列に呼ぶ）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_poll_notifications(handle: NagiServerHandle) -> *mut c_char {
    let result = catch_unwind(|| {
        if handle.is_null() {
            return String::new();
        }
        // SAFETY: C 呼び出し側の責務で handle は有効なポインタ
        let handle_ref = unsafe { &mut *handle };
        handle_ref.server.poll_notifications().join("\n")
    });
    CString::new(result.unwrap_or_default())
        .unwrap_or_default()
        .into_raw()
}

// ---- 解放 ----

/// UTF-8 C 文字列を解放する。
///
/// # Arguments
/// - `s`: `nagi_handle_line` または `nagi_last_notes` から返された文字列ポインタ
///
/// # Safety
/// - `s` は `nagi_handle_line` または `nagi_last_notes` から返された有効なポインタであること。
/// - 二重解放やダングリングポインタの使用は未定義動作。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: C 呼び出し側の責務で s は CString::into_raw から返されたポインタ
        let _ = unsafe { CString::from_raw(s) };
    }
}

/// サーバーハンドルを解放する。
///
/// # Arguments
/// - `handle`: `nagi_server_new` から返されたハンドル
///
/// # Safety
/// - `handle` は `nagi_server_new` から返された有効なハンドルであること。
/// - 二重解放は未定義動作。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_server_free(handle: NagiServerHandle) {
    if !handle.is_null() {
        // SAFETY: C 呼び出し側の責務で handle は Box::into_raw から返されたポインタ
        let handle = unsafe { Box::from_raw(handle) };
        // 先にコールバックの登録を外す（実行中のコールバックがあれば終わるまで待つ）。
        // この後にバックグラウンドスレッドが起こそうとしても、何も呼ばれない
        if let Ok(mut slot) = handle.wake.lock() {
            *slot = None;
        }
        // 学習データの保存を試みる（失敗・パニックしても解放は続ける）
        let _ = catch_unwind(AssertUnwindSafe(|| {
            handle.server.runtime().flush_learning(true);
        }));
    }
}

// ---- メタデータ ----

/// Nagi コアエンジンのバージョン文字列（静的）を返す。
///
/// # Returns
/// バージョン文字列（ワークスペースの version。例：`0.1.0`）。解放不要。
#[unsafe(no_mangle)]
pub extern "C" fn nagi_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// サーバー初期化時の診断情報を返す。
///
/// # Arguments
/// - `handle`: `nagi_server_new` から返されたハンドル
///
/// # Returns
/// 診断情報（起動時の notes + プロファイル情報）を UTF-8 C 文字列で返す。
/// 呼び出し側で `nagi_string_free` で解放すること。
/// handle が NULL の場合は、空文字列を返す。
///
/// # Safety
/// - `handle` は `nagi_server_new` から返された有効なハンドルであること。
/// - 返された文字列は `nagi_string_free` で解放すること。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nagi_last_notes(handle: NagiServerHandle) -> *mut c_char {
    if handle.is_null() {
        return CString::new("").unwrap().into_raw();
    }

    // SAFETY: C 呼び出し側の責務で handle は有効なポインタ
    let handle_ref = unsafe { &*handle };
    if let Ok(cstring) = CString::new(handle_ref.notes.as_str()) {
        cstring.into_raw()
    } else {
        CString::new("").unwrap().into_raw()
    }
}
