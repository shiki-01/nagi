//! 入力モードの OS 側の表現: TSF のコンパートメント
//! （`GUID_COMPARTMENT_KEYBOARD_OPENCLOSE` / `GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION`）。
//! 読み書きと変更通知の購読だけを行い、値の対応と「同じモードなら送らない」は `nagi_windows::mode`。
//! タスクトレイ・言語バーの「あ / A」の表示は OS がこれらの値から行う
//! （入力方式は `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT` で登録済み）。

#![allow(unsafe_code)]

use nagi_windows::mode::CompartmentValues;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
    ITfCompartment, ITfCompartmentEventSink, ITfCompartmentMgr, ITfSource, ITfThreadMgr,
};
use windows::core::{GUID, IUnknown, Interface, Result};

/// 監視するコンパートメントの GUID
pub const WATCHED: [GUID; 2] = [
    GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
    GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
];

/// モードのコンパートメント 2 つと、変更通知の購読
pub struct ModeCompartments {
    client_id: u32,
    open_close: ITfCompartment,
    conversion: ITfCompartment,
    cookies: Vec<(ITfSource, u32)>,
}

impl ModeCompartments {
    /// スレッドマネージャーのコンパートメントを取得し、`sink` に変更を通知させる
    pub fn new(
        thread_mgr: &ITfThreadMgr,
        client_id: u32,
        sink: &ITfCompartmentEventSink,
    ) -> Result<Self> {
        let manager: ITfCompartmentMgr = thread_mgr.cast()?;
        let open_close = unsafe { manager.GetCompartment(&WATCHED[0]) }?;
        let conversion = unsafe { manager.GetCompartment(&WATCHED[1]) }?;
        let mut cookies = Vec::new();
        let unknown: IUnknown = sink.cast()?;
        for comp in [&open_close, &conversion] {
            let source: ITfSource = comp.cast()?;
            let cookie = unsafe { source.AdviseSink(&ITfCompartmentEventSink::IID, &unknown) }?;
            cookies.push((source, cookie));
        }
        Ok(Self {
            client_id,
            open_close,
            conversion,
            cookies,
        })
    }

    fn read_int(comp: &ITfCompartment) -> Option<i32> {
        let value = unsafe { comp.GetValue() }.ok()?;
        // 未設定は VT_EMPTY（変換できない）
        i32::try_from(&value).ok()
    }

    /// 現在の値。どちらかが未設定なら `None`（まだ誰も書いていない）
    pub fn read(&self) -> Option<CompartmentValues> {
        let open = Self::read_int(&self.open_close)?;
        let conversion = Self::read_int(&self.conversion)?;
        Some(CompartmentValues {
            open: open != 0,
            conversion: conversion as u32,
        })
    }

    /// 値を書く。変換モードを先に書き、開閉を後に書く（開閉の通知時点で変換モードが揃っているように）
    pub fn write(&self, values: CompartmentValues) -> Result<()> {
        unsafe {
            self.conversion
                .SetValue(self.client_id, &VARIANT::from(values.conversion as i32))?;
            self.open_close
                .SetValue(self.client_id, &VARIANT::from(i32::from(values.open)))
        }
    }
}

impl Drop for ModeCompartments {
    fn drop(&mut self) {
        for (source, cookie) in self.cookies.drain(..) {
            let _ = unsafe { source.UnadviseSink(cookie) };
        }
    }
}
