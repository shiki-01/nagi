//! 表示属性（下線の見た目）。`ITfDisplayAttributeProvider` の実装と、preedit の描画計画
//! （`nagi_windows::preedit_style::plan`）を `GUID_PROP_ATTRIBUTE` に書く処理。
//!
//! 属性は `nagi_windows::preedit_style::DISPLAY_ATTRS`（4 種）ごとに GUID を 1 つ割り当てる。
//! GUID は固定（変えると、ホストアプリが保持する属性の対応が変わる）。色は指定しない。

#![allow(unsafe_code)]

use std::cell::Cell;

use nagi_windows::preedit_style::{
    AttrRun, DISPLAY_ATTRS, DisplayAttr, UnderlineKind, range_shifts,
};
use windows::Win32::Foundation::{E_INVALIDARG, E_POINTER, S_FALSE};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, GUID_PROP_ATTRIBUTE, IEnumTfDisplayAttributeInfo,
    IEnumTfDisplayAttributeInfo_Impl, ITfCategoryMgr, ITfContext, ITfDisplayAttributeInfo,
    ITfDisplayAttributeInfo_Impl, ITfRange, TF_ATTR_INPUT, TF_ATTR_TARGET_CONVERTED,
    TF_DISPLAYATTRIBUTE, TF_LS_DASH, TF_LS_DOT, TF_LS_SOLID,
};
use windows::core::{BSTR, GUID, Result, implement};

/// 属性ごとの GUID（`DISPLAY_ATTRS` と同じ並び）
pub const ATTR_GUIDS: [GUID; DISPLAY_ATTRS.len()] = [
    GUID::from_u128(0x2FEC1D3F_D9EC_4029_B1C2_02AE15A3EFC5),
    GUID::from_u128(0xC154BAC5_3427_4517_B3D3_4BC1CC67F39D),
    GUID::from_u128(0x98D5FD9D_016D_49AD_B522_9A0BDA448579),
    GUID::from_u128(0xA3890215_65BB_4443_B21B_E4424619BFAA),
];

/// 属性の説明（設定 UI 等に出ることがある）
const DESCRIPTIONS: [&str; DISPLAY_ATTRS.len()] = [
    "Nagi 入力中",
    "Nagi 注目文節",
    "Nagi 確定済み・保留",
    "Nagi スタイル変換",
];

/// 登録済みの属性 GUID の識別子（`ITfCategoryMgr::RegisterGUID` の結果）。`GUID_PROP_ATTRIBUTE` の値に使う
pub type AttrAtoms = [u32; DISPLAY_ATTRS.len()];

/// 属性 → TSF の `TF_DISPLAYATTRIBUTE`。注目文節（太線）は「変換中の対象」、それ以外は「入力中」
fn tf_attribute(attr: DisplayAttr) -> TF_DISPLAYATTRIBUTE {
    TF_DISPLAYATTRIBUTE {
        lsStyle: match attr.underline {
            UnderlineKind::Solid => TF_LS_SOLID,
            UnderlineKind::Dot => TF_LS_DOT,
            UnderlineKind::Dash => TF_LS_DASH,
        },
        fBoldLine: attr.bold.into(),
        bAttr: if attr.bold {
            TF_ATTR_TARGET_CONVERTED
        } else {
            TF_ATTR_INPUT
        },
        // 色は既定（TF_CT_NONE）= ホストの色に従う
        ..Default::default()
    }
}

#[implement(ITfDisplayAttributeInfo)]
struct AttrInfo {
    index: usize,
}

impl ITfDisplayAttributeInfo_Impl for AttrInfo_Impl {
    fn GetGUID(&self) -> Result<GUID> {
        Ok(ATTR_GUIDS[self.index])
    }

    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from(DESCRIPTIONS[self.index]))
    }

    fn GetAttributeInfo(&self, pda: *mut TF_DISPLAYATTRIBUTE) -> Result<()> {
        if pda.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *pda = tf_attribute(DISPLAY_ATTRS[self.index]) };
        Ok(())
    }

    fn SetAttributeInfo(&self, _pda: *const TF_DISPLAYATTRIBUTE) -> Result<()> {
        // 利用者による変更は受けない（固定の見た目）
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        Ok(())
    }
}

/// `guid` に対応する属性情報
pub fn info_for(guid: &GUID) -> Result<ITfDisplayAttributeInfo> {
    match ATTR_GUIDS.iter().position(|g| g == guid) {
        Some(index) => Ok(AttrInfo { index }.into()),
        None => Err(E_INVALIDARG.into()),
    }
}

#[implement(IEnumTfDisplayAttributeInfo)]
struct AttrEnum {
    position: Cell<usize>,
}

impl IEnumTfDisplayAttributeInfo_Impl for AttrEnum_Impl {
    fn Clone(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(AttrEnum {
            position: Cell::new(self.position.get()),
        }
        .into())
    }

    fn Next(
        &self,
        ulcount: u32,
        rginfo: *mut Option<ITfDisplayAttributeInfo>,
        pcfetched: *mut u32,
    ) -> Result<()> {
        if rginfo.is_null() {
            return Err(E_POINTER.into());
        }
        let mut fetched = 0u32;
        while fetched < ulcount && self.position.get() < ATTR_GUIDS.len() {
            let info: ITfDisplayAttributeInfo = AttrInfo {
                index: self.position.get(),
            }
            .into();
            // 呼び出し側の未初期化メモリなので、ドロップせずに書き込む
            unsafe { rginfo.add(fetched as usize).write(Some(info)) };
            self.position.set(self.position.get() + 1);
            fetched += 1;
        }
        if !pcfetched.is_null() {
            unsafe { *pcfetched = fetched };
        }
        if fetched == ulcount {
            Ok(())
        } else {
            // 列挙の終端: S_FALSE（成功コードだが、要求した数に満たない）
            Err(S_FALSE.into())
        }
    }

    fn Reset(&self) -> Result<()> {
        self.position.set(0);
        Ok(())
    }

    fn Skip(&self, ulcount: u32) -> Result<()> {
        let next = self.position.get().saturating_add(ulcount as usize);
        if next > ATTR_GUIDS.len() {
            self.position.set(ATTR_GUIDS.len());
            return Err(S_FALSE.into());
        }
        self.position.set(next);
        Ok(())
    }
}

/// すべての属性の列挙子
pub fn enumerate() -> IEnumTfDisplayAttributeInfo {
    AttrEnum {
        position: Cell::new(0),
    }
    .into()
}

/// 属性の GUID を TSF に登録して識別子を得る（`Activate` で 1 回）
pub fn register_atoms() -> Result<AttrAtoms> {
    let categories: ITfCategoryMgr =
        unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }?;
    let mut atoms: AttrAtoms = [0; DISPLAY_ATTRS.len()];
    for (atom, guid) in atoms.iter_mut().zip(&ATTR_GUIDS) {
        *atom = unsafe { categories.RegisterGUID(guid) }?;
    }
    Ok(atoms)
}

/// `range`（composition の範囲。全文を覆う）に、描画計画の属性を書く。
/// 事前に全体の属性を消してから、区間ごとに設定する
pub fn apply(
    context: &ITfContext,
    ec: u32,
    range: &ITfRange,
    runs: &[AttrRun],
    atoms: &AttrAtoms,
) -> Result<()> {
    let property = unsafe { context.GetProperty(&GUID_PROP_ATTRIBUTE) }?;
    unsafe { property.Clear(ec, range) }?;
    let total = runs.last().map_or(0, |r| r.end);
    for run in runs {
        let index = nagi_windows::preedit_style::attr_index(run.attr);
        let part = unsafe { range.Clone() }?;
        let (start_shift, end_shift) = range_shifts(total, run.start, run.end);
        let mut moved = 0i32;
        unsafe { part.ShiftStart(ec, start_shift, &mut moved, std::ptr::null()) }?;
        unsafe { part.ShiftEnd(ec, end_shift, &mut moved, std::ptr::null()) }?;
        let value = VARIANT::from(atoms[index] as i32);
        unsafe { property.SetValue(ec, &part, &value) }?;
    }
    Ok(())
}

/// `range` の表示属性を消す（composition を終えてテキストを残すとき。消さないと確定後も下線が残る）
pub fn clear(context: &ITfContext, ec: u32, range: &ITfRange) -> Result<()> {
    let property = unsafe { context.GetProperty(&GUID_PROP_ATTRIBUTE) }?;
    unsafe { property.Clear(ec, range) }
}
