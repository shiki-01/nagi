//! `ITfEditSession` による文書の編集。`render::EditPlan` を実行するだけで、判断はしない。

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::FALSE;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
    ITfEditSession_Impl, ITfInsertAtSelection, ITfRange, TF_AE_NONE, TF_ANCHOR_END,
    TF_ANCHOR_START, TF_DEFAULT_SELECTION, TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY,
    TF_SELECTION, TF_SELECTIONSTYLE,
};
use windows::core::{Interface, Result, implement};

use crate::render::{EditPlan, PreeditUpdate, to_utf16};

/// 編集セッションをまたいで持つ composition（UI スレッドの内側でのみ触る）
pub type SharedComposition = Rc<RefCell<Option<ITfComposition>>>;

/// 編集セッションの中身
enum Op {
    /// 応答の編集計画を反映する
    Plan(EditPlan),
    /// composition をテキストを残したまま終了する（復元失敗時の「表示中の文字列の確定」）
    EndKeepingText,
}

#[implement(ITfEditSession)]
struct EditSession {
    context: ITfContext,
    composition: SharedComposition,
    sink: ITfCompositionSink,
    op: Op,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        match &self.op {
            Op::Plan(plan) => apply_plan(&self.context, ec, &self.composition, &self.sink, plan),
            Op::EndKeepingText => end_keeping_text(ec, &self.composition),
        }
    }
}

/// 同期の読み書きセッションを要求する（キーイベントのコールバック内で許される）。
/// 戻り値の HRESULT は `DoEditSession` の結果
fn request(context: &ITfContext, client_id: u32, session: ITfEditSession) -> Result<()> {
    let hr =
        unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READWRITE) }?;
    hr.ok()
}

/// 計画を同期の編集セッションで反映する
pub fn run_plan(
    context: &ITfContext,
    client_id: u32,
    composition: &SharedComposition,
    sink: &ITfCompositionSink,
    plan: EditPlan,
) -> Result<()> {
    let session: ITfEditSession = EditSession {
        context: context.clone(),
        composition: Rc::clone(composition),
        sink: sink.clone(),
        op: Op::Plan(plan),
    }
    .into();
    request(context, client_id, session)
}

/// composition をテキストを残して終了する
pub fn run_end_keeping_text(
    context: &ITfContext,
    client_id: u32,
    composition: &SharedComposition,
    sink: &ITfCompositionSink,
) -> Result<()> {
    let session: ITfEditSession = EditSession {
        context: context.clone(),
        composition: Rc::clone(composition),
        sink: sink.clone(),
        op: Op::EndKeepingText,
    }
    .into();
    request(context, client_id, session)
}

fn end_keeping_text(ec: u32, composition: &SharedComposition) -> Result<()> {
    let taken = composition.borrow_mut().take();
    if let Some(c) = taken {
        unsafe { c.EndComposition(ec) }?;
    }
    Ok(())
}

/// キャレットを `range` の末尾に置く
fn set_caret_at_end(context: &ITfContext, ec: u32, range: &ITfRange) -> Result<()> {
    let caret = unsafe { range.Clone() }?;
    unsafe { caret.Collapse(ec, TF_ANCHOR_END) }?;
    set_selection(context, ec, caret)
}

fn set_selection(context: &ITfContext, ec: u32, range: ITfRange) -> Result<()> {
    let selection = TF_SELECTION {
        range: std::mem::ManuallyDrop::new(Some(range)),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: FALSE,
        },
    };
    unsafe { context.SetSelection(ec, &[selection]) }
}

/// 現在の選択範囲（なければキャレット位置）
fn selection_range(context: &ITfContext, ec: u32) -> Result<ITfRange> {
    let mut selection = [TF_SELECTION::default()];
    let mut fetched = 0u32;
    unsafe { context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched) }?;
    let [first] = selection;
    // 所有権を取り戻して Drop させる
    std::mem::ManuallyDrop::into_inner(first.range)
        .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

/// 編集計画を反映する。順序は「直前の確定の削除 → 確定 → preedit の更新」
fn apply_plan(
    context: &ITfContext,
    ec: u32,
    composition: &SharedComposition,
    sink: &ITfCompositionSink,
    plan: &EditPlan,
) -> Result<()> {
    if let Some(n) = plan.delete_before {
        delete_before_caret(context, ec, n)?;
    }
    if let Some(text) = &plan.commit {
        commit(context, ec, composition, text)?;
    }
    match &plan.preedit {
        PreeditUpdate::Keep => {}
        PreeditUpdate::Clear => clear_preedit(ec, composition)?,
        PreeditUpdate::Set {
            text,
            cursor,
            spans: _,
        } => {
            // TODO: spans を表示属性（ITfDisplayAttributeProvider + GUID_PROP_ATTRIBUTE）として設定する
            set_preedit(context, ec, composition, sink, text, *cursor)?;
        }
    }
    Ok(())
}

/// 確定文字列を入れる。composition があればその範囲を置き換えて終了し、無ければ選択位置に挿入する
fn commit(
    context: &ITfContext,
    ec: u32,
    composition: &SharedComposition,
    text: &str,
) -> Result<()> {
    let utf16 = to_utf16(text);
    let current = composition.borrow_mut().take();
    let range = match current {
        Some(c) => {
            let range = unsafe { c.GetRange() }?;
            unsafe { range.SetText(ec, 0, &utf16) }?;
            set_caret_at_end(context, ec, &range)?;
            unsafe { c.EndComposition(ec) }?;
            range
        }
        None => {
            let insert: ITfInsertAtSelection = context.cast()?;
            let range = unsafe { insert.InsertTextAtSelection(ec, Default::default(), &utf16) }?;
            set_caret_at_end(context, ec, &range)?;
            range
        }
    };
    drop(range);
    Ok(())
}

fn clear_preedit(ec: u32, composition: &SharedComposition) -> Result<()> {
    let taken = composition.borrow_mut().take();
    if let Some(c) = taken {
        let range = unsafe { c.GetRange() }?;
        unsafe { range.SetText(ec, 0, &[]) }?;
        unsafe { c.EndComposition(ec) }?;
    }
    Ok(())
}

/// preedit を composition の範囲に書き、キャレットを `cursor`（UTF-16）に置く。composition が無ければ開始する
fn set_preedit(
    context: &ITfContext,
    ec: u32,
    composition: &SharedComposition,
    sink: &ITfCompositionSink,
    text: &str,
    cursor: u32,
) -> Result<()> {
    let existing = composition.borrow().clone();
    let comp = match existing {
        Some(c) => c,
        None => {
            let insert: ITfInsertAtSelection = context.cast()?;
            // 空の範囲を選択位置に得る（文書は変更しない）
            let at = unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[]) }?;
            let ctx_comp: ITfContextComposition = context.cast()?;
            let c = unsafe { ctx_comp.StartComposition(ec, &at, sink) }?;
            *composition.borrow_mut() = Some(c.clone());
            c
        }
    };
    let range = unsafe { comp.GetRange() }?;
    unsafe { range.SetText(ec, 0, &to_utf16(text)) }?;

    // キャレット: 範囲の先頭に潰してから cursor 単位だけ終端を進め、終端に潰す
    let caret = unsafe { range.Clone() }?;
    unsafe { caret.Collapse(ec, TF_ANCHOR_START) }?;
    let mut moved = 0i32;
    let shift = i32::try_from(cursor).unwrap_or(i32::MAX);
    unsafe { caret.ShiftEnd(ec, shift, &mut moved, std::ptr::null()) }?;
    unsafe { caret.Collapse(ec, TF_ANCHOR_END) }?;
    set_selection(context, ec, caret)
}

/// キャレット直前の `n`（UTF-16）を削除する（確定取消）。composition 中は何もしない
fn delete_before_caret(context: &ITfContext, ec: u32, n: u32) -> Result<()> {
    let range = selection_range(context, ec)?;
    let target = unsafe { range.Clone() }?;
    unsafe { target.Collapse(ec, TF_ANCHOR_START) }?;
    let mut moved = 0i32;
    let back = -i32::try_from(n).unwrap_or(i32::MAX);
    unsafe { target.ShiftStart(ec, back, &mut moved, std::ptr::null()) }?;
    unsafe { target.SetText(ec, 0, &[]) }
}
