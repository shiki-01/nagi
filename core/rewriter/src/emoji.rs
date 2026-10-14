//! 絵文字・顔文字の読み表（`config/emoji.tsv`）からの候補。
//!
//! 表はバイナリに `include_str!` で同梱する。形式は `読み<TAB>候補1<TAB>候補2...`（UTF-8）、
//! `#` 始まりはコメント、`[emoji]` / `[kaomoji]` のセクション行で以降の種類を切り替える。
//! 読みが完全一致したときだけ候補を返す（前方一致・部分一致はしない）。

use crate::Rewriter;
use std::collections::HashMap;
use std::sync::OnceLock;

/// 同梱の表（`config/emoji.tsv`）
const BUILTIN_TSV: &str = include_str!("../../../config/emoji.tsv");

/// 読み → 候補（表の順）
type Entries = HashMap<String, Vec<String>>;

/// 絵文字・顔文字の読み表
#[derive(Debug, Clone, Default)]
pub struct EmojiTable {
    emoji: Entries,
    kaomoji: Entries,
}

impl EmojiTable {
    /// 同梱の表（初回に一度だけ解析する）
    pub fn builtin() -> &'static EmojiTable {
        static TABLE: OnceLock<EmojiTable> = OnceLock::new();
        TABLE.get_or_init(|| EmojiTable::parse(BUILTIN_TSV))
    }

    /// TSV から表を作る。セクション行より前の行・不明なセクションの行・候補が無い行は無視する。
    /// 同じセクションで読みが重複したら、候補を後ろに足す（重複する候補は足さない）
    pub fn parse(tsv: &str) -> EmojiTable {
        #[derive(Clone, Copy)]
        enum Section {
            None,
            Emoji,
            Kaomoji,
        }
        let mut table = EmojiTable::default();
        let mut section = Section::None;
        for line in tsv.lines() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            match line.trim() {
                "[emoji]" => {
                    section = Section::Emoji;
                    continue;
                }
                "[kaomoji]" => {
                    section = Section::Kaomoji;
                    continue;
                }
                s if s.starts_with('[') && s.ends_with(']') => {
                    section = Section::None;
                    continue;
                }
                _ => {}
            }
            let target = match section {
                Section::None => continue,
                Section::Emoji => &mut table.emoji,
                Section::Kaomoji => &mut table.kaomoji,
            };
            let mut fields = line.split('\t');
            let reading = fields.next().unwrap_or("").trim();
            if reading.is_empty() {
                continue;
            }
            let list = target.entry(reading.to_string()).or_default();
            let before = list.len();
            for cand in fields.filter(|c| !c.is_empty()) {
                if !list.iter().any(|x| x == cand) {
                    list.push(cand.to_string());
                }
            }
            if list.len() == before && before == 0 {
                target.remove(reading);
            }
        }
        table
    }

    /// 絵文字の読みの数
    pub fn emoji_readings(&self) -> usize {
        self.emoji.len()
    }

    /// 顔文字の読みの数
    pub fn kaomoji_readings(&self) -> usize {
        self.kaomoji.len()
    }

    /// 絵文字の（読み, 候補）の一覧（検査用）
    pub fn emoji_entries(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.emoji.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// 顔文字の（読み, 候補）の一覧（検査用）
    pub fn kaomoji_entries(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.kaomoji.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// `reading` に一致する候補（絵文字 → 顔文字の順。無効な種類は含めない）
    pub fn lookup(&self, reading: &str, emoji: bool, kaomoji: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let sources = [(emoji, &self.emoji), (kaomoji, &self.kaomoji)];
        for (enabled, entries) in sources {
            if !enabled {
                continue;
            }
            for cand in entries.get(reading).into_iter().flatten() {
                if !out.contains(cand) {
                    out.push(cand.clone());
                }
            }
        }
        out
    }
}

/// 読みから絵文字・顔文字の候補を出す Rewriter（同梱の表を使う）
#[derive(Debug, Clone, Copy)]
pub struct EmojiRewriter {
    /// 絵文字を出すか
    pub emoji: bool,
    /// 顔文字を出すか
    pub kaomoji: bool,
}

impl Default for EmojiRewriter {
    fn default() -> Self {
        EmojiRewriter {
            emoji: true,
            kaomoji: true,
        }
    }
}

impl Rewriter for EmojiRewriter {
    fn id(&self) -> &str {
        "emoji"
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        if reading.is_empty() || !(self.emoji || self.kaomoji) {
            return Vec::new();
        }
        EmojiTable::builtin().lookup(reading, self.emoji, self.kaomoji)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_table_loads_with_expected_sizes() {
        let t = EmojiTable::builtin();
        assert!(t.emoji_readings() >= 250, "{}", t.emoji_readings());
        assert!(t.kaomoji_readings() >= 80, "{}", t.kaomoji_readings());
    }

    #[test]
    fn builtin_table_has_no_duplicates_or_empty_candidates() {
        let mut seen_emoji = std::collections::HashSet::new();
        let mut seen_kao = std::collections::HashSet::new();
        // 生のファイルを行ごとに検査する（解析で吸収された重複も検出する）
        let mut section = "";
        for line in BUILTIN_TSV.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                section = if line.trim() == "[emoji]" {
                    "emoji"
                } else {
                    "kaomoji"
                };
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            assert!(fields.len() >= 2, "候補が無い行: {line}");
            assert!(fields.iter().all(|f| !f.is_empty()), "空の欄: {line}");
            let seen = if section == "emoji" {
                &mut seen_emoji
            } else {
                &mut seen_kao
            };
            assert!(seen.insert(fields[0]), "読みの重複 [{section}]: {}", fields[0]);
            let mut cands = std::collections::HashSet::new();
            for c in &fields[1..] {
                assert!(cands.insert(*c), "候補の重複: {line}");
            }
        }
    }

    #[test]
    fn readings_are_hiragana() {
        let t = EmojiTable::builtin();
        for (r, _) in t.emoji_entries().chain(t.kaomoji_entries()) {
            assert!(
                r.chars()
                    .all(|c| ('\u{3041}'..='\u{3096}').contains(&c) || c == 'ー'),
                "読みはひらがなのみ: {r}"
            );
        }
    }

    #[test]
    fn lookup_examples() {
        let t = EmojiTable::builtin();
        assert_eq!(t.lookup("わらう", true, false)[..3], ["😄", "😆", "🤣"]);
        assert!(t.lookup("ありがとう", true, false).contains(&"🙏".to_string()));
        assert!(t.lookup("ねこ", true, false).starts_with(&["🐱".into(), "🐈".into()]));
        assert!(t.lookup("かおもじ", false, true).contains(&"(^_^)".to_string()));
        assert!(t.lookup("かおもじ", true, false).is_empty());
        assert!(t.lookup("えがお", false, true).contains(&"(＾▽＾)".to_string()));
        assert!(t.lookup("はーと", true, false).contains(&"❤\u{fe0f}".to_string()));
    }

    #[test]
    fn lookup_order_emoji_before_kaomoji() {
        let t = EmojiTable::builtin();
        let both = t.lookup("えがお", true, true);
        let first_kao = both.iter().position(|c| c.starts_with('(')).unwrap();
        assert!(both[..first_kao].iter().all(|c| !c.starts_with('(')));
        assert!(first_kao > 0);
    }

    #[test]
    fn exact_match_only() {
        let t = EmojiTable::builtin();
        assert!(t.lookup("わら", true, true).is_empty());
        assert!(t.lookup("わらうひと", true, true).is_empty());
        assert!(t.lookup("", true, true).is_empty());
    }

    #[test]
    fn rewriter_respects_flags() {
        let on = EmojiRewriter::default();
        assert!(!on.rewrite("えがお").is_empty());
        let only_emoji = EmojiRewriter {
            emoji: true,
            kaomoji: false,
        };
        assert!(only_emoji.rewrite("えがお").iter().all(|c| !c.starts_with('(')));
        let off = EmojiRewriter {
            emoji: false,
            kaomoji: false,
        };
        assert!(off.rewrite("わらう").is_empty());
        assert!(off.rewrite("かおもじ").is_empty());
    }

    #[test]
    fn parse_handles_comments_sections_and_dups() {
        let t = EmojiTable::parse("あ\tx\n# c\n[emoji]\nいう\ta\tb\ta\n\nいう\tc\tb\nえお\n[kaomoji]\nいう\tk\n");
        assert_eq!(t.lookup("いう", true, false), ["a", "b", "c"]);
        assert_eq!(t.lookup("いう", false, true), ["k"]);
        assert!(t.lookup("あ", true, true).is_empty());
        assert!(t.lookup("えお", true, true).is_empty());
    }
}
