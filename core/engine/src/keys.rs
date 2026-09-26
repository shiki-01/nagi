//! キー名 ⇔ KeyEvent 変換

use nagi_protocol::key::keysym;
use nagi_protocol::key::{KeyEvent, KeyKind, Modifiers};
use thiserror::Error;

/// キー解析エラー
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum KeyParseError {
    /// キー仕様文字列が空
    #[error("空のキー仕様")]
    EmptySpec,

    /// 不明なキー名
    #[error("不明なキー: {0}")]
    UnknownKey(String),

    /// 不正な修飾キー名
    #[error("不正な修飾キー: {0}")]
    InvalidModifier(String),
}

/// キー仕様文字列をパースして KeyEvent を作成
///
/// 書式は `Ctrl+Shift+Right` 形式（修飾は順不同、`+` 区切り）。
/// 修飾キー: `Ctrl`, `Shift`, `Alt`, `Meta` (`Cmd` / `Super` も可）
/// キー: 1文字（そのまま keysym=コードポイント、text=その文字）
///       または名前 `Space`, `Enter`, `Escape`, `BackSpace`, `Delete`, `Tab`,
///       `Left`, `Right`, `Up`, `Down`, `Home`, `End`, `PageUp`, `PageDown`,
///       `F1`～`F12`, `Eisu`, `Kana`, `Zenkaku`, `Muhenkan`, `Henkan`
///
/// **Ctrl/Alt/Meta を伴うキーは text を空**（修飾キーの処理用）。
/// 単独の大文字 ASCII 英字（`A`）は Shift 押下として `shift=true, text="A", keysym='A'`。
/// `Shift+a` も同じ結果。
pub fn parse_key(spec: &str) -> Result<KeyEvent, KeyParseError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(KeyParseError::EmptySpec);
    }

    let parts: Vec<&str> = spec.split('+').collect();
    if parts.is_empty() {
        return Err(KeyParseError::EmptySpec);
    }

    let mut modifiers = Modifiers::default();
    let mut key_name = "";

    // 最後の要素がキー、それより前が修飾
    for i in 0..parts.len() {
        let part = parts[i].trim();
        if part.is_empty() {
            return Err(KeyParseError::UnknownKey(spec.to_string()));
        }

        if i < parts.len() - 1 {
            // 修飾キー
            match part {
                "Ctrl" | "Control" => modifiers.ctrl = true,
                "Shift" => modifiers.shift = true,
                "Alt" | "Option" => modifiers.alt = true,
                "Meta" | "Cmd" | "Super" => modifiers.meta = true,
                other => return Err(KeyParseError::InvalidModifier(other.to_string())),
            }
        } else {
            // キー
            key_name = part;
        }
    }

    // キーを解析
    if key_name.len() == 1 {
        // 1文字キー
        let mut ch = key_name.chars().next().unwrap();

        // Shift + 小文字の英字は、OS の配列が生成する大文字（keysym・text とも大文字）にする
        if modifiers.shift
            && ch.is_ascii_lowercase()
            && !modifiers.ctrl
            && !modifiers.alt
            && !modifiers.meta
        {
            ch = ch.to_ascii_uppercase();
        }

        // 大文字 ASCII 英字なら shift を付与
        if ch.is_ascii_uppercase() {
            modifiers.shift = true;
        }

        // 印字可能文字（0x21..=0x7e）なら text に含める
        let text = if ch as u32 >= 0x20 {
            ch.to_string()
        } else {
            String::new()
        };

        // Ctrl/Alt/Meta が付いていたら text は空
        let text = if modifiers.ctrl || modifiers.alt || modifiers.meta {
            String::new()
        } else {
            text
        };

        // keysym は文字のコードポイント
        let keysym = ch as u32;

        Ok(KeyEvent {
            keysym,
            keycode: 0,
            text,
            modifiers,
            kind: KeyKind::Down,
        })
    } else {
        // 名前付きキー
        let (keysym, text) = parse_named_key(key_name)?;

        // 名前付きキーは修飾があれば text は空
        let text = if modifiers.ctrl || modifiers.alt || modifiers.meta {
            String::new()
        } else if key_name == "Space" {
            " ".to_string()
        } else {
            text.to_string()
        };

        Ok(KeyEvent {
            keysym,
            keycode: 0,
            text,
            modifiers,
            kind: KeyKind::Down,
        })
    }
}

/// 名前付きキーのkeysymとtext（デフォルト）を取得
fn parse_named_key(name: &str) -> Result<(u32, &'static str), KeyParseError> {
    match name {
        "Space" => Ok((keysym::SPACE, " ")),
        "Enter" | "Return" => Ok((keysym::RETURN, "")),
        "Escape" | "Esc" => Ok((keysym::ESCAPE, "")),
        "BackSpace" | "Backspace" | "BS" => Ok((keysym::BACKSPACE, "")),
        "Delete" | "Del" => Ok((keysym::DELETE, "")),
        "Tab" => Ok((keysym::TAB, "")),
        "Left" => Ok((keysym::LEFT, "")),
        "Right" => Ok((keysym::RIGHT, "")),
        "Up" => Ok((keysym::UP, "")),
        "Down" => Ok((keysym::DOWN, "")),
        "Home" => Ok((keysym::HOME, "")),
        "End" => Ok((keysym::END, "")),
        "PageUp" | "Page_Up" => Ok((keysym::PAGE_UP, "")),
        "PageDown" | "Page_Down" => Ok((keysym::PAGE_DOWN, "")),
        "F1" => Ok((keysym::F1, "")),
        "F2" => Ok((keysym::F2, "")),
        "F3" => Ok((keysym::F3, "")),
        "F4" => Ok((keysym::F4, "")),
        "F5" => Ok((keysym::F5, "")),
        "F6" => Ok((keysym::F6, "")),
        "F7" => Ok((keysym::F7, "")),
        "F8" => Ok((keysym::F8, "")),
        "F9" => Ok((keysym::F9, "")),
        "F10" => Ok((keysym::F10, "")),
        "F11" => Ok((keysym::F11, "")),
        "F12" => Ok((keysym::F12, "")),
        "Eisu" => Ok((keysym::EISU_TOGGLE, "")),
        "Kana" | "Hiragana_Katakana" => Ok((keysym::HIRAGANA_KATAKANA, "")),
        "Zenkaku" => Ok((keysym::ZENKAKU_HANKAKU, "")),
        "Muhenkan" => Ok((keysym::MUHENKAN, "")),
        "Henkan" => Ok((keysym::HENKAN, "")),
        other => Err(KeyParseError::UnknownKey(other.to_string())),
    }
}

/// 1 文字を KeyEvent に変換（テスト用）
///
/// keysym=0、修飾なし、kind=Down
pub fn key_from_text(text: &str) -> KeyEvent {
    KeyEvent {
        keysym: 0,
        keycode: 0,
        text: text.to_string(),
        modifiers: Modifiers::default(),
        kind: KeyKind::Down,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_char_lowercase() {
        let key = parse_key("a").unwrap();
        assert_eq!(key.keysym, 'a' as u32);
        assert_eq!(key.text, "a");
        assert!(!key.modifiers.shift);
    }

    #[test]
    fn test_single_char_uppercase() {
        let key = parse_key("A").unwrap();
        assert_eq!(key.keysym, 'A' as u32);
        assert_eq!(key.text, "A");
        assert!(key.modifiers.shift);
    }

    #[test]
    fn test_space() {
        let key = parse_key("Space").unwrap();
        assert_eq!(key.keysym, keysym::SPACE);
        assert_eq!(key.text, " ");
    }

    #[test]
    fn test_enter() {
        let key = parse_key("Enter").unwrap();
        assert_eq!(key.keysym, keysym::RETURN);
        assert_eq!(key.text, "");
    }

    #[test]
    fn test_escape() {
        let key = parse_key("Escape").unwrap();
        assert_eq!(key.keysym, keysym::ESCAPE);
        assert_eq!(key.text, "");
    }

    #[test]
    fn test_backspace() {
        let key = parse_key("BackSpace").unwrap();
        assert_eq!(key.keysym, keysym::BACKSPACE);
        assert_eq!(key.text, "");
    }

    #[test]
    fn test_left() {
        let key = parse_key("Left").unwrap();
        assert_eq!(key.keysym, keysym::LEFT);
        assert_eq!(key.text, "");
    }

    #[test]
    fn test_right() {
        let key = parse_key("Right").unwrap();
        assert_eq!(key.keysym, keysym::RIGHT);
        assert_eq!(key.text, "");
    }

    #[test]
    fn test_ctrl_a() {
        let key = parse_key("Ctrl+a").unwrap();
        assert_eq!(key.keysym, 'a' as u32);
        assert!(key.modifiers.ctrl);
        assert_eq!(key.text, ""); // Ctrl 付きは text なし
    }

    #[test]
    fn test_shift_right() {
        let key = parse_key("Shift+Right").unwrap();
        assert_eq!(key.keysym, keysym::RIGHT);
        assert!(key.modifiers.shift);
        assert_eq!(key.text, "");
    }

    #[test]
    fn test_ctrl_shift_right() {
        let key = parse_key("Ctrl+Shift+Right").unwrap();
        assert_eq!(key.keysym, keysym::RIGHT);
        assert!(key.modifiers.ctrl);
        assert!(key.modifiers.shift);
    }

    #[test]
    fn test_modifiers_order_irrelevant() {
        let key1 = parse_key("Ctrl+Shift+Right").unwrap();
        let key2 = parse_key("Shift+Ctrl+Right").unwrap();
        assert_eq!(key1.keysym, key2.keysym);
        assert_eq!(key1.modifiers.ctrl, key2.modifiers.ctrl);
        assert_eq!(key1.modifiers.shift, key2.modifiers.shift);
    }

    #[test]
    fn test_empty_spec() {
        assert_eq!(parse_key(""), Err(KeyParseError::EmptySpec));
    }

    #[test]
    fn test_unknown_key() {
        assert!(matches!(
            parse_key("UnknownKey"),
            Err(KeyParseError::UnknownKey(_))
        ));
    }

    #[test]
    fn test_f1_to_f12() {
        for i in 1..=12 {
            let name = format!("F{}", i);
            let key = parse_key(&name).unwrap();
            assert!(key.text.is_empty());
        }
    }

    #[test]
    fn test_key_from_text() {
        let key = key_from_text("a");
        assert_eq!(key.text, "a");
        assert_eq!(key.keysym, 0);
        assert!(!key.modifiers.shift);
    }

    #[test]
    fn test_key_from_text_emoji() {
        let key = key_from_text("😀");
        assert_eq!(key.text, "😀");
    }
}
