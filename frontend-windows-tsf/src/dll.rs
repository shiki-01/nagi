//! DLL のエントリポイント（COM のインプロセスサーバー）。

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HMODULE, S_FALSE, S_OK,
    TRUE,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::core::{BOOL, GUID, HRESULT, IUnknown, Interface, Ref, Result, implement};

use crate::guids::CLSID_NAGI_TEXT_SERVICE;
use crate::register;
use crate::text_service::TextService;

const DLL_PROCESS_ATTACH: u32 = 1;

/// この DLL のモジュールハンドル（`DllMain` で保存）
static MODULE: AtomicIsize = AtomicIsize::new(0);
/// 生きているオブジェクト数 + `LockServer` の数。0 なら `DllCanUnloadNow` が `S_OK`
static LIVE_OBJECTS: AtomicI32 = AtomicI32::new(0);

/// オブジェクトを 1 つ作った
pub fn add_object() {
    LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
}

/// オブジェクトを 1 つ破棄した
pub fn release_object() {
    LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
}

/// この DLL のハンドル
pub fn module_handle() -> HMODULE {
    HMODULE(MODULE.load(Ordering::SeqCst) as *mut c_void)
}

/// この DLL の絶対パス
pub fn module_path() -> Option<PathBuf> {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(Some(module_handle()), &mut buf) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

/// この DLL のあるディレクトリ
pub fn module_directory() -> Option<PathBuf> {
    module_path()?.parent().map(PathBuf::from)
}

#[implement(IClassFactory)]
struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        if ppvobject.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppvobject = std::ptr::null_mut() };
        if punkouter.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let result = std::panic::catch_unwind(|| {
            let service: IUnknown = TextService::new().into();
            unsafe { service.query(riid, ppvobject) }.ok()
        });
        result.unwrap_or_else(|_| Err(E_FAIL.into()))
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        if flock.as_bool() {
            add_object();
        } else {
            release_object();
        }
        Ok(())
    }
}

/// DLL のロード・アンロード通知
///
/// # Safety
/// OS のローダーから呼ばれる。ローダーロック下なので、ここでは何もしない（スレッド通知を止めるだけ）
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(hinst: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(hinst.0 as isize, Ordering::SeqCst);
        let _ = unsafe { DisableThreadLibraryCalls(hinst) };
    }
    TRUE
}

/// COM がクラスファクトリを要求する
///
/// # Safety
/// `rclsid` / `riid` は有効な GUID へのポインタ、`ppv` は書き込み可能なポインタであること（COM の規約）
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = std::ptr::null_mut() };
    if unsafe { *rclsid } != CLSID_NAGI_TEXT_SERVICE {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = ClassFactory.into();
    unsafe { factory.query(riid, ppv) }
}

/// アンロードしてよいか
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE_OBJECTS.load(Ordering::SeqCst) <= 0 {
        S_OK
    } else {
        S_FALSE
    }
}

/// `regsvr32` からの登録（レジストリ + TSF のプロファイル・カテゴリ）
#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    match std::panic::catch_unwind(register::register) {
        Ok(Ok(())) => S_OK,
        Ok(Err(e)) => e.code(),
        Err(_) => E_FAIL,
    }
}

/// `regsvr32 /u` からの登録解除
#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    match std::panic::catch_unwind(register::unregister) {
        Ok(Ok(())) => S_OK,
        Ok(Err(e)) => e.code(),
        Err(_) => E_FAIL,
    }
}
