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
    ConfigError, EmptyResult, InitializeParams, InitializeResult, NotifyParams, ProcessKeyParams,
    ReloadConfigResult, RestoreSessionParams, SelectCandidateParams, ServerCapabilities,
    SessionParams, SetContextParams, SetEnvironmentParams, method,
};
use nagi_protocol::view::InputMode;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Duration;

/// 設定ファイル・ユーザー辞書の変更を確認する最短の間隔
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// JSON-RPC サーバー
pub struct Server {
    engine: Engine,
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
            engine: Engine::new(backend, config),
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
            engine,
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
        }
        lines
    }

    fn encode(response: &Response) -> String {
        serde_json::to_string(response).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"内部エラー"}}"#
                .to_string()
        })
    }

    /// 共有状態に積まれた出来事を `notify` 通知の行にする
    fn notifications(&self) -> Vec<String> {
        self.runtime
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
            .collect()
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
                    self.engine.update_config(new_config);
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
                to_value(InitializeResult {
                    protocol_version: PROTOCOL_VERSION,
                    server_name: "nagi-server".to_string(),
                    server_version: env!("CARGO_PKG_VERSION").to_string(),
                    profile: self.runtime.profile().as_str().to_string(),
                    mode: self.initial_mode,
                    capabilities: ServerCapabilities {
                        reconvert: false,
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
                to_value(self.engine.process_key(&p.session_id, &p.key_event()))
            }
            method::FOCUS_IN => {
                self.require_initialized()?;
                let p: SessionParams = params(req)?;
                to_value(self.engine.focus_in(&p.session_id))
            }
            method::FOCUS_OUT => {
                self.require_initialized()?;
                let p: SessionParams = params(req)?;
                let result = self.engine.focus_out(&p.session_id);
                // フォーカス喪失時に学習を保存する（変更があるときだけ書かれる）
                self.runtime.flush_learning(true);
                to_value(result)
            }
            method::RESET => {
                self.require_initialized()?;
                let p: SessionParams = params(req)?;
                to_value(self.engine.reset(&p.session_id))
            }
            method::RESTORE_SESSION => {
                self.require_initialized()?;
                let p: RestoreSessionParams = params(req)?;
                to_value(self.engine.restore_session(&p.session_id.clone(), &p))
            }
            method::SET_CONTEXT => {
                self.require_initialized()?;
                let p: SetContextParams = params(req)?;
                self.engine.set_context(&p.session_id.clone(), &p);
                to_value(EmptyResult {})
            }
            method::SET_ENVIRONMENT => {
                self.require_initialized()?;
                let p: SetEnvironmentParams = params(req)?;
                self.runtime.set_environment(&p);
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
            method::RELOAD_CONFIG => {
                self.require_initialized()?;
                to_value(self.reload_all())
            }
            method::SELECT_CANDIDATE => {
                self.require_initialized()?;
                let p: SelectCandidateParams = params(req)?;
                to_value(self.engine.select_candidate(&p.session_id, p.index))
            }
            method::RECONVERT => {
                self.require_initialized()?;
                Err(ErrorObject::new(
                    error_code::NOT_IMPLEMENTED,
                    format!("{} は未実装です", req.method),
                ))
            }
            other => Err(ErrorObject::new(
                error_code::METHOD_NOT_FOUND,
                format!("未知のメソッド: {other}"),
            )),
        }
    }

    /// 入力を EOF または `shutdown` まで処理する。終了時に学習を保存する
    ///
    /// 標準入力を読む専用スレッドとメインループをチャンネルで結び、`Runtime::push_event` が
    /// 積まれた瞬間（次のリクエストを待たずに）`notify` 通知を配信できるようにする。
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

        // Runtime にイベントが積まれたら Wake を送る
        let tx_wake = tx.clone();
        self.runtime.set_event_hook(Box::new(move || {
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
                    if self.initialized {
                        for out in self.notifications() {
                            output.write_all(out.as_bytes())?;
                            output.write_all(b"\n")?;
                        }
                        output.flush()?;
                    }
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
