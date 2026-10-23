//! JSON-RPC サーバー本体。`handle_line` は 1 行の入力に対する出力行を返す形で、
//! プロセスを起こさずにテストできる。

use crate::learning::LearningObserver;
use crate::runtime::Runtime;
use crate::setup::{Built, Reloadable, engine_config_from, issue_events, user_dict_events};
use nagi_dict::UserDictionary;
use nagi_engine::{ConversionBackend, Engine, EngineConfig};
use nagi_protocol::PROTOCOL_VERSION;
use nagi_protocol::jsonrpc::{
    ErrorObject, Notification, Request, Response, error_code, parse_message,
};
use nagi_protocol::methods::{
    ConfigError, EmptyResult, InitializeParams, InitializeResult, InsertReadingTouchParams,
    NotifyParams, PerformActionParams, ProcessKeyParams, ReconvertParams, ReloadConfigResult,
    RestoreSessionParams, SelectCandidateParams, ServerCapabilities, SessionParams,
    SetContextParams, SetEnvironmentParams, SetLayoutGeometryParams, UpdateCandidatesParams,
    UpdatePreeditParams, method,
};
use nagi_protocol::settings::{UpdateSettingsParams, UpdateSettingsResult};
use nagi_protocol::view::InputMode;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 設定ファイル・ユーザー辞書の変更を確認する最短の間隔
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// 入力中の候補を作るまでのデバウンス。キー入力が続く間は、最後のキーのあとに 1 回だけ作る
const COMPOSING_CANDIDATES_DELAY: Duration = Duration::from_millis(25);

/// JSON-RPC サーバー
pub struct Server {
    engine: Arc<Mutex<Engine>>,
    runtime: Arc<Runtime>,
    reload: Reloadable,
    initial_mode: InputMode,
    initialized: bool,
    shutdown: bool,
}

type RpcResult = Result<Value, ErrorObject>;

fn to_value(v: impl Serialize) -> RpcResult {
    serde_json::to_value(v).map_err(|_| ErrorObject::new(error_code::INTERNAL_ERROR, "内部エラー"))
}

/// パラメータを型に変換する。serde のエラー文言には入力値が含まれ得るので、分類だけを返す
fn params<T: DeserializeOwned>(req: &Request) -> Result<T, ErrorObject> {
    let value = req.params.clone().unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|e| {
        ErrorObject::new(
            error_code::INVALID_PARAMS,
            format!("params が不正です（{:?}）", e.classify()),
        )
    })
}

impl Server {
    /// 変換バックエンドだけを指定して作る（学習・設定・ユーザー辞書なし。テスト・簡易用）
    pub fn new(backend: Arc<dyn ConversionBackend>, _profile: &str, config: EngineConfig) -> Self {
        Server {
            initial_mode: config.initial_mode,
            engine: Arc::new(Mutex::new(Engine::new(backend, config))),
            runtime: Runtime::detached(false),
            reload: Reloadable::detached(),
            initialized: false,
            shutdown: false,
        }
    }

    /// [`crate::setup::build`] の結果から作る（学習・環境プロファイル・設定の再読み込みを含む）
    pub fn from_built(built: Built) -> Self {
        let (config, keymap_events) = engine_config_from(&built.config);
        for e in keymap_events {
            built.runtime.push_event(e);
        }
        let mut engine = Engine::new(built.pipeline.clone(), config.clone());
        engine.set_commit_observer(Arc::new(LearningObserver::new(built.runtime.clone())));
        Server {
            initial_mode: config.initial_mode,
            engine: Arc::new(Mutex::new(engine)),
            runtime: built.runtime,
            reload: built.reload,
            initialized: false,
            shutdown: false,
        }
    }

    /// 共有状態（診断・テスト用）
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    /// `shutdown` を受けたか
    pub fn should_shutdown(&self) -> bool {
        self.shutdown
    }

    /// 次のリクエストを待たずに配信すべき通知（`notify`・`update_preedit`・`update_candidates`）の行を
    /// 取り出す。`run` の Wake 処理と同じもの。`run` を使わず自前のループを持つ呼び出し側（C ABI の
    /// iOS フロントエンドなど）が、[`Self::set_wake_callback`] で起こされたあとに呼ぶ。
    /// 初期化前は通知を取り出さず、積んだままにする
    pub fn poll_notifications(&mut self) -> Vec<String> {
        if !self.initialized {
            return Vec::new();
        }
        let mut lines = self.notifications();
        lines.extend(self.async_notifications());
        lines
    }

    /// 配信すべき通知が積まれたときに呼ばれるコールバックを設定する。**任意のスレッドから呼ばれる**
    /// ため、コールバックの中で `Server` を触らず、呼び出し側のキューに積み直して
    /// [`Self::poll_notifications`] を呼ぶこと（`run` はチャンネルへの送信で同じことをしている）
    pub fn set_wake_callback(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        let for_events = Arc::clone(&wake);
        // Runtime にイベント（設定再読み込み・ライブ変換等）が積まれたら起こす
        self.runtime.set_event_hook(Box::new(move || for_events()));
        // バックエンドの非同期完了（LLM の sync_budget 超過分）が積まれたら起こす
        self.engine_lock().set_backend_waker(wake);
    }

    /// 1 行の入力を処理し、出力する行（0 行以上）を返す。応答のあとに、積まれた `notify` 通知が続く
    pub fn handle_line(&mut self, line: &str) -> Vec<String> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        self.poll_changes();
        let request = match parse_message(line) {
            Ok(r) => r,
            Err(e) => return vec![Self::encode(&Response::error(None, e))],
        };
        let id = request.id.clone();
        let outcome = self.dispatch(&request);
        let mut lines = match (id, outcome) {
            // 通知には応答しない
            (None, _) => Vec::new(),
            (Some(id), Ok(result)) => {
                let response = Response {
                    jsonrpc: "2.0".to_string(),
                    id: Some(id),
                    result: Some(result),
                    error: None,
                };
                vec![Self::encode(&response)]
            }
            (Some(id), Err(e)) => vec![Self::encode(&Response::error(Some(id), e))],
        };
        // 初期化前は通知を出さない（フロントが受け取れる状態になってから）
        if self.initialized {
            lines.extend(self.notifications());
            lines.extend(self.async_notifications());
        }
        lines
    }

    fn encode(response: &Response) -> String {
        serde_json::to_string(response).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"内部エラー"}}"#
                .to_string()
        })
    }

    /// バックエンドの非同期完了をプリエディット差し替え通知に変換
    fn async_notifications(&mut self) -> Vec<String> {
        use nagi_protocol::methods::UpdatePreeditParams;
        // 初期化前は通知を出さない（完了通知は取り出さずに残す）
        if !self.initialized {
            return Vec::new();
        }
        let done = self.engine_lock().backend_completions();
        if done.is_empty() {
            return Vec::new();
        }
        let encode = |n: Notification| serde_json::to_string(&n).ok();
        let mut lines = Vec::new();
        for (session_id, result) in self.engine_lock().apply_completions(&done) {
            let revision = result.revision;
            if let Some(preedit) = result.preedit {
                let params = UpdatePreeditParams {
                    session_id: session_id.clone(),
                    revision,
                    preedit,
                };
                lines.extend(
                    Notification::new(method::UPDATE_PREEDIT, params)
                        .ok()
                        .and_then(encode),
                );
            }
            if let Some(candidates) = result.candidates {
                let params = UpdateCandidatesParams {
                    session_id,
                    revision,
                    candidates,
                };
                lines.extend(
                    Notification::new(method::UPDATE_CANDIDATES, params)
                        .ok()
                        .and_then(encode),
                );
            }
        }
        lines
    }

    /// 共有状態に積まれた出来事を `notify` 通知の行にする
    fn notifications(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .runtime
            .drain_events()
            .into_iter()
            .filter_map(|e| {
                let params = NotifyParams {
                    level: e.level,
                    message: e.message,
                    code: Some(e.code.to_string()),
                };
                Notification::new(method::NOTIFY, params)
                    .ok()
                    .and_then(|n| serde_json::to_string(&n).ok())
            })
            .collect();
        lines.extend(
            self.runtime
                .drain_candidates_updates()
                .into_iter()
                .filter_map(|p| {
                    Notification::new(method::UPDATE_CANDIDATES, p)
                        .ok()
                        .and_then(|n| serde_json::to_string(&n).ok())
                }),
        );
        lines.extend(
            self.runtime
                .drain_preedit_updates()
                .into_iter()
                .filter_map(|p| {
                    Notification::new(method::UPDATE_PREEDIT, p)
                        .ok()
                        .and_then(|n| serde_json::to_string(&n).ok())
                }),
        );
        lines
    }

    /// 入力中（変換前）の候補を、デバウンス後にバックグラウンドで作り、`update_candidates` で届ける。
    /// 入力のたびに呼ぶ。古いスレッドは、実行時点の入力と読みが一致しなければ何もしない
    fn maybe_schedule_composing_candidates(&self, session_id: &str) {
        let Some(reading) = self.engine_lock().composing_candidate_reading(session_id) else {
            return;
        };
        let engine = self.engine.clone();
        let runtime = self.runtime.clone();
        let session_id = session_id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(COMPOSING_CANDIDATES_DELAY);
            let result = engine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .refresh_composing_candidates(&session_id, &reading);
            if let Some(r) = result
                && let Some(candidates) = r.candidates
            {
                runtime.push_candidates_update(UpdateCandidatesParams {
                    session_id,
                    revision: r.revision,
                    candidates,
                });
            }
        });
    }

    /// ライブ変換の対象なら、デバウンス後に `live_convert` を試みるスレッドを立てる。
    /// スレッドが増えても無害（`Engine::live_convert` が、実行時点の入力と reading が
    /// 一致しない限り何もしないため。古いスレッドをキャンセルする仕組みは不要）
    fn maybe_schedule_live_conversion(&self, session_id: &str) {
        let tunables = self.runtime.tunables();
        if !tunables.live_conversion_enabled {
            return;
        }
        let Some(reading) = self.engine_lock().composing_reading(session_id) else {
            return;
        };
        if reading.chars().count() < tunables.live_conversion_min_key_length {
            return;
        }
        let engine = self.engine.clone();
        let runtime = self.runtime.clone();
        let session_id = session_id.to_string();
        let delay = std::time::Duration::from_millis(tunables.live_conversion_delay_ms);
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let result = engine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .live_convert(&session_id, &reading);
            if let Some(r) = result
                && let Some(preedit) = r.preedit
            {
                runtime.push_preedit_update(UpdatePreeditParams {
                    session_id,
                    revision: r.revision,
                    preedit,
                });
            }
        });
    }

    /// 設定ファイル・ユーザー辞書の変更の確認、プロファイルの時間経過による復帰、学習の定期保存
    fn poll_changes(&mut self) {
        self.runtime.tick();
        self.runtime.flush_learning(false);
        if self.reload.changed(POLL_INTERVAL) {
            self.reload_all();
        }
    }

    /// 設定ファイルとユーザー辞書を読み直す。設定が壊れていれば旧設定のまま（`ok = false`）
    fn reload_all(&mut self) -> ReloadConfigResult {
        let mut errors = Vec::new();
        let mut ok = true;
        if let Some(path) = self.reload.config_path.clone() {
            match nagi_config::load_file(&path) {
                Ok((mut config, issues)) => {
                    if let Some(ms) = self.reload.cli_sync_budget_ms {
                        config.conversion.sync_budget_ms = ms;
                    }
                    let (new_config, keymap_events) = engine_config_from(&config);
                    self.engine_lock().update_config(new_config);
                    for e in keymap_events {
                        self.runtime.push_event(e);
                    }
                    self.runtime.apply_config(&config);
                    self.runtime.set_config_state(Some(path.clone()), false);
                    if config.conversion.model != self.reload.applied_model {
                        self.runtime.push_event(crate::runtime::Event {
                            level: nagi_protocol::methods::NotifyLevel::Info,
                            code: "restart_required",
                            message: "conversion.model の変更は次回起動時に反映されます"
                                .to_string(),
                        });
                    }
                    for e in issue_events(Some(&path), &issues) {
                        self.runtime.push_event(e);
                    }
                    errors.extend(issues.iter().map(|i| ConfigError {
                        line: i.line,
                        message: i.message.clone(),
                    }));
                }
                Err(issues) => {
                    ok = false;
                    self.runtime.set_config_state(Some(path.clone()), true);
                    for e in issue_events(Some(&path), &issues) {
                        self.runtime.push_event(e);
                    }
                    errors.extend(issues.iter().map(|i| ConfigError {
                        line: i.line,
                        message: i.message.clone(),
                    }));
                }
            }
        }
        if let (Some(dir), Some(dict)) =
            (self.reload.user_dict_dir.clone(), self.reload.dict.clone())
        {
            let (user, issues) = UserDictionary::load_dir(&dir);
            self.runtime.set_overlay(dict.make_overlay(&user));
            for e in user_dict_events(&issues) {
                self.runtime.push_event(e);
            }
        }
        self.reload.mark_read();
        ReloadConfigResult { ok, errors }
    }

    /// `engine` のロックを取る。ポイズン（他スレッドでのパニック中の取得）でも中身を使って継続する
    /// （IME サーバーは動き続けることを優先し、パニックで全体を落とさない方針）
    fn engine_lock(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn require_initialized(&self) -> Result<(), ErrorObject> {
        if self.initialized {
            Ok(())
        } else {
            Err(ErrorObject::new(
                error_code::NOT_INITIALIZED,
                "initialize を先に呼んでください",
            ))
        }
    }

    fn dispatch(&mut self, req: &Request) -> RpcResult {
        match req.method.as_str() {
            method::INITIALIZE => {
                let p: InitializeParams = params(req)?;
                if p.protocol_version != PROTOCOL_VERSION {
                    return Err(ErrorObject::new(
                        error_code::UNSUPPORTED_PROTOCOL_VERSION,
                        format!(
                            "未対応のプロトコルバージョンです（サーバー {PROTOCOL_VERSION}、クライアント {}）",
                            p.protocol_version
                        ),
                    ));
                }
                self.initialized = true;
                // ホストが申告した実行環境（任意）。プロファイル判定に反映する
                if let Some(caps) = &p.capabilities {
                    if let Some(method) = caps.input_method {
                        self.runtime.set_input_method(method);
                    }
                    if let Some(env) = &caps.environment {
                        self.runtime.set_environment(env);
                    }
                }
                to_value(InitializeResult {
                    protocol_version: PROTOCOL_VERSION,
                    server_name: "nagi-server".to_string(),
                    server_version: env!("CARGO_PKG_VERSION").to_string(),
                    profile: self.runtime.profile().as_str().to_string(),
                    mode: self.initial_mode,
                    capabilities: ServerCapabilities {
                        reconvert: true,
                        live_conversion: false,
                        style_conversion: false,
                    },
                })
            }
            method::PING => to_value(EmptyResult {}),
            method::SHUTDOWN => {
                self.shutdown = true;
                self.runtime.flush_learning(true);
                to_value(EmptyResult {})
            }
            method::PROCESS_KEY => {
                self.require_initialized()?;
                let p: ProcessKeyParams = params(req)?;
                let result = self
                    .engine_lock()
                    .process_key(&p.session_id, &p.key_event());
                self.maybe_schedule_live_conversion(&p.session_id);
                self.maybe_schedule_composing_candidates(&p.session_id);
                to_value(result)
            }
            method::INSERT_READING => {
                self.require_initialized()?;
                // 座標つき（上位互換）の型で受ける。座標を省略した `InsertReadingParams` も読める
                let p: InsertReadingTouchParams = params(req)?;
                let result = self.engine_lock().insert_reading_touch(
                    &p.session_id,
                    &p.input,
                    p.touch,
                    p.flick,
                );
                self.maybe_schedule_live_conversion(&p.session_id);
                self.maybe_schedule_composing_candidates(&p.session_id);
                to_value(result)
            }
            method::SET_LAYOUT_GEOMETRY => {
                self.require_initialized()?;
                let p: SetLayoutGeometryParams = params(req)?;
                self.engine_lock().set_layout_geometry(&p);
                to_value(EmptyResult {})
            }
            method::PERFORM_ACTION => {
                self.require_initialized()?;
                let p: PerformActionParams = params(req)?;
                let result = self.engine_lock().perform_action(&p.session_id, &p.action);
                self.maybe_schedule_live_conversion(&p.session_id);
                self.maybe_schedule_composing_candidates(&p.session_id);
                to_value(result)
            }
            method::FOCUS_IN => {
                self.require_initialized()?;
                let p: SessionParams = params(req)?;
                to_value(self.engine_lock().focus_in(&p.session_id))
            }
            method::FOCUS_OUT => {
                self.require_initialized()?;
                let p: SessionParams = params(req)?;
                let result = self.engine_lock().focus_out(&p.session_id);
                // フォーカス喪失時に学習を保存する（変更があるときだけ書かれる）
                self.runtime.flush_learning(true);
                to_value(result)
            }
            method::RESET => {
                self.require_initialized()?;
                let p: SessionParams = params(req)?;
                to_value(self.engine_lock().reset(&p.session_id))
            }
            method::RESTORE_SESSION => {
                self.require_initialized()?;
                let p: RestoreSessionParams = params(req)?;
                to_value(
                    self.engine_lock()
                        .restore_session(&p.session_id.clone(), &p),
                )
            }
            method::SET_CONTEXT => {
                self.require_initialized()?;
                let p: SetContextParams = params(req)?;
                let cleared = self.engine_lock().set_context(&p.session_id.clone(), &p);
                // 文脈が変わって入力中のサジェストを捨てたら、消えた差分を届けて新しい文脈で作り直す
                if let Some(r) = cleared
                    && let Some(candidates) = r.candidates
                {
                    self.runtime.push_candidates_update(UpdateCandidatesParams {
                        session_id: p.session_id.clone(),
                        revision: r.revision,
                        candidates,
                    });
                }
                self.maybe_schedule_composing_candidates(&p.session_id);
                to_value(EmptyResult {})
            }
            method::SET_ENVIRONMENT => {
                self.require_initialized()?;
                let p: SetEnvironmentParams = params(req)?;
                self.runtime.set_environment(&p);
                to_value(EmptyResult {})
            }
            method::GET_KEY_TABLE => {
                self.require_initialized()?;
                to_value({
                    let order = self.engine_lock().config().dakuten_order;
                    nagi_engine::touch::key_table(order)
                })
            }
            method::UNLOAD_MODEL => {
                self.require_initialized()?;
                self.runtime.unload_model();
                to_value(EmptyResult {})
            }
            method::RELOAD_MODEL => {
                self.require_initialized()?;
                self.runtime.reload_model();
                to_value(EmptyResult {})
            }
            method::GET_STATUS => {
                self.require_initialized()?;
                to_value(self.runtime.status())
            }
            method::CLEAR_LEARNING => {
                self.require_initialized()?;
                self.runtime.learn_clear();
                to_value(EmptyResult {})
            }
            method::GET_SETTINGS => {
                self.require_initialized()?;
                to_value(crate::settings::snapshot(
                    self.reload.config_path.as_deref(),
                ))
            }
            method::UPDATE_SETTINGS => {
                self.require_initialized()?;
                let p: UpdateSettingsParams = params(req)?;
                let outcome = crate::settings::apply(self.reload.config_path.as_deref(), p.changes);
                if !outcome.applied.is_empty() {
                    // 次のポーリングを待たず、既存のホットリロード経路で即座に反映する
                    self.reload_all();
                }
                to_value(UpdateSettingsResult {
                    applied: outcome.applied,
                    errors: outcome.errors,
                    snapshot: crate::settings::snapshot(self.reload.config_path.as_deref()),
                })
            }
            method::RELOAD_CONFIG => {
                self.require_initialized()?;
                to_value(self.reload_all())
            }
            method::SELECT_CANDIDATE => {
                self.require_initialized()?;
                let p: SelectCandidateParams = params(req)?;
                let result = self.engine_lock().select_candidate(&p.session_id, p.index);
                // 入力中の候補を選んだあとは、残った読みの候補を作り直す
                self.maybe_schedule_composing_candidates(&p.session_id);
                to_value(result)
            }
            method::RECONVERT => {
                self.require_initialized()?;
                let p: ReconvertParams = params(req)?;
                to_value(self.engine_lock().reconvert(&p.session_id, &p.text))
            }
            other => Err(ErrorObject::new(
                error_code::METHOD_NOT_FOUND,
                format!("未知のメソッド: {other}"),
            )),
        }
    }

    /// 入力を EOF または `shutdown` まで処理する。終了時に学習を保存する
    ///
    /// 標準入力を読む専用スレッドとメインループをチャンネルで結び、`Runtime::push_event`
    /// （設定再読み込み・キャリブレーション・ライブ変換等）と、バックエンドの非同期 LLM
    /// 完了通知（`sync_budget` 超過分。[`Self::async_notifications`]）の両方が、次のリクエスト
    /// を待たずに `notify` / `update_preedit` 等を配信できるようにする。
    pub fn run<R, W>(&mut self, input: R, output: &mut W) -> std::io::Result<()>
    where
        R: BufRead + Send + 'static,
        W: Write,
    {
        enum Msg {
            Line(String),
            Eof,
            IoErr(std::io::Error),
            Wake,
        }

        let (tx, rx) = std::sync::mpsc::channel::<Msg>();

        // 通知（Runtime のイベント・バックエンドの非同期完了）が積まれたら Wake を送る
        let tx_wake = tx.clone();
        self.set_wake_callback(Arc::new(move || {
            let _ = tx_wake.send(Msg::Wake);
        }));

        // 標準入力（等）を読む専用スレッド
        let tx_reader = tx.clone();
        let reader_handle = std::thread::Builder::new()
            .name("nagi-stdin-reader".into())
            .spawn(move || {
                let mut input = input;
                let mut line = String::new();
                loop {
                    line.clear();
                    match input.read_line(&mut line) {
                        Ok(0) => {
                            let _ = tx_reader.send(Msg::Eof);
                            break;
                        }
                        Ok(_) => {
                            if tx_reader
                                .send(Msg::Line(std::mem::take(&mut line)))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx_reader.send(Msg::IoErr(e));
                            break;
                        }
                    }
                }
            });

        let result = loop {
            match rx.recv() {
                Ok(Msg::Line(line)) => {
                    for out in self.handle_line(&line) {
                        output.write_all(out.as_bytes())?;
                        output.write_all(b"\n")?;
                    }
                    output.flush()?;
                    if self.shutdown {
                        break Ok(());
                    }
                }
                Ok(Msg::Wake) => {
                    // 初期化前は通知を出さない（handle_line と同じ方針）。溜めたままにする
                    let lines = self.poll_notifications();
                    for out in lines {
                        output.write_all(out.as_bytes())?;
                        output.write_all(b"\n")?;
                    }
                    output.flush()?;
                }
                Ok(Msg::Eof) => break Ok(()),
                Ok(Msg::IoErr(e)) => break Err(e),
                Err(_) => break Ok(()),
            }
        };

        self.runtime.flush_learning(true);
        // reader スレッドは detach してよい（プロセス終了時に回収される。shutdown 後に読み残しを
        // 処理する必要は無い）。ハンドルを明示的に drop して警告を避ける
        if let Ok(handle) = reader_handle {
            drop(handle);
        }
        result
    }
}
