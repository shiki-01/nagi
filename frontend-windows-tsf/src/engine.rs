//! エンジン（imserver 子プロセス）との接続。`nagi-windows` の `EngineSupervisor` を TSF 向けにまとめる。
//!
//! OS 非依存（Win32 を触らない）。UI スレッドを塞がないために、起動（`initialize`。モデル読み込みで
//! 数秒かかりうる）は別スレッドで行い、キー処理は短いタイムアウトで打ち切る。
//! 非同期通知（`update_preedit` 等）は別スレッドから届くため、キューに積んで UI スレッドが取り出す
//! （TSF のオブジェクトは UI スレッドからしか触れない）。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nagi_protocol::{KeyEvent, ProcessKeyParams, ProcessKeyResult};
use nagi_windows::client::ClientError;
use nagi_windows::process::{ProcessEngineConfiguration, ProcessEngineLauncher};
use nagi_windows::session::SessionStore;
use nagi_windows::supervisor::{Configuration, EngineEvent, EngineSupervisor};

/// キー処理（`process_key` 等）の応答待ちの上限。
/// UI スレッドで同期的に待つため、設計書 §3.3 の通常要求（500ms）より短くする
pub const KEY_TIMEOUT: Duration = Duration::from_millis(250);

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

/// エンジン 1 つ分の接続とセッション状態
pub struct EngineHost {
    supervisor: Arc<EngineSupervisor>,
    sessions: Arc<SessionStore>,
    events: Arc<Mutex<VecDeque<EngineEvent>>>,
}

impl EngineHost {
    /// 起動はしない（[`start_in_background`](Self::start_in_background) で行う）
    pub fn new(location: &EngineLocation, client_version: &str) -> Self {
        let mut process = ProcessEngineConfiguration::new(&location.executable);
        process.arguments = engine_arguments(location);
        let launcher = Arc::new(ProcessEngineLauncher::new(process));

        let mut config = Configuration::new(client_version);
        config.request_timeout = KEY_TIMEOUT;
        let supervisor = Arc::new(EngineSupervisor::new(launcher, config));

        let sessions = Arc::new(SessionStore::new());
        let provider = Arc::clone(&sessions);
        supervisor.set_restore_provider(move || provider.restore_params());

        let events: Arc<Mutex<VecDeque<EngineEvent>>> = Arc::default();
        let queue = Arc::clone(&events);
        supervisor.set_on_event(move |event| {
            let mut q = lock(&queue);
            if q.len() >= MAX_QUEUED_EVENTS {
                q.pop_front();
            }
            q.push_back(event);
        });

        Self {
            supervisor,
            sessions,
            events,
        }
    }

    /// 別スレッドで起動する。失敗時の再起動はスーパーバイザーが予約する。
    /// 起動前・起動中のキー入力は `NotConnected` になり、フロントは素通しする
    pub fn start_in_background(&self) {
        let supervisor = Arc::clone(&self.supervisor);
        std::thread::spawn(move || {
            // 失敗は `EngineEvent::LaunchFailed` として通知され、再起動も予約済み
            let _ = supervisor.start();
        });
    }

    /// スリープ復帰時などの強制再起動（`restart_now` は接続を待つので別スレッドで行う）
    pub fn restart_in_background(&self) {
        let supervisor = Arc::clone(&self.supervisor);
        std::thread::spawn(move || supervisor.restart_now());
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

    /// セッションを忘れる（`Deactivate` 時）
    pub fn forget_session(&self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    /// 溜まっている非同期イベントを取り出す。UI スレッドから呼ぶこと
    pub fn drain_events(&self) -> Vec<EngineEvent> {
        lock(&self.events).drain(..).collect()
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
        assert!(host.drain_events().is_empty());
    }
}
