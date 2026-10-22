//! 再変換の OS 側の入口: `ITfFunctionProvider` と `ITfFnReconversion`。
//! アプリ（Office 等）や OS の再変換操作が、選択範囲を指定して呼ぶ。実際の変換はコアの `reconvert`
//! （`Core::reconvert_range`）。コアの要求（`reconvert_selection`）による再変換は `Core` が直接行う。

#![allow(unsafe_code)]

use std::rc::Weak;

use windows::Win32::Foundation::{E_NOINTERFACE, E_NOTIMPL, E_POINTER};
use windows::Win32::UI::TextServices::{
    ITfCandidateList, ITfFnReconversion, ITfFnReconversion_Impl, ITfFunction, ITfFunction_Impl,
    ITfFunctionProvider, ITfFunctionProvider_Impl, ITfRange,
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

    fn GetReconversion(&self, _prange: Ref<ITfRange>) -> Result<ITfCandidateList> {
        // 候補一覧の提供は未対応（Nagi の候補はコアが持つ。再変換は `Reconvert` で composition にする）
        Err(E_NOTIMPL.into())
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
