//! 子プロセス（imserver）を stdin/stdout の JSON-RPC 通信路として扱う実装。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::client::{ClientError, CloseHandler, ExitReport, LineBuffer, LineHandler, Transport};
use crate::engine_log::{self, LogEntry};
use crate::supervisor::EngineLauncher;

/// EOF の後、終了コードが取れるまで待つ上限
const EXIT_WAIT: Duration = Duration::from_millis(1000);
const EXIT_POLL: Duration = Duration::from_millis(10);

/// imserver の stderr の保存先
#[derive(Debug, Clone)]
pub struct StderrLog {
    /// ログのディレクトリ（`%LOCALAPPDATA%\Nagi\logs`）
    pub dir: PathBuf,
    /// ホストプロセスの pid（ファイル名 `imserver-<pid>.log` になる）
    pub host_pid: u32,
}

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
    /// 子プロセスの stderr の保存先。`None`、またはファイルを開けないときは捨てる（`Stdio::null()`）。
    /// 親の stderr は継承しない（GUI ホストでは壊れていることがあるため）
    pub stderr_log: Option<StderrLog>,
}

impl ProcessEngineConfiguration {
    /// 実行ファイルを指定して作る（minimal は `--no-llm` を追加）
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            arguments: Vec::new(),
            minimal_extra_arguments: vec!["--no-llm".into()],
            environment: None,
            stderr_log: None,
        }
    }
}

/// [`ProcessEngineConfiguration`] から子プロセスの通信路を作るランチャー
#[derive(Debug, Clone)]
pub struct ProcessEngineLauncher {
    configuration: ProcessEngineConfiguration,
    /// 古いログの掃除を済ませたか（ホストごとに 1 回）
    cleaned: Arc<AtomicBool>,
}

impl ProcessEngineLauncher {
    /// 設定を受け取って作る
    pub fn new(configuration: ProcessEngineConfiguration) -> Self {
        Self {
            configuration,
            cleaned: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 古い `imserver-*.log` を消す（失敗は無視）
fn clean_old_logs(log: &StderrLog) {
    let Ok(read) = fs::read_dir(&log.dir) else {
        return;
    };
    let entries: Vec<LogEntry> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let modified = e.metadata().ok()?.modified().ok()?;
            let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some(LogEntry {
                name,
                modified_unix_secs: secs,
            })
        })
        .collect();
    let current = engine_log::file_name(log.host_pid);
    for name in engine_log::files_to_delete(
        &entries,
        &current,
        unix_now(),
        engine_log::KEEP_FILES,
        engine_log::MAX_AGE_SECS,
    ) {
        let _ = fs::remove_file(log.dir.join(name));
    }
}

/// stderr の保存先を開く（追記。上限超えなら作り直す）。開けなければ `None`
fn open_stderr_file(log: &StderrLog, minimal: bool) -> Option<File> {
    fs::create_dir_all(&log.dir).ok()?;
    let path: PathBuf = log.dir.join(engine_log::file_name(log.host_pid));
    let recreate = fs::metadata(&path).is_ok_and(|m| engine_log::should_recreate(m.len()));
    let mut options = OpenOptions::new();
    options.create(true);
    if recreate {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    let mut file = options.open(&path).ok()?;
    let _ = file.write_all(engine_log::launch_header(unix_now(), log.host_pid, minimal).as_bytes());
    Some(file)
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
        let stderr = c.stderr_log.as_ref().and_then(|log| {
            if !self.cleaned.swap(true, Ordering::SeqCst) {
                clean_old_logs(log);
            }
            open_stderr_file(log, minimal)
        });
        Ok(Box::new(ProcessTransport::with_stderr(command, stderr)))
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
    /// stderr の保存先（`None` なら捨てる）
    stderr: Mutex<Option<File>>,
    started_at: Mutex<Option<Instant>>,
    /// 自発的な終了の記録（`notify_close` より前に確定する）
    exit: Mutex<Option<ExitReport>>,
    /// こちらから終了させている（終了コードを記録しない）
    terminating: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    /// 通信路が EOF になった子プロセスの終了コードと生存時間を記録する（こちらから終了させた場合を除く）
    fn record_exit(&self) {
        let deadline = Instant::now() + EXIT_WAIT;
        loop {
            if self.terminating.load(Ordering::SeqCst) {
                return;
            }
            let status = match lock(&self.child).as_mut() {
                Some(child) => child.try_wait(),
                None => return,
            };
            match status {
                Ok(Some(status)) => {
                    let lifetime = lock(&self.started_at).map_or(Duration::ZERO, |t| t.elapsed());
                    *lock(&self.exit) = Some(ExitReport {
                        code: status.code(),
                        lifetime,
                    });
                    return;
                }
                // stdout だけ閉じて生きている・取得に失敗: 待ち切ったら諦める
                Ok(None) if Instant::now() < deadline => std::thread::sleep(EXIT_POLL),
                Ok(None) | Err(_) => return,
            }
        }
    }

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
        Self::with_stderr(command, None)
    }

    /// stderr の保存先つきで作る。`None` なら子の stderr は捨てる（継承しない）
    pub fn with_stderr(command: Command, stderr: Option<File>) -> Self {
        Self {
            inner: Arc::new(Inner {
                command: Mutex::new(Some(command)),
                child: Mutex::new(None),
                stdin: Mutex::new(None),
                handlers: Mutex::default(),
                stderr: Mutex::new(stderr),
                started_at: Mutex::new(None),
                exit: Mutex::new(None),
                terminating: AtomicBool::new(false),
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
        // stderr は継承しない（GUI ホストの stderr は壊れていることがあり、子の最初のログ出力で
        // 落ちうる）。保存先のファイルへ、無ければ捨てる。サーバーは入力内容をログに出さない方針
        let stderr = lock(&self.inner.stderr)
            .take()
            .map_or_else(Stdio::null, Stdio::from);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr);
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
        *lock(&self.inner.started_at) = Some(Instant::now());

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
            inner.record_exit();
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
        self.inner.terminating.store(true, Ordering::SeqCst);
        drop(lock(&self.inner.stdin).take());
        if let Some(mut child) = lock(&self.inner.child).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.inner.notify_close();
    }

    fn exit_report(&self) -> Option<ExitReport> {
        *lock(&self.inner.exit)
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nagi-process-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    }

    fn wait_closed(client: &JsonRpcClient) {
        for _ in 0..500 {
            if client.is_closed() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("close が通知されない");
    }

    #[test]
    fn exit_code_and_lifetime_are_reported_before_close() {
        let transport = ProcessTransport::new(sh("sleep 0.1; exit 7"));
        let client = JsonRpcClient::new(Box::new(transport), Duration::from_millis(500));
        client.start().unwrap();
        wait_closed(&client);
        let report = client.exit_report().expect("終了コードが記録される");
        assert_eq!(report.code, Some(7));
        assert!(report.lifetime >= Duration::from_millis(90), "{report:?}");
    }

    #[test]
    fn terminated_by_us_has_no_exit_report() {
        let transport = ProcessTransport::new(sh("sleep 30"));
        let client = JsonRpcClient::new(Box::new(transport), Duration::from_millis(500));
        client.start().unwrap();
        client.close();
        assert_eq!(client.exit_report(), None);
    }

    #[test]
    fn stderr_goes_to_the_per_host_file_and_not_to_the_parent() {
        let dir = temp_dir("stderr");
        let mut configuration = ProcessEngineConfiguration::new("sh");
        configuration.arguments = vec!["-c".into(), "echo boom >&2; exit 3".into()];
        configuration.stderr_log = Some(StderrLog {
            dir: dir.join("logs"),
            host_pid: 4242,
        });
        let launcher = ProcessEngineLauncher::new(configuration);
        let t = launcher.make_connection(true).unwrap();
        let client = JsonRpcClient::new(t, Duration::from_millis(500));
        client.start().unwrap();
        wait_closed(&client);
        let text = fs::read_to_string(dir.join("logs").join("imserver-4242.log")).unwrap();
        assert!(
            text.contains("[launch]") && text.contains("minimal=true"),
            "{text}"
        );
        assert!(text.contains("boom"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unwritable_log_dir_falls_back_to_null_stderr() {
        let dir = temp_dir("fallback");
        fs::create_dir_all(&dir).unwrap();
        // ディレクトリの場所にファイルがあるので作れない
        let blocker = dir.join("blocker");
        fs::write(&blocker, "x").unwrap();
        let mut configuration = ProcessEngineConfiguration::new("sh");
        configuration.arguments = vec!["-c".into(), "echo boom >&2; exit 0".into()];
        configuration.stderr_log = Some(StderrLog {
            dir: blocker.join("logs"),
            host_pid: 1,
        });
        let launcher = ProcessEngineLauncher::new(configuration);
        let client = JsonRpcClient::new(
            launcher.make_connection(false).unwrap(),
            Duration::from_millis(500),
        );
        client.start().unwrap();
        wait_closed(&client);
        assert_eq!(client.exit_report().unwrap().code, Some(0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_logs_are_cleaned_once_per_launcher() {
        let dir = temp_dir("clean");
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("imserver-1.log");
        fs::write(&old, "old").unwrap();
        let file = File::options().write(true).open(&old).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(1000))
            .unwrap();
        drop(file);
        fs::write(dir.join("tsf.log"), "keep").unwrap();
        let mut configuration = ProcessEngineConfiguration::new("true");
        configuration.stderr_log = Some(StderrLog {
            dir: dir.clone(),
            host_pid: 2,
        });
        let launcher = ProcessEngineLauncher::new(configuration);
        launcher.make_connection(false).unwrap();
        assert!(!old.exists());
        assert!(dir.join("tsf.log").exists());
        assert!(dir.join("imserver-2.log").exists());
        let _ = fs::remove_dir_all(&dir);
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
