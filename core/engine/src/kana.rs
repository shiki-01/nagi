//! かな変換の小さなユーティリティ。

/// ひらがなをカタカナにする（それ以外の文字はそのまま。長音 `ー` も維持）
pub fn to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3041}'..='\u{3096}' | '\u{309d}'..='\u{309e}' => {
                char::from_u32(c as u32 + 0x60).unwrap_or(c)
            }
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiragana_becomes_katakana() {
        assert_eq!(to_katakana("きょうは"), "キョウハ");
        assert_eq!(to_katakana("ゔぁ"), "ヴァ");
        assert_eq!(to_katakana("らーめん"), "ラーメン");
    }

    #[test]
    fn other_characters_are_kept() {
        assert_eq!(to_katakana("abc123、。漢字カナ"), "abc123、。漢字カナ");
        assert_eq!(to_katakana(""), "");
    }
}
