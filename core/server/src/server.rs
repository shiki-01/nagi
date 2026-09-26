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
#[derive(Debug, Clone, Default)]
pub struct ServerOptions {
    /// 取得済みデータ（`dict/nagi-dict.bin` など）のディレクトリ。`--data-dir` または `NAGI_DATA_DIR`
    pub data_dir: Option<PathBuf>,
}

/// 変換バックエンドを用意する。辞書があれば辞書 + Viterbi、無い・壊れている場合は読みをそのまま
/// 返すバックエンドで起動する（入力は常に続けられる）。戻り値の 2 つ目はプロファイル名、
/// 3 つ目は診断メッセージ（入力内容を含まない）。
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
    match Dictionary::load(&path) {
        Ok(dict) => (
            Arc::new(DictBackend::new(Arc::new(dict))),
            "minimal",
            format!("辞書を読み込みました: {}", path.display()),
        ),
        Err(e) => (
            Arc::new(KanaBackend),
            "minimal",
            format!("辞書の読み込みに失敗したため辞書なしで起動します: {e}"),
        ),
    }
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
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                let value = args.next().ok_or("--data-dir に値がありません")?;
                options.data_dir = Some(Path::new(&value).to_path_buf());
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    Ok(options)
}
