//! TSF の入力方式本体: `ITfTextInputProcessorEx`、`ITfKeyEventSink`、`ITfCompositionSink`。
//!
//! 役割は変換と描画のみ。キーは `nagi_windows::keynorm` で正規化してエンジンへ送り、
//! 応答の確定・preedit を編集セッションで反映する。何を食うか・何を確定するかはエンジンが決める。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use nagi_protocol::{KeyEvent, ProcessKeyResult};
use nagi_windows::keynorm;
use nagi_windows::supervisor::EngineEvent;
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, LPARAM, WPARAM};
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfDocumentMgr,
    ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr, ITfTextInputProcessor_Impl,
    ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl, ITfThreadMgr,
};
use windows::core::{BOOL, GUID, Interface, Ref, Result, implement};

use crate::dll;
use crate::edit::{self, SharedComposition};
use crate::engine::{EngineHost, EngineLocation};
use crate::render::{self, EditPlan, HandoffCache};
use crate::stubs::{CandidateWindow, Reconversion};

/// プロセス内で共有するエンジン（ホストプロセスごとに 1 つ）。
/// TODO: 複数のホストプロセスでエンジンを共有する（名前付きパイプ + 常駐ブローカー）。
/// 現状は各ホストプロセスが imserver を起動する（メモリ・起動時間の観点で要再検討。decisions.md H-39）
static ENGINE: OnceLock<Arc<EngineHost>> = OnceLock::new();

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

fn engine_location() -> EngineLocation {
    // imserver は DLL と同じディレクトリ。辞書・モデルは %APPDATA%\Nagi\data
    let executable = dll::module_directory()
        .unwrap_or_default()
        .join("imserver.exe");
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    EngineLocation {
        executable,
        data_dir: appdata.join("Nagi").join("data"),
    }
}

fn shared_engine() -> Arc<EngineHost> {
    ENGINE
        .get_or_init(|| {
            let host = Arc::new(EngineHost::new(
                &engine_location(),
                env!("CARGO_PKG_VERSION"),
            ));
            host.start_in_background();
            host
        })
        .clone()
}

/// COM のエントリポイントでパニックを止める（パニックを FFI 境界から出すとホストアプリごと落ちる）
fn guarded<T>(fallback: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fallback())
}

/// 1 キー分の判断結果（`OnTestKeyDown` で得て `OnKeyDown` へ引き継ぐ）
#[derive(Debug)]
struct KeyOutcome {
    /// TSF に「食った」と返すか
    eaten: bool,
    /// 反映する編集（無ければ空の計画）
    plan: EditPlan,
}

/// UI スレッド上の共有状態（TSF のオブジェクトは単一スレッド・アパートメントで使う）
struct Core {
    thread_mgr: RefCell<Option<ITfThreadMgr>>,
    client_id: Cell<u32>,
    session_id: String,
    engine: Arc<EngineHost>,
    composition: SharedComposition,
    composition_sink: RefCell<Option<ITfCompositionSink>>,
    handoff: RefCell<HandoffCache<KeyOutcome>>,
    candidate_window: CandidateWindow,
    reconversion: Reconversion,
}

impl Core {
    fn new() -> Self {
        let n = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            thread_mgr: RefCell::new(None),
            client_id: Cell::new(0),
            session_id: format!("win-{}-{n}", std::process::id()),
            engine: shared_engine(),
            composition: Rc::new(RefCell::new(None)),
            composition_sink: RefCell::new(None),
            handoff: RefCell::new(HandoffCache::new()),
            candidate_window: CandidateWindow,
            reconversion: Reconversion,
        }
    }

    fn is_composing(&self) -> bool {
        self.composition.borrow().is_some()
    }

    /// キーをエンジンに送り、食うか・何を反映するかを決める。
    /// エンジンが答えられない（未接続・タイムアウト）ときは、composition 中は食い（入力中の文字列を
    /// 壊さない）、そうでなければ素通しする
    fn evaluate(&self, wparam: WPARAM, lparam: LPARAM, is_up: bool) -> KeyOutcome {
        let passthrough = KeyOutcome {
            eaten: false,
            plan: render::plan_from_result(&ProcessKeyResult::default()),
        };
        let raw = crate::keys::raw_key_input(wparam, lparam, is_up);
        let Some(key) = keynorm::normalize(&raw) else {
            return passthrough;
        };
        match self.engine.process_key(&self.session_id, &key) {
            Ok(result) => self.outcome_from(&key, &result),
            Err(_) => KeyOutcome {
                eaten: self.is_composing(),
                ..passthrough
            },
        }
    }

    fn outcome_from(&self, _key: &KeyEvent, result: &ProcessKeyResult) -> KeyOutcome {
        let plan = render::plan_from_result(result);
        // 反映すべき編集があるのに「食わない」と返すと、TSF が OnKeyDown を呼ばず編集が失われる
        let eaten = result.consumed || !plan.is_empty();
        // TODO: result.candidates → CandidateWindow、result.mode → 言語バー、
        //       result.reconvert_selection → Reconversion
        self.candidate_window.update(result.candidates.as_ref());
        if result.reconvert_selection == Some(true) {
            self.reconversion.reconvert_selection();
        }
        KeyOutcome { eaten, plan }
    }

    fn sink(&self) -> Option<ITfCompositionSink> {
        self.composition_sink.borrow().clone()
    }

    fn apply(&self, context: &ITfContext, plan: EditPlan) -> Result<()> {
        if plan.is_empty() {
            return Ok(());
        }
        let sink = self
            .sink()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        edit::run_plan(
            context,
            self.client_id.get(),
            &self.composition,
            &sink,
            plan,
        )
    }

    /// フォーカス中のコンテキスト
    fn focused_context(&self) -> Option<ITfContext> {
        let tm = self.thread_mgr.borrow().clone()?;
        let dm: ITfDocumentMgr = unsafe { tm.GetFocus() }.ok()?;
        unsafe { dm.GetTop() }.ok()
    }

    /// 非同期イベント（別スレッドで受けてキューに積んだもの）を反映する。UI スレッドで呼ぶ。
    /// TODO: いまは次のキー入力のときに呼ぶ。隠しウィンドウへの PostMessage で即時に呼ぶ
    fn drain_engine_events(&self, context: Option<&ITfContext>) {
        for event in self.engine.drain_events() {
            match event {
                EngineEvent::UpdatePreedit(p) if p.session_id == self.session_id => {
                    if let Some(ctx) = context {
                        let _ = self.apply(ctx, render::plan_from_preedit(&p.preedit));
                    }
                }
                EngineEvent::UpdateCandidates(c) if c.session_id == self.session_id => {
                    self.candidate_window.update(Some(&c.candidates));
                }
                EngineEvent::Restored { session_id, result } if session_id == self.session_id => {
                    if let Some(ctx) = context {
                        let _ = self.apply(ctx, render::plan_from_result(&result));
                    }
                }
                EngineEvent::RestoreFailed { session_id, .. } if session_id == self.session_id => {
                    // 表示中の文字列をそのまま確定する（設計書 §3.3「入力を失わない」）
                    if let (Some(ctx), Some(sink)) = (context, self.sink()) {
                        let _ = edit::run_end_keeping_text(
                            ctx,
                            self.client_id.get(),
                            &self.composition,
                            &sink,
                        );
                    }
                }
                // TODO: Notify（設定エラー・警告の表示）、Crashed / EnteredMinimalMode の通知
                _ => {}
            }
        }
    }

    fn key_down(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
        test_only: bool,
    ) -> bool {
        let id = render::key_id((wparam.0 & 0xFFFF) as u16, lparam.0);
        if test_only {
            let outcome = self.evaluate(wparam, lparam, false);
            let eaten = outcome.eaten;
            self.handoff.borrow_mut().store(id, outcome);
            return eaten;
        }
        self.drain_engine_events(context);
        let cached = self.handoff.borrow_mut().take(id);
        let outcome = cached.unwrap_or_else(|| self.evaluate(wparam, lparam, false));
        if let Some(ctx) = context
            && self.apply(ctx, outcome.plan).is_err()
        {
            // 編集に失敗したらエンジン側の状態と食い違う。次の入力で揃うようリセットする
            let _ = self.engine.reset(&self.session_id);
        }
        outcome.eaten
    }

    fn key_up(&self, wparam: WPARAM, lparam: LPARAM) -> bool {
        // 修飾キーの離上は flags_changed としてエンジンに届ける（macOS と同じ）。キー離上は食わない
        // TODO: 押下を食ったキーの離上も食うか（アプリへ離上だけが届く問題）を実機で確認する
        let raw = crate::keys::raw_key_input(wparam, lparam, true);
        if let Some(key) = keynorm::normalize(&raw) {
            let _ = self.engine.process_key(&self.session_id, &key);
        }
        false
    }

    fn set_focus(&self, foreground: bool) {
        // フォーカス取得・喪失をエンジンに伝える。喪失時の確定文字列は composition を閉じて反映する
        if foreground {
            let _ = self.engine.focus_in(&self.session_id);
            return;
        }
        self.handoff.borrow_mut().clear();
        if let Ok(result) = self.engine.focus_out(&self.session_id) {
            let plan = render::plan_from_result(&result);
            if let Some(ctx) = self.focused_context() {
                let _ = self.apply(&ctx, plan);
            }
        }
    }
}

fn ctx_ref<'a>(pic: &'a Ref<'_, ITfContext>) -> Option<&'a ITfContext> {
    pic.as_ref()
}

/// 入力方式本体（`ITfTextInputProcessorEx`）
#[implement(ITfTextInputProcessorEx)]
pub struct TextService {
    core: Rc<Core>,
}

impl TextService {
    /// 新しいインスタンス（`IClassFactory::CreateInstance` から呼ぶ）
    pub(crate) fn new() -> Self {
        dll::add_object();
        Self {
            core: Rc::new(Core::new()),
        }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        dll::release_object();
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                let core = &self.core;
                if let (Some(tm), id) = (core.thread_mgr.borrow().clone(), core.client_id.get())
                    && let Ok(km) = tm.cast::<ITfKeystrokeMgr>()
                {
                    let _ = unsafe { km.UnadviseKeyEventSink(id) };
                }
                // 表示中の文字列は確定してから閉じる
                core.set_focus(false);
                core.engine.forget_session(&core.session_id);
                *core.composition.borrow_mut() = None;
                *core.composition_sink.borrow_mut() = None;
                *core.thread_mgr.borrow_mut() = None;
                core.client_id.set(0);
                Ok(())
            },
        )
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, _dwflags: u32) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                let tm = ptim.ok()?.clone();
                let core = &self.core;
                core.client_id.set(tid);
                *core.thread_mgr.borrow_mut() = Some(tm.clone());

                let comp_sink: ITfCompositionSink = CompositionSink {
                    core: Rc::clone(core),
                }
                .into();
                *core.composition_sink.borrow_mut() = Some(comp_sink);

                let key_sink: ITfKeyEventSink = KeyEventSink {
                    core: Rc::clone(core),
                }
                .into();
                let km: ITfKeystrokeMgr = tm.cast()?;
                // fforeground = true: フォーカスが無くても先にキーを受ける（IME として標準）
                unsafe { km.AdviseKeyEventSink(tid, &key_sink, true) }?;
                Ok(())
            },
        )
    }
}

#[implement(ITfKeyEventSink)]
struct KeyEventSink {
    core: Rc<Core>,
}

impl ITfKeyEventSink_Impl for KeyEventSink_Impl {
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        guarded(
            || Err(E_FAIL.into()),
            || {
                self.core.set_focus(fforeground.as_bool());
                Ok(())
            },
        )
    }

    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(
            || Ok(BOOL(0)),
            || {
                Ok(self
                    .core
                    .key_down(ctx_ref(&pic), wparam, lparam, true)
                    .into())
            },
        )
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(
            || Ok(BOOL(0)),
            || {
                Ok(self
                    .core
                    .key_down(ctx_ref(&pic), wparam, lparam, false)
                    .into())
            },
        )
    }

    fn OnKeyUp(&self, _pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        guarded(
            || Ok(BOOL(0)),
            || Ok(self.core.key_up(wparam, lparam).into()),
        )
    }

    fn OnPreservedKey(&self, _pic: Ref<ITfContext>, rguid: *const GUID) -> Result<BOOL> {
        // TODO: 半角/全角などを ITfKeystrokeMgr::PreserveKey で登録する場合に使う。いまは使わない
        if rguid.is_null() {
            return Err(E_INVALIDARG.into());
        }
        Ok(BOOL(0))
    }
}

#[implement(ITfCompositionSink)]
struct CompositionSink {
    core: Rc<Core>,
}

impl ITfCompositionSink_Impl for CompositionSink_Impl {
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _pcomposition: Ref<ITfComposition>,
    ) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                // アプリ側が composition を終了した（フォーカス移動・クリック等）。エンジンの状態も揃える
                *self.core.composition.borrow_mut() = None;
                self.core.handoff.borrow_mut().clear();
                let _ = self.core.engine.reset(&self.core.session_id);
                Ok(())
            },
        )
    }
}
