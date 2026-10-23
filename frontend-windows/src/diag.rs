//! 診断ログ（`%LOCALAPPDATA%\Nagi\logs\tsf.log`）の整形・ローテーション判断・イベントの文言。
//!
//! ファイル I/O は持たない純関数だけ（書き込みは TSF 層の薄いラッパー）。
//! **入力内容（キー・テキスト・preedit）は一切書かない**。書くのは起動・Activate・エンジンの起動の成否・
//! 失敗理由・言語バーの登録結果などのイベントのみ。エンジンからの `notify` の本文も書かない
//! （レベルとコードだけ）。

use std::path::{Path, PathBuf};

use crate::client::ClientError;
use crate::supervisor::EngineEvent;

/// ログファイルの上限（超えたら 1 世代前へローテーションする）
pub const MAX_LOG_BYTES: u64 = 256 * 1024;

/// 1 行のメッセージの最大文字数（異常に長い文言でファイルを膨らませない）
const MAX_MESSAGE_CHARS: usize = 300;

/// ログファイルのパス（`<LOCALAPPDATA>\Nagi\logs\tsf.log`）
pub fn log_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Nagi").join("logs").join("tsf.log")
}

/// 1 世代前のログファイルのパス（`tsf.log.1`）
pub fn rotated_path(log: &Path) -> PathBuf {
    let mut name = log.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    log.with_file_name(name)
}

/// `line_len` バイトを追記すると上限を超えるか（超えるなら先にローテーションする）。
/// 空のファイルには、1 行が上限より長くてもローテーションしない
pub fn needs_rotation(current_len: u64, line_len: u64, max: u64) -> bool {
    current_len > 0 && current_len.saturating_add(line_len) > max
}

/// UNIX 時刻（秒）→ UTC の `YYYY-MM-DDTHH:MM:SSZ`
pub fn format_timestamp(unix_secs: u64) -> String {
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    // 日数 → 西暦年月日（Howard Hinnant の civil_from_days。1970-01-01 起点）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// 1 行に整形する（末尾の改行を含む）。改行・制御文字は空白にし、長すぎる文言は切る
pub fn format_line(unix_secs: u64, pid: u32, tag: &str, message: &str) -> String {
    let mut text: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_MESSAGE_CHARS)
        .collect();
    if message.chars().count() > MAX_MESSAGE_CHARS {
        text.push('…');
    }
    format!("{} pid={pid} [{tag}] {text}\n", format_timestamp(unix_secs))
}

/// HRESULT の表記（`0x80004005`）
pub fn hresult_text(code: i32) -> String {
    format!("0x{:08X}", code as u32)
}

/// エラーの理由（入力内容を含みうる応答の本文やサーバーのメッセージは書かない）
pub fn describe_error(error: &ClientError) -> String {
    match error {
        ClientError::NotConnected => "not connected".into(),
        ClientError::ConnectionClosed => "connection closed (engine exited)".into(),
        ClientError::Timeout => "timeout".into(),
        ClientError::Rpc { code, .. } => format!("rpc error code={code}"),
        ClientError::InvalidResponse(_) => "invalid response".into(),
        // 起動時は `spawn: <OS のエラー>` / パイプの取得失敗。パスと OS のエラーだけで入力は含まない
        ClientError::SendFailed(reason) => format!("send failed: {reason}"),
    }
}

/// 起動時の失敗の種類（ログの読み手が原因を絞れるよう、`LaunchFailed` の理由に添える）
fn launch_failure_kind(error: &ClientError) -> &'static str {
    match error {
        ClientError::SendFailed(reason) if reason.starts_with("spawn") => "spawn failed",
        ClientError::SendFailed(_) => "pipe/send failed",
        ClientError::Timeout => "initialize timeout",
        ClientError::Rpc { .. } | ClientError::InvalidResponse(_) => "initialize failed",
        ClientError::ConnectionClosed => "engine exited before initialize",
        ClientError::NotConnected => "not connected",
    }
}

/// エンジンのイベントのログ文言。記録しないイベント（preedit・候補の更新）は `None`
pub fn describe_event(event: &EngineEvent) -> Option<(&'static str, String)> {
    Some(match event {
        EngineEvent::Started { result, is_restart } => (
            "engine",
            format!(
                "started restart={is_restart} server={} {} profile={} protocol={}",
                result.server_name, result.server_version, result.profile, result.protocol_version
            ),
        ),
        EngineEvent::Crashed { attempt, delay } => (
            "engine",
            format!(
                "stopped (crash or start failure) attempt={attempt} retry_in_ms={}",
                delay.as_millis()
            ),
        ),
        EngineEvent::EnteredMinimalMode { restarts } => (
            "engine",
            format!("entering minimal profile after restarts={restarts}"),
        ),
        EngineEvent::LaunchFailed(error) => (
            "engine",
            format!(
                "launch failed: {} ({})",
                launch_failure_kind(error),
                describe_error(error)
            ),
        ),
        EngineEvent::Restored { .. } => ("engine", "session restored".into()),
        EngineEvent::RestoreFailed { error, .. } => (
            "engine",
            format!("session restore failed: {}", describe_error(error)),
        ),
        EngineEvent::Notify(params) => (
            "engine",
            format!(
                "notify level={:?} code={}",
                params.level,
                params.code.as_deref().unwrap_or("-")
            ),
        ),
        EngineEvent::UpdatePreedit(_) | EngineEvent::UpdateCandidates(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamp_known_values() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_timestamp(1_761_221_696), "2025-10-23T12:14:56Z");
        // 閏年でない年の 3/1
        assert_eq!(format_timestamp(1_677_628_800), "2023-03-01T00:00:00Z");
    }

    #[test]
    fn line_is_single_line_and_bounded() {
        let line = format_line(0, 42, "tsf", "a\nb\r\nc");
        assert_eq!(line, "1970-01-01T00:00:00Z pid=42 [tsf] a b  c\n");
        let long = format_line(0, 1, "tsf", &"あ".repeat(1000));
        assert_eq!(long.matches('\n').count(), 1);
        assert!(long.chars().count() < 400);
        assert!(long.trim_end().ends_with('…'));
    }

    #[test]
    fn rotation_decision() {
        assert!(!needs_rotation(0, 10_000_000, MAX_LOG_BYTES));
        assert!(!needs_rotation(100, 100, MAX_LOG_BYTES));
        assert!(!needs_rotation(MAX_LOG_BYTES - 10, 10, MAX_LOG_BYTES));
        assert!(needs_rotation(MAX_LOG_BYTES - 10, 11, MAX_LOG_BYTES));
        assert!(!needs_rotation(0, u64::MAX, 10));
        assert!(needs_rotation(u64::MAX, u64::MAX, 10));
    }

    #[test]
    fn paths() {
        let log = log_path(Path::new("base"));
        assert_eq!(
            log,
            Path::new("base").join("Nagi").join("logs").join("tsf.log")
        );
        assert_eq!(rotated_path(&log).file_name().unwrap(), "tsf.log.1");
        assert_eq!(rotated_path(&log).parent(), log.parent());
    }

    #[test]
    fn hresult_is_unsigned_hex() {
        assert_eq!(hresult_text(0x8000_4005_u32 as i32), "0x80004005");
        assert_eq!(hresult_text(0), "0x00000000");
    }

    #[test]
    fn launch_failures_are_classified() {
        let spawn = EngineEvent::LaunchFailed(ClientError::SendFailed("spawn: not found".into()));
        let (_, text) = describe_event(&spawn).unwrap();
        assert!(text.contains("spawn failed"), "{text}");
        let timeout = EngineEvent::LaunchFailed(ClientError::Timeout);
        assert!(
            describe_event(&timeout)
                .unwrap()
                .1
                .contains("initialize timeout")
        );
        let rpc = EngineEvent::LaunchFailed(ClientError::Rpc {
            code: -32001,
            message: "secret".into(),
        });
        let text = describe_event(&rpc).unwrap().1;
        assert!(
            text.contains("code=-32001") && !text.contains("secret"),
            "{text}"
        );
    }

    #[test]
    fn content_bearing_events_are_not_logged() {
        let preedit = EngineEvent::UpdatePreedit(nagi_protocol::UpdatePreeditParams {
            session_id: "s".into(),
            preedit: Default::default(),
            revision: 1,
        });
        assert!(describe_event(&preedit).is_none());
        let notify = EngineEvent::Notify(nagi_protocol::NotifyParams {
            level: nagi_protocol::NotifyLevel::Warning,
            message: "本文は書かない".into(),
            code: Some("config".into()),
        });
        let text = describe_event(&notify).unwrap().1;
        assert!(text.contains("Warning") && text.contains("config"));
        assert!(!text.contains("本文"));
        let crashed = EngineEvent::Crashed {
            attempt: 2,
            delay: Duration::from_millis(400),
        };
        assert!(
            describe_event(&crashed)
                .unwrap()
                .1
                .contains("retry_in_ms=400")
        );
    }
}
