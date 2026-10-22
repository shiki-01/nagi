//! この IME を識別する GUID。値は固定（変えると登録済みの入力方式が別物になる）。

use windows::core::GUID;

/// TextService の CLSID
pub const CLSID_NAGI_TEXT_SERVICE: GUID = GUID::from_u128(0xB0D6897E_C414_4BB1_B621_46EEE659E0CD);

/// 入力方式（言語プロファイル）の GUID
pub const GUID_NAGI_PROFILE: GUID = GUID::from_u128(0x5B02A01F_6B4F_430F_A6A8_D9B4D83C1918);

/// 言語バーのボタンの GUID
pub const GUID_NAGI_LANGBAR_BUTTON: GUID = GUID::from_u128(0x7E3C0B52_9A41_4D6B_8F27_1C5A94D0E3B8);

/// 日本語（ja-JP）の LANGID
pub const LANGID_JAPANESE: u16 = 0x0411;

/// 入力方式の表示名
pub const PROFILE_DESCRIPTION: &str = "Nagi";

/// GUID をレジストリのキー名の形式 `{XXXXXXXX-XXXX-...}` にする
pub fn braced(guid: &GUID) -> String {
    format!("{{{guid:?}}}")
}
