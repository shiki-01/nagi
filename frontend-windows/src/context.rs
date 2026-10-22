//! `set_context` の params を作る（OS 非依存）。macOS 版 `NagiInputController` の
//! `reportContext` / `textBeforeCaret` と同じ規約。
//!
//! - secure（パスワード欄など）のときは周辺テキストを**読まず、送らない**（CLAUDE.md の停止条件）。
//!   `secure_input: true` は常に伝え、学習・ログの停止はコアが判断する
//! - 周辺テキストを取得できないアプリでは `surrounding_text` を送らない（コアは前回の文脈を維持する）。
//!   空文字列を送ると次語候補が消えるため、「取得できた結果が空（キャレットが先頭）」とは区別する
//! - 送る長さはキャレット直前の最大 [`CONTEXT_UTF16`] UTF-16 コード単位

use nagi_protocol::{InputMode, SetContextParams, SurroundingText};

/// コアへ送る直前テキストの最大長（UTF-16 コード単位。コアは末尾 40 文字だけを文脈に使う）
pub const CONTEXT_UTF16: usize = 64;

/// `InputScope` の `IS_PASSWORD`（inputscope.h の値。windows クレートの定数と一致することを実機層で確認する）
pub const INPUT_SCOPE_PASSWORD: i32 = 31;

/// キャレット直前のテキストの取得結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurroundingRead {
    /// アプリが対応していない・取得に失敗した
    Unavailable,
    /// 取得できた（キャレットより前の部分。キャレットが先頭なら空）
    Text(String),
}

/// 入力欄が機密（パスワード等）か。TSF の `GUID_PROP_INPUTSCOPE` の値の列から判定する
pub fn is_secure_input_scope(scopes: &[i32]) -> bool {
    scopes.contains(&INPUT_SCOPE_PASSWORD)
}

/// 末尾から最大 `max_utf16` コード単位だけ残す（サロゲートペアの途中では切らない）
pub fn tail_utf16(text: &str, max_utf16: usize) -> &str {
    let total: usize = text.chars().map(char::len_utf16).sum();
    if total <= max_utf16 {
        return text;
    }
    let mut skip = total - max_utf16;
    for (i, c) in text.char_indices() {
        if skip == 0 {
            return &text[i..];
        }
        let w = c.len_utf16();
        // 分割位置がペアの途中になるときは、そのペアも落とす
        skip = skip.saturating_sub(w);
    }
    ""
}

/// アプリ ID（実行ファイル名など）。空文字列は未取得として扱う
fn normalize_app_id(app_id: Option<&str>) -> Option<String> {
    app_id.filter(|s| !s.is_empty()).map(str::to_owned)
}

/// `set_context` の params を作る。`secure` のときは `read` を無視する（呼び出し側も読まないこと）
pub fn build_context(
    session_id: &str,
    secure: bool,
    app_id: Option<&str>,
    read: SurroundingRead,
) -> SetContextParams {
    let surrounding_text = if secure {
        None
    } else {
        match read {
            SurroundingRead::Unavailable => None,
            SurroundingRead::Text(text) => {
                let tail = tail_utf16(&text, CONTEXT_UTF16);
                let cursor = u32::try_from(tail.encode_utf16().count()).unwrap_or(u32::MAX);
                Some(SurroundingText {
                    text: tail.to_owned(),
                    cursor,
                })
            }
        }
    };
    SetContextParams {
        session_id: session_id.to_owned(),
        surrounding_text,
        app_id: normalize_app_id(app_id),
        secure_input: Some(secure),
    }
}

/// キー処理の前に文脈を送るべきか。入力開始時（preedit が空）、ひらがなモード、
/// 修飾キー単独（`flags_changed`）以外のとき
pub fn should_report_before_key(
    mode: InputMode,
    preedit_is_empty: bool,
    is_flags_changed: bool,
) -> bool {
    !is_flags_changed && mode == InputMode::Hiragana && preedit_is_empty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_never_sends_surrounding_text() {
        let p = build_context(
            "s",
            true,
            Some("app.exe"),
            SurroundingRead::Text("秘密".into()),
        );
        assert_eq!(p.surrounding_text, None);
        assert_eq!(p.secure_input, Some(true));
        assert_eq!(p.app_id.as_deref(), Some("app.exe"));
    }

    #[test]
    fn unavailable_sends_no_surrounding_text() {
        let p = build_context("s", false, None, SurroundingRead::Unavailable);
        assert_eq!(p.surrounding_text, None);
        assert_eq!(p.secure_input, Some(false));
        assert_eq!(p.app_id, None);
    }

    #[test]
    fn empty_text_is_sent_as_empty_not_omitted() {
        let p = build_context("s", false, None, SurroundingRead::Text(String::new()));
        assert_eq!(
            p.surrounding_text,
            Some(SurroundingText {
                text: String::new(),
                cursor: 0
            })
        );
    }

    #[test]
    fn cursor_is_utf16_length_of_sent_text() {
        let p = build_context("s", false, None, SurroundingRead::Text("a😀b".into()));
        let s = p.surrounding_text.unwrap();
        assert_eq!(s.text, "a😀b");
        assert_eq!(s.cursor, 4);
    }

    #[test]
    fn long_text_is_trimmed_to_tail_without_splitting_pairs() {
        let text = "あ".repeat(100);
        let p = build_context("s", false, None, SurroundingRead::Text(text));
        let s = p.surrounding_text.unwrap();
        assert_eq!(s.text.chars().count(), 64);
        assert_eq!(s.cursor, 64);

        // 先頭側の 1 単位を落とすとサロゲートペアの途中になる場合は、そのペアごと落とす
        let text = format!("😀{}", "x".repeat(CONTEXT_UTF16 - 1));
        assert_eq!(
            tail_utf16(&text, CONTEXT_UTF16),
            "x".repeat(CONTEXT_UTF16 - 1)
        );
        assert_eq!(tail_utf16("abc", 10), "abc");
        assert_eq!(tail_utf16("abc", 0), "");
        assert_eq!(tail_utf16("abc", 2), "bc");
    }

    #[test]
    fn empty_app_id_is_omitted() {
        let p = build_context("s", false, Some(""), SurroundingRead::Unavailable);
        assert_eq!(p.app_id, None);
    }

    #[test]
    fn password_input_scope_detected() {
        assert!(is_secure_input_scope(&[0, INPUT_SCOPE_PASSWORD]));
        assert!(!is_secure_input_scope(&[0, 1, 5]));
        assert!(!is_secure_input_scope(&[]));
    }

    #[test]
    fn report_before_key_conditions() {
        assert!(should_report_before_key(InputMode::Hiragana, true, false));
        assert!(!should_report_before_key(InputMode::Hiragana, false, false));
        assert!(!should_report_before_key(
            InputMode::Alphanumeric,
            true,
            false
        ));
        assert!(!should_report_before_key(InputMode::Hiragana, true, true));
    }
}
