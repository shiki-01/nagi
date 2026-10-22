//! JSON-RPC 2.0 クライアント（改行区切り）。通信路は [`Transport`] で抽象化する。
//!
//! - 要求は連番 ID で管理し、応答を ID で対応付ける
//! - 通知（サーバー→フロント）は `on_notification` に生の params で渡す
//! - 通信路が閉じたら未応答の要求はすべて [`ClientError::ConnectionClosed`] で失敗する

use std::collections::HashMap;
use std::fmt;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// クライアントが返すエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// エンジンと接続していない（起動前・再起動待ち）
    NotConnected,
    /// 通信路が閉じた（エンジンのクラッシュ等）。未応答の要求はこれで失敗する
    ConnectionClosed,
    /// 応答が制限時間内に来なかった
    Timeout,
    /// サーバーが JSON-RPC エラーを返した
    Rpc {
        /// エラーコード（`nagi_protocol::jsonrpc::error_code`）
        code: i32,
        /// エラーメッセージ
        message: String,
    },
    /// 応答の形式が不正
    InvalidResponse(String),
    /// 送信・エンコードに失敗
    SendFailed(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "エンジンと接続していません"),
            Self::ConnectionClosed => write!(f, "エンジンとの通信路が閉じました"),
            Self::Timeout => write!(f, "エンジンの応答がタイムアウトしました"),
            Self::Rpc { code, message } => {
                write!(f, "エンジンがエラーを返しました ({code}): {message}")
            }
            Self::InvalidResponse(s) => write!(f, "応答の形式が不正です: {s}"),
            Self::SendFailed(s) => write!(f, "送信に失敗しました: {s}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// 受信行のハンドラ（改行を除く 1 行）。任意のスレッドから呼ばれうる
pub type LineHandler = Arc<dyn Fn(&str) + Send + Sync>;
/// 通信路が閉じたときのハンドラ。高々 1 回
pub type CloseHandler = Arc<dyn Fn() + Send + Sync>;
/// サーバー通知のハンドラ `(method, params)`。任意のスレッドから呼ばれうる
pub type NotificationHandler = Arc<dyn Fn(&str, Option<Value>) + Send + Sync>;

/// エンジンとの通信路（1 接続 = 1 プロセス）。テストではモックに差し替える。
///
/// 1 メッセージ = 改行を含まない 1 行の JSON。改行の付与・分割は実装側の責務。
pub trait Transport: Send + Sync {
    /// 受信・終了のハンドラを設定する（[`start`](Self::start) より前に呼ばれる）
    fn set_handlers(&self, on_line: LineHandler, on_close: CloseHandler);
    /// 通信を開始する（プロセスの場合は起動）
    fn start(&self) -> Result<(), ClientError>;
    /// 1 メッセージを送る（改行なしの JSON）
    fn send(&self, line: &str) -> Result<(), ClientError>;
    /// 通信路を閉じ、プロセスなら終了させる。呼び出し後の `on_close` は呼んでも呼ばなくてもよい
    fn terminate(&self);
}

/// 受信バイト列を改行区切りの行に分割する。チャンク境界をまたぐ行・複数行を扱う
#[derive(Debug, Default)]
pub struct LineBuffer {
    buffer: Vec<u8>,
}

impl LineBuffer {
    /// 空のバッファを作る
    pub fn new() -> Self {
        Self::default()
    }

    /// バイト列を追加し、完成した行（改行・末尾の CR を除く・空行は捨てる）を返す。
    /// 0x0A は UTF-8 の継続バイトに現れないため、バイト単位で分割してよい
    pub fn append(&mut self, data: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(data);
        let mut lines = Vec::new();
        while let Some(idx) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=idx).collect();
            line.pop(); // '\n'
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if !line.is_empty() {
                lines.push(String::from_utf8_lossy(&line).into_owned());
            }
        }
        lines
    }
}

type Completion = Sender<Result<Value, ClientError>>;

#[derive(Default)]
struct State {
    next_id: i64,
    pending: HashMap<i64, Completion>,
    closed: bool,
    on_notification: Option<NotificationHandler>,
    on_close: Option<CloseHandler>,
}

struct Inner {
    transport: Box<dyn Transport>,
    state: Mutex<State>,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// pending から取り出して一度だけ完了させる（応答・タイムアウト・切断の競合対策）
    fn complete(&self, id: i64, result: Result<Value, ClientError>) {
        let completion = self.state().pending.remove(&id);
        if let Some(tx) = completion {
            let _ = tx.send(result);
        }
    }

    fn handle_line(&self, line: &str) {
        let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(line) else {
            return; // 壊れた行は捨てる（後続の行は処理する）
        };

        if let Some(method) = obj.get("method").and_then(Value::as_str) {
            // サーバー通知（id なし）。サーバー→フロントの要求は現状存在しない
            let params = obj.get("params").filter(|p| !p.is_null()).cloned();
            let handler = self.state().on_notification.clone();
            if let Some(h) = handler {
                h(method, params);
            }
            return;
        }

        let Some(id) = obj.get("id").and_then(Value::as_i64) else {
            return;
        };
        if let Some(err) = obj.get("error").filter(|e| e.is_object()) {
            let code = err
                .get("code")
                .and_then(Value::as_i64)
                .and_then(|c| i32::try_from(c).ok())
                .unwrap_or(nagi_protocol::jsonrpc::error_code::INTERNAL_ERROR);
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            self.complete(id, Err(ClientError::Rpc { code, message }));
        } else if let Some(result) = obj.get("result") {
            self.complete(id, Ok(result.clone()));
        } else {
            self.complete(
                id,
                Err(ClientError::InvalidResponse(
                    "result も error も無い応答".into(),
                )),
            );
        }
    }

    fn handle_close(&self) {
        let (waiting, handler) = {
            let mut st = self.state();
            if st.closed {
                return;
            }
            st.closed = true;
            (std::mem::take(&mut st.pending), st.on_close.clone())
        };
        for (_, tx) in waiting {
            let _ = tx.send(Err(ClientError::ConnectionClosed));
        }
        if let Some(h) = handler {
            h();
        }
    }
}

/// JSON-RPC 2.0 クライアント。同期（ブロッキング）で、どのスレッドからでも呼べる
pub struct JsonRpcClient {
    inner: Arc<Inner>,
    /// 要求ごとの既定タイムアウト。設計書 §3.3: 応答が 500ms 無ければ異常
    pub default_timeout: Duration,
}

impl JsonRpcClient {
    /// 通信路を受け取ってクライアントを作る（[`start`](Self::start) まで通信しない）
    pub fn new(transport: Box<dyn Transport>, default_timeout: Duration) -> Self {
        let inner = Arc::new(Inner {
            transport,
            state: Mutex::new(State {
                next_id: 1,
                ..State::default()
            }),
        });
        let on_line: LineHandler = {
            let weak: Weak<Inner> = Arc::downgrade(&inner);
            Arc::new(move |line| {
                if let Some(i) = weak.upgrade() {
                    i.handle_line(line);
                }
            })
        };
        let on_close: CloseHandler = {
            let weak: Weak<Inner> = Arc::downgrade(&inner);
            Arc::new(move || {
                if let Some(i) = weak.upgrade() {
                    i.handle_close();
                }
            })
        };
        inner.transport.set_handlers(on_line, on_close);
        Self {
            inner,
            default_timeout,
        }
    }

    /// サーバー通知のハンドラを設定する
    pub fn set_on_notification(&self, handler: NotificationHandler) {
        self.inner.state().on_notification = Some(handler);
    }

    /// 通信路が閉じたときのハンドラを設定する（高々 1 回呼ばれる）
    pub fn set_on_close(&self, handler: CloseHandler) {
        self.inner.state().on_close = Some(handler);
    }

    /// 通信路が閉じ済みか
    pub fn is_closed(&self) -> bool {
        self.inner.state().closed
    }

    /// 通信を開始する（プロセス起動）
    pub fn start(&self) -> Result<(), ClientError> {
        self.inner.transport.start()
    }

    /// 通信路を閉じる。未応答の要求は [`ClientError::ConnectionClosed`] で失敗する
    pub fn close(&self) {
        self.inner.transport.terminate();
        self.inner.handle_close();
    }

    /// 要求を送り、応答の result を `R` にデコードして返す
    pub fn request<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
        timeout: Option<Duration>,
    ) -> Result<R, ClientError> {
        let body = encode_params(params)?;
        let limit = timeout.unwrap_or(self.default_timeout);
        let (tx, rx) = mpsc::channel();
        let id = {
            let mut st = self.inner.state();
            if st.closed {
                return Err(ClientError::ConnectionClosed);
            }
            let id = st.next_id;
            st.next_id += 1;
            st.pending.insert(id, tx);
            id
        };

        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": body});
        // 送信中に応答が同期的に届いても受け取れるよう、pending 登録後・ロック外で送る
        if let Err(e) = self.inner.transport.send(&message.to_string()) {
            self.inner.state().pending.remove(&id);
            return Err(e);
        }

        let value = match rx.recv_timeout(limit) {
            Ok(r) => r?,
            Err(RecvTimeoutError::Timeout) => {
                self.inner.state().pending.remove(&id);
                // タイムアウト直後に応答が滑り込んだ場合は採用する
                match rx.try_recv() {
                    Ok(r) => r?,
                    Err(_) => return Err(ClientError::Timeout),
                }
            }
            Err(RecvTimeoutError::Disconnected) => return Err(ClientError::ConnectionClosed),
        };
        serde_json::from_value(value)
            .map_err(|e| ClientError::InvalidResponse(format!("{method}: {e}")))
    }

    /// 通知（応答なし）を送る
    pub fn notify<P: Serialize>(&self, method: &str, params: &P) -> Result<(), ClientError> {
        let body = encode_params(params)?;
        let message = json!({"jsonrpc": "2.0", "method": method, "params": body});
        self.inner.transport.send(&message.to_string())
    }
}

fn encode_params<P: Serialize>(params: &P) -> Result<Value, ClientError> {
    serde_json::to_value(params).map_err(|e| ClientError::SendFailed(format!("encode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{MockTransport, response};
    use nagi_protocol::EmptyResult;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn client(t: &MockTransport) -> JsonRpcClient {
        JsonRpcClient::new(Box::new(t.clone()), Duration::from_millis(500))
    }

    #[test]
    fn request_and_response_are_correlated_by_id() {
        let t = MockTransport::new();
        // 応答の id で要求と対応付く
        t.set_responder(|req| {
            let id = req["id"].as_i64()?;
            Some(vec![response(id, &format!("{{\"n\":{id}}}"))])
        });
        let c = client(&t);
        c.start().unwrap();
        let a: Value = c.request("ping", &json!({}), None).unwrap();
        let b: Value = c.request("ping", &json!({}), None).unwrap();
        assert_eq!(a["n"], 1);
        assert_eq!(b["n"], 2);
    }

    #[test]
    fn request_encodes_jsonrpc_envelope() {
        let t = MockTransport::new();
        t.set_responder(|req| Some(vec![response(req["id"].as_i64()?, "{}")]));
        let c = client(&t);
        let _: EmptyResult = c.request("get_status", &json!({"a": 1}), None).unwrap();
        let sent = t.sent();
        assert_eq!(sent[0]["jsonrpc"], "2.0");
        assert_eq!(sent[0]["id"], 1);
        assert_eq!(sent[0]["method"], "get_status");
        assert_eq!(sent[0]["params"]["a"], 1);
        // 1 メッセージ = 1 行
        assert!(!t.raw_sent()[0].contains('\n'));
    }

    #[test]
    fn error_response_becomes_rpc_error() {
        let t = MockTransport::new();
        t.set_responder(|req| {
            let id = req["id"].as_i64()?;
            Some(vec![format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":-32001,\"message\":\"no\"}}}}"
            )])
        });
        let c = client(&t);
        let r: Result<EmptyResult, _> = c.request("focus_in", &json!({}), None);
        assert_eq!(
            r.unwrap_err(),
            ClientError::Rpc {
                code: -32001,
                message: "no".into()
            }
        );
    }

    #[test]
    fn invalid_result_shape_is_invalid_response() {
        let t = MockTransport::new();
        t.set_responder(|req| Some(vec![response(req["id"].as_i64()?, "\"str\"")]));
        let c = client(&t);
        let r: Result<EmptyResult, _> = c.request("ping", &json!({}), None);
        assert!(matches!(r, Err(ClientError::InvalidResponse(_))));
    }

    #[test]
    fn timeout_without_response() {
        let t = MockTransport::new();
        let c = client(&t);
        let r: Result<EmptyResult, _> =
            c.request("ping", &json!({}), Some(Duration::from_millis(20)));
        assert_eq!(r.unwrap_err(), ClientError::Timeout);
        // 遅れて届いた応答は無視される（pending から消えている）
        t.receive(&response(1, "{}"));
    }

    #[test]
    fn close_fails_pending_and_fires_on_close_once() {
        let t = MockTransport::new();
        let c = Arc::new(client(&t));
        let fired = Arc::new(AtomicUsize::new(0));
        let f = fired.clone();
        c.set_on_close(Arc::new(move || {
            f.fetch_add(1, Ordering::SeqCst);
        }));
        let c2 = c.clone();
        let handle = std::thread::spawn(move || {
            c2.request::<_, EmptyResult>("ping", &json!({}), Some(Duration::from_secs(5)))
        });
        while t.sent().is_empty() {
            std::thread::yield_now();
        }
        t.simulate_close();
        assert_eq!(
            handle.join().unwrap().unwrap_err(),
            ClientError::ConnectionClosed
        );
        c.close(); // 二重 close でも on_close は 1 回
        assert_eq!(fired.load(Ordering::SeqCst), 1);
        assert!(c.is_closed());
        // 閉じた後の要求は即失敗
        let r: Result<EmptyResult, _> = c.request("ping", &json!({}), None);
        assert_eq!(r.unwrap_err(), ClientError::ConnectionClosed);
    }

    #[test]
    fn notification_handler_receives_method_and_params() {
        let t = MockTransport::new();
        let c = client(&t);
        type Got = Arc<Mutex<Vec<(String, Option<Value>)>>>;
        let got: Got = Arc::default();
        let g = got.clone();
        c.set_on_notification(Arc::new(move |m, p| {
            g.lock().unwrap().push((m.to_string(), p));
        }));
        t.receive(
            r#"{"jsonrpc":"2.0","method":"notify","params":{"level":"info","message":"hi"}}"#,
        );
        t.receive(r#"{"jsonrpc":"2.0","method":"ping"}"#);
        t.receive("not json"); // 壊れた行は捨てて後続を処理する
        t.receive(r#"{"jsonrpc":"2.0","method":"notify","params":null}"#);
        let got = got.lock().unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].0, "notify");
        assert_eq!(got[0].1.as_ref().unwrap()["message"], "hi");
        assert_eq!(got[1].1, None);
        assert_eq!(got[2].1, None);
    }

    #[test]
    fn notify_sends_message_without_id() {
        let t = MockTransport::new();
        let c = client(&t);
        c.notify("set_context", &json!({"session_id": "s1"}))
            .unwrap();
        let sent = t.sent();
        assert_eq!(sent[0]["method"], "set_context");
        assert!(sent[0].get("id").is_none());
        assert_eq!(sent[0]["params"]["session_id"], "s1");
    }

    #[test]
    fn send_failure_is_reported_and_pending_is_cleaned() {
        let t = MockTransport::new();
        t.set_fail_send(true);
        let c = client(&t);
        let r: Result<EmptyResult, _> = c.request("ping", &json!({}), None);
        assert!(matches!(r, Err(ClientError::SendFailed(_))));
    }

    #[test]
    fn line_buffer_splits_across_chunks() {
        let mut b = LineBuffer::new();
        assert!(b.append(b"{\"a\":").is_empty());
        assert_eq!(
            b.append(b"1}\n{\"b\":2}\r\n\n{\"c\""),
            vec!["{\"a\":1}", "{\"b\":2}"]
        );
        assert_eq!(b.append(b":3}\n"), vec!["{\"c\":3}"]);
        // マルチバイト文字の途中で分割されても壊れない
        let bytes = "{\"s\":\"あ\"}\n".as_bytes();
        assert!(b.append(&bytes[..7]).is_empty());
        assert_eq!(b.append(&bytes[7..]), vec!["{\"s\":\"あ\"}"]);
    }
}
