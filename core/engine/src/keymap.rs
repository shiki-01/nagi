//! キーマップ: キー入力 → [`Action`] の解決。
//!
//! 内蔵の既定テーブルに加えて、`composing` / `converting` の 2 文脈に限り TOML（`[keymap]`）
//! での個別キーの上書きに対応する（[`KeyMap::with_overrides`]）。解決結果が `None` の
//! ときの扱い（素通し / 消費のみ）は状態機械側が文脈で決める。

use crate::action::{Action, Direction};
use crate::keys::parse_key;
use nagi_protocol::key::{KeyEvent, KeyKind, keysym};
use nagi_protocol::view::InputMode;

/// キー入力の文脈（状態機械の状態に対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyContext {
    /// 未入力（英数モードもここ）
    Direct,
    /// ローマ字入力中
    Composing,
    /// 変換中
    Converting,
}

/// キー入力の識別（上書きテーブルのキー）。テキストは無視し、キーそのものと修飾だけを見る
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct KeySpec {
    keysym: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

impl KeySpec {
    fn from_event(key: &KeyEvent) -> Self {
        KeySpec {
            keysym: key.keysym,
            ctrl: key.modifiers.ctrl,
            shift: key.modifiers.shift,
            alt: key.modifiers.alt,
            meta: key.modifiers.meta,
        }
    }
}

/// キーマップ。`composing` / `converting` 文脈のキー上書き（[`KeyMap::with_overrides`]）を持つ
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    overrides: std::collections::HashMap<(KeyContext, KeySpec), Action>,
}

/// 印字可能文字（Ctrl/Alt/Meta なし、Space 除く）。`text`（OS の配列が生成した文字）を優先する
fn printable(key: &KeyEvent) -> Option<char> {
    let m = key.modifiers;
    if m.ctrl || m.alt || m.meta {
        return None;
    }
    let mut chars = key.text.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && !c.is_control()
        && c != ' '
    {
        return Some(c);
    }
    if key.text.is_empty() && (0x21..=0x7e).contains(&key.keysym) {
        return char::from_u32(key.keysym);
    }
    None
}

/// 修飾なしで 2 文字以上のテキストが生成されたときの挿入（絵文字の ZWJ 列など）
fn multi_text(key: &KeyEvent) -> Option<Action> {
    let m = key.modifiers;
    if m.ctrl
        || m.alt
        || m.meta
        || key.text.chars().count() < 2
        || key.text.chars().any(char::is_control)
    {
        return None;
    }
    Some(Action::InsertText(key.text.clone()))
}

/// Ctrl 単独（Shift 可）+ 英字のときのその小文字
fn ctrl_letter(key: &KeyEvent) -> Option<char> {
    let m = key.modifiers;
    if !m.ctrl || m.alt || m.meta {
        return None;
    }
    let c = char::from_u32(key.keysym)?;
    c.is_ascii_alphabetic().then(|| c.to_ascii_lowercase())
}

/// Ctrl / Alt / Meta が押されている
fn has_command_modifier(key: &KeyEvent) -> bool {
    let m = key.modifiers;
    m.ctrl || m.alt || m.meta
}

/// 修飾付きの「文字キー」（Cmd+C など）。確定して素通しする対象
fn is_command_chord(key: &KeyEvent) -> bool {
    has_command_modifier(key) && (!key.text.is_empty() || (0x20..=0x7e).contains(&key.keysym))
}

impl KeyMap {
    /// 既定プリセット（上書きなし）
    pub fn default_preset() -> Self {
        KeyMap::default()
    }

    /// `composing` / `converting` のキー上書きを持つキーマップを作る。
    ///
    /// `bindings` は `(文脈, キー仕様文字列, Action 仕様文字列)` の組。解析できないキー・
    /// Action はスキップし、その理由を人間向けの文言として 2 番目の戻り値に集める
    /// （呼び出し側が `notify` などで知らせるため）。有効なものだけで上書きテーブルを作る
    pub fn with_overrides(bindings: &[(KeyContext, String, String)]) -> (KeyMap, Vec<String>) {
        let mut overrides = std::collections::HashMap::new();
        let mut warnings = Vec::new();
        for (context, key_spec, action_spec) in bindings {
            let key_event = match parse_key(key_spec) {
                Ok(k) => k,
                Err(e) => {
                    warnings.push(format!("キーマップ: 不明なキー '{key_spec}'（{e}）"));
                    continue;
                }
            };
            let action = match parse_action(action_spec) {
                Ok(a) => a,
                Err(e) => {
                    warnings.push(format!(
                        "キーマップ: 不明なアクション '{action_spec}'（{e}）"
                    ));
                    continue;
                }
            };
            overrides.insert((*context, KeySpec::from_event(&key_event)), action);
        }
        (KeyMap { overrides }, warnings)
    }

    /// キーを Action に解決する
    ///
    /// `None` = 割当なし。英数モードでは入力モード切替キー以外は常に `None`（素通し）。
    pub fn resolve(&self, context: KeyContext, mode: InputMode, key: &KeyEvent) -> Option<Action> {
        if key.kind == KeyKind::FlagsChanged {
            return None;
        }
        // 共通（全文脈・全モード）
        match key.keysym {
            keysym::EISU_TOGGLE => return Some(Action::SetMode(InputMode::Alphanumeric)),
            keysym::HIRAGANA_KATAKANA | keysym::HIRAGANA => {
                return Some(Action::SetMode(InputMode::Hiragana));
            }
            keysym::ZENKAKU_HANKAKU => return Some(Action::ToggleMode),
            _ => {}
        }
        if mode == InputMode::Alphanumeric {
            return None;
        }
        if let Some(action) = self.overrides.get(&(context, KeySpec::from_event(key))) {
            return Some(action.clone());
        }
        match context {
            KeyContext::Direct => printable(key)
                .map(Action::Insert)
                .or_else(|| multi_text(key)),
            KeyContext::Composing => self.resolve_composing(key),
            KeyContext::Converting => self.resolve_converting(key),
        }
    }

    fn resolve_composing(&self, key: &KeyEvent) -> Option<Action> {
        let m = key.modifiers;
        if let Some(c) = ctrl_letter(key) {
            return match c {
                'h' => Some(Action::Backspace),
                'a' => Some(Action::MoveCursorHome),
                'e' => Some(Action::MoveCursorEnd),
                'k' => Some(Action::ToKatakana),
                _ => Some(Action::CommitAndPassThrough),
            };
        }
        if let Some(c) = printable(key) {
            return Some(Action::Insert(c));
        }
        if let Some(action) = multi_text(key) {
            return Some(action);
        }
        match key.keysym {
            keysym::SPACE if !m.ctrl && !m.alt && !m.meta => Some(Action::StartConvert),
            keysym::RETURN => Some(Action::Commit),
            keysym::ESCAPE => Some(Action::Cancel),
            keysym::BACKSPACE => Some(Action::Backspace),
            keysym::DELETE => Some(Action::DeleteForward),
            keysym::LEFT => Some(Action::MoveCursor(Direction::Left)),
            keysym::RIGHT => Some(Action::MoveCursor(Direction::Right)),
            keysym::HOME => Some(Action::MoveCursorHome),
            keysym::END => Some(Action::MoveCursorEnd),
            keysym::F7 => Some(Action::ToKatakana),
            keysym::F6 => Some(Action::ToHiragana),
            _ if is_command_chord(key) => Some(Action::CommitAndPassThrough),
            keysym::UP | keysym::DOWN | keysym::TAB | keysym::PAGE_UP | keysym::PAGE_DOWN => {
                Some(Action::NoOp)
            }
            _ => None,
        }
    }

    fn resolve_converting(&self, key: &KeyEvent) -> Option<Action> {
        let m = key.modifiers;
        if let Some(c) = ctrl_letter(key) {
            return match c {
                'n' => Some(Action::NextCand),
                'p' => Some(Action::PrevCand),
                'k' => Some(Action::ToKatakana),
                'h' => Some(Action::Cancel),
                _ => Some(Action::CommitAndPassThrough),
            };
        }
        if let Some(c) = printable(key) {
            return Some(match c.to_digit(10) {
                Some(n) if (1..=9).contains(&n) => Action::SelectCand(n as usize),
                _ => Action::Insert(c),
            });
        }
        if let Some(action) = multi_text(key) {
            return Some(action);
        }
        let dir = |d: Direction| {
            if m.ctrl && m.shift {
                Action::ExtendSelection(d)
            } else if m.shift {
                Action::ResizeSegment(d)
            } else {
                Action::MoveFocus(d)
            }
        };
        match key.keysym {
            keysym::SPACE if m.shift && !m.ctrl && !m.alt && !m.meta => Some(Action::PrevCand),
            keysym::SPACE if !m.ctrl && !m.alt && !m.meta => Some(Action::NextCand),
            keysym::DOWN => Some(Action::NextCand),
            keysym::UP => Some(Action::PrevCand),
            keysym::LEFT => Some(dir(Direction::Left)),
            keysym::RIGHT => Some(dir(Direction::Right)),
            keysym::RETURN => Some(Action::Commit),
            keysym::ESCAPE | keysym::BACKSPACE => Some(Action::Cancel),
            keysym::DELETE if m.ctrl => Some(Action::DeleteLearning),
            keysym::F7 => Some(Action::ToKatakana),
            keysym::F6 => Some(Action::ToHiragana),
            _ if is_command_chord(key) => Some(Action::CommitAndPassThrough),
            keysym::TAB | keysym::PAGE_UP | keysym::PAGE_DOWN | keysym::HOME | keysym::END => {
                Some(Action::NoOp)
            }
            _ => None,
        }
    }
}

/// キーマップの上書き設定で使う Action 名のパース。
///
/// 引数を持たない Action はそのままの名前、`Direction` を持つものは `名前:Left` / `名前:Right`、
/// `SelectCand` は `SelectCand:1`〜`SelectCand:9`。
/// `Insert` / `InsertText` / `SetMode` は対象外（挿入・モード切替は上書きの対象外。設計上の判断）
pub fn parse_action(spec: &str) -> Result<Action, String> {
    let (name, arg) = match spec.split_once(':') {
        Some((n, a)) => (n, Some(a)),
        None => (spec, None),
    };
    let direction = |a: Option<&str>| -> Result<Direction, String> {
        match a {
            Some("Left") => Ok(Direction::Left),
            Some("Right") => Ok(Direction::Right),
            other => Err(format!("Direction が必要です（Left/Right）: {other:?}")),
        }
    };
    match (name, arg) {
        ("StartConvert", None) => Ok(Action::StartConvert),
        ("NextCand", None) => Ok(Action::NextCand),
        ("PrevCand", None) => Ok(Action::PrevCand),
        ("Commit", None) => Ok(Action::Commit),
        ("Cancel", None) => Ok(Action::Cancel),
        ("Backspace", None) => Ok(Action::Backspace),
        ("DeleteForward", None) => Ok(Action::DeleteForward),
        ("MoveCursorHome", None) => Ok(Action::MoveCursorHome),
        ("MoveCursorEnd", None) => Ok(Action::MoveCursorEnd),
        ("DeleteLearning", None) => Ok(Action::DeleteLearning),
        ("ToKatakana", None) => Ok(Action::ToKatakana),
        ("ToHiragana", None) => Ok(Action::ToHiragana),
        ("ToggleMode", None) => Ok(Action::ToggleMode),
        ("CommitAndPassThrough", None) => Ok(Action::CommitAndPassThrough),
        ("NoOp", None) => Ok(Action::NoOp),
        ("MoveCursor", a) => Ok(Action::MoveCursor(direction(a)?)),
        ("MoveFocus", a) => Ok(Action::MoveFocus(direction(a)?)),
        ("ResizeSegment", a) => Ok(Action::ResizeSegment(direction(a)?)),
        ("ExtendSelection", a) => Ok(Action::ExtendSelection(direction(a)?)),
        ("SelectCand", Some(n)) => n
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=9).contains(n))
            .map(Action::SelectCand)
            .ok_or_else(|| format!("SelectCand は 1〜9 の数字が必要です: {n}")),
        _ => Err(format!(
            "不明な Action 名、または引数の付け方が不正です: {spec}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::parse_key;

    fn resolve(ctx: KeyContext, mode: InputMode, key: &str) -> Option<Action> {
        KeyMap::default_preset().resolve(ctx, mode, &parse_key(key).unwrap())
    }
    use InputMode::{Alphanumeric, Hiragana};
    use KeyContext::{Composing, Converting, Direct};

    #[test]
    fn mode_keys_work_everywhere() {
        for ctx in [Direct, Composing, Converting] {
            for mode in [Hiragana, Alphanumeric] {
                assert_eq!(
                    resolve(ctx, mode, "Eisu"),
                    Some(Action::SetMode(Alphanumeric))
                );
                assert_eq!(resolve(ctx, mode, "Kana"), Some(Action::SetMode(Hiragana)));
                assert_eq!(resolve(ctx, mode, "Zenkaku"), Some(Action::ToggleMode));
            }
        }
    }

    #[test]
    fn alphanumeric_passes_everything_else() {
        for key in ["a", "Space", "Enter", "Left", "Ctrl+k", "A"] {
            assert_eq!(resolve(Direct, Alphanumeric, key), None, "{key}");
        }
    }

    #[test]
    fn direct_inserts_printables_only() {
        assert_eq!(resolve(Direct, Hiragana, "a"), Some(Action::Insert('a')));
        assert_eq!(resolve(Direct, Hiragana, "A"), Some(Action::Insert('A')));
        assert_eq!(resolve(Direct, Hiragana, "-"), Some(Action::Insert('-')));
        assert_eq!(resolve(Direct, Hiragana, "1"), Some(Action::Insert('1')));
        for key in [
            "Space",
            "Enter",
            "Escape",
            "BackSpace",
            "Left",
            "Ctrl+a",
            "Meta+c",
            "Alt+a",
            "Tab",
        ] {
            assert_eq!(resolve(Direct, Hiragana, key), None, "{key}");
        }
    }

    #[test]
    fn composing_table() {
        let cases = [
            ("a", Action::Insert('a')),
            ("Shift+a", Action::Insert('A')),
            ("Space", Action::StartConvert),
            ("Shift+Space", Action::StartConvert),
            ("Enter", Action::Commit),
            ("Escape", Action::Cancel),
            ("BackSpace", Action::Backspace),
            ("Ctrl+h", Action::Backspace),
            ("Delete", Action::DeleteForward),
            ("Left", Action::MoveCursor(Direction::Left)),
            ("Right", Action::MoveCursor(Direction::Right)),
            ("Home", Action::MoveCursorHome),
            ("Ctrl+a", Action::MoveCursorHome),
            ("End", Action::MoveCursorEnd),
            ("Ctrl+e", Action::MoveCursorEnd),
            ("Ctrl+k", Action::ToKatakana),
            ("F7", Action::ToKatakana),
            ("F6", Action::ToHiragana),
            ("Meta+c", Action::CommitAndPassThrough),
            ("Ctrl+c", Action::CommitAndPassThrough),
            ("Alt+x", Action::CommitAndPassThrough),
            ("Up", Action::NoOp),
            ("Down", Action::NoOp),
            ("Tab", Action::NoOp),
            ("PageUp", Action::NoOp),
        ];
        for (key, expected) in cases {
            assert_eq!(resolve(Composing, Hiragana, key), Some(expected), "{key}");
        }
        assert_eq!(resolve(Composing, Hiragana, "F1"), None);
    }

    #[test]
    fn converting_table() {
        let cases = [
            ("Space", Action::NextCand),
            ("Down", Action::NextCand),
            ("Ctrl+n", Action::NextCand),
            ("Shift+Space", Action::PrevCand),
            ("Up", Action::PrevCand),
            ("Ctrl+p", Action::PrevCand),
            ("Left", Action::MoveFocus(Direction::Left)),
            ("Right", Action::MoveFocus(Direction::Right)),
            ("Shift+Left", Action::ResizeSegment(Direction::Left)),
            ("Shift+Right", Action::ResizeSegment(Direction::Right)),
            ("Ctrl+Shift+Left", Action::ExtendSelection(Direction::Left)),
            (
                "Ctrl+Shift+Right",
                Action::ExtendSelection(Direction::Right),
            ),
            ("Enter", Action::Commit),
            ("Escape", Action::Cancel),
            ("BackSpace", Action::Cancel),
            ("Ctrl+k", Action::ToKatakana),
            ("F7", Action::ToKatakana),
            ("Ctrl+Delete", Action::DeleteLearning),
            ("F6", Action::ToHiragana),
            ("1", Action::SelectCand(1)),
            ("9", Action::SelectCand(9)),
            ("0", Action::Insert('0')),
            ("a", Action::Insert('a')),
            ("Meta+c", Action::CommitAndPassThrough),
            ("Tab", Action::NoOp),
        ];
        for (key, expected) in cases {
            assert_eq!(resolve(Converting, Hiragana, key), Some(expected), "{key}");
        }
    }

    #[test]
    fn flags_changed_is_never_bound() {
        let mut key = parse_key("Eisu").unwrap();
        key.kind = KeyKind::FlagsChanged;
        assert_eq!(
            KeyMap::default_preset().resolve(Composing, Hiragana, &key),
            None
        );
    }

    #[test]
    fn text_from_layout_wins_over_keysym() {
        // Dvorak 等: keysym と text が食い違う場合は text を使う
        let mut key = parse_key("a").unwrap();
        key.text = "ー".into();
        assert_eq!(
            KeyMap::default_preset().resolve(Direct, Hiragana, &key),
            Some(Action::Insert('ー'))
        );
    }

    #[test]
    fn multi_scalar_text_becomes_insert_text() {
        let mut key = parse_key("a").unwrap();
        key.keysym = 0;
        key.text = "👨\u{200d}👩".into();
        for ctx in [Direct, Composing, Converting] {
            assert_eq!(
                KeyMap::default_preset().resolve(ctx, Hiragana, &key),
                Some(Action::InsertText("👨\u{200d}👩".into())),
                "{ctx:?}"
            );
        }
        // Alphanumeric・修飾付きでは素通し / 確定して素通し
        assert_eq!(
            KeyMap::default_preset().resolve(Direct, Alphanumeric, &key),
            None
        );
        key.modifiers.meta = true;
        assert_eq!(
            KeyMap::default_preset().resolve(Direct, Hiragana, &key),
            None
        );
    }

    #[test]
    fn overrides_replace_composing_key() {
        let (km, warnings) =
            KeyMap::with_overrides(&[(Composing, "Ctrl+j".to_string(), "ToKatakana".to_string())]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            km.resolve(Composing, Hiragana, &parse_key("Ctrl+j").unwrap()),
            Some(Action::ToKatakana)
        );
    }

    #[test]
    fn overrides_leave_unmapped_keys_at_default() {
        let (km, warnings) =
            KeyMap::with_overrides(&[(Composing, "Ctrl+j".to_string(), "ToKatakana".to_string())]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            km.resolve(Composing, Hiragana, &parse_key("a").unwrap()),
            Some(Action::Insert('a'))
        );
    }

    #[test]
    fn overrides_report_unknown_key_and_action() {
        let (km, warnings) = KeyMap::with_overrides(&[
            (Composing, "NotAKey".to_string(), "ToKatakana".to_string()),
            (Composing, "Ctrl+j".to_string(), "NotAnAction".to_string()),
        ]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        // どちらも上書きテーブルには入らず、既定の挙動のまま
        assert_eq!(
            km.resolve(Composing, Hiragana, &parse_key("Ctrl+j").unwrap()),
            Some(Action::CommitAndPassThrough)
        );
    }

    #[test]
    fn parse_action_no_arg() {
        assert_eq!(parse_action("NextCand"), Ok(Action::NextCand));
    }

    #[test]
    fn parse_action_direction() {
        assert_eq!(
            parse_action("MoveCursor:Left"),
            Ok(Action::MoveCursor(Direction::Left))
        );
    }

    #[test]
    fn parse_action_select_cand() {
        assert_eq!(parse_action("SelectCand:3"), Ok(Action::SelectCand(3)));
        assert!(parse_action("SelectCand:0").is_err());
        assert!(parse_action("SelectCand:10").is_err());
    }

    #[test]
    fn parse_action_rejects_insert_and_unknown() {
        assert!(parse_action("Insert").is_err());
        assert!(parse_action("NotAnAction").is_err());
    }
}
