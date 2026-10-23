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
        | EngineEvent::UpdateCandidates(_)
        | EngineEvent::ProcessExited(_)
        | EngineEvent::MinimalSwitchReport { .. }
        | EngineEvent::Diagnostic(_) => None,
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

/// 言語バーのボタンのメニューで選べる操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    /// 入力モードを切り替える
    SetMode(InputMode),
    /// 設定ファイル（`config.toml`）を開く
    OpenSettings,
}

/// 言語バーのメニューの 1 行
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// 選べる項目。`id` は `ITfMenu::AddMenuItem` の識別子（[`MenuCommand`] と 1 対 1）
    Command {
        /// メニュー項目の識別子
        id: u32,
        /// 表示文言
        label: String,
        /// 現在の選択（チェック）
        checked: bool,
        /// 選べるか
        enabled: bool,
    },
    /// 選べない説明の行（状態の表示）
    Info(String),
    /// 区切り線
    Separator,
}

const ID_HIRAGANA: u32 = 1;
const ID_ALPHANUMERIC: u32 = 2;
const ID_SETTINGS: u32 = 3;

/// 操作 → メニュー項目の識別子
pub fn command_id(command: MenuCommand) -> u32 {
    match command {
        MenuCommand::SetMode(InputMode::Hiragana) => ID_HIRAGANA,
        MenuCommand::SetMode(InputMode::Alphanumeric) => ID_ALPHANUMERIC,
        MenuCommand::OpenSettings => ID_SETTINGS,
    }
}

/// メニュー項目の識別子 → 操作（未知の識別子は `None`）
pub fn command_from_id(id: u32) -> Option<MenuCommand> {
    match id {
        ID_HIRAGANA => Some(MenuCommand::SetMode(InputMode::Hiragana)),
        ID_ALPHANUMERIC => Some(MenuCommand::SetMode(InputMode::Alphanumeric)),
        ID_SETTINGS => Some(MenuCommand::OpenSettings),
        _ => None,
    }
}

/// ボタンを押したときの切り替え先（ひらがな ⇄ 英数）
pub fn toggled(mode: InputMode) -> InputMode {
    match mode {
        InputMode::Hiragana => InputMode::Alphanumeric,
        InputMode::Alphanumeric => InputMode::Hiragana,
    }
}

/// ボタンのツールチップ
pub fn button_tooltip(mode: Option<InputMode>) -> String {
    match mode {
        Some(m) => format!("Nagi: {}", mode_name(m)),
        None => "Nagi".to_owned(),
    }
}

/// ボタンに出す短い表示（接続前などモード不明のときは既定のひらがな）
pub fn button_text(mode: Option<InputMode>) -> &'static str {
    mode_indicator(mode.unwrap_or(InputMode::Hiragana))
}

/// 設定ファイルの場所。コアが教える `config_path` が無ければ開けない
pub fn settings_path(status: Option<&StatusResult>) -> Option<&str> {
    status?.config_path.as_deref().filter(|p| !p.is_empty())
}

/// 言語バーのボタンのメニュー。現在のモードにチェックを付け、状態の行（`status` が取れなければ
/// [`UNAVAILABLE`]）を挟み、設定を開く項目は設定ファイルの場所が分かるときだけ選べる
pub fn menu_entries(mode: Option<InputMode>, status: Option<&StatusResult>) -> Vec<MenuEntry> {
    let mode_item = |target: InputMode| MenuEntry::Command {
        id: command_id(MenuCommand::SetMode(target)),
        label: mode_name(target).to_owned(),
        checked: mode == Some(target),
        enabled: true,
    };
    let mut entries = vec![
        mode_item(InputMode::Hiragana),
        mode_item(InputMode::Alphanumeric),
        MenuEntry::Separator,
    ];
    match status {
        Some(s) => entries.extend(menu_lines(s).into_iter().map(MenuEntry::Info)),
        None => entries.push(MenuEntry::Info(UNAVAILABLE.to_owned())),
    }
    entries.push(MenuEntry::Separator);
    entries.push(MenuEntry::Command {
        id: command_id(MenuCommand::OpenSettings),
        label: "設定ファイルを開く…".to_owned(),
        checked: false,
        enabled: settings_path(status).is_some(),
    });
    entries
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

    fn sample_status(config_path: Option<&str>) -> StatusResult {
        StatusResult {
            profile: "balanced".into(),
            profile_reason: "r".into(),
            backend: "x".into(),
            llm: false,
            learning: true,
            learned_entries: 1,
            user_dict_entries: 0,
            config_path: config_path.map(str::to_owned),
            config_error: false,
        }
    }

    #[test]
    fn menu_ids_round_trip_and_are_unique() {
        let commands = [
            MenuCommand::SetMode(InputMode::Hiragana),
            MenuCommand::SetMode(InputMode::Alphanumeric),
            MenuCommand::OpenSettings,
        ];
        let mut ids: Vec<u32> = commands.iter().map(|c| command_id(*c)).collect();
        for c in commands {
            assert_eq!(command_from_id(command_id(c)), Some(c));
        }
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), commands.len());
        assert_eq!(command_from_id(0), None);
        assert_eq!(command_from_id(999), None);
    }

    #[test]
    fn toggle_and_button_labels() {
        assert_eq!(toggled(InputMode::Hiragana), InputMode::Alphanumeric);
        assert_eq!(toggled(toggled(InputMode::Hiragana)), InputMode::Hiragana);
        assert_eq!(button_text(Some(InputMode::Alphanumeric)), "A");
        assert_eq!(button_text(None), "あ");
        assert_eq!(button_tooltip(Some(InputMode::Hiragana)), "Nagi: ひらがな");
        assert_eq!(button_tooltip(None), "Nagi");
    }

    #[test]
    fn menu_checks_current_mode_and_enables_settings_only_with_a_path() {
        let status = sample_status(Some("C:/Users/x/AppData/Roaming/Nagi/config.toml"));
        let entries = menu_entries(Some(InputMode::Alphanumeric), Some(&status));
        let checked: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { label, checked, .. } if *checked => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(checked, ["英数"]);
        let Some(MenuEntry::Command { enabled, id, .. }) = entries.last() else {
            panic!("最後は設定の項目");
        };
        assert!(*enabled);
        assert_eq!(command_from_id(*id), Some(MenuCommand::OpenSettings));
        // 状態の行（menu_lines）が入る
        assert!(entries.contains(&MenuEntry::Info("変換: 辞書のみ".into())));

        // 設定ファイルの場所が無い・空・状態が取れないときは選べない
        for st in [
            Some(sample_status(None)),
            Some(sample_status(Some(""))),
            None,
        ] {
            let entries = menu_entries(None, st.as_ref());
            assert!(matches!(
                entries.last(),
                Some(MenuEntry::Command { enabled: false, .. })
            ));
            // モード不明ならチェックは付けない
            assert!(
                !entries
                    .iter()
                    .any(|e| matches!(e, MenuEntry::Command { checked: true, .. }))
            );
        }
        let none = menu_entries(Some(InputMode::Hiragana), None);
        assert!(none.contains(&MenuEntry::Info(UNAVAILABLE.into())));
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
