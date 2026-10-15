//! ひらがな → ローマ字の逆変換。
//!
//! Composer が保持している実際の打鍵列は engine から渡されないため（バックエンドの入力は読みだけ）、
//! 読みから「打たれたはずの」ローマ字を復元する。1 つの仮名に複数の綴りがある（si / shi など）ので、
//! 最短の綴りを選ぶ。実際の綴りと違うと隣接キーの当て推量がずれる（M2 で実際の打鍵列を渡す余地）。

use nagi_romaji::RomajiTable;
use std::collections::BTreeMap;

/// 打ちにくい・珍しい綴りの先頭文字。最短が同じ長さなら避ける（`ci`, `la`, `xa`, `qa` など）
const DISCOURAGED_INITIALS: &str = "clxq";

/// 仮名 → ローマ字の逆引き
#[derive(Debug, Clone)]
pub struct ReverseRomaji {
    /// 仮名（1〜2 文字）→ 最短の綴り
    best: BTreeMap<String, String>,
}

impl ReverseRomaji {
    /// ローマ字テーブルから作る（保留付きのエントリ = 促音の `kk` などは使わない）
    pub fn new(table: &RomajiTable) -> Self {
        let mut best: BTreeMap<String, String> = BTreeMap::new();
        // 英字だけの綴りを優先し、次に短いもの、打ちにくい先頭文字を避け、最後は辞書順（決定的）
        let key = |s: &str| {
            (
                !s.chars().all(|c| c.is_ascii_lowercase()),
                s.len(),
                s.chars()
                    .next()
                    .is_some_and(|c| DISCOURAGED_INITIALS.contains(c)),
                s.to_string(),
            )
        };
        for (input, output, pending) in table.entries() {
            if !pending.is_empty() || output.is_empty() {
                continue;
            }
            match best.get(output) {
                Some(cur) if key(cur) <= key(input) => {}
                _ => {
                    best.insert(output.to_string(), input.to_string());
                }
            }
        }
        ReverseRomaji { best }
    }

    /// 読みをローマ字にする。英小文字はそのまま（Composer が未変換のまま残したもの）。
    /// 変換できない文字（カタカナ・漢字・未知の記号）があれば `None`
    pub fn to_romaji(&self, reading: &str) -> Option<String> {
        enum Token {
            Letter(char),
            Sokuon,
            Hatsuon,
            Spelling(String),
        }
        let chars: Vec<char> = reading.chars().collect();
        let mut tokens: Vec<Token> = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_lowercase() {
                tokens.push(Token::Letter(c));
            } else if c == 'っ' {
                tokens.push(Token::Sokuon);
            } else if c == 'ん' {
                tokens.push(Token::Hatsuon);
            } else {
                // 長い仮名（きゃ 等）を優先
                let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
                if two.chars().count() == 2
                    && let Some(r) = self.best.get(&two)
                {
                    tokens.push(Token::Spelling(r.clone()));
                    i += 2;
                    continue;
                }
                tokens.push(Token::Spelling(self.best.get(&c.to_string())?.clone()));
            }
            i += 1;
        }
        // 次のトークンの綴りの先頭文字（促音の子音重ね・撥音の `nn` の判定用）
        let initial = |t: Option<&Token>| match t {
            Some(Token::Letter(c)) => Some(*c),
            Some(Token::Spelling(s)) => s.chars().next(),
            Some(Token::Sokuon) => Some('x'),
            Some(Token::Hatsuon) => Some('n'),
            None => None,
        };
        let mut out = String::new();
        for (k, token) in tokens.iter().enumerate() {
            let next = initial(tokens.get(k + 1));
            match token {
                Token::Letter(c) => out.push(*c),
                Token::Spelling(s) => out.push_str(s),
                Token::Sokuon => match next {
                    // 子音なら重ねる（`kk` → っ + k）。母音・n・語末は `xtu`
                    Some(n) if n.is_ascii_lowercase() && !"aiueon".contains(n) => out.push(n),
                    _ => out.push_str("xtu"),
                },
                // 母音・y・n が続くと `n` 1 つでは次と結びつくので `nn`。語末の `n` は変換器の flush で「ん」
                Token::Hatsuon => match next {
                    Some(n) if "aiueony".contains(n) => out.push_str("nn"),
                    _ => out.push('n'),
                },
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_romaji::{Emit, RomajiConverter};

    fn reverse() -> ReverseRomaji {
        ReverseRomaji::new(&RomajiTable::standard())
    }

    /// ローマ字 → かな（語末の保留は flush）
    fn forward(romaji: &str) -> String {
        let mut conv = RomajiConverter::new(RomajiTable::standard());
        let mut out = String::new();
        let mut push = |emits: Vec<Emit>| {
            for e in emits {
                match e {
                    Emit::Converted(s) => out.push_str(&s),
                    Emit::Raw(c) => out.push(c),
                }
            }
        };
        for c in romaji.chars() {
            push(conv.feed(c));
        }
        push(conv.flush());
        out
    }

    #[test]
    fn round_trip_through_converter() {
        let r = reverse();
        for reading in [
            "かんじ",
            "きょう",
            "しんぶん",
            "がっこう",
            "ちゃ",
            "んあ",
            "かんい",
            "ーー",
            "てんき",
            "ふじさん",
            "じゃんぷ",
            "っあ",
            "あっ",
            "ぱぴぷぺぽ",
            "ゔぁ",
            "しゅっぱつ",
        ] {
            let romaji = r.to_romaji(reading).unwrap_or_else(|| panic!("{reading}"));
            assert_eq!(forward(&romaji), reading, "{reading} -> {romaji}");
        }
    }

    #[test]
    fn keeps_ascii_and_rejects_unknown() {
        let r = reverse();
        assert_eq!(r.to_romaji("かmじ").as_deref(), Some("kamji"));
        assert_eq!(r.to_romaji("かんじ").as_deref(), Some("kanji"));
        assert_eq!(r.to_romaji("きょう").as_deref(), Some("kyou"));
        assert_eq!(r.to_romaji("がっこう").as_deref(), Some("gakkou"));
        assert_eq!(r.to_romaji("カンジ"), None);
        assert_eq!(r.to_romaji("漢字"), None);
    }
}
