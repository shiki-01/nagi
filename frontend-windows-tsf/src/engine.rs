//! エンジン（imserver 子プロセス）との接続。`nagi-windows` の `EngineSupervisor` を TSF 向けにまとめる。
//!
//! OS 非依存（Win32 を触らない）。UI スレッドを塞がないために、起動（`initialize`。モデル読み込みで
//! 数秒かかりうる）は別スレッドで行い、キー処理は短いタイムアウトで打ち切る。
//! 非同期通知（`update_preedit` 等）は別スレッドから届くため、セッションごとのキューに積み、
//! 購読時に渡された waker（Win32 層では隠しウィンドウへの `PostMessage`）で UI スレッドを起こす。
//! 取り出し（TSF のオブジェクトを触る処理）は UI スレッドで行う。
//! セッション宛てのイベント（`update_preedit` / `update_candidates` / `Restored` / `RestoreFailed`）は
//! そのセッションだけに、プロセス全体のイベント（`Started` / `Crashed` / `Notify` 等）は購読中の全セッションに配る。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nagi_protocol::{
    InputMode, KeyEvent, PerformActionParams, ProcessKeyParams, ProcessKeyResult, ReconvertParams,
    SetContextParams, SetEnvironmentParams, TouchAction,
};
use nagi_windows::client::ClientError;
use nagi_windows::process::{ProcessEngineConfiguration, ProcessEngineLauncher, StderrLog};
use nagi_windows::session::SessionStore;
use nagi_windows::supervisor::{Configuration, EngineEvent, EngineSupervisor};

/// キー処理（`process_key` 等）の応答待ちの上限。
/// UI スレッドで同期的に待つため、設計書 §3.3 の通常要求（500ms）より短くする
pub const KEY_TIMEOUT: Duration = Duration::from_millis(250);

/// 再変換の応答待ちの上限（macOS 版の `reconvertTimeout` と同じ）。
/// 初回は辞書の逆引き索引を作るため、通常のキー処理より長く待つ
pub const RECONVERT_TIMEOUT: Duration = Duration::from_secs(2);

/// 未処理の非同期イベントを溜める上限（超えたら古いものから捨てる。revision で古い通知は無効なので問題ない）
const MAX_QUEUED_EVENTS: usize = 64;

/// エンジンの置き場所
#[derive(Debug, Clone)]
pub struct EngineLocation {
    /// imserver の実行ファイル（通常は DLL と同じディレクトリ）
    pub executable: PathBuf,
    /// 辞書・モデルのディレクトリ（`--data-dir`）
    pub data_dir: PathBuf,
}

/// `imserver` に渡す引数
pub fn engine_arguments(location: &EngineLocation) -> Vec<String> {
    vec![
        "--data-dir".to_string(),
        location.data_dir.to_string_lossy().into_owned(),
    ]
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// UI スレッドを起こす関数（任意のスレッドから呼ばれる）
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// イベントの宛先
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route<'a> {
    /// このセッションだけ
    Session(&'a str),
    /// 購読中の全セッション
    Broadcast,
}

/// イベントの宛先を決める
pub fn route(event: &EngineEvent) -> Route<'_> {
    match event {
        EngineEvent::UpdatePreedit(p) => Route::Session(&p.session_id),
        EngineEvent::UpdateCandidates(c) => Route::Session(&c.session_id),
        EngineEvent::Restored { session_id, .. }
        | EngineEvent::RestoreFailed { session_id, .. } => Route::Session(session_id),
        EngineEvent::Started { .. }
        | EngineEvent::Crashed { .. }
        | EngineEvent::EnteredMinimalMode { .. }
        | EngineEvent::Notify(_)
        | EngineEvent::LaunchFailed(_)
        | EngineEvent::ProcessExited(_)
        | EngineEvent::MinimalSwitchReport { .. }
        | EngineEvent::Diagnostic(_) => Route::Broadcast,
    }
}

struct Subscriber {
    queue: VecDeque<EngineEvent>,
    waker: Waker,
}

type Subscribers = Arc<Mutex<HashMap<String, Subscriber>>>;

/// イベントを宛先のキューに積み、起こすべき waker を返す（呼び出し側がロックの外で呼ぶ）
fn deliver(subscribers: &Mutex<HashMap<String, Subscriber>>, event: EngineEvent) -> Vec<Waker> {
    let mut map = lock(subscribers);
    let mut wake = Vec::new();
    let mut push = |sub: &mut Subscriber, event: EngineEvent| {
        if sub.queue.len() >= MAX_QUEUED_EVENTS {
            sub.queue.pop_front();
        }
        sub.queue.push_back(event);
        wake.push(Arc::clone(&sub.waker));
    };
    match route(&event) {
        Route::Session(id) => {
            if let Some(sub) = map.get_mut(id) {
                push(sub, event);
            }
        }
        Route::Broadcast => {
            for sub in map.values_mut() {
                push(sub, event.clone());
            }
        }
    }
    wake
}

/// エンジン 1 つ分の接続とセッション状態
pub struct EngineHost {
    supervisor: Arc<EngineSupervisor>,
    sessions: Arc<SessionStore>,
    subscribers: Subscribers,
}

impl EngineHost {
    /// 起動はしない（[`start_in_background`](Self::start_in_background) で行う）
    pub fn new(location: &EngineLocation, client_version: &str) -> Self {
        let mut process = ProcessEngineConfiguration::new(&location.executable);
        process.arguments = engine_arguments(location);
        // imserver の stderr は親から継承せず、ホストごとのファイル（imserver-<pid>.log）へ向ける
        process.stderr_log = crate::tsf_log::log_dir().map(|dir| StderrLog {
            dir,
            host_pid: std::process::id(),
        });
        let launcher = Arc::new(ProcessEngineLauncher::new(process));

        let mut config = Configuration::new(client_version);
        config.request_timeout = KEY_TIMEOUT;
        let supervisor = Arc::new(EngineSupervisor::new(launcher, config));

        let sessions = Arc::new(SessionStore::new());
        let provider = Arc::clone(&sessions);
        supervisor.set_restore_provider(move || provider.restore_params());

        let subscribers: Subscribers = Arc::default();
        let target = Arc::clone(&subscribers);
        supervisor.set_on_event(move |event| {
            // 診断ログ（起動の成否・クラッシュ・minimal 切替など。入力内容を含むイベントは記録しない）
            if let Some((tag, message)) = nagi_windows::diag::describe_event(&event) {
                crate::tsf_log::log(tag, &message);
            }
            for waker in deliver(&target, event) {
                waker();
            }
        });

        Self {
            supervisor,
            sessions,
            subscribers,
        }
    }

    /// 別スレッドで起動する。失敗時の再起動はスーパーバイザーが予約する。
    /// 起動前・起動中のキー入力は `NotConnected` になり、フロントは素通しする
    pub fn start_in_background(&self) {
        let supervisor = Arc::clone(&self.supervisor);
        std::thread::spawn(move || {
            // 失敗は `EngineEvent::LaunchFailed` として通知され、再起動も予約済み。
            // 通信路が起動直後に閉じた場合（`LaunchFailed` にならない）の理由もここで記録する
            crate::tsf_log::log("engine", "starting");
            if let Err(error) = supervisor.start() {
                crate::tsf_log::log(
                    "engine",
                    &format!(
                        "start failed: {}",
                        nagi_windows::diag::describe_error(&error)
                    ),
                );
            }
        });
    }

    /// スリープ復帰時などの強制再起動（`restart_now` は接続を待つので別スレッドで行う）
    pub fn restart_in_background(&self) {
        let supervisor = Arc::clone(&self.supervisor);
        std::thread::spawn(move || {
            crate::tsf_log::log("engine", "restart requested (resume from sleep)");
            supervisor.restart_now();
        });
    }

    /// 正常終了（`shutdown` を送って閉じる。学習の保存のため）
    pub fn stop(&self) {
        self.supervisor.stop();
    }

    /// 現在つながっているか
    pub fn is_connected(&self) -> bool {
        self.supervisor.is_connected()
    }

    /// キーを処理する。成功した応答の preedit は `SessionStore` に記録する
    pub fn process_key(
        &self,
        session_id: &str,
        key: &KeyEvent,
    ) -> Result<ProcessKeyResult, ClientError> {
        let params = ProcessKeyParams {
            session_id: session_id.to_string(),
            keysym: key.keysym,
            keycode: key.keycode,
            text: key.text.clone(),
            modifiers: key.modifiers,
            kind: key.kind,
        };
        let result = self.supervisor.process_key(&params)?;
        self.track(session_id, &result);
        Ok(result)
    }

    /// 応答に preedit があれば記録する（`None` は「変化なし」なので消さない）
    fn track(&self, session_id: &str, result: &ProcessKeyResult) {
        if let Some(p) = &result.preedit {
            self.sessions.update(session_id, Some(p));
        }
    }

    /// フォーカス取得
    pub fn focus_in(&self, session_id: &str) -> Result<ProcessKeyResult, ClientError> {
        self.supervisor.focus_in(session_id)
    }

    /// フォーカス喪失。表示中の文字列の確定が `commit` で返る
    pub fn focus_out(&self, session_id: &str) -> Result<ProcessKeyResult, ClientError> {
        let result = self.supervisor.focus_out(session_id)?;
        self.track(session_id, &result);
        Ok(result)
    }

    /// セッションのリセット（composition が外から終了されたとき）
    pub fn reset(&self, session_id: &str) -> Result<ProcessKeyResult, ClientError> {
        let result = self.supervisor.reset(session_id)?;
        self.track(session_id, &result);
        Ok(result)
    }

    /// セッションを忘れる（`Deactivate` 時）。購読も解除する
    pub fn forget_session(&self, session_id: &str) {
        self.sessions.remove(session_id);
        self.unsubscribe(session_id);
    }

    /// セッションのイベントを購読する。イベントが積まれるたびに `waker` が（任意のスレッドから）呼ばれる
    pub fn subscribe(&self, session_id: &str, waker: Waker) {
        lock(&self.subscribers).insert(
            session_id.to_string(),
            Subscriber {
                queue: VecDeque::new(),
                waker,
            },
        );
    }

    /// 購読を解除する（積まれたイベントは捨てる）
    pub fn unsubscribe(&self, session_id: &str) {
        lock(&self.subscribers).remove(session_id);
    }

    /// このセッション宛ての溜まっている非同期イベントを取り出す。UI スレッドから呼ぶこと
    pub fn drain_events(&self, session_id: &str) -> Vec<EngineEvent> {
        lock(&self.subscribers)
            .get_mut(session_id)
            .map(|s| s.queue.drain(..).collect())
            .unwrap_or_default()
    }

    /// 入力モードを切り替える（`perform_action(SetMode)`）。入力中の文字列は確定される
    pub fn set_mode(
        &self,
        session_id: &str,
        mode: InputMode,
    ) -> Result<ProcessKeyResult, ClientError> {
        let params = PerformActionParams {
            session_id: session_id.to_string(),
            action: TouchAction::SetMode(mode),
        };
        let result = self.supervisor.perform_action(&params)?;
        self.track(session_id, &result);
        Ok(result)
    }

    /// 文脈を通知する
    pub fn set_context(&self, params: &SetContextParams) -> Result<(), ClientError> {
        self.supervisor.set_context(params)
    }

    /// 環境を通知する
    pub fn set_environment(&self, params: &SetEnvironmentParams) -> Result<(), ClientError> {
        self.supervisor.set_environment(params)
    }

    /// 確定済みテキストの再変換。初回は辞書の逆引き索引を作るため `timeout` まで待つ
    pub fn reconvert(
        &self,
        session_id: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<ProcessKeyResult, ClientError> {
        let params = ReconvertParams {
            session_id: session_id.to_string(),
            text: text.to_string(),
        };
        let result = self.supervisor.reconvert(&params, Some(timeout))?;
        self.track(session_id, &result);
        Ok(result)
    }

    /// 現在の状態（プロファイル・学習・辞書。メニュー表示用）
    pub fn status(&self) -> Result<nagi_protocol::StatusResult, ClientError> {
        self.supervisor.get_status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_pass_data_dir() {
        let loc = EngineLocation {
            executable: PathBuf::from("imserver.exe"),
            data_dir: PathBuf::from("C:/Users/x/AppData/Roaming/Nagi/data"),
        };
        assert_eq!(
            engine_arguments(&loc),
            ["--data-dir", "C:/Users/x/AppData/Roaming/Nagi/data"]
        );
    }

    #[test]
    fn missing_executable_reports_not_connected_without_blocking() {
        let loc = EngineLocation {
            executable: PathBuf::from("/nonexistent/imserver"),
            data_dir: PathBuf::from("/nonexistent"),
        };
        let host = EngineHost::new(&loc, "0.0.0");
        assert!(!host.is_connected());
        let key = nagi_windows::keynorm::normalize(&nagi_windows::keynorm::RawKeyInput {
            vk: 0x41,
            scan_code: 0x1E,
            extended: false,
            is_key_up: false,
            text: "a".into(),
            unmodified_text: "a".into(),
            modifiers: Default::default(),
        })
        .unwrap();
        assert!(host.process_key("s", &key).is_err());
        assert!(host.drain_events("s").is_empty());
    }

    fn preedit_event(session: &str) -> EngineEvent {
        EngineEvent::UpdatePreedit(nagi_protocol::UpdatePreeditParams {
            session_id: session.to_string(),
            preedit: Default::default(),
            revision: 1,
        })
    }

    fn counting_waker() -> (Waker, Arc<std::sync::atomic::AtomicUsize>) {
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = Arc::clone(&n);
        let w: Waker = Arc::new(move || {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        (w, n)
    }

    #[test]
    fn session_events_reach_only_their_session() {
        let subs: Subscribers = Arc::default();
        let (wa, na) = counting_waker();
        let (wb, nb) = counting_waker();
        for (id, w) in [("a", wa), ("b", wb)] {
            lock(&subs).insert(
                id.into(),
                Subscriber {
                    queue: VecDeque::new(),
                    waker: w,
                },
            );
        }
        for w in deliver(&subs, preedit_event("a")) {
            w();
        }
        assert_eq!(na.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(nb.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(lock(&subs)["a"].queue.len(), 1);
        assert!(lock(&subs)["b"].queue.is_empty());
    }

    #[test]
    fn process_wide_events_are_broadcast() {
        let subs: Subscribers = Arc::default();
        for id in ["a", "b"] {
            lock(&subs).insert(
                id.into(),
                Subscriber {
                    queue: VecDeque::new(),
                    waker: Arc::new(|| {}),
                },
            );
        }
        let wake = deliver(
            &subs,
            EngineEvent::Crashed {
                attempt: 1,
                delay: Duration::from_secs(1),
            },
        );
        assert_eq!(wake.len(), 2);
        assert_eq!(lock(&subs)["a"].queue.len(), 1);
        assert_eq!(lock(&subs)["b"].queue.len(), 1);
    }

    #[test]
    fn unknown_session_and_overflow() {
        let subs: Subscribers = Arc::default();
        assert!(deliver(&subs, preedit_event("none")).is_empty());
        lock(&subs).insert(
            "a".into(),
            Subscriber {
                queue: VecDeque::new(),
                waker: Arc::new(|| {}),
            },
        );
        for _ in 0..(MAX_QUEUED_EVENTS + 10) {
            deliver(&subs, preedit_event("a"));
        }
        assert_eq!(lock(&subs)["a"].queue.len(), MAX_QUEUED_EVENTS);
    }
}
