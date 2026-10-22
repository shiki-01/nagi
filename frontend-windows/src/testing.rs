//! テスト用のモック（通信路・ランチャー・スケジューラ・時計）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::client::{ClientError, CloseHandler, LineHandler, Transport};
use crate::supervisor::{Clock, EngineLauncher, Scheduler};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 受信した JSON オブジェクトに対する応答行を返す関数（None なら無応答）
pub type Responder = Arc<dyn Fn(&Value) -> Option<Vec<String>> + Send + Sync>;

#[derive(Default)]
struct MockState {
    on_line: Option<LineHandler>,
    on_close: Option<CloseHandler>,
    sent: Vec<String>,
    responder: Option<Responder>,
    closed: bool,
}

/// 送られた要求を記録し、`responder` があれば同期的に応答する通信路。
/// clone はハンドル（同じ状態を共有する）
#[derive(Clone, Default)]
pub struct MockTransport {
    state: Arc<Mutex<MockState>>,
    fail_send: Arc<AtomicBool>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_responder(&self, f: impl Fn(&Value) -> Option<Vec<String>> + Send + Sync + 'static) {
        lock(&self.state).responder = Some(Arc::new(f));
    }

    pub fn set_fail_send(&self, v: bool) {
        self.fail_send.store(v, Ordering::SeqCst);
    }

    pub fn raw_sent(&self) -> Vec<String> {
        lock(&self.state).sent.clone()
    }

    pub fn sent(&self) -> Vec<Value> {
        self.raw_sent()
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }

    pub fn sent_methods(&self) -> Vec<String> {
        self.sent()
            .iter()
            .filter_map(|v| v["method"].as_str().map(str::to_string))
            .collect()
    }

    /// サーバーからの 1 行を注入する
    pub fn receive(&self, line: &str) {
        let h = lock(&self.state).on_line.clone();
        if let Some(h) = h {
            h(line);
        }
    }

    /// プロセス終了（EOF）を模擬する
    pub fn simulate_close(&self) {
        let h = {
            let mut st = lock(&self.state);
            if st.closed {
                return;
            }
            st.closed = true;
            st.on_close.clone()
        };
        if let Some(h) = h {
            h();
        }
    }
}

impl Transport for MockTransport {
    fn set_handlers(&self, on_line: LineHandler, on_close: CloseHandler) {
        let mut st = lock(&self.state);
        st.on_line = Some(on_line);
        st.on_close = Some(on_close);
    }

    fn start(&self) -> Result<(), ClientError> {
        Ok(())
    }

    fn send(&self, line: &str) -> Result<(), ClientError> {
        if self.fail_send.load(Ordering::SeqCst) {
            return Err(ClientError::SendFailed("mock".into()));
        }
        let responder = {
            let mut st = lock(&self.state);
            st.sent.push(line.to_string());
            st.responder.clone()
        };
        let value: Value = serde_json::from_str(line).unwrap();
        if let Some(lines) = responder.and_then(|r| r(&value)) {
            for l in lines {
                self.receive(&l);
            }
        }
        Ok(())
    }

    fn terminate(&self) {
        self.simulate_close();
    }
}

/// 接続ごとの初期設定（起動回数・minimal 指定・接続）
pub type Configure = Box<dyn Fn(usize, bool, &MockTransport) + Send + Sync>;

/// 起動のたびに新しい [`MockTransport`] を作るランチャー
pub struct MockLauncher {
    connections: Mutex<Vec<MockTransport>>,
    minimal_flags: Mutex<Vec<bool>>,
    configure: Mutex<Configure>,
}

impl MockLauncher {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            connections: Mutex::default(),
            minimal_flags: Mutex::default(),
            configure: Mutex::new(Box::new(|_, _, c| c.set_responder(standard_responder(0)))),
        })
    }

    pub fn set_configure(&self, f: impl Fn(usize, bool, &MockTransport) + Send + Sync + 'static) {
        *lock(&self.configure) = Box::new(f);
    }

    pub fn connections(&self) -> Vec<MockTransport> {
        lock(&self.connections).clone()
    }

    pub fn minimal_flags(&self) -> Vec<bool> {
        lock(&self.minimal_flags).clone()
    }
}

impl EngineLauncher for MockLauncher {
    fn make_connection(&self, minimal: bool) -> Result<Box<dyn Transport>, ClientError> {
        let c = MockTransport::new();
        let n = {
            let mut conns = lock(&self.connections);
            conns.push(c.clone());
            lock(&self.minimal_flags).push(minimal);
            conns.len()
        };
        (lock(&self.configure))(n, minimal, &c);
        Ok(Box::new(c))
    }
}

/// 待たずに即実行し、遅延を記録するスケジューラ
#[derive(Default)]
pub struct ImmediateScheduler {
    delays: Mutex<Vec<Duration>>,
}

impl ImmediateScheduler {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    pub fn delays(&self) -> Vec<Duration> {
        lock(&self.delays).clone()
    }
}

impl Scheduler for ImmediateScheduler {
    fn schedule(&self, delay: Duration, work: Box<dyn FnOnce() + Send>) {
        lock(&self.delays).push(delay);
        work();
    }
}

/// 実行を保留するスケジューラ（再起動待ちの状態を保つ）
pub struct HoldScheduler;

impl Scheduler for HoldScheduler {
    fn schedule(&self, _delay: Duration, _work: Box<dyn FnOnce() + Send>) {}
}

/// 手動で進める時計
pub struct MockClock {
    base: Instant,
    offset: Mutex<Duration>,
}

impl MockClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            base: Instant::now(),
            offset: Mutex::default(),
        })
    }

    pub fn advance(&self, d: Duration) {
        *lock(&self.offset) += d;
    }
}

impl Clock for MockClock {
    fn now(&self) -> Instant {
        self.base + *lock(&self.offset)
    }
}

// JSON 応答の組み立て

pub const INITIALIZE_RESULT: &str = r#"{"protocol_version":1,"server_name":"nagi-engine","server_version":"0.0.0","profile":"balanced","mode":"hiragana","capabilities":{"reconvert":true,"live_conversion":false,"style_conversion":false}}"#;

pub fn response(id: i64, result_json: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result_json}}}")
}

pub fn process_key_result(revision: u32) -> String {
    format!("{{\"consumed\":true,\"revision\":{revision}}}")
}

/// initialize / ping / shutdown 等に標準的に応答し、それ以外は描画差分で応答する
pub fn standard_responder(
    revision: u32,
) -> impl Fn(&Value) -> Option<Vec<String>> + Send + Sync + 'static {
    move |req| {
        let id = req["id"].as_i64()?;
        let method = req["method"].as_str()?;
        let body = match method {
            "initialize" => INITIALIZE_RESULT.to_string(),
            "ping" | "shutdown" | "clear_learning" => "{}".to_string(),
            _ => process_key_result(revision),
        };
        Some(vec![response(id, &body)])
    }
}
