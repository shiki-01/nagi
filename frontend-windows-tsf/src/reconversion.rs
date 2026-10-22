//! 再変換の OS 側の入口: `ITfFunctionProvider` と `ITfFnReconversion`。
//! アプリ（Office 等）や OS の再変換操作が、選択範囲を指定して呼ぶ。実際の変換はコアの `reconvert`。
//! - `Reconvert`: 範囲を composition にして変換中にする（`Core::reconvert_range`）
//! - `GetReconversion`: 範囲全体を置き換える候補の一覧（`ITfCandidateList`）を返す。アプリが候補を
//!   選ぶと `SetResult` で範囲を置き換える。候補の組み立ては `nagi_windows::reconvert::range_candidates`
//!
//! コアの要求（`reconvert_selection`）による再変換は `Core` が直接行う。

#![allow(unsafe_code)]

use std::cell::Cell;
use std::rc::{Rc, Weak};

use windows::Win32::Foundation::{
    E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER, S_FALSE,
};
use windows::Win32::UI::TextServices::{
    CAND_FINALIZED, IEnumTfCandidates, IEnumTfCandidates_Impl, ITfCandidateList,
    ITfCandidateList_Impl, ITfCandidateString, ITfCandidateString_Impl, ITfFnReconversion,
    ITfFnReconversion_Impl, ITfFunction, ITfFunction_Impl, ITfFunctionProvider,
    ITfFunctionProvider_Impl, ITfRange, TfCandidateResult,
};
use windows::core::{BOOL, BSTR, GUID, IUnknown, Interface, OutRef, Ref, Result, implement};

use crate::guids::{CLSID_NAGI_TEXT_SERVICE, PROFILE_DESCRIPTION};
use crate::text_service::Core;

#[implement(ITfFnReconversion)]
struct Reconversion {
    core: Weak<Core>,
}

impl ITfFunction_Impl for Reconversion_Impl {
    fn GetDisplayName(&self) -> Result<BSTR> {
        Ok(BSTR::from("Nagi 再変換"))
    }
}

impl ITfFnReconversion_Impl for Reconversion_Impl {
    fn QueryRange(
        &self,
        prange: Ref<ITfRange>,
        ppnewrange: OutRef<ITfRange>,
        pfconvertable: *mut BOOL,
    ) -> Result<()> {
        if pfconvertable.is_null() {
            return Err(E_POINTER.into());
        }
        // 範囲の中身は編集セッション（ec）が無いと読めないので、ここでは渡された範囲をそのまま対象とする。
        // 空・長すぎる場合は `Reconvert` で何もしない
        let range = prange.ok()?;
        let clone = unsafe { range.Clone() }?;
        ppnewrange.write(Some(clone))?;
        unsafe { *pfconvertable = BOOL(1) };
        Ok(())
    }

    fn GetReconversion(&self, prange: Ref<ITfRange>) -> Result<ITfCandidateList> {
        // 範囲の文字列を読み、コアの再変換で「範囲全体を置き換える候補」を作って返す。
        // 候補が選ばれたら `SetResult` で範囲を置き換える（composition にはしない）
        let range = prange.ok()?.clone();
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| windows::core::Error::from(E_NOTIMPL))?;
        let items = core.reconversion_candidates(&range)?;
        Ok(CandidateList {
            range,
            items: Rc::new(items),
            core: self.core.clone(),
        }
        .into())
    }

    fn Reconvert(&self, prange: Ref<ITfRange>) -> Result<()> {
        let range = prange.ok()?.clone();
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| windows::core::Error::from(E_NOTIMPL))?;
        core.reconvert_range(range)
    }
}

/// `GetReconversion` の候補一覧
#[implement(ITfCandidateList)]
struct CandidateList {
    range: ITfRange,
    items: Rc<Vec<String>>,
    core: Weak<Core>,
}

impl ITfCandidateList_Impl for CandidateList_Impl {
    fn EnumCandidates(&self) -> Result<IEnumTfCandidates> {
        Ok(CandidateEnum {
            items: Rc::clone(&self.items),
            position: Cell::new(0),
        }
        .into())
    }

    fn GetCandidate(&self, nindex: u32) -> Result<ITfCandidateString> {
        let text = self
            .items
            .get(nindex as usize)
            .ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?;
        Ok(CandidateText {
            text: text.clone(),
            index: nindex,
        }
        .into())
    }

    fn GetCandidateNum(&self) -> Result<u32> {
        Ok(self.items.len() as u32)
    }

    fn SetResult(&self, nindex: u32, imcr: TfCandidateResult) -> Result<()> {
        // 選択中（CAND_SELECTED）や取り消しでは何もしない。確定のときだけ範囲を置き換える
        if imcr != CAND_FINALIZED {
            return Ok(());
        }
        let text = self
            .items
            .get(nindex as usize)
            .ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?;
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        core.replace_range(self.range.clone(), text.clone())
    }
}

#[implement(ITfCandidateString)]
struct CandidateText {
    text: String,
    index: u32,
}

impl ITfCandidateString_Impl for CandidateText_Impl {
    fn GetString(&self) -> Result<BSTR> {
        Ok(BSTR::from(self.text.as_str()))
    }

    fn GetIndex(&self) -> Result<u32> {
        Ok(self.index)
    }
}

#[implement(IEnumTfCandidates)]
struct CandidateEnum {
    items: Rc<Vec<String>>,
    position: Cell<usize>,
}

impl IEnumTfCandidates_Impl for CandidateEnum_Impl {
    fn Clone(&self) -> Result<IEnumTfCandidates> {
        Ok(CandidateEnum {
            items: Rc::clone(&self.items),
            position: Cell::new(self.position.get()),
        }
        .into())
    }

    fn Next(
        &self,
        ulcount: u32,
        ppcand: *mut Option<ITfCandidateString>,
        pcfetched: *mut u32,
    ) -> Result<()> {
        if ppcand.is_null() {
            return Err(E_POINTER.into());
        }
        let mut fetched = 0u32;
        while fetched < ulcount && self.position.get() < self.items.len() {
            let index = self.position.get();
            let candidate: ITfCandidateString = CandidateText {
                text: self.items[index].clone(),
                index: index as u32,
            }
            .into();
            // 呼び出し側の未初期化メモリなので、ドロップせずに書き込む
            unsafe { ppcand.add(fetched as usize).write(Some(candidate)) };
            self.position.set(index + 1);
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
        if next > self.items.len() {
            self.position.set(self.items.len());
            return Err(S_FALSE.into());
        }
        self.position.set(next);
        Ok(())
    }
}

#[implement(ITfFunctionProvider)]
pub(crate) struct FunctionProvider {
    core: Weak<Core>,
}

impl FunctionProvider {
    /// 新しいプロバイダ（`Core` は弱い参照で持つ。TSF が保持するため循環させない）
    pub(crate) fn create(core: Weak<Core>) -> ITfFunctionProvider {
        Self { core }.into()
    }
}

impl ITfFunctionProvider_Impl for FunctionProvider_Impl {
    fn GetType(&self) -> Result<GUID> {
        Ok(CLSID_NAGI_TEXT_SERVICE)
    }

    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from(PROFILE_DESCRIPTION))
    }

    fn GetFunction(&self, _rguid: *const GUID, riid: *const GUID) -> Result<IUnknown> {
        if riid.is_null() {
            return Err(E_POINTER.into());
        }
        let wanted = unsafe { *riid };
        if wanted == ITfFnReconversion::IID || wanted == ITfFunction::IID {
            let function: ITfFnReconversion = Reconversion {
                core: self.core.clone(),
            }
            .into();
            return function.cast();
        }
        Err(E_NOINTERFACE.into())
    }
}
