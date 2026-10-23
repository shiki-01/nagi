//! エンジン子プロセスの管理: 起動・クラッシュ検知・指数バックオフ再起動・`restore_session`。
//!
//! あわせて revision による古い通知の破棄と、型付きのメソッド呼び出しを提供する。
//! 時刻（[`Clock`]）と待ち（[`Scheduler`]）は注入でき、テストでは即時実行に差し替える。
//! スリープ復帰時は OS 層（TSF 側の電源通知）が [`EngineSupervisor::restart_now`] を呼ぶ。

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use nagi_protocol::methods::method;
use nagi_protocol::{
    EmptyResult, InitializeParams, InitializeResult, NotifyParams, PROTOCOL_VERSION,
    PerformActionParams, ProcessKeyParams, ProcessKeyResult, ReconvertParams, ReloadConfigResult,
    RestoreSessionParams, SelectCandidateParams, SessionParams, SetContextParams,
    SetEnvironmentParams, SettingChange, SettingsSnapshot, StatusResult, UpdateCandidatesParams,
    UpdatePreeditParams, UpdateSettingsParams, UpdateSettingsResult,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::client::{ClientDiagnostic, ClientError, ExitReport, JsonRpcClient, Transport};
use crate::revision::RevisionGate;

/// 接続の生成器。再起動のたびに新しい接続を作る
pub trait EngineLauncher: Send + Sync {
    /// `minimal` が true なら minimal プロファイル（LLM なし）で起動する
    fn make_connection(&self, minimal: bool) -> Result<Box<dyn Transport>, ClientError>;
}

/// 遅延実行。再起動のバックオフ待ちに使い、テストでは即時実行に差し替える
pub trait Scheduler: Send + Sync {
    /// `delay` 後に `work` を（別スレッドで）実行する
    fn schedule(&self, delay: Duration, work: Box<dyn FnOnce() + Send>);
}

/// 時刻。連続クラッシュ判定（稼働時間）に使う
pub trait Clock: Send + Sync {
    /// 現在の単調時刻
    fn now(&self) -> Instant;
}

/// 実時刻の [`Clock`]
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// スレッドを作って `sleep` する [`Scheduler`]
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadScheduler;

impl Scheduler for ThreadScheduler {
    fn schedule(&self, delay: Duration, work: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            work();
        });
    }
}

/// 再起動のバックオフ設定
#[derive(Debug, Clone, PartialEq)]
pub struct BackoffPolicy {
    /// 1 回目の再起動前に待つ時間
    pub initial_delay: Duration,
    /// 再起動ごとの待ち時間の倍率
    pub multiplier: f64,
    /// 待ち時間の上限
    pub max_delay: Duration,
    /// 連続再起動がこの回数に達したら通知し、minimal プロファイルで起動する（設計書 §3.3）
    pub minimal_after_restarts: u32,
    /// この時間以上生き延びてからのクラッシュは「連続」に数えない（カウンタを 0 に戻す）
    pub stable_uptime: Duration,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(200),
            multiplier: 2.0,
            max_delay: Duration::from_secs(10),
            minimal_after_restarts: 5,
            stable_uptime: Duration::from_secs(30),
        }
    }
}

impl BackoffPolicy {
    /// n 回目（1 始まり）の再起動前に待つ時間
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let exp = i32::try_from(attempt.saturating_sub(1)).unwrap_or(i32::MAX);
        let factor = self.multiplier.powi(exp);
        if !factor.is_finite()
            || factor
                >= self.max_delay.as_secs_f64()
                    / self.initial_delay.as_secs_f64().max(f64::MIN_POSITIVE)
        {
            return self.max_delay;
        }
        self.initial_delay.mul_f64(factor).min(self.max_delay)
    }
}

/// エンジンが落ちたのが `initialize` の前か後か
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitPhase {
    /// `initialize` に成功する前（起動失敗・即時終了・応答なし）
    BeforeInitialize,
    /// `initialize` に成功した後（稼働中のクラッシュ）
    AfterInitialize,
}

/// エンジンの起動失敗・停止 1 回分の記録（原因の切り分け用。入力内容は含まない）
#[derive(Debug, Clone, PartialEq)]
pub struct FailureRecord {
    /// 連続再起動の回数（1 始まり）
    pub attempt: u32,
    /// minimal プロファイル（`--no-llm`）での起動だったか
    pub minimal: bool,
    /// `initialize` の前後どちらで落ちたか
    pub phase: ExitPhase,
    /// 子プロセスの終了コードと生存時間（起動できなかった・こちらから終了させた場合は `None`）
    pub exit: Option<ExitReport>,
    /// 起動・初期化のエラー（稼働中の切断は `None`）
    pub error: Option<ClientError>,
}

/// 記録を残す最大件数（連続失敗の一覧）
const MAX_FAILURE_RECORDS: usize = 50;

/// スーパーバイザーが通知するイベント
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// 起動・初期化に成功（再起動後を含む）
    Started {
        /// `initialize` の結果
        result: InitializeResult,
        /// 再起動後か
        is_restart: bool,
    },
    /// 通信路が閉じた（クラッシュ等）。`attempt` 回目の再起動を `delay` 後に行う
    Crashed {
        /// 連続再起動の回数（1 始まり）
        attempt: u32,
        /// 再起動までの待ち
        delay: Duration,
    },
    /// 再起動回数が閾値を超えたため minimal で起動する（ユーザーへ通知すること）
    EnteredMinimalMode {
        /// 連続再起動の回数
        restarts: u32,
    },
    /// 再起動後の `restore_session` に成功した。フロントは結果の preedit を描画する
    Restored {
        /// 復元したセッション
        session_id: String,
        /// 復元後の描画差分
        result: ProcessKeyResult,
    },
    /// 復元に失敗した。フロントは表示中の文字列をそのまま確定すること（設計書 §3.3）
    RestoreFailed {
        /// 復元に失敗したセッション
        session_id: String,
        /// 失敗の理由
        error: ClientError,
    },
    /// 非同期通知 `update_preedit`（古い revision は破棄済み）
    UpdatePreedit(UpdatePreeditParams),
    /// 非同期通知 `update_candidates`（古い revision は破棄済み）
    UpdateCandidates(UpdateCandidatesParams),
    /// `notify`（設定エラー・警告等）
    Notify(NotifyParams),
    /// 起動または初期化に失敗した（再起動は自動で予約される）
    LaunchFailed(ClientError),
    /// エンジンが停止・起動失敗した 1 回分の詳細（`Crashed` の直前に通知される）
    ProcessExited(FailureRecord),
    /// minimal へ切り替えた。通常起動での連続失敗の一覧（LLM が原因かの切り分け用）
    MinimalSwitchReport {
        /// 通常（LLM あり）起動での失敗の記録
        failures: Vec<FailureRecord>,
    },
    /// 通信路の診断（捨てた行など。内容は含まない）
    Diagnostic(ClientDiagnostic),
}

/// スーパーバイザーの設定
#[derive(Debug, Clone)]
pub struct Configuration {
    /// `initialize` に載せるクライアント名
    pub client_name: String,
    /// クライアントのバージョン
    pub client_version: String,
    /// OS 名
    pub os: String,
    /// `initialize` のタイムアウト（コールドスタート < 2s が目標。モデルロードを見込んで長め）
    pub initialize_timeout: Duration,
    /// 通常要求のタイムアウト（設計書 §3.3: 500ms）
    pub request_timeout: Duration,
    /// 再起動のバックオフ
    pub backoff: BackoffPolicy,
}

impl Configuration {
    /// Windows 用の既定値で設定を作る
    pub fn new(client_version: impl Into<String>) -> Self {
        Self {
            client_name: "nagi-windows".into(),
            client_version: client_version.into(),
            os: "windows".into(),
            initialize_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_millis(500),
            backoff: BackoffPolicy::default(),
        }
    }
}

/// 空の params（`ping` / `shutdown` / `get_status` 等の引数なし要求）
#[derive(Serialize)]
struct EmptyParams {}

type EventHandler = Arc<dyn Fn(EngineEvent) + Send + Sync>;
type RestoreProvider = Arc<dyn Fn() -> Vec<RestoreSessionParams> + Send + Sync>;

struct State {
    client: Option<Arc<JsonRpcClient>>,
    generation: u64,
    restart_count: u32,
    minimal_mode: bool,
    launched_at: Instant,
    stopped: bool,
    has_started_once: bool,
    /// 起動中に落ちた子プロセスの終了記録（次の `handle_failure` が取り出す）
    last_exit: Option<ExitReport>,
    /// 連続失敗のうち、通常（LLM あり）起動でのもの
    normal_failures: Vec<FailureRecord>,
}

struct Inner {
    launcher: Arc<dyn EngineLauncher>,
    scheduler: Arc<dyn Scheduler>,
    clock: Arc<dyn Clock>,
    config: Configuration,
    gate: RevisionGate,
    state: Mutex<State>,
    on_event: Mutex<Option<EventHandler>>,
    restore_provider: Mutex<Option<RestoreProvider>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// エンジン子プロセスの管理と、型付きのメソッド呼び出し
pub struct EngineSupervisor {
    inner: Arc<Inner>,
}

impl EngineSupervisor {
    /// 実時刻・スレッドスケジューラで作る
    pub fn new(launcher: Arc<dyn EngineLauncher>, config: Configuration) -> Self {
        Self::with_parts(
            launcher,
            config,
            Arc::new(ThreadScheduler),
            Arc::new(SystemClock),
        )
    }

    /// スケジューラと時計を注入して作る（テスト用）
    pub fn with_parts(
        launcher: Arc<dyn EngineLauncher>,
        config: Configuration,
        scheduler: Arc<dyn Scheduler>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let now = clock.now();
        Self {
            inner: Arc::new(Inner {
                launcher,
                scheduler,
                clock,
                config,
                gate: RevisionGate::new(),
                state: Mutex::new(State {
                    client: None,
                    generation: 0,
                    restart_count: 0,
                    minimal_mode: false,
                    launched_at: now,
                    stopped: false,
                    has_started_once: false,
                    last_exit: None,
                    normal_failures: Vec::new(),
                }),
                on_event: Mutex::new(None),
                restore_provider: Mutex::new(None),
            }),
        }
    }

    /// イベントの受け手を設定する。任意のスレッドから呼ばれる
    pub fn set_on_event(&self, handler: impl Fn(EngineEvent) + Send + Sync + 'static) {
        *lock(&self.inner.on_event) = Some(Arc::new(handler));
    }

    /// 再起動後に復元すべきセッション（フロントが保持している最新の preedit）を返す関数を設定する。
    /// 任意のスレッドから呼ばれる
    pub fn set_restore_provider(
        &self,
        provider: impl Fn() -> Vec<RestoreSessionParams> + Send + Sync + 'static,
    ) {
        *lock(&self.inner.restore_provider) = Some(Arc::new(provider));
    }

    /// 最初の起動。失敗した場合は自動再起動を予約したうえでエラーを返す
    pub fn start(&self) -> Result<InitializeResult, ClientError> {
        lock(&self.inner.state).stopped = false;
        match self.inner.connect() {
            Ok(r) => Ok(r),
            Err(e) => {
                let gen_ = self.inner.current_generation();
                self.inner.handle_failure(e.clone(), gen_);
                Err(e)
            }
        }
    }

    /// 正常終了（`shutdown` を送ってから閉じる）。以後は自動再起動しない
    pub fn stop(&self) {
        let client = {
            let mut st = lock(&self.inner.state);
            st.stopped = true;
            st.generation += 1;
            st.client.take()
        };
        if let Some(c) = client {
            let _: Result<EmptyResult, _> = c.request(
                method::SHUTDOWN,
                &EmptyParams {},
                Some(self.inner.config.request_timeout),
            );
            c.close();
        }
    }

    /// スリープ復帰時などの強制再起動。クラッシュ扱いにはせず、バックオフも増やさない
    pub fn restart_now(&self) {
        let old = {
            let mut st = lock(&self.inner.state);
            if st.stopped {
                return;
            }
            st.generation += 1;
            st.client.take()
        };
        if let Some(c) = old {
            c.close();
        }
        if let Err(e) = self.inner.connect() {
            let gen_ = self.inner.current_generation();
            self.inner.handle_failure(e, gen_);
        }
    }

    /// 現在エンジンと接続しているか
    pub fn is_connected(&self) -> bool {
        lock(&self.inner.state).client.is_some()
    }

    /// minimal プロファイルに落ちているか
    pub fn is_minimal_mode(&self) -> bool {
        lock(&self.inner.state).minimal_mode
    }

    // 型付きメソッド

    fn connected_client(&self) -> Result<Arc<JsonRpcClient>, ClientError> {
        lock(&self.inner.state)
            .client
            .clone()
            .ok_or(ClientError::NotConnected)
    }

    fn diff<P: Serialize>(
        &self,
        method: &str,
        params: &P,
        session_id: &str,
        timeout: Option<Duration>,
    ) -> Result<ProcessKeyResult, ClientError> {
        let c = self.connected_client()?;
        let result: ProcessKeyResult = c.request(method, params, timeout)?;
        self.inner.gate.record(session_id, result.revision);
        Ok(result)
    }

    fn session_diff(
        &self,
        method: &str,
        session_id: &str,
    ) -> Result<ProcessKeyResult, ClientError> {
        let params = SessionParams {
            session_id: session_id.to_string(),
        };
        self.diff(method, &params, session_id, None)
    }

    /// キー入力を処理する
    pub fn process_key(&self, params: &ProcessKeyParams) -> Result<ProcessKeyResult, ClientError> {
        self.diff(method::PROCESS_KEY, params, &params.session_id, None)
    }

    /// フォーカス取得
    pub fn focus_in(&self, session_id: &str) -> Result<ProcessKeyResult, ClientError> {
        self.session_diff(method::FOCUS_IN, session_id)
    }

    /// フォーカス喪失。表示中の surface を確定して返す
    pub fn focus_out(&self, session_id: &str) -> Result<ProcessKeyResult, ClientError> {
        self.session_diff(method::FOCUS_OUT, session_id)
    }

    /// セッションをリセットする
    pub fn reset(&self, session_id: &str) -> Result<ProcessKeyResult, ClientError> {
        self.session_diff(method::RESET, session_id)
    }

    /// 候補を選択する
    pub fn select_candidate(
        &self,
        params: &SelectCandidateParams,
    ) -> Result<ProcessKeyResult, ClientError> {
        self.diff(method::SELECT_CANDIDATE, params, &params.session_id, None)
    }

    /// キーを介さない操作（入力メニューからのモード切替等）
    pub fn perform_action(
        &self,
        params: &PerformActionParams,
    ) -> Result<ProcessKeyResult, ClientError> {
        self.diff(method::PERFORM_ACTION, params, &params.session_id, None)
    }

    /// 再変換。初回は辞書の逆引き索引を作るため、通常の要求より長く待つ（`timeout` 既定は `initialize` と同じ）
    pub fn reconvert(
        &self,
        params: &ReconvertParams,
        timeout: Option<Duration>,
    ) -> Result<ProcessKeyResult, ClientError> {
        let t = timeout.unwrap_or(self.inner.config.initialize_timeout);
        self.diff(method::RECONVERT, params, &params.session_id, Some(t))
    }

    /// 文脈（周辺テキスト・アプリ ID・secure input）を通知する
    pub fn set_context(&self, params: &SetContextParams) -> Result<(), ClientError> {
        self.connected_client()?.notify(method::SET_CONTEXT, params)
    }

    /// 環境（電源・負荷）を通知する
    pub fn set_environment(&self, params: &SetEnvironmentParams) -> Result<(), ClientError> {
        self.connected_client()?
            .notify(method::SET_ENVIRONMENT, params)
    }

    fn call<R: DeserializeOwned>(&self, method: &str, long: bool) -> Result<R, ClientError> {
        let t = long.then_some(self.inner.config.initialize_timeout);
        self.connected_client()?.request(method, &EmptyParams {}, t)
    }

    /// エンジンの状態を取得する
    pub fn get_status(&self) -> Result<StatusResult, ClientError> {
        self.call(method::GET_STATUS, false)
    }

    /// 設定を再読み込みする
    pub fn reload_config(&self) -> Result<ReloadConfigResult, ClientError> {
        self.call(method::RELOAD_CONFIG, true)
    }

    /// 設定項目の記述子と現在値を取得する
    pub fn get_settings(&self) -> Result<SettingsSnapshot, ClientError> {
        self.call(method::GET_SETTINGS, true)
    }

    /// 設定を更新する。`value` が None の変更は明示指定を削除して既定へ戻す。
    /// 適用後にコアがホットリロードするため、通常要求より長いタイムアウトで待つ
    pub fn update_settings(
        &self,
        changes: Vec<SettingChange>,
    ) -> Result<UpdateSettingsResult, ClientError> {
        self.connected_client()?.request(
            method::UPDATE_SETTINGS,
            &UpdateSettingsParams { changes },
            Some(self.inner.config.initialize_timeout),
        )
    }

    /// 学習データを全削除する
    pub fn clear_learning(&self) -> Result<(), ClientError> {
        self.call::<EmptyResult>(method::CLEAR_LEARNING, false)
            .map(|_| ())
    }

    /// 疎通確認
    pub fn ping(&self) -> Result<(), ClientError> {
        self.call::<EmptyResult>(method::PING, false).map(|_| ())
    }
}

impl Inner {
    fn current_generation(&self) -> u64 {
        lock(&self.state).generation
    }

    fn emit(&self, event: EngineEvent) {
        let handler = lock(&self.on_event).clone();
        if let Some(h) = handler {
            h(event);
        }
    }

    /// 起動して `initialize` まで行い、成功すれば接続を登録する。再起動なら `restore_session` も行う
    fn connect(self: &Arc<Self>) -> Result<InitializeResult, ClientError> {
        let (gen_, minimal, is_restart) = {
            let mut st = lock(&self.state);
            st.generation += 1;
            (st.generation, st.minimal_mode, st.has_started_once)
        };

        let transport = self.launcher.make_connection(minimal)?;
        let rpc = Arc::new(JsonRpcClient::new(transport, self.config.request_timeout));
        let weak: Weak<Inner> = Arc::downgrade(self);
        rpc.set_on_notification(Arc::new(move |method, params| {
            if let Some(i) = weak.upgrade() {
                i.handle_notification(method, params, gen_);
            }
        }));
        let weak: Weak<Inner> = Arc::downgrade(self);
        rpc.set_on_diagnostic(Arc::new(move |diagnostic| {
            if let Some(i) = weak.upgrade()
                && i.current_generation() == gen_
            {
                i.emit(EngineEvent::Diagnostic(diagnostic));
            }
        }));
        // on_close は初期化に成功して client を登録した後で設定する。
        // 初期化中の切断は request のエラーとして呼び出し元（start / 再起動処理）が扱う
        rpc.start()?;

        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION,
            client_name: self.config.client_name.clone(),
            client_version: self.config.client_version.clone(),
            os: self.config.os.clone(),
            capabilities: None,
        };
        let result: InitializeResult = match rpc.request(
            method::INITIALIZE,
            &params,
            Some(self.config.initialize_timeout),
        ) {
            Ok(r) => r,
            Err(e) => {
                // 閉じる前に終了コードを控える（`handle_failure` が記録に使う）
                lock(&self.state).last_exit = rpc.exit_report();
                rpc.close();
                return Err(e);
            }
        };

        {
            let mut st = lock(&self.state);
            if st.generation != gen_ || st.stopped {
                drop(st);
                rpc.close();
                return Err(ClientError::ConnectionClosed);
            }
            st.client = Some(rpc.clone());
            st.launched_at = self.clock.now();
            st.has_started_once = true;
        }

        let weak: Weak<Inner> = Arc::downgrade(self);
        rpc.set_on_close(Arc::new(move || {
            if let Some(i) = weak.upgrade() {
                i.handle_failure(ClientError::ConnectionClosed, gen_);
            }
        }));
        if rpc.is_closed() {
            self.handle_failure(ClientError::ConnectionClosed, gen_);
        }

        self.gate.reset();
        self.emit(EngineEvent::Started {
            result: result.clone(),
            is_restart,
        });
        if is_restart {
            self.restore_sessions(&rpc, gen_);
        }
        Ok(result)
    }

    fn restore_sessions(&self, rpc: &JsonRpcClient, gen_: u64) {
        let provider = lock(&self.restore_provider).clone();
        let sessions = provider.map(|p| p()).unwrap_or_default();
        for params in sessions {
            if self.current_generation() != gen_ {
                return;
            }
            let r: Result<ProcessKeyResult, _> = rpc.request(
                method::RESTORE_SESSION,
                &params,
                Some(self.config.initialize_timeout),
            );
            match r {
                Ok(result) => {
                    self.gate.record(&params.session_id, result.revision);
                    self.emit(EngineEvent::Restored {
                        session_id: params.session_id,
                        result,
                    });
                }
                Err(error) => self.emit(EngineEvent::RestoreFailed {
                    session_id: params.session_id,
                    error,
                }),
            }
        }
    }

    /// 通信路が閉じた・起動に失敗した: 世代が古ければ無視し、バックオフ後に再起動を予約する
    fn handle_failure(self: &Arc<Self>, error: ClientError, gen_: u64) {
        let (old, attempt, entered_minimal, record, report) = {
            let mut st = lock(&self.state);
            if st.generation != gen_ || st.stopped {
                return;
            }
            // 同じ世代の失敗を二重に処理しない（世代を進めて以後の通知を無効化）
            st.generation += 1;
            let old = st.client.take();
            let uptime = self.clock.now().saturating_duration_since(st.launched_at);
            if old.is_some() && uptime >= self.config.backoff.stable_uptime {
                st.restart_count = 0;
                st.normal_failures.clear();
            }
            st.restart_count += 1;
            let attempt = st.restart_count;
            let was_minimal = st.minimal_mode;
            let entered = attempt >= self.config.backoff.minimal_after_restarts && !st.minimal_mode;
            if entered {
                st.minimal_mode = true;
            }
            // 接続済みだった（= initialize 後）か、起動中に落ちたか
            let exit = old
                .as_ref()
                .and_then(|c| c.exit_report())
                .or_else(|| st.last_exit.take());
            st.last_exit = None;
            let record = FailureRecord {
                attempt,
                minimal: was_minimal,
                phase: if old.is_some() {
                    ExitPhase::AfterInitialize
                } else {
                    ExitPhase::BeforeInitialize
                },
                exit,
                error: (error != ClientError::ConnectionClosed).then(|| error.clone()),
            };
            if !was_minimal {
                if st.normal_failures.len() >= MAX_FAILURE_RECORDS {
                    st.normal_failures.remove(0);
                }
                st.normal_failures.push(record.clone());
            }
            let report = entered.then(|| st.normal_failures.clone());
            (old, attempt, entered, record, report)
        };

        if let Some(c) = old {
            c.close();
        }
        let delay = self.config.backoff.delay_for_attempt(attempt);
        if error != ClientError::ConnectionClosed {
            self.emit(EngineEvent::LaunchFailed(error));
        }
        self.emit(EngineEvent::ProcessExited(record));
        self.emit(EngineEvent::Crashed { attempt, delay });
        if entered_minimal {
            self.emit(EngineEvent::EnteredMinimalMode { restarts: attempt });
            if let Some(failures) = report {
                self.emit(EngineEvent::MinimalSwitchReport { failures });
            }
        }

        let weak: Weak<Inner> = Arc::downgrade(self);
        self.scheduler.schedule(
            delay,
            Box::new(move || {
                let Some(inner) = weak.upgrade() else { return };
                if let Err(e) = inner.connect() {
                    let g = inner.current_generation();
                    inner.handle_failure(e, g);
                }
            }),
        );
    }

    fn handle_notification(&self, method: &str, params: Option<Value>, gen_: u64) {
        let Some(params) = params else { return };
        if self.current_generation() != gen_ {
            return;
        }
        match method {
            method::UPDATE_PREEDIT => {
                if let Ok(p) = serde_json::from_value::<UpdatePreeditParams>(params)
                    && self.gate.accept(&p.session_id, p.revision)
                {
                    self.emit(EngineEvent::UpdatePreedit(p));
                }
            }
            method::UPDATE_CANDIDATES => {
                if let Ok(p) = serde_json::from_value::<UpdateCandidatesParams>(params)
                    && self.gate.accept(&p.session_id, p.revision)
                {
                    self.emit(EngineEvent::UpdateCandidates(p));
                }
            }
            method::NOTIFY => {
                if let Ok(p) = serde_json::from_value::<NotifyParams>(params) {
                    self.emit(EngineEvent::Notify(p));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        HoldScheduler, ImmediateScheduler, MockClock, MockLauncher, response, standard_responder,
    };
    use nagi_protocol::{KeyKind, Modifiers, RestoreSegment, SegmentKind};

    type Log = Arc<Mutex<Vec<EngineEvent>>>;

    fn make(
        launcher: &Arc<MockLauncher>,
        scheduler: Arc<dyn Scheduler>,
        clock: Arc<dyn Clock>,
        backoff: BackoffPolicy,
    ) -> (EngineSupervisor, Log) {
        let mut config = Configuration::new("0.0.0");
        config.backoff = backoff;
        let sup = EngineSupervisor::with_parts(launcher.clone(), config, scheduler, clock);
        let log: Log = Arc::default();
        let l = log.clone();
        sup.set_on_event(move |e| l.lock().unwrap().push(e));
        (sup, log)
    }

    fn simple(launcher: &Arc<MockLauncher>) -> (EngineSupervisor, Log, Arc<ImmediateScheduler>) {
        let s = ImmediateScheduler::new();
        let (sup, log) = make(
            launcher,
            s.clone(),
            MockClock::new(),
            BackoffPolicy::default(),
        );
        (sup, log, s)
    }

    fn key_params(session: &str) -> ProcessKeyParams {
        ProcessKeyParams {
            session_id: session.into(),
            keysym: 0x61,
            keycode: 0,
            text: "a".into(),
            modifiers: Modifiers::default(),
            kind: KeyKind::Down,
        }
    }

    fn restore_params(session: &str, cursor: u32) -> RestoreSessionParams {
        RestoreSessionParams {
            session_id: session.into(),
            segments: vec![RestoreSegment {
                reading: "にほん".into(),
                surface: "にほん".into(),
                kind: SegmentKind::Japanese,
            }],
            cursor,
            focus: None,
            converting: false,
        }
    }

    fn count(log: &Log, f: impl Fn(&EngineEvent) -> bool) -> usize {
        log.lock().unwrap().iter().filter(|e| f(e)).count()
    }

    #[test]
    fn backoff_delays_grow_and_cap() {
        let b = BackoffPolicy::default();
        let ms: Vec<u128> = (1..=8)
            .map(|n| b.delay_for_attempt(n).as_millis())
            .collect();
        assert_eq!(ms, [200, 400, 800, 1600, 3200, 6400, 10000, 10000]);
        assert_eq!(b.delay_for_attempt(0), Duration::from_millis(200));
        assert_eq!(b.delay_for_attempt(u32::MAX), Duration::from_secs(10));
    }

    #[test]
    fn start_sends_initialize_and_process_key_round_trip() {
        let launcher = MockLauncher::new();
        launcher.set_configure(|_, _, c| c.set_responder(standard_responder(1)));
        let (sup, _, _) = simple(&launcher);
        let result = sup.start().unwrap();
        assert_eq!(result.server_name, "nagi-engine");
        assert!(sup.process_key(&key_params("s1")).unwrap().consumed);
        let conn = &launcher.connections()[0];
        assert_eq!(conn.sent_methods(), ["initialize", "process_key"]);
        let p = &conn.sent()[0]["params"];
        assert_eq!(p["protocol_version"], 1);
        assert_eq!(p["os"], "windows");
        assert_eq!(p["client_name"], "nagi-windows");
    }

    #[test]
    fn crash_restarts_with_backoff_and_restores_session() {
        let launcher = MockLauncher::new();
        let (sup, log, sched) = simple(&launcher);
        sup.set_restore_provider(|| vec![restore_params("s1", 3)]);
        sup.start().unwrap();
        // 初回起動では restore_session を呼ばない
        assert!(
            !launcher.connections()[0]
                .sent_methods()
                .contains(&"restore_session".into())
        );

        launcher.connections()[0].simulate_close(); // クラッシュ

        // ImmediateScheduler なので同期的に再起動まで終わっている
        assert!(sup.is_connected());
        assert_eq!(launcher.connections().len(), 2);
        let second = &launcher.connections()[1];
        assert_eq!(second.sent_methods(), ["initialize", "restore_session"]);
        let p = second.sent().pop().unwrap()["params"].clone();
        assert_eq!(p["session_id"], "s1");
        assert_eq!(p["cursor"], 3);
        assert_eq!(
            sched.delays(),
            [BackoffPolicy::default().delay_for_attempt(1)]
        );
        assert_eq!(
            count(&log, |e| matches!(e, EngineEvent::Restored { .. })),
            1
        );
        assert_eq!(
            count(&log, |e| matches!(
                e,
                EngineEvent::Crashed { attempt: 1, .. }
            )),
            1
        );
        assert_eq!(
            count(&log, |e| matches!(
                e,
                EngineEvent::Started {
                    is_restart: true,
                    ..
                }
            )),
            1
        );
        // 復帰後は通常のキー入力ができる
        assert!(sup.process_key(&key_params("s1")).unwrap().consumed);
    }

    #[test]
    fn restore_failure_is_reported() {
        let launcher = MockLauncher::new();
        launcher.set_configure(|n, _, c| {
            let base = standard_responder(0);
            c.set_responder(move |req| {
                if n > 1
                    && req["method"] == "restore_session"
                    && let Some(id) = req["id"].as_i64()
                {
                    return Some(vec![format!(
                        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":-32004,\"message\":\"x\"}}}}"
                    )]);
                }
                base(req)
            });
        });
        let (sup, log, _) = simple(&launcher);
        sup.set_restore_provider(|| vec![restore_params("s1", 0)]);
        sup.start().unwrap();
        launcher.connections()[0].simulate_close();
        assert_eq!(
            count(&log, |e| matches!(e, EngineEvent::RestoreFailed { .. })),
            1
        );
    }

    #[test]
    fn backoff_grows_and_enters_minimal_mode_after_threshold() {
        let launcher = MockLauncher::new();
        // 最初の起動と minimal 起動だけ成功させ、それ以外は即クラッシュさせる（起動失敗ループ）
        launcher.set_configure(|n, minimal, c| {
            if n == 1 || minimal {
                c.set_responder(standard_responder(0));
            } else {
                let conn = c.clone();
                c.set_responder(move |_| {
                    conn.simulate_close();
                    None
                });
            }
        });
        let backoff = BackoffPolicy {
            initial_delay: Duration::from_millis(100),
            multiplier: 2.0,
            max_delay: Duration::from_millis(500),
            minimal_after_restarts: 3,
            stable_uptime: Duration::from_secs(1000),
        };
        let sched = ImmediateScheduler::new();
        let (sup, log) = make(&launcher, sched.clone(), MockClock::new(), backoff);
        sup.start().unwrap();
        launcher.connections()[0].simulate_close();

        assert!(sup.is_connected());
        assert!(sup.is_minimal_mode());
        let ms: Vec<u128> = sched.delays().iter().map(Duration::as_millis).collect();
        assert_eq!(ms, [100, 200, 400]);
        assert_eq!(launcher.minimal_flags(), [false, false, false, true]);
        assert_eq!(
            count(&log, |e| matches!(
                e,
                EngineEvent::EnteredMinimalMode { restarts: 3 }
            )),
            1
        );
    }

    #[test]
    fn failure_records_distinguish_phase_and_exit_code() {
        let launcher = MockLauncher::new();
        launcher.set_configure(|n, minimal, c| {
            let exit = |code| ExitReport {
                code: Some(code),
                lifetime: Duration::from_millis(50),
            };
            if n == 1 {
                c.set_responder(standard_responder(0));
                c.set_exit_report(exit(-1));
            } else if minimal {
                c.set_responder(standard_responder(0));
            } else {
                // initialize に応答せず終了コード 101 で落ちる
                c.set_exit_report(exit(101));
                let conn = c.clone();
                c.set_responder(move |_| {
                    conn.simulate_close();
                    None
                });
            }
        });
        let backoff = BackoffPolicy {
            minimal_after_restarts: 3,
            stable_uptime: Duration::from_secs(1000),
            ..BackoffPolicy::default()
        };
        let (sup, log) = make(
            &launcher,
            ImmediateScheduler::new(),
            MockClock::new(),
            backoff,
        );
        sup.start().unwrap();
        launcher.connections()[0].simulate_close();
        assert!(sup.is_minimal_mode());

        let events = log.lock().unwrap();
        let records: Vec<&FailureRecord> = events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::ProcessExited(r) => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].phase, ExitPhase::AfterInitialize);
        assert_eq!(records[0].exit.unwrap().code, Some(-1));
        assert_eq!(records[0].error, None);
        assert_eq!(records[1].phase, ExitPhase::BeforeInitialize);
        assert_eq!(records[1].exit.unwrap().code, Some(101));
        assert_eq!(records[2].attempt, 3);
        assert!(records.iter().all(|r| !r.minimal));
        let summary = events.iter().find_map(|e| match e {
            EngineEvent::MinimalSwitchReport { failures } => Some(failures.clone()),
            _ => None,
        });
        assert_eq!(summary.unwrap().len(), 3);
    }

    #[test]
    fn malformed_engine_line_is_forwarded_as_diagnostic() {
        let launcher = MockLauncher::new();
        launcher.set_configure(|_, _, c| {
            let inner = standard_responder(0);
            c.set_responder(move |req| {
                let mut lines = inner(req)?;
                lines.insert(0, "garbage".into());
                Some(lines)
            });
        });
        let (sup, log, _) = simple(&launcher);
        sup.start().unwrap();
        assert_eq!(
            count(&log, |e| matches!(
                e,
                EngineEvent::Diagnostic(ClientDiagnostic::MalformedLine {
                    len: 7,
                    first: "alpha",
                    count: 1
                })
            )),
            1
        );
    }

    #[test]
    fn long_uptime_resets_restart_counter() {
        let launcher = MockLauncher::new();
        let clock = MockClock::new();
        let sched = ImmediateScheduler::new();
        let (sup, _) = make(
            &launcher,
            sched.clone(),
            clock.clone(),
            BackoffPolicy::default(),
        );
        sup.start().unwrap();
        launcher.connections()[0].simulate_close();
        clock.advance(Duration::from_secs(60)); // 安定稼働
        launcher.connections()[1].simulate_close();
        // 2 回目のクラッシュは連続に数えない（どちらも attempt 1 の待ち）
        let first = BackoffPolicy::default().delay_for_attempt(1);
        assert_eq!(sched.delays(), [first, first]);
    }

    #[test]
    fn launch_failure_schedules_retry_and_reports() {
        struct FailingLauncher;
        impl EngineLauncher for FailingLauncher {
            fn make_connection(&self, _: bool) -> Result<Box<dyn Transport>, ClientError> {
                Err(ClientError::SendFailed("spawn".into()))
            }
        }
        let log: Log = Arc::default();
        let sup = EngineSupervisor::with_parts(
            Arc::new(FailingLauncher),
            Configuration::new("0"),
            Arc::new(HoldScheduler),
            MockClock::new(),
        );
        let l = log.clone();
        sup.set_on_event(move |e| l.lock().unwrap().push(e));
        assert!(sup.start().is_err());
        assert_eq!(
            count(&log, |e| matches!(e, EngineEvent::LaunchFailed(_))),
            1
        );
        assert_eq!(
            count(&log, |e| matches!(
                e,
                EngineEvent::Crashed { attempt: 1, .. }
            )),
            1
        );
    }

    fn update_preedit(rev: u32, text: &str) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"update_preedit\",\"params\":{{\"session_id\":\"s1\",\"revision\":{rev},\"preedit\":{{\"text\":\"{text}\",\"segments\":[],\"cursor\":0}}}}}}"
        )
    }

    #[test]
    fn stale_async_notifications_are_discarded() {
        let launcher = MockLauncher::new();
        launcher.set_configure(|_, _, c| c.set_responder(standard_responder(5)));
        let (sup, log, _) = simple(&launcher);
        sup.start().unwrap();
        sup.process_key(&key_params("s1")).unwrap(); // s1 の revision = 5

        let conn = launcher.connections()[0].clone();
        conn.receive(&update_preedit(4, "古い")); // 破棄される
        conn.receive(&update_preedit(6, "新しい")); // 反映される
        conn.receive(&update_preedit(5, "さらに古い")); // 6 を受けた後なので破棄される

        let texts: Vec<String> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EngineEvent::UpdatePreedit(p) => Some(p.preedit.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["新しい"]);
    }

    #[test]
    fn stale_candidates_discarded_and_notify_delivered() {
        let launcher = MockLauncher::new();
        launcher.set_configure(|_, _, c| c.set_responder(standard_responder(2)));
        let (sup, log, _) = simple(&launcher);
        sup.start().unwrap();
        sup.process_key(&key_params("s1")).unwrap();
        let conn = launcher.connections()[0].clone();
        let cands = |rev: u32| {
            format!(
                "{{\"jsonrpc\":\"2.0\",\"method\":\"update_candidates\",\"params\":{{\"session_id\":\"s1\",\"revision\":{rev},\"candidates\":{{\"items\":[],\"selected\":0,\"page_size\":9,\"visible\":true}}}}}}"
            )
        };
        conn.receive(&cands(1));
        conn.receive(&cands(3));
        conn.receive(
            r#"{"jsonrpc":"2.0","method":"notify","params":{"level":"warning","message":"w"}}"#,
        );
        let revs: Vec<u32> = log
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EngineEvent::UpdateCandidates(p) => Some(p.revision),
                _ => None,
            })
            .collect();
        assert_eq!(revs, [3]);
        assert_eq!(count(&log, |e| matches!(e, EngineEvent::Notify(_))), 1);
    }

    #[test]
    fn notifications_from_old_generation_are_ignored() {
        let launcher = MockLauncher::new();
        let (sup, log, _) = simple(&launcher);
        sup.start().unwrap();
        let old = launcher.connections()[0].clone();
        sup.restart_now();
        old.receive(&update_preedit(9, "旧"));
        assert_eq!(
            count(&log, |e| matches!(e, EngineEvent::UpdatePreedit(_))),
            0
        );
    }

    #[test]
    fn request_while_disconnected_is_not_connected() {
        let launcher = MockLauncher::new();
        let (sup, _) = make(
            &launcher,
            Arc::new(HoldScheduler),
            MockClock::new(),
            BackoffPolicy::default(),
        );
        sup.start().unwrap();
        launcher.connections()[0].simulate_close();
        assert!(!sup.is_connected());
        assert_eq!(
            sup.process_key(&key_params("s1")).unwrap_err(),
            ClientError::NotConnected
        );
    }

    #[test]
    fn restart_now_restores_without_backoff() {
        let launcher = MockLauncher::new();
        let (sup, _, sched) = simple(&launcher);
        sup.set_restore_provider(|| vec![restore_params("s1", 0)]);
        sup.start().unwrap();
        sup.restart_now();
        assert_eq!(launcher.connections().len(), 2);
        assert!(
            launcher.connections()[1]
                .sent_methods()
                .contains(&"restore_session".into())
        );
        assert!(sched.delays().is_empty());
        assert!(sup.is_connected());
    }

    #[test]
    fn stop_does_not_restart() {
        let launcher = MockLauncher::new();
        let (sup, _, sched) = simple(&launcher);
        sup.start().unwrap();
        sup.stop();
        assert_eq!(launcher.connections().len(), 1);
        assert!(
            launcher.connections()[0]
                .sent_methods()
                .contains(&"shutdown".into())
        );
        assert!(!sup.is_connected());
        assert!(sched.delays().is_empty());
        sup.restart_now(); // 停止後は何もしない
        assert_eq!(launcher.connections().len(), 1);
    }

    #[test]
    fn get_and_update_settings() {
        let launcher = MockLauncher::new();
        let snapshot = r#"{"items":[{"key":"conversion.sync_budget_ms","group":"変換","label":"待ち時間","description":"d","kind":{"type":"int","params":{"min":0,"max":5000,"step":10}},"value":{"type":"int","value":150},"default":{"type":"int","value":150},"explicit":false,"requires_restart":false,"level":"detail","platforms":"both"}]}"#;
        launcher.set_configure(move |_, _, c| {
            c.set_responder(move |req| {
                let id = req["id"].as_i64()?;
                let body = match req["method"].as_str()? {
                    "initialize" => crate::testing::INITIALIZE_RESULT.to_string(),
                    "get_settings" => snapshot.to_string(),
                    "update_settings" => format!(
                        "{{\"applied\":[\"a\"],\"errors\":[{{\"key\":\"b\",\"message\":\"m\"}}],\"snapshot\":{snapshot}}}"
                    ),
                    _ => "{}".to_string(),
                };
                Some(vec![response(id, &body)])
            });
        });
        let (sup, _, _) = simple(&launcher);
        sup.start().unwrap();
        assert_eq!(sup.get_settings().unwrap().items.len(), 1);
        let result = sup
            .update_settings(vec![SettingChange {
                key: "a".into(),
                value: None,
            }])
            .unwrap();
        assert_eq!(result.applied, ["a"]);
        assert_eq!(result.errors[0].key, "b");
        let sent = launcher.connections()[0].sent();
        assert_eq!(sent[2]["method"], "update_settings");
        assert_eq!(sent[2]["params"]["changes"][0]["key"], "a");
    }

    #[test]
    fn set_context_is_a_notification() {
        let launcher = MockLauncher::new();
        let (sup, _, _) = simple(&launcher);
        sup.start().unwrap();
        sup.set_context(&SetContextParams {
            session_id: "s1".into(),
            surrounding_text: None,
            app_id: Some("notepad.exe".into()),
            secure_input: Some(false),
        })
        .unwrap();
        let sent = launcher.connections()[0].sent();
        assert_eq!(sent[1]["method"], "set_context");
        assert!(sent[1].get("id").is_none());
    }
}
