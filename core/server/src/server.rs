//! JSON-RPC サーバー本体。`handle_line` は 1 行の入力に対する出力行を返す純粋な関数に近い形で、
//! プロセスを起こさずにテストできる。

use crate::backend::DictBackend;
use nagi_dict::Dictionary;
use nagi_engine::{ConversionBackend, Engine, EngineConfig, KanaBackend};
use nagi_protocol::PROTOCOL_VERSION;
use nagi_protocol::jsonrpc::{ErrorObject, Request, Response, error_code, parse_message};
use nagi_protocol::methods::{
    EmptyResult, InitializeParams, InitializeResult, ProcessKeyParams, ReloadConfigResult,
    RestoreSessionParams, ServerCapabilities, SessionParams, SetContextParams, method,
};
use nagi_protocol::view::InputMode;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 起動オプション
#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// 取得済みデータ（`dict/nagi-dict.bin`、`models/*.gguf`）のディレクトリ。`--data-dir` または `NAGI_DATA_DIR`
    pub data_dir: Option<PathBuf>,
    /// LLM を使うか（`--no-llm` で無効化）。feature `llama` が無い、またはモデルが無ければ使えない
    pub use_llm: bool,
    /// LLM の推論を待つ最長時間（ミリ秒、`--sync-budget-ms`。設計書の `conversion.sync_budget_ms`）
    pub sync_budget_ms: u64,
}

impl Default for ServerOptions {
    fn default() -> Self {
        ServerOptions {
            data_dir: None,
            use_llm: true,
            sync_budget_ms: 150,
        }
    }
}

/// 既定の変換モデルのファイル名（`<data_dir>/models/` 下。`cargo xtask fetch-model` の出力）
pub const DEFAULT_MODEL_FILE: &str = "zenz-v3.1-small-Q5_K_M.gguf";

/// 変換バックエンドを用意する。戻り値は (バックエンド, プロファイル名, 診断メッセージ)。
///
/// 縮退の順: 辞書 + LLM（`balanced`）→ 辞書のみ（`minimal`）→ 読みをそのまま返す（`minimal`）。
/// どの段階でも入力は続けられる。診断メッセージに入力内容は含まない。
pub fn load_backend(options: &ServerOptions) -> (Arc<dyn ConversionBackend>, &'static str, String) {
    let Some(dir) = &options.data_dir else {
        return (
            Arc::new(KanaBackend),
            "minimal",
            "データディレクトリが未指定のため辞書なしで起動します".to_string(),
        );
    };
    let path = dir.join("dict").join("nagi-dict.bin");
    if !path.exists() {
        return (
            Arc::new(KanaBackend),
            "minimal",
            format!("辞書が見つかりません: {}", path.display()),
        );
    }
    let dict = match Dictionary::load(&path) {
        Ok(dict) => Arc::new(dict),
        Err(e) => {
            return (
                Arc::new(KanaBackend),
                "minimal",
                format!("辞書の読み込みに失敗したため辞書なしで起動します: {e}"),
            );
        }
    };
    let dict_only = |note: String| -> (Arc<dyn ConversionBackend>, &'static str, String) {
        (Arc::new(DictBackend::new(dict.clone())), "minimal", note)
    };
    if !options.use_llm {
        return dict_only("LLM は無効です（辞書のみで起動します）".to_string());
    }
    load_llm(options, dir, &dict).unwrap_or_else(dict_only)
}

#[cfg(feature = "llama")]
fn load_llm(
    options: &ServerOptions,
    dir: &Path,
    dict: &Arc<Dictionary>,
) -> Result<(Arc<dyn ConversionBackend>, &'static str, String), String> {
    use crate::llm_backend::{HybridBackend, LlmOptions};
    use nagi_infer::llama::{LlamaModel, LlamaOptions};
    let model_path = dir.join("models").join(DEFAULT_MODEL_FILE);
    if !model_path.exists() {
        return Err(format!(
            "変換モデルが見つかりません（辞書のみで起動します）: {}",
            model_path.display()
        ));
    }
    let model = LlamaModel::load(&model_path, LlamaOptions::default())
        .map_err(|e| format!("変換モデルの読み込みに失敗したため辞書のみで起動します: {e}"))?;
    let backend = HybridBackend::new(
        dict.clone(),
        Box::new(model),
        LlmOptions {
            sync_budget: std::time::Duration::from_millis(options.sync_budget_ms),
            ..LlmOptions::default()
        },
    );
    backend.warm_up();
    Ok((
        Arc::new(backend),
        "balanced",
        format!("辞書 + LLM で起動します: {}", model_path.display()),
    ))
}

#[cfg(not(feature = "llama"))]
fn load_llm(
    _options: &ServerOptions,
    _dir: &Path,
    _dict: &Arc<Dictionary>,
) -> Result<(Arc<dyn ConversionBackend>, &'static str, String), String> {
    Err("このビルドは LLM 推論（feature `llama`）を含みません（辞書のみで起動します）".to_string())
}

/// JSON-RPC サーバー
pub struct Server {
    engine: Engine,
    profile: String,
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
    /// サーバーを作る
    pub fn new(backend: Arc<dyn ConversionBackend>, profile: &str, config: EngineConfig) -> Self {
        Server {
            initial_mode: config.initial_mode,
            engine: Engine::new(backend, config),
            profile: profile.to_string(),
            initialized: false,
            shutdown: false,
        }
    }

    /// `shutdown` を受けたか
    pub fn should_shutdown(&self) -> bool {
        self.shutdown
    }

    /// 1 行の入力を処理し、出力する行（0 行以上）を返す
    pub fn handle_line(&mut self, line: &str) -> Vec<String> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let request = match parse_message(line) {
            Ok(r) => r,
            Err(e) => return vec![Self::encode(&Response::error(None, e))],
        };
        let id = request.id.clone();
        let outcome = self.dispatch(&request);
        match (id, outcome) {
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
        }
    }

    fn encode(response: &Response) -> String {
        serde_json::to_string(response).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"内部エラー"}}"#
                .to_string()
        })
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
                    profile: self.profile.clone(),
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
                to_value(self.engine.focus_out(&p.session_id))
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
            method::RELOAD_CONFIG => {
                self.require_initialized()?;
                // 設定ファイルの読み込みは未実装（設計書 §6）。現時点では常に成功として扱う
                to_value(ReloadConfigResult {
                    ok: true,
                    errors: Vec::new(),
                })
            }
            method::SELECT_CANDIDATE | method::RECONVERT => {
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

    /// 入力を EOF または `shutdown` まで処理する
    pub fn run<R: BufRead, W: Write>(
        &mut self,
        mut input: R,
        output: &mut W,
    ) -> std::io::Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            if input.read_line(&mut line)? == 0 {
                return Ok(());
            }
            for out in self.handle_line(&line) {
                output.write_all(out.as_bytes())?;
                output.write_all(b"\n")?;
            }
            output.flush()?;
            if self.shutdown {
                return Ok(());
            }
        }
    }
}

/// 引数から起動オプションを作る（`--data-dir <path>`、環境変数 `NAGI_DATA_DIR`）
pub fn parse_args<I: Iterator<Item = String>>(mut args: I) -> Result<ServerOptions, String> {
    let mut options = ServerOptions {
        data_dir: std::env::var_os("NAGI_DATA_DIR").map(PathBuf::from),
        ..ServerOptions::default()
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                let value = args.next().ok_or("--data-dir に値がありません")?;
                options.data_dir = Some(Path::new(&value).to_path_buf());
            }
            "--no-llm" => options.use_llm = false,
            "--sync-budget-ms" => {
                let value = args.next().ok_or("--sync-budget-ms に値がありません")?;
                options.sync_budget_ms = value
                    .parse()
                    .map_err(|_| format!("--sync-budget-ms が数値ではありません: {value}"))?;
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    Ok(options)
}
