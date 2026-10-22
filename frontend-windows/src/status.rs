//! 利用者に見せる状態の文言・入力モードの表示（OS 非依存）。macOS 版 `StatusMessages.swift` に相当する。
//! 入力内容は含めない。表示（トースト・言語バー・ツールチップ）は次の層が担う。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use nagi_protocol::{InputMode, NotifyLevel, StatusResult};

use crate::supervisor::EngineEvent;

/// エンジンのイベントのうち、利用者に知らせるべきものの文言。`None` なら知らせない
pub fn toast_for(event: &EngineEvent) -> Option<String> {
    match event {
        EngineEvent::Notify(n) => {
            // 起動時のアップデート確認（コアの `update.auto_check`）は info でも知らせる
            if n.code.as_deref() == Some("update_available") {
                return Some(format!(
                    "{}。PowerShell でインストールコマンドをもう一度実行すると更新できます（nagi.shiki-01.com）",
                    n.message
                ));
            }
            // ほかの info は頻繁に来うるため出さない（ログのみ）
            (n.level != NotifyLevel::Info).then(|| n.message.clone())
        }
        EngineEvent::Crashed { attempt, .. } => {
            (*attempt == 1).then(|| "変換エンジンが停止しました。再起動しています".to_owned())
        }
        EngineEvent::EnteredMinimalMode { restarts } => Some(format!(
            "変換エンジンが {restarts} 回続けて停止したため、辞書のみで動作しています"
        )),
        EngineEvent::LaunchFailed(_) => {
            Some("変換エンジンを起動できません。再試行しています".to_owned())
        }
        EngineEvent::RestoreFailed { .. } => Some(
            "変換エンジンの再起動で入力中の文字列を復元できなかったため、そのまま確定しました"
                .to_owned(),
        ),
        EngineEvent::Started { .. }
        | EngineEvent::Restored { .. }
        | EngineEvent::UpdatePreedit(_)
        | EngineEvent::UpdateCandidates(_) => None,
    }
}

/// 状態を取得できなかったときの行
pub const UNAVAILABLE: &str = "変換エンジンに接続していません";

/// メニューに出す状態の行（選択不可の項目として並べる）
pub fn menu_lines(status: &StatusResult) -> Vec<String> {
    let mut lines = vec![format!(
        "プロファイル: {}（{}）",
        status.profile, status.profile_reason
    )];
    lines.push(
        if status.llm {
            "変換: LLM + 辞書"
        } else {
            "変換: 辞書のみ"
        }
        .to_owned(),
    );
    lines.push(if status.learning {
        format!("学習: 有効（{} 件）", status.learned_entries)
    } else {
        "学習: 無効".to_owned()
    });
    lines.push(format!("ユーザー辞書: {} 語", status.user_dict_entries));
    if status.config_error {
        lines.push("設定ファイルの読み込みに失敗しました（以前の設定で動作中）".to_owned());
    }
    lines
}

/// 言語バー・トレイに出す入力モードの短い表示
pub fn mode_indicator(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Hiragana => "あ",
        InputMode::Alphanumeric => "A",
    }
}

/// ツールチップ・アクセシビリティ用のモード名
pub fn mode_name(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Hiragana => "ひらがな",
        InputMode::Alphanumeric => "英数",
    }
}

/// 同じ文言を短時間に繰り返し出さないための間引き
#[derive(Debug, Clone)]
pub struct ToastThrottle {
    interval: Duration,
    last_shown: HashMap<String, Instant>,
}

impl Default for ToastThrottle {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl ToastThrottle {
    /// 間引きの間隔を指定して作る（既定は 30 秒）
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_shown: HashMap::new(),
        }
    }

    /// `message` を今出してよいか。出してよければ記録する
    pub fn should_show(&mut self, message: &str, now: Instant) -> bool {
        if let Some(last) = self.last_shown.get(message)
            && now.saturating_duration_since(*last) < self.interval
        {
            return false;
        }
        self.last_shown.insert(message.to_owned(), now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientError;
    use nagi_protocol::NotifyParams;

    fn notify(level: NotifyLevel, code: Option<&str>) -> EngineEvent {
        EngineEvent::Notify(NotifyParams {
            level,
            message: "メッセージ".into(),
            code: code.map(str::to_owned),
        })
    }

    #[test]
    fn notify_levels() {
        assert_eq!(toast_for(&notify(NotifyLevel::Info, None)), None);
        assert_eq!(
            toast_for(&notify(NotifyLevel::Warning, None)).as_deref(),
            Some("メッセージ")
        );
        assert!(toast_for(&notify(NotifyLevel::Error, None)).is_some());
        let update = toast_for(&notify(NotifyLevel::Info, Some("update_available"))).unwrap();
        assert!(update.starts_with("メッセージ。"));
    }

    #[test]
    fn engine_failures() {
        let crashed = |attempt| EngineEvent::Crashed {
            attempt,
            delay: Duration::ZERO,
        };
        assert!(toast_for(&crashed(1)).is_some());
        assert_eq!(toast_for(&crashed(2)), None);
        assert!(
            toast_for(&EngineEvent::EnteredMinimalMode { restarts: 5 })
                .unwrap()
                .contains("5 回")
        );
        assert!(toast_for(&EngineEvent::LaunchFailed(ClientError::NotConnected)).is_some());
    }

    #[test]
    fn menu_lines_reflect_status() {
        let mut s = StatusResult {
            profile: "balanced".into(),
            profile_reason: "メモリ 16GB".into(),
            backend: "x".into(),
            llm: true,
            learning: true,
            learned_entries: 12,
            user_dict_entries: 3,
            config_path: None,
            config_error: false,
        };
        let lines = menu_lines(&s);
        assert_eq!(lines[0], "プロファイル: balanced（メモリ 16GB）");
        assert_eq!(lines[1], "変換: LLM + 辞書");
        assert_eq!(lines[2], "学習: 有効（12 件）");
        assert_eq!(lines[3], "ユーザー辞書: 3 語");
        assert_eq!(lines.len(), 4);
        s.llm = false;
        s.learning = false;
        s.config_error = true;
        let lines = menu_lines(&s);
        assert_eq!(lines[1], "変換: 辞書のみ");
        assert_eq!(lines[2], "学習: 無効");
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn mode_labels() {
        assert_eq!(mode_indicator(InputMode::Hiragana), "あ");
        assert_eq!(mode_indicator(InputMode::Alphanumeric), "A");
        assert_eq!(mode_name(InputMode::Alphanumeric), "英数");
    }

    #[test]
    fn throttle_suppresses_repeats_within_interval() {
        let mut t = ToastThrottle::new(Duration::from_secs(30));
        let t0 = Instant::now();
        assert!(t.should_show("a", t0));
        assert!(!t.should_show("a", t0 + Duration::from_secs(10)));
        assert!(t.should_show("b", t0 + Duration::from_secs(10)));
        assert!(t.should_show("a", t0 + Duration::from_secs(30)));
    }
}
