//! UTF-16 ユーティリティ（offset 変換）
//!
//! プロトコルではすべての位置を UTF-16 コード単位で表現します。
//! この モジュールは Rust の内部表現（Unicode スカラー値）との変換を提供します。

/// 文字列の UTF-16 コード単位での長さを返す
///
/// # 例
///
/// ```
/// use nagi_protocol::utf16::utf16_len;
///
/// // ASCII は 1 UTF-16 コード単位
/// assert_eq!(utf16_len("hello"), 5);
///
/// // 日本語（2～3 bytes in UTF-8）は 1 UTF-16 コード単位
/// assert_eq!(utf16_len("こんにちは"), 5);
///
/// // 絵文字（サロゲートペア）は 2 UTF-16 コード単位
/// assert_eq!(utf16_len("😀"), 2);
/// ```
pub fn utf16_len(s: &str) -> u32 {
    let mut count = 0u32;
    for ch in s.chars() {
        count += if (ch as u32) > 0xffff { 2 } else { 1 };
    }
    count
}

/// Unicode スカラー値のインデックスを UTF-16 コード単位の offset に変換
///
/// # Arguments
///
/// * `s` - 対象の文字列
/// * `char_index` - 何番目の Unicode スカラー値か（0-based）
///
/// # Returns
///
/// その文字が始まる UTF-16 コード単位での位置。文字列の長さ以上なら None を返す。
///
/// # 例
///
/// ```
/// use nagi_protocol::utf16::char_to_utf16_offset;
///
/// let s = "hello";
/// assert_eq!(char_to_utf16_offset(s, 0), Some(0));
/// assert_eq!(char_to_utf16_offset(s, 5), Some(5));
/// assert_eq!(char_to_utf16_offset(s, 6), None);
///
/// let s = "😀x";  // 絵文字 + x
/// assert_eq!(char_to_utf16_offset(s, 0), Some(0));    // 絵文字は UTF-16 offset 0
/// assert_eq!(char_to_utf16_offset(s, 1), Some(2));    // x は UTF-16 offset 2
/// assert_eq!(char_to_utf16_offset(s, 2), Some(3));    // EOF
/// ```
pub fn char_to_utf16_offset(s: &str, char_index: usize) -> Option<u32> {
    let mut offset = 0u32;
    for (i, ch) in s.chars().enumerate() {
        if i == char_index {
            return Some(offset);
        }
        offset += if (ch as u32) > 0xffff { 2 } else { 1 };
    }
    if char_index == s.chars().count() {
        Some(offset)
    } else {
        None
    }
}

/// UTF-16 コード単位の offset を Unicode スカラー値のインデックスに変換
///
/// # Arguments
///
/// * `s` - 対象の文字列
/// * `offset` - UTF-16 コード単位での位置
///
/// # Returns
///
/// その offset が指す Unicode スカラー値のインデックス。
/// offset がサロゲートペアの途中を指す場合は None を返す。
///
/// # 例
///
/// ```
/// use nagi_protocol::utf16::utf16_to_char_index;
///
/// let s = "hello";
/// assert_eq!(utf16_to_char_index(s, 0), Some(0));
/// assert_eq!(utf16_to_char_index(s, 5), Some(5));
/// assert_eq!(utf16_to_char_index(s, 6), None);
///
/// let s = "😀x";  // 絵文字 + x
/// assert_eq!(utf16_to_char_index(s, 0), Some(0));  // 絵文字開始
/// assert_eq!(utf16_to_char_index(s, 1), None);     // サロゲートペアの途中
/// assert_eq!(utf16_to_char_index(s, 2), Some(1));  // x
/// assert_eq!(utf16_to_char_index(s, 3), Some(2));  // EOF
/// ```
pub fn utf16_to_char_index(s: &str, offset: u32) -> Option<usize> {
    let mut current_offset = 0u32;
    for (i, ch) in s.chars().enumerate() {
        if current_offset == offset {
            return Some(i);
        }
        let size = if (ch as u32) > 0xffff { 2 } else { 1 };
        current_offset += size;
    }
    if current_offset == offset {
        Some(s.chars().count())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf16_len_ascii() {
        assert_eq!(utf16_len("hello"), 5);
        assert_eq!(utf16_len("a"), 1);
        assert_eq!(utf16_len(""), 0);
    }

    #[test]
    fn test_utf16_len_japanese() {
        assert_eq!(utf16_len("こんにちは"), 5);
        assert_eq!(utf16_len("あ"), 1);
    }

    #[test]
    fn test_utf16_len_emoji() {
        // 絵文字（サロゲートペア）は 2 コード単位
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!(utf16_len("🎉"), 2);
    }

    #[test]
    fn test_utf16_len_mixed() {
        // "a😀b" = a(1) + 😀(2) + b(1) = 4
        assert_eq!(utf16_len("a😀b"), 4);
        // "あ😀い" = あ(1) + 😀(2) + い(1) = 4
        assert_eq!(utf16_len("あ😀い"), 4);
    }

    #[test]
    fn test_char_to_utf16_offset_ascii() {
        let s = "hello";
        assert_eq!(char_to_utf16_offset(s, 0), Some(0));
        assert_eq!(char_to_utf16_offset(s, 1), Some(1));
        assert_eq!(char_to_utf16_offset(s, 5), Some(5));
        assert_eq!(char_to_utf16_offset(s, 6), None);
    }

    #[test]
    fn test_char_to_utf16_offset_emoji() {
        let s = "😀x";
        assert_eq!(char_to_utf16_offset(s, 0), Some(0));
        assert_eq!(char_to_utf16_offset(s, 1), Some(2));
        assert_eq!(char_to_utf16_offset(s, 2), Some(3)); // EOF position
        assert_eq!(char_to_utf16_offset(s, 3), None); // beyond end
    }

    #[test]
    fn test_char_to_utf16_offset_mixed() {
        let s = "a😀b";
        assert_eq!(char_to_utf16_offset(s, 0), Some(0));
        assert_eq!(char_to_utf16_offset(s, 1), Some(1));
        assert_eq!(char_to_utf16_offset(s, 2), Some(3));
        assert_eq!(char_to_utf16_offset(s, 3), Some(4)); // EOF position
        assert_eq!(char_to_utf16_offset(s, 4), None); // beyond end
    }

    #[test]
    fn test_utf16_to_char_index_ascii() {
        let s = "hello";
        assert_eq!(utf16_to_char_index(s, 0), Some(0));
        assert_eq!(utf16_to_char_index(s, 1), Some(1));
        assert_eq!(utf16_to_char_index(s, 5), Some(5));
        assert_eq!(utf16_to_char_index(s, 6), None);
    }

    #[test]
    fn test_utf16_to_char_index_emoji() {
        let s = "😀x";
        assert_eq!(utf16_to_char_index(s, 0), Some(0));
        assert_eq!(utf16_to_char_index(s, 1), None); // サロゲートペアの途中
        assert_eq!(utf16_to_char_index(s, 2), Some(1));
        assert_eq!(utf16_to_char_index(s, 3), Some(2)); // EOF position
        assert_eq!(utf16_to_char_index(s, 4), None); // beyond end
    }

    #[test]
    fn test_utf16_to_char_index_mixed() {
        let s = "a😀b";
        assert_eq!(utf16_to_char_index(s, 0), Some(0));
        assert_eq!(utf16_to_char_index(s, 1), Some(1));
        assert_eq!(utf16_to_char_index(s, 2), None); // サロゲートペアの途中
        assert_eq!(utf16_to_char_index(s, 3), Some(2));
        assert_eq!(utf16_to_char_index(s, 4), Some(3)); // EOF position
        assert_eq!(utf16_to_char_index(s, 5), None); // beyond end
    }

    #[test]
    fn test_round_trip_ascii() {
        let s = "hello";
        for i in 0..=s.chars().count() {
            let offset = char_to_utf16_offset(s, i).unwrap();
            assert_eq!(utf16_to_char_index(s, offset), Some(i));
        }
    }

    #[test]
    fn test_round_trip_mixed() {
        let s = "a😀b";
        for i in 0..=s.chars().count() {
            let offset = char_to_utf16_offset(s, i).unwrap();
            assert_eq!(utf16_to_char_index(s, offset), Some(i));
        }
    }

    #[test]
    fn test_combined_characters() {
        // "が" = か + ゛（濁音点）= 2 Unicode スカラー値
        let s = "が"; // U+304C または U+304B + U+3099
        let len = utf16_len(s);
        assert!(len >= 1); // 少なくとも 1 コード単位
    }

    #[test]
    fn test_surrogates_in_middle() {
        // サロゲートペアの途中を指す offset は None を返す
        let s = "😀"; // サロゲートペア
        assert_eq!(utf16_to_char_index(s, 0), Some(0));
        assert_eq!(utf16_to_char_index(s, 1), None);
        assert_eq!(utf16_to_char_index(s, 2), Some(1));
    }
}
