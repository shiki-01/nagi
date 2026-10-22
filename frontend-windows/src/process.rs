//! 子プロセス（imserver）を stdin/stdout の JSON-RPC 通信路として扱う実装。

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::client::{ClientError, CloseHandler, LineBuffer, LineHandler, Transport};
use crate::supervisor::EngineLauncher;

/// 子プロセスの起動設定
#[derive(Debug, Clone)]
pub struct ProcessEngineConfiguration {
    /// imserver の実行ファイル
    pub executable: PathBuf,
    /// 常に渡す引数
    pub arguments: Vec<String>,
    /// minimal プロファイル（モデル起因のクラッシュループ回避）で起動するときに追加する引数
    pub minimal_extra_arguments: Vec<String>,
    /// 子プロセスの環境変数の追加分（None なら親を継承するだけ）
    pub environment: Option<Vec<(String, String)>>,
}

impl ProcessEngineConfiguration {
    /// 実行ファイルを指定して作る（minimal は `--no-llm` を追加）
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            arguments: Vec::new(),
            minimal_extra_arguments: vec!["--no-llm".into()],
            environment: None,
        }
    }
}

/// [`ProcessEngineConfiguration`] から子プロセスの通信路を作るランチャー
#[derive(Debug, Clone)]
pub struct ProcessEngineLauncher {
    configuration: ProcessEngineConfiguration,
}

impl ProcessEngineLauncher {
    /// 設定を受け取って作る
    pub fn new(configuration: ProcessEngineConfiguration) -> Self {
        Self { configuration }
    }
}

impl EngineLauncher for ProcessEngineLauncher {
    fn make_connection(&self, minimal: bool) -> Result<Box<dyn Transport>, ClientError> {
        let c = &self.configuration;
        let mut command = Command::new(&c.executable);
        command.args(&c.arguments);
        if minimal {
            command.args(&c.minimal_extra_arguments);
        }
        if let Some(env) = &c.environment {
            command.envs(env.iter().map(|(k, v)| (k, v)));
        }
        Ok(Box::new(ProcessTransport::new(command)))
    }
}

#[derive(Default)]
struct Handlers {
    on_line: Option<LineHandler>,
    on_close: Option<CloseHandler>,
    close_notified: bool,
}

struct Inner {
    command: Mutex<Option<Command>>,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    handlers: Mutex<Handlers>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    fn notify_close(&self) {
        let handler = {
            let mut h = lock(&self.handlers);
            if h.close_notified {
                return;
            }
            h.close_notified = true;
            h.on_close.clone()
        };
        if let Some(h) = handler {
            h();
        }
    }
}

/// 子プロセスの stdin/stdout を改行区切りの通信路にする [`Transport`]
pub struct ProcessTransport {
    inner: Arc<Inner>,
}

impl ProcessTransport {
    /// 起動前のコマンドを受け取る。stdin/stdout は [`start`](Transport::start) でパイプにする
    pub fn new(command: Command) -> Self {
        Self {
            inner: Arc::new(Inner {
                command: Mutex::new(Some(command)),
                child: Mutex::new(None),
                stdin: Mutex::new(None),
                handlers: Mutex::default(),
            }),
        }
    }
}

impl Transport for ProcessTransport {
    fn set_handlers(&self, on_line: LineHandler, on_close: CloseHandler) {
        let mut h = lock(&self.inner.handlers);
        h.on_line = Some(on_line);
        h.on_close = Some(on_close);
    }

    fn start(&self) -> Result<(), ClientError> {
        let mut command = lock(&self.inner.command)
            .take()
            .ok_or_else(|| ClientError::SendFailed("既に起動済みです".into()))?;
        // stderr は親から継承する（サーバーは入力内容をログに出さない方針）
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        platform::configure_command(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| ClientError::SendFailed(format!("spawn: {e}")))?;
        let (Some(stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(ClientError::SendFailed("パイプを取得できません".into()));
        };
        *lock(&self.inner.stdin) = Some(stdin);
        *lock(&self.inner.child) = Some(child);

        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let mut buffer = LineBuffer::new();
            let mut chunk = [0u8; 8192];
            loop {
                match stdout.read(&mut chunk) {
                    // EOF = 子プロセスの終了
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let handler = lock(&inner.handlers).on_line.clone();
                        for line in buffer.append(&chunk[..n]) {
                            if let Some(h) = &handler {
                                h(&line);
                            }
                        }
                    }
                }
            }
            inner.notify_close();
        });
        Ok(())
    }

    fn send(&self, line: &str) -> Result<(), ClientError> {
        let mut guard = lock(&self.inner.stdin);
        let stdin = guard.as_mut().ok_or(ClientError::ConnectionClosed)?;
        let mut payload = Vec::with_capacity(line.len() + 1);
        payload.extend_from_slice(line.as_bytes());
        payload.push(b'\n');
        stdin
            .write_all(&payload)
            .and_then(|()| stdin.flush())
            .map_err(|e| ClientError::SendFailed(e.to_string()))
    }

    fn terminate(&self) {
        // stdin を閉じてから終了させる（子が EOF で自発終了する場合に備える）
        drop(lock(&self.inner.stdin).take());
        if let Some(mut child) = lock(&self.inner.child).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.inner.notify_close();
    }
}

/// OS 依存の起動設定。Windows 固有の処理はここに隔離する
mod platform {
    use std::process::Command;

    /// Windows: エンジンのコンソールウィンドウを出さない（CREATE_NO_WINDOW）
    #[cfg(windows)]
    pub(super) fn configure_command(command: &mut Command) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    /// Windows 以外: 追加の設定は不要
    #[cfg(not(windows))]
    pub(super) fn configure_command(_command: &mut Command) {}
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::client::JsonRpcClient;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn echo_process_round_trips_lines_and_reports_close() {
        // cat は送った行をそのまま返すので、要求は「通知」として戻ってくる
        let transport = ProcessTransport::new(Command::new("cat"));
        let client = JsonRpcClient::new(Box::new(transport), Duration::from_millis(500));
        let (tx, rx) = mpsc::channel();
        client.set_on_notification(Arc::new(move |m, p| {
            let _ = tx.send((m.to_string(), p));
        }));
        let (ctx, crx) = mpsc::channel();
        client.set_on_close(Arc::new(move || {
            let _ = ctx.send(());
        }));
        client.start().unwrap();
        client
            .notify("ping", &serde_json::json!({"text": "あ"}))
            .unwrap();
        let (method, params) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(method, "ping");
        assert_eq!(params.unwrap()["text"], "あ");
        client.close();
        crx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn process_exit_is_reported_as_close() {
        let transport = ProcessTransport::new(Command::new("true"));
        let client = JsonRpcClient::new(Box::new(transport), Duration::from_millis(500));
        let (ctx, crx) = mpsc::channel();
        client.set_on_close(Arc::new(move || {
            let _ = ctx.send(());
        }));
        client.start().unwrap();
        crx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(client.is_closed());
    }

    #[test]
    fn missing_executable_fails_to_start() {
        let launcher = ProcessEngineLauncher::new(ProcessEngineConfiguration::new(
            "/nonexistent/nagi-imserver",
        ));
        let t = launcher.make_connection(false).unwrap();
        assert!(t.start().is_err());
    }
}
