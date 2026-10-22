//! 登録・登録解除（`DllRegisterServer` / `DllUnregisterServer` の実体）。
//!
//! COM の CLSID（`HKCR\CLSID\{...}\InprocServer32`）と、TSF の入力方式プロファイル・カテゴリを登録する。
//! 管理者権限での `regsvr32` を想定する。

#![allow(unsafe_code)]

use windows::Win32::Foundation::{E_FAIL, ERROR_SUCCESS};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED, ITfCategoryMgr, ITfInputProcessorProfileMgr,
};
use windows::core::{PCWSTR, Result};

use crate::dll;
use crate::guids::{
    CLSID_NAGI_TEXT_SERVICE, GUID_NAGI_PROFILE, LANGID_JAPANESE, PROFILE_DESCRIPTION, braced,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn clsid_key() -> String {
    format!("CLSID\\{}", braced(&CLSID_NAGI_TEXT_SERVICE))
}

/// 文字列値（`name` が空なら既定値）を書く
fn set_string(key: HKEY, name: Option<&str>, value: &str) -> Result<()> {
    let name_w = name.map(wide);
    let value_w = wide(value);
    let bytes: Vec<u8> = value_w.iter().flat_map(|u| u.to_le_bytes()).collect();
    let name_ptr = name_w
        .as_ref()
        .map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    let status = unsafe { RegSetValueExW(key, name_ptr, None, REG_SZ, Some(&bytes)) };
    status.ok()
}

fn create_key(parent: HKEY, path: &str) -> Result<HKEY> {
    let path_w = wide(path);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            parent,
            PCWSTR(path_w.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    }
    .ok()?;
    Ok(key)
}

fn register_com_server(dll_path: &str) -> Result<()> {
    let base = clsid_key();
    let key = create_key(HKEY_CLASSES_ROOT, &base)?;
    let r = set_string(key, None, PROFILE_DESCRIPTION);
    let _ = unsafe { RegCloseKey(key) };
    r?;

    let inproc = create_key(HKEY_CLASSES_ROOT, &format!("{base}\\InprocServer32"))?;
    let r = set_string(inproc, None, dll_path)
        .and_then(|()| set_string(inproc, Some("ThreadingModel"), "Apartment"));
    let _ = unsafe { RegCloseKey(inproc) };
    r
}

fn register_tsf(dll_path: &str) -> Result<()> {
    let profiles: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }?;
    let description: Vec<u16> = PROFILE_DESCRIPTION.encode_utf16().collect();
    let icon: Vec<u16> = dll_path.encode_utf16().collect();
    // TODO: アイコン（リソース）を DLL に埋め込み、アイコン番号を指定する。いまは 0 番
    unsafe {
        profiles.RegisterProfile(
            &CLSID_NAGI_TEXT_SERVICE,
            LANGID_JAPANESE,
            &GUID_NAGI_PROFILE,
            &description,
            &icon,
            0,
            HKL::default(),
            0,
            true,
            0,
        )
    }?;

    let categories: ITfCategoryMgr =
        unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }?;
    for category in [
        GUID_TFCAT_TIP_KEYBOARD,
        GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
        GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
        GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
    ] {
        unsafe {
            categories.RegisterCategory(
                &CLSID_NAGI_TEXT_SERVICE,
                &category,
                &CLSID_NAGI_TEXT_SERVICE,
            )
        }?;
    }
    Ok(())
}

/// 登録する
pub fn register() -> Result<()> {
    let path = dll::module_path().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let dll_path = path.to_string_lossy().into_owned();
    // regsvr32 は COM を初期化済みだが、他の呼び出し元でも動くよう初期化する（二重初期化は無害）
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    register_com_server(&dll_path)?;
    register_tsf(&dll_path)
}

/// 登録を解除する。途中で失敗しても残りを続ける（最初のエラーを返す）
pub fn unregister() -> Result<()> {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let mut first_error: Option<windows::core::Error> = None;
    let mut keep = |r: Result<()>| {
        if let Err(e) = r {
            first_error.get_or_insert(e);
        }
    };

    if let Ok(profiles) = unsafe {
        CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        )
    } {
        keep(unsafe {
            profiles.UnregisterProfile(
                &CLSID_NAGI_TEXT_SERVICE,
                LANGID_JAPANESE,
                &GUID_NAGI_PROFILE,
                0,
            )
        });
    }
    if let Ok(categories) = unsafe {
        CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
    } {
        for category in [
            GUID_TFCAT_TIP_KEYBOARD,
            GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
            GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
            GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
        ] {
            keep(unsafe {
                categories.UnregisterCategory(
                    &CLSID_NAGI_TEXT_SERVICE,
                    &category,
                    &CLSID_NAGI_TEXT_SERVICE,
                )
            });
        }
    }
    let key_w = wide(&clsid_key());
    let status = unsafe { RegDeleteTreeW(HKEY_CLASSES_ROOT, PCWSTR(key_w.as_ptr())) };
    if status != ERROR_SUCCESS {
        keep(status.ok());
    }
    first_error.map_or(Ok(()), Err)
}
