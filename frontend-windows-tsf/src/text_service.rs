//! TSF の入力方式本体: `ITfTextInputProcessorEx`、`ITfKeyEventSink`、`ITfCompositionSink`、
//! `ITfDisplayAttributeProvider`、`ITfCompartmentEventSink`。
//!
//! 役割は変換と描画のみ。キーは `nagi_windows::keynorm` で正規化してエンジンへ送り、
//! 応答の確定・preedit を編集セッションで反映する。何を食うか・何を確定するかはエンジンが決める。
//! インスタンスは `Activate` までエンジンに触れない（表示属性の提供など、アクティブ化されない
//! プロセスでも COM オブジェクトだけが作られるため）。

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use nagi_protocol::{CandidatesView, InputMode, KeyKind, ProcessKeyResult};
use nagi_windows::candidates::{CandidatePage, Rect};
use nagi_windows::context::{build_context, should_report_before_key};
use nagi_windows::environment::{EnvironmentReporter, ResumeDetector};
use nagi_windows::keynorm;
use nagi_windows::mode::{ModeSync, compartment_values};
use nagi_windows::status::{ToastThrottle, toast_for};
use nagi_windows::supervisor::EngineEvent;
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER, LPARAM, WPARAM};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, ITfCompartmentEventSink, ITfCompartmentEventSink_Impl,
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext,
    ITfDisplayAttributeInfo, ITfDisplayAttributeProvider, ITfDisplayAttributeProvider_Impl,
    ITfDocumentMgr, ITfFunctionProvider, ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr,
    ITfRange, ITfSourceSingle, ITfTextInputProcessor_Impl, ITfTextInputProcessorEx,
    ITfTextInputProcessorEx_Impl, ITfThreadMgr,
};
use windows::core::{BOOL, GUID, Interface, Ref, Result, implement};

use crate::candidate_window::CandidateWindow;
use crate::display_attr::{self, AttrAtoms};
use crate::dll;
use crate::edit::{self, Env, SharedComposition, Timing};
use crate::engine::{EngineHost, EngineLocation, RECONVERT_TIMEOUT};
use crate::message_window::{MessageEvent, MessageWindow};
use crate::mode_comp::{self, ModeCompartments};
use crate::probe::Win32Probe;
use crate::reconversion::FunctionProvider;
use crate::render::{self, EditPlan, HandoffCache};
use crate::toast_window::ToastWindow;

/// プロセス内で共有するエンジン（ホストプロセスごとに 1 つ）。
/// TODO: 複数のホストプロセスでエンジンを共有する（名前付きパイプ + 常駐ブローカー）。
/// 現状は各ホストプロセスが imserver を起動する（メモリ・起動時間の観点で要再検討。decisions.md H-39）
static ENGINE: OnceLock<Arc<EngineHost>> = OnceLock::new();

/// プロセス内で共有する環境の報告（変化したときだけ送る。エンジンは 1 つなので 1 つ）
static ENVIRONMENT: OnceLock<Mutex<EnvironmentReporter<Win32Probe>>> = OnceLock::new();

/// 通知の間引き（同じ文言を 30 秒は繰り返さない）。プロセス内で 1 つ
static TOASTS: OnceLock<Mutex<ToastThrottle>> = OnceLock::new();

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

/// 再変換に時間がかかったときの通知
const RECONVERT_SLOW: &str =
    "再変換の準備に時間がかかっています。少し待ってからもう一度お試しください";

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

fn environment() -> &'static Mutex<EnvironmentReporter<Win32Probe>> {
    ENVIRONMENT.get_or_init(|| Mutex::new(EnvironmentReporter::new(Win32Probe)))
}

fn toasts() -> &'static Mutex<ToastThrottle> {
    TOASTS.get_or_init(|| Mutex::new(ToastThrottle::default()))
}

/// COM のエントリポイントでパニックを止める（パニックを FFI 境界から出すとホストアプリごと落ちる）
fn guarded<T>(fallback: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fallback())
}

/// 応答のうち、編集の反映後に UI へ伝える部分（候補・モード・再変換の要求）
#[derive(Debug, Default)]
struct Followup {
    candidates: Option<CandidatesView>,
    mode: Option<InputMode>,
    reconvert: bool,
}

impl Followup {
    fn from_result(result: &ProcessKeyResult) -> Self {
        Self {
            candidates: result.candidates.clone(),
            mode: result.mode,
            reconvert: result.reconvert_selection == Some(true),
        }
    }
}

/// 1 キー分の判断結果（`OnTestKeyDown` で得て `OnKeyDown` へ引き継ぐ）
#[derive(Debug)]
struct KeyOutcome {
    /// TSF に「食った」と返すか
    eaten: bool,
    /// 反映する編集（無ければ空の計画）
    plan: EditPlan,
    /// 反映後の UI 更新
    followup: Followup,
}

/// UI スレッド上の共有状態（TSF のオブジェクトは単一スレッド・アパートメントで使う）
pub(crate) struct Core {
    me: Weak<Core>,
    thread_mgr: RefCell<Option<ITfThreadMgr>>,
    client_id: Cell<u32>,
    session_id: String,
    engine: Arc<EngineHost>,
    composition: SharedComposition,
    composition_sink: RefCell<Option<ITfCompositionSink>>,
    atoms: Cell<Option<AttrAtoms>>,
    /// 直近に測ったキャレット（composition）の矩形。候補ウィンドウの基準
    caret: Rc<Cell<Option<Rect>>>,
    handoff: RefCell<HandoffCache<KeyOutcome>>,
    candidate_window: CandidateWindow,
    toast: ToastWindow,
    /// このセッションが前面でフォーカスを持っているか（通知はここだけ出す）
    focused: Cell<bool>,
    message_window: RefCell<Option<MessageWindow>>,
    mode_sync: RefCell<ModeSync>,
    compartments: RefCell<Option<ModeCompartments>>,
    /// 自分でコンパートメントを書いている間（2 つの値を順に書く途中の通知を OS 側の操作と取り違えない）
    writing_mode: Cell<bool>,
    resume: RefCell<ResumeDetector>,
}

impl Core {
    fn new(me: Weak<Core>) -> Self {
        let n = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let interval =
            std::time::Duration::from_millis(u64::from(crate::message_window::TICK_INTERVAL_MS));
        Self {
            me,
            thread_mgr: RefCell::new(None),
            client_id: Cell::new(0),
            session_id: format!("win-{}-{n}", std::process::id()),
            engine: shared_engine(),
            composition: Rc::new(RefCell::new(None)),
            composition_sink: RefCell::new(None),
            atoms: Cell::new(None),
            caret: Rc::new(Cell::new(None)),
            handoff: RefCell::new(HandoffCache::new()),
            candidate_window: CandidateWindow::new(),
            toast: ToastWindow::new(),
            focused: Cell::new(false),
            message_window: RefCell::new(None),
            mode_sync: RefCell::new(ModeSync::new()),
            compartments: RefCell::new(None),
            writing_mode: Cell::new(false),
            resume: RefCell::new(ResumeDetector::new(
                interval,
                std::time::Duration::from_secs(60),
            )),
        }
    }

    fn is_composing(&self) -> bool {
        self.composition.borrow().is_some()
    }

    fn env(&self) -> Option<Env> {
        Some(Env {
            composition: Rc::clone(&self.composition),
            sink: self.composition_sink.borrow().clone()?,
            atoms: self.atoms.get(),
            caret: Rc::clone(&self.caret),
        })
    }

    // ---- 起動・終了 ----

    fn activate(self: &Rc<Self>, thread_mgr: ITfThreadMgr, client_id: u32) -> Result<()> {
        self.client_id.set(client_id);
        *self.thread_mgr.borrow_mut() = Some(thread_mgr.clone());

        let sink: ITfCompositionSink = CompositionSink {
            core: Rc::clone(self),
        }
        .into();
        *self.composition_sink.borrow_mut() = Some(sink);
        // 表示属性の登録に失敗しても入力はできる（下線の装飾が無くなるだけ）
        self.atoms.set(display_attr::register_atoms().ok());

        // 隠しウィンドウ: 別スレッドのエンジン通知・タイマー・電源通知を UI スレッドで受ける
        let weak = Rc::downgrade(self);
        let window = MessageWindow::create(Box::new(move |event| {
            if let Some(core) = weak.upgrade() {
                core.on_message(event);
            }
        }));
        if let Some(window) = &window {
            self.engine.subscribe(&self.session_id, window.waker());
        }
        *self.message_window.borrow_mut() = window;

        let key_sink: ITfKeyEventSink = KeyEventSink {
            core: Rc::clone(self),
        }
        .into();
        let keystroke: ITfKeystrokeMgr = thread_mgr.cast()?;
        // fforeground = true: フォーカスが無くても先にキーを受ける（IME として標準）
        unsafe { keystroke.AdviseKeyEventSink(client_id, &key_sink, true) }?;

        // モード（コンパートメント）。失敗しても入力はできる（OS 側の表示とは同期しない）
        let compartment_sink: ITfCompartmentEventSink = CompartmentSink {
            core: Rc::downgrade(self),
        }
        .into();
        *self.compartments.borrow_mut() =
            ModeCompartments::new(&thread_mgr, client_id, &compartment_sink).ok();
        self.sync_mode_with_os();

        // 再変換（ITfFnReconversion）の入口
        if let Ok(single) = thread_mgr.cast::<ITfSourceSingle>() {
            let provider = FunctionProvider::create(Rc::downgrade(self));
            let _ =
                unsafe { single.AdviseSingleSink(client_id, &ITfFunctionProvider::IID, &provider) };
        }

        self.poll_environment();
        Ok(())
    }

    fn deactivate(&self) {
        let thread_mgr = self.thread_mgr.borrow().clone();
        let id = self.client_id.get();
        if let Some(tm) = &thread_mgr {
            if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                let _ = unsafe { km.UnadviseKeyEventSink(id) };
            }
            if let Ok(single) = tm.cast::<ITfSourceSingle>() {
                let _ = unsafe { single.UnadviseSingleSink(id, &ITfFunctionProvider::IID) };
            }
        }
        // 表示中の文字列は確定してから閉じる
        self.set_focus(false);
        self.engine.forget_session(&self.session_id);
        self.candidate_window.destroy();
        self.toast.destroy();
        *self.message_window.borrow_mut() = None;
        *self.compartments.borrow_mut() = None;
        *self.composition.borrow_mut() = None;
        *self.composition_sink.borrow_mut() = None;
        *self.thread_mgr.borrow_mut() = None;
        self.client_id.set(0);
    }

    // ---- 隠しウィンドウのイベント ----

    fn on_message(&self, event: MessageEvent) {
        match event {
            MessageEvent::EngineEvents => {
                self.pump_events(self.focused_context().as_ref(), Timing::Deferred)
            }
            MessageEvent::Tick => {
                self.poll_environment();
                if self.resume.borrow_mut().tick(SystemTime::now()) {
                    self.engine.restart_in_background();
                }
            }
            MessageEvent::Resume => {
                if self.resume.borrow_mut().notified(SystemTime::now()) {
                    self.engine.restart_in_background();
                }
                self.poll_environment();
            }
            MessageEvent::PowerChanged => self.poll_environment(),
        }
    }

    // ---- 環境 ----

    fn poll_environment(&self) {
        let params = {
            let mut reporter = environment().lock().unwrap_or_else(|e| e.into_inner());
            reporter.poll()
        };
        if let Some(params) = params {
            let _ = self.engine.set_environment(&params);
        }
    }

    /// エンジンが（再）起動した。新しいエンジンは前回値もモードも知らない
    fn on_engine_started(&self) {
        environment()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reset();
        self.poll_environment();
        self.sync_mode_with_os();
    }

    // ---- 入力モード ----

    /// 新しいセッション（エンジン）はひらがなで始まる。OS 側の値と合わせる:
    /// OS が未設定ならひらがなを書き、設定済みで違えばコアへ `SetMode` を送る
    fn sync_mode_with_os(&self) {
        self.mode_sync.borrow_mut().engine_restarted();
        let current = self
            .compartments
            .borrow()
            .as_ref()
            .map(ModeCompartments::read);
        match current {
            None => {}
            Some(None) => self.write_mode_values(compartment_values(InputMode::Hiragana)),
            Some(Some(v)) => self.request_mode_from_os(v.open, v.conversion),
        }
    }

    /// モードの値をコンパートメントへ書く。書き込み中の変更通知は無視する
    fn write_mode_values(&self, values: nagi_windows::mode::CompartmentValues) {
        self.writing_mode.set(true);
        if let Some(c) = self.compartments.borrow().as_ref() {
            let _ = c.write(values);
        }
        self.writing_mode.set(false);
    }

    /// OS のモードがコアと違うなら、コアへ送って結果を反映する
    fn request_mode_from_os(&self, open: bool, conversion: u32) {
        let requested = self.mode_sync.borrow_mut().os_changed(open, conversion);
        let Some(mode) = requested else {
            return;
        };
        if let Ok(result) = self.engine.set_mode(&self.session_id, mode) {
            // SetMode は入力中の文字列を確定する
            if let Some(ctx) = self.focused_context() {
                let _ = self.apply(&ctx, render::plan_from_result(&result), Timing::Deferred);
            }
            self.finish(&Followup::from_result(&result));
        }
    }

    /// コンパートメントが変わった（トレイのクリック・他の IME との往復・自分の書き込み）
    fn on_compartment_changed(&self) {
        if self.writing_mode.get() {
            return;
        }
        let values = self
            .compartments
            .borrow()
            .as_ref()
            .and_then(ModeCompartments::read);
        if let Some(v) = values {
            self.request_mode_from_os(v.open, v.conversion);
        }
    }

    /// コアでモードが変わった。OS へ書く（同じなら書かない）
    fn apply_core_mode(&self, mode: InputMode) {
        let values = self.mode_sync.borrow_mut().core_changed(mode);
        if let Some(values) = values {
            self.write_mode_values(values);
        }
    }

    // ---- 候補・通知 ----

    fn show_candidates(&self, view: Option<&CandidatesView>) {
        match view.and_then(CandidatePage::from_view) {
            Some(page) => self.candidate_window.show(&page, self.caret.get()),
            None => self.candidate_window.hide(),
        }
    }

    fn show_toast(&self, message: &str) {
        if !self.focused.get() {
            return;
        }
        let allowed = toasts()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .should_show(message, Instant::now());
        if allowed {
            self.toast.show(message);
        }
    }

    /// 編集を反映した後の UI 更新（モード・候補・再変換の要求）
    fn finish(&self, followup: &Followup) {
        if let Some(mode) = followup.mode {
            self.apply_core_mode(mode);
        }
        if let Some(view) = &followup.candidates {
            self.show_candidates(Some(view));
        }
        // 入力中でなくなったら候補は出さない
        if !self.is_composing() {
            self.candidate_window.hide();
        }
    }

    // ---- 再変換 ----

    /// エンジンに再変換を頼む。待ちきれなければ状態を戻して通知する
    fn reconvert_text(&self, text: &str) -> Option<ProcessKeyResult> {
        match self
            .engine
            .reconvert(&self.session_id, text, RECONVERT_TIMEOUT)
        {
            Ok(result) => Some(result),
            Err(_) => {
                // コアは遅れて変換中になりうるので取り消す（要求は順に処理される）
                let _ = self.engine.reset(&self.session_id);
                self.show_toast(RECONVERT_SLOW);
                None
            }
        }
    }

    /// 範囲を再変換する（`range` が `None` なら現在の選択）
    fn reconvert(
        &self,
        context: &ITfContext,
        range: Option<ITfRange>,
        timing: Timing,
    ) -> Result<()> {
        let env = self
            .env()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let (convert, applied) = (self.me.clone(), self.me.clone());
        edit::run_reconvert(
            context,
            self.client_id.get(),
            &env,
            range,
            timing,
            move |text| convert.upgrade()?.reconvert_text(text),
            move |result| {
                if let Some(core) = applied.upgrade() {
                    core.finish(&Followup::from_result(result));
                }
            },
        )
    }

    /// アプリや OS からの再変換（`ITfFnReconversion::Reconvert`）
    pub(crate) fn reconvert_range(&self, range: ITfRange) -> Result<()> {
        let context = self
            .focused_context()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        self.reconvert(&context, Some(range), Timing::Deferred)
    }

    // ---- キー ----

    /// キー処理の前に文脈（キャレット直前のテキスト・入力欄の種別・アプリ ID）をコアへ送る
    fn report_context(&self, context: &ITfContext) {
        let Some(info) = edit::read_surroundings(context, self.client_id.get()) else {
            return;
        };
        let params = build_context(
            &self.session_id,
            info.secure,
            crate::context_info::app_id(),
            info.text,
        );
        let _ = self.engine.set_context(&params);
    }

    /// キーをエンジンに送り、食うか・何を反映するかを決める。
    /// エンジンが答えられない（未接続・タイムアウト）ときは、composition 中は食い（入力中の文字列を
    /// 壊さない）、そうでなければ素通しする
    fn evaluate(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
        is_up: bool,
    ) -> KeyOutcome {
        let passthrough = KeyOutcome {
            eaten: false,
            plan: render::plan_from_result(&ProcessKeyResult::default()),
            followup: Followup::default(),
        };
        let raw = crate::keys::raw_key_input(wparam, lparam, is_up);
        let Some(key) = keynorm::normalize(&raw) else {
            return passthrough;
        };
        // 入力の開始になりうるキー（文字を生むキー）だけ、文脈を送る。毎キーで文書を読まないため
        if let Some(ctx) = context
            && !key.text.is_empty()
            && should_report_before_key(
                self.mode_sync
                    .borrow()
                    .known()
                    .unwrap_or(InputMode::Hiragana),
                !self.is_composing(),
                key.kind == KeyKind::FlagsChanged,
            )
        {
            self.report_context(ctx);
        }
        match self.engine.process_key(&self.session_id, &key) {
            Ok(result) => self.outcome_from(&result),
            Err(_) => KeyOutcome {
                eaten: self.is_composing(),
                ..passthrough
            },
        }
    }

    fn outcome_from(&self, result: &ProcessKeyResult) -> KeyOutcome {
        let plan = render::plan_from_result(result);
        // 反映すべき編集があるのに「食わない」と返すと、TSF が OnKeyDown を呼ばず編集が失われる
        let eaten = result.consumed || !plan.is_empty();
        KeyOutcome {
            eaten,
            plan,
            followup: Followup::from_result(result),
        }
    }

    fn apply(&self, context: &ITfContext, plan: EditPlan, timing: Timing) -> Result<()> {
        if plan.is_empty() {
            return Ok(());
        }
        let env = self
            .env()
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        edit::run_plan(context, self.client_id.get(), &env, plan, timing)
    }

    /// フォーカス中のコンテキスト
    fn focused_context(&self) -> Option<ITfContext> {
        let tm = self.thread_mgr.borrow().clone()?;
        let dm: ITfDocumentMgr = unsafe { tm.GetFocus() }.ok()?;
        unsafe { dm.GetTop() }.ok()
    }

    /// このセッション宛ての非同期イベント（別スレッドで受けてキューに積んだもの）を反映する。
    /// UI スレッドで呼ぶ。キーイベント内では同期（`Sync`）、隠しウィンドウ経由では文書ロック次第（`Deferred`）
    fn pump_events(&self, context: Option<&ITfContext>, timing: Timing) {
        for event in self.engine.drain_events(&self.session_id) {
            if let Some(message) = toast_for(&event) {
                self.show_toast(&message);
            }
            match event {
                EngineEvent::UpdatePreedit(p) => {
                    if let Some(ctx) = context {
                        let _ = self.apply(ctx, render::plan_from_preedit(&p.preedit), timing);
                    }
                }
                EngineEvent::UpdateCandidates(c) => self.show_candidates(Some(&c.candidates)),
                EngineEvent::Restored { result, .. } => {
                    if let Some(ctx) = context {
                        let _ = self.apply(ctx, render::plan_from_result(&result), timing);
                    }
                    self.finish(&Followup::from_result(&result));
                }
                EngineEvent::RestoreFailed { .. } => {
                    // 表示中の文字列をそのまま確定する（設計書 §3.3「入力を失わない」）
                    if let (Some(ctx), Some(env)) = (context, self.env()) {
                        let _ = edit::run_end_keeping_text(ctx, self.client_id.get(), &env, timing);
                    }
                    self.candidate_window.hide();
                }
                EngineEvent::Started { .. } => self.on_engine_started(),
                // 通知は上の `toast_for` で表示済み。それ以外は表示するものが無い
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
            let outcome = self.evaluate(context, wparam, lparam, false);
            let eaten = outcome.eaten;
            self.handoff.borrow_mut().store(id, outcome);
            return eaten;
        }
        self.pump_events(context, Timing::Sync);
        let cached = self.handoff.borrow_mut().take(id);
        let outcome = cached.unwrap_or_else(|| self.evaluate(context, wparam, lparam, false));
        if let Some(ctx) = context
            && self.apply(ctx, outcome.plan, Timing::Sync).is_err()
        {
            // 編集に失敗したらエンジン側の状態と食い違う。次の入力で揃うようリセットする
            let _ = self.engine.reset(&self.session_id);
        }
        self.finish(&outcome.followup);
        if outcome.followup.reconvert
            && let Some(ctx) = context
        {
            let _ = self.reconvert(ctx, None, Timing::Sync);
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
        self.focused.set(foreground);
        if foreground {
            let _ = self.engine.focus_in(&self.session_id);
            return;
        }
        self.handoff.borrow_mut().clear();
        self.candidate_window.hide();
        if let Ok(result) = self.engine.focus_out(&self.session_id) {
            let plan = render::plan_from_result(&result);
            if let Some(ctx) = self.focused_context() {
                let _ = self.apply(&ctx, plan, Timing::Sync);
            }
        }
    }
}

fn ctx_ref<'a>(pic: &'a Ref<'_, ITfContext>) -> Option<&'a ITfContext> {
    pic.as_ref()
}

/// 入力方式本体（`ITfTextInputProcessorEx` + 表示属性の提供者）
#[implement(ITfTextInputProcessorEx, ITfDisplayAttributeProvider)]
pub struct TextService {
    /// `Activate` から `Deactivate` の間だけ存在する
    core: RefCell<Option<Rc<Core>>>,
}

impl TextService {
    /// 新しいインスタンス（`IClassFactory::CreateInstance` から呼ぶ）
    pub(crate) fn new() -> Self {
        dll::add_object();
        Self {
            core: RefCell::new(None),
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
                let core = self.core.borrow_mut().take();
                if let Some(core) = core {
                    core.deactivate();
                }
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
                // 二重の Activate に備えて、前の状態を閉じる
                if let Some(old) = self.core.borrow_mut().take() {
                    old.deactivate();
                }
                let core = Rc::new_cyclic(|me| Core::new(me.clone()));
                *self.core.borrow_mut() = Some(Rc::clone(&core));
                core.activate(tm, tid)
            },
        )
    }
}

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(display_attr::enumerate())
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if guid.is_null() {
            return Err(E_POINTER.into());
        }
        display_attr::info_for(unsafe { &*guid })
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
                self.core.candidate_window.hide();
                let _ = self.core.engine.reset(&self.core.session_id);
                Ok(())
            },
        )
    }
}

/// モードのコンパートメントの変更通知。`Core` は弱い参照（TSF が保持するため循環させない）
#[implement(ITfCompartmentEventSink)]
struct CompartmentSink {
    core: Weak<Core>,
}

impl ITfCompartmentEventSink_Impl for CompartmentSink_Impl {
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        guarded(
            || Ok(()),
            || {
                if rguid.is_null() {
                    return Ok(());
                }
                let guid = unsafe { *rguid };
                if mode_comp::WATCHED.contains(&guid)
                    && let Some(core) = self.core.upgrade()
                {
                    core.on_compartment_changed();
                }
                Ok(())
            },
        )
    }
}
