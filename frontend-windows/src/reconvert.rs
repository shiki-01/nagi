//! 確定済みテキストの再変換の入口の判定（OS 非依存）。macOS 版 `reconvertSelection` と同じ規約。

/// 再変換の対象にする選択の最大長（UTF-16 コード単位。macOS 版の `reconvertMaxLength` と同じ）
pub const RECONVERT_MAX_UTF16: usize = 200;

/// 再変換にかけるテキスト。空・長すぎる選択は対象外（何もしない）
pub fn eligible_text(text: &str) -> Option<&str> {
    if text.is_empty() {
        return None;
    }
    let len: usize = text.chars().map(char::len_utf16).sum();
    (len <= RECONVERT_MAX_UTF16).then_some(text)
}

/// 取得時に「最大長 + 1」だけ読めば長すぎるかを判定できる。読む範囲の上限（UTF-16）
pub const RECONVERT_READ_LIMIT: usize = RECONVERT_MAX_UTF16 + 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_too_long_are_not_eligible() {
        assert_eq!(eligible_text(""), None);
        assert_eq!(eligible_text("今日"), Some("今日"));
        let max = "あ".repeat(RECONVERT_MAX_UTF16);
        assert!(eligible_text(&max).is_some());
        assert!(eligible_text(&format!("{max}あ")).is_none());
    }

    #[test]
    fn counts_utf16_units_not_chars() {
        // サロゲートペアは 2 単位
        let s = "😀".repeat(RECONVERT_MAX_UTF16 / 2);
        assert!(eligible_text(&s).is_some());
        assert!(eligible_text(&format!("{s}😀")).is_none());
    }
}
