//! `ITfEditSession` による文書の編集。`render::EditPlan` を実行するだけで、判断はしない。
//!
//! 編集セッションの中身は `FnOnce(ec)` のクロージャで渡す。キーイベントのコールバック内では同期
//! （`TF_ES_SYNC`）、非同期通知の反映など文書ロックの外では `TF_ES_ASYNCDONTCARE`（ロックが空いていれば
//! 即時、塞がっていれば後で実行）。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use nagi_protocol::ProcessKeyResult;
use nagi_windows::candidates::Rect;
use nagi_windows::preedit_style::AttrRun;
use nagi_windows::reconvert::{RECONVERT_READ_LIMIT, eligible_text};
use windows::Win32::Foundation::{E_FAIL, FALSE, RECT};
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
    ITfEditSession_Impl, ITfInsertAtSelection, ITfRange, TF_AE_NONE, TF_ANCHOR_END,
    TF_ANCHOR_START, TF_CONTEXT_EDIT_CONTEXT_FLAGS, TF_DEFAULT_SELECTION, TF_ES_ASYNCDONTCARE,
    TF_ES_READ, TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE,
};
use windows::core::{BOOL, Interface, Result, implement};

use crate::context_info::{self, Surroundings};
use crate::display_attr::{self, AttrAtoms};
use crate::render::{EditPlan, PreeditUpdate, plan_from_result, to_utf16};
use crate::win::to_rect;

/// 編集セッションをまたいで持つ composition（UI スレッドの内側でのみ触る）
pub type SharedComposition = Rc<RefCell<Option<ITfComposition>>>;

/// 編集に必要な共有状態
#[derive(Clone)]
pub struct Env {
    /// 現在の composition
    pub composition: SharedComposition,
    /// composition の終了通知先
    pub sink: ITfCompositionSink,
    /// 表示属性の GUID 識別子（登録に失敗したら `None`＝装飾なし）
    pub atoms: Option<AttrAtoms>,
    /// 直近に測った composition（なければキャレット）の矩形（スクリーン座標）。候補ウィンドウの基準
    pub caret: Rc<Cell<Option<Rect>>>,
}

/// 実行の仕方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// 同期（キーイベントのコールバック内）
    Sync,
    /// 文書ロックの空き次第（即時か後で）
    Deferred,
}

type Op = Box<dyn FnOnce(u32) -> Result<()>>;

#[implement(ITfEditSession)]
struct EditSession {
    op: RefCell<Option<Op>>,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        let op = self.op.borrow_mut().take();
        match op {
            // パニックを COM の境界（TSF の呼び出し元）から出さない
            Some(op) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(ec)))
                .unwrap_or_else(|_| Err(E_FAIL.into())),
            None => Ok(()),
        }
    }
}

/// 編集セッションを要求する。戻り値は同期なら `DoEditSession` の結果、`Deferred` が後で実行されるなら `Ok`
pub fn run(
    context: &ITfContext,
    client_id: u32,
    timing: Timing,
    write: bool,
    op: impl FnOnce(u32) -> Result<()> + 'static,
) -> Result<()> {
    let session: ITfEditSession = EditSession {
        op: RefCell::new(Some(Box::new(op))),
    }
    .into();
    let access = if write { TF_ES_READWRITE } else { TF_ES_READ };
    let flags = TF_CONTEXT_EDIT_CONTEXT_FLAGS(
        access.0
            | match timing {
                Timing::Sync => TF_ES_SYNC.0,
                Timing::Deferred => TF_ES_ASYNCDONTCARE.0,
            },
    );
    let hr = unsafe { context.RequestEditSession(client_id, &session, flags) }?;
    // 延期された場合は成功コード（TF_S_ASYNC）。失敗コードだけをエラーにする
    hr.ok()
}

/// 計画を反映する
pub fn run_plan(
    context: &ITfContext,
    client_id: u32,
    env: &Env,
    plan: EditPlan,
    timing: Timing,
) -> Result<()> {
    let (ctx, env) = (context.clone(), env.clone());
    run(context, client_id, timing, true, move |ec| {
        apply_plan(&ctx, ec, &env, &plan, None)
    })
}

/// composition をテキストを残したまま終了する（復元失敗時の「表示中の文字列の確定」）
pub fn run_end_keeping_text(
    context: &ITfContext,
    client_id: u32,
    env: &Env,
    timing: Timing,
) -> Result<()> {
    let (ctx, composition) = (context.clone(), Rc::clone(&env.composition));
    run(context, client_id, timing, true, move |ec| {
        let taken = composition.borrow_mut().take();
        if let Some(c) = taken {
            if let Ok(range) = unsafe { c.GetRange() } {
                let _ = display_attr::clear(&ctx, ec, &range);
            }
            unsafe { c.EndComposition(ec) }?;
        }
        Ok(())
    })
}

/// キャレット直前のテキストと入力欄の種別を読む（同期・読み取り）。失敗は `None`
pub fn read_surroundings(context: &ITfContext, client_id: u32) -> Option<Surroundings> {
    let out: Rc<RefCell<Option<Surroundings>>> = Rc::default();
    let slot = Rc::clone(&out);
    let ctx = context.clone();
    run(context, client_id, Timing::Sync, false, move |ec| {
        *slot.borrow_mut() = Some(context_info::read_surroundings(&ctx, ec));
        Ok(())
    })
    .ok()?;
    out.borrow_mut().take()
}

/// 確定済みテキストの再変換。`range` が `None` なら現在の選択を対象にする（空の選択なら何もしない）。
/// `convert` は対象のテキストを受けてエンジンへ `reconvert` を送り、応答（preedit）を返す。
/// 応答の preedit で対象の範囲を置き換え、composition にする
pub fn run_reconvert(
    context: &ITfContext,
    client_id: u32,
    env: &Env,
    range: Option<ITfRange>,
    timing: Timing,
    convert: impl FnOnce(&str) -> Option<ProcessKeyResult> + 'static,
    applied: impl FnOnce(&ProcessKeyResult) + 'static,
) -> Result<()> {
    let (ctx, env) = (context.clone(), env.clone());
    run(context, client_id, timing, true, move |ec| {
        // 入力中の文字列があるときは再変換しない（コアも変換中は別の状態）
        if env.composition.borrow().is_some() {
            return Ok(());
        }
        let target = match range {
            Some(r) => r,
            None => selection_range(&ctx, ec)?,
        };
        if unsafe { target.IsEmpty(ec) }?.as_bool() {
            return Ok(());
        }
        let mut buf = [0u16; RECONVERT_READ_LIMIT];
        let mut fetched = 0u32;
        unsafe { target.GetText(ec, 0, &mut buf, &mut fetched) }?;
        let text = String::from_utf16_lossy(&buf[..(fetched as usize).min(buf.len())]);
        let Some(text) = eligible_text(&text) else {
            return Ok(());
        };
        let Some(result) = convert(text) else {
            return Ok(());
        };
        apply_plan(&ctx, ec, &env, &plan_from_result(&result), Some(&target))?;
        applied(&result);
        Ok(())
    })
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
pub fn selection_range(context: &ITfContext, ec: u32) -> Result<ITfRange> {
    let mut selection = [TF_SELECTION::default()];
    let mut fetched = 0u32;
    unsafe { context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched) }?;
    let [first] = selection;
    // 所有権を取り戻して Drop させる
    std::mem::ManuallyDrop::into_inner(first.range)
        .ok_or_else(|| windows::core::Error::from(E_FAIL))
}

/// composition（なければ選択）の画面上の矩形を測って `env.caret` に入れる。失敗したら `None`
fn measure_caret(context: &ITfContext, ec: u32, env: &Env) {
    let measure = || -> Result<Rect> {
        let current = env.composition.borrow().clone();
        let range = match current {
            Some(c) => unsafe { c.GetRange() }?,
            None => selection_range(context, ec)?,
        };
        let view = unsafe { context.GetActiveView() }?;
        let mut rc = RECT::default();
        let mut clipped = BOOL::default();
        unsafe { view.GetTextExt(ec, &range, &mut rc, &mut clipped) }?;
        Ok(to_rect(rc))
    };
    env.caret.set(measure().ok());
}

/// 編集計画を反映する。順序は「直前の確定の削除 → 確定 → preedit の更新」。
/// `start_at` は composition を始める範囲（再変換で選択範囲を置き換えるときに指定する）
fn apply_plan(
    context: &ITfContext,
    ec: u32,
    env: &Env,
    plan: &EditPlan,
    start_at: Option<&ITfRange>,
) -> Result<()> {
    if let Some(n) = plan.delete_before {
        delete_before_caret(context, ec, n)?;
    }
    if let Some(text) = &plan.commit {
        commit(context, ec, &env.composition, text)?;
    }
    match &plan.preedit {
        PreeditUpdate::Keep => {}
        PreeditUpdate::Clear => clear_preedit(ec, &env.composition)?,
        PreeditUpdate::Set {
            text,
            cursor,
            attrs,
        } => set_preedit(context, ec, env, text, *cursor, attrs, start_at)?,
    }
    measure_caret(context, ec, env);
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
            // 確定したテキストに入力中の下線を残さない
            let _ = display_attr::clear(context, ec, &range);
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

/// preedit を composition の範囲に書き、表示属性とキャレット（`cursor`、UTF-16）を設定する。
/// composition が無ければ開始する
fn set_preedit(
    context: &ITfContext,
    ec: u32,
    env: &Env,
    text: &str,
    cursor: u32,
    attrs: &[AttrRun],
    start_at: Option<&ITfRange>,
) -> Result<()> {
    let existing = env.composition.borrow().clone();
    let comp = match existing {
        Some(c) => c,
        None => {
            let at = match start_at {
                Some(r) => r.clone(),
                None => {
                    let insert: ITfInsertAtSelection = context.cast()?;
                    // 空の範囲を選択位置に得る（文書は変更しない）
                    unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[]) }?
                }
            };
            let ctx_comp: ITfContextComposition = context.cast()?;
            let c = unsafe { ctx_comp.StartComposition(ec, &at, &env.sink) }?;
            *env.composition.borrow_mut() = Some(c.clone());
            c
        }
    };
    let range = unsafe { comp.GetRange() }?;
    unsafe { range.SetText(ec, 0, &to_utf16(text)) }?;

    // 表示属性は失敗しても入力は続ける（装飾が無くなるだけ）
    if let Some(atoms) = &env.atoms {
        let _ = display_attr::apply(context, ec, &range, attrs, atoms);
    }

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
