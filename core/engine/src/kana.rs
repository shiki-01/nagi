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

/// 読みの 1 文字をひらがなへ正規化し、日本語（`true`）か Literal（`false`）かを返す。
///
/// ひらがな・長音・CJK 記号（`、。「」・` 等）は日本語。カタカナはひらがなにして日本語。
/// それ以外（英数・半角カナ・漢字など）はそのままの文字で Literal
pub fn normalize_reading_char(c: char) -> (char, bool) {
    match c {
        '\u{3041}'..='\u{3096}' | '\u{309d}'..='\u{309f}' | '\u{3000}'..='\u{303f}' => (c, true),
        '\u{30fc}' | '\u{30fb}' => (c, true),
        '\u{30a1}'..='\u{30f6}' | '\u{30fd}'..='\u{30fe}' => {
            (char::from_u32(c as u32 - 0x60).unwrap_or(c), true)
        }
        _ => (c, false),
    }
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
    fn reading_char_normalization() {
        assert_eq!(normalize_reading_char('か'), ('か', true));
        assert_eq!(normalize_reading_char('カ'), ('か', true));
        assert_eq!(normalize_reading_char('ー'), ('ー', true));
        assert_eq!(normalize_reading_char('、'), ('、', true));
        assert_eq!(normalize_reading_char('a'), ('a', false));
        assert_eq!(normalize_reading_char('漢'), ('漢', false));
    }

    #[test]
    fn other_characters_are_kept() {
        assert_eq!(to_katakana("abc123、。漢字カナ"), "abc123、。漢字カナ");
        assert_eq!(to_katakana(""), "");
    }
}
