//! アプリの種類ごとの既定モード・素通しの設定（`config.toml` の `[apps]`）。
//!
//! 分類表の解釈（利用者の表と同梱の表の優先順位）は設定側が済ませ、ここには優先順に並べた規則を渡す。

use nagi_protocol::view::InputMode;

/// アプリの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppClass {
    /// コードエディタ・ターミナル
    Code,
    /// チャット
    Chat,
    /// それ以外
    General,
}

/// アプリごとの既定モード・素通しの設定。`Default` は何もしない（すべて無効）
#[derive(Debug, Clone, Default)]
pub struct AppsConfig {
    /// 分類の規則（優先順。パターンは小文字。末尾が `*` なら前方一致、それ以外は完全一致）
    pub rules: Vec<(String, AppClass)>,
    /// `Code` のアプリでフォーカスを得たときのモード。`None` は前回のまま
    pub code_mode: Option<InputMode>,
    /// `Chat` のアプリでフォーカスを得たときのモード
    pub chat_mode: Option<InputMode>,
    /// `General` のアプリでフォーカスを得たときのモード
    pub general_mode: Option<InputMode>,
    /// 手で切り替えたモードをアプリごとに覚えるか
    pub remember_mode: bool,
    /// 素通しの先頭記号。空なら素通しは無効
    pub passthrough_chars: Vec<char>,
}

impl AppsConfig {
    /// `app_id` の種類。どの規則にも当たらなければ `General`
    pub fn classify(&self, app_id: &str) -> AppClass {
        let id = app_id.to_lowercase();
        self.rules
            .iter()
            .find(|(pattern, _)| match pattern.strip_suffix('*') {
                Some(prefix) => id.starts_with(prefix),
                None => id == *pattern,
            })
            .map_or(AppClass::General, |(_, class)| *class)
    }

    /// 種類ごとの、フォーカスを得たときのモード
    pub fn default_mode(&self, class: AppClass) -> Option<InputMode> {
        match class {
            AppClass::Code => self.code_mode,
            AppClass::Chat => self.chat_mode,
            AppClass::General => self.general_mode,
        }
    }

    /// `c` が素通しの先頭記号か
    pub fn is_passthrough_start(&self, c: char) -> bool {
        self.passthrough_chars.contains(&c)
    }
}
