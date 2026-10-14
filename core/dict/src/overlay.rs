//! ユーザー辞書を Viterbi の格子に重ねる「オーバーレイ」。
//!
//! システム辞書（不変・53MB）を作り直さず、ユーザーの語を実行時に格子へ追加する。
//! 品詞は Mozc のユーザー辞書の品詞名を、システム辞書の `id.def` の品詞 ID に対応づける。

use crate::dictionary::Dictionary;
use crate::user::UserDictionary;
use std::collections::HashMap;

/// ユーザー語の語コスト。システム辞書の語（おおむね 3000〜9000）より小さくして優先させる
const USER_WORD_COST: u16 = 1_500;

/// オーバーレイの 1 語
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayEntry {
    /// 表記
    pub surface: String,
    /// 左文脈 ID
    pub lid: u16,
    /// 右文脈 ID
    pub rid: u16,
    /// 語コスト
    pub cost: u16,
}

/// ユーザー辞書の語とスニペットを引くための索引
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    by_reading: HashMap<String, Vec<OverlayEntry>>,
    snippets: HashMap<String, Vec<String>>,
    max_reading_chars: usize,
}

impl Overlay {
    /// 読みが一致するユーザー語（登録順）
    pub fn lookup(&self, reading: &str) -> &[OverlayEntry] {
        self.by_reading.get(reading).map_or(&[], Vec::as_slice)
    }

    /// 読みが前方一致するユーザー語（表記とエントリの組。読みの昇順ではなく登録順の走査。
    /// 件数が少ない前提の線形探索）
    pub fn lookup_prefix(&self, prefix: &str, limit: usize) -> Vec<(&str, &OverlayEntry)> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (reading, entries) in &self.by_reading {
            if reading == prefix || !reading.starts_with(prefix) {
                continue;
            }
            for entry in entries {
                out.push((reading.as_str(), entry));
                if out.len() >= limit {
                    return out;
                }
            }
        }
        out
    }

    /// 読みが一致するスニペットの定型文（登録順）
    pub fn snippets(&self, reading: &str) -> &[String] {
        self.snippets.get(reading).map_or(&[], Vec::as_slice)
    }

    /// 登録されている読みの最大文字数（格子への追加で試す長さの上限）
    pub fn max_reading_chars(&self) -> usize {
        self.max_reading_chars
    }

    /// 語の数（スニペットを除く）
    pub fn word_count(&self) -> usize {
        self.by_reading.values().map(Vec::len).sum()
    }

    /// スニペットの数
    pub fn snippet_count(&self) -> usize {
        self.snippets.values().map(Vec::len).sum()
    }

    /// 語・スニペットのうち使わない方を除いた複製（`userdict.enabled` / `snippet.enabled`）
    pub fn filtered(&self, words: bool, snippets: bool) -> Overlay {
        let mut o = self.clone();
        if !words {
            o.by_reading.clear();
            o.max_reading_chars = 0;
        }
        if !snippets {
            o.snippets.clear();
        }
        o
    }

    /// 空か
    pub fn is_empty(&self) -> bool {
        self.by_reading.is_empty() && self.snippets.is_empty()
    }
}

/// Mozc のユーザー辞書の品詞名 → `id.def` の品詞文字列の前方一致パターン（先頭から順に試す）
fn pos_patterns(user_pos: &str) -> &'static [&'static str] {
    match user_pos {
        "固有名詞" => &["名詞,固有名詞,一般", "名詞,固有名詞"],
        "人名" => &["名詞,固有名詞,人名,一般", "名詞,固有名詞,人名"],
        "姓" => &["名詞,固有名詞,人名,姓"],
        "名" => &["名詞,固有名詞,人名,名"],
        "地名" => &["名詞,固有名詞,地域,一般", "名詞,固有名詞,地域"],
        "組織" => &["名詞,固有名詞,組織"],
        "顔文字" | "記号" => &["記号,一般"],
        "副詞" => &["副詞,一般", "副詞"],
        "接続詞" => &["接続詞"],
        "感動詞" => &["感動詞"],
        "連体詞" => &["連体詞"],
        "形容詞" => &["形容詞,自立"],
        p if p.starts_with("動詞") => &["動詞,自立"],
        "名詞サ変" | "名詞サ変接続" => &["名詞,サ変接続"],
        _ => &["名詞,一般"],
    }
}

impl Dictionary {
    /// ユーザー辞書からオーバーレイを作る。品詞名はシステム辞書の品詞 ID に対応づける
    /// （対応が無い品詞は `名詞,一般`）。動詞は基本形の 1 形だけを登録する（活用は展開しない）
    pub fn make_overlay(&self, user: &UserDictionary) -> Overlay {
        let defs = self.pos_defs();
        let find = |patterns: &[&str]| -> Option<u16> {
            patterns
                .iter()
                .find_map(|p| defs.iter().position(|d| d.starts_with(p)))
                .map(|i| i as u16)
        };
        let default_id = find(&["名詞,一般"]).unwrap_or(1);
        let mut ids: HashMap<&str, u16> = HashMap::new();
        let mut overlay = Overlay::default();
        for e in &user.entries {
            let reading_chars = e.reading.chars().count();
            if e.is_snippet() {
                overlay
                    .snippets
                    .entry(e.reading.clone())
                    .or_default()
                    .push(e.surface.clone());
                continue;
            }
            let id = *ids
                .entry(e.pos.as_str())
                .or_insert_with(|| find(pos_patterns(&e.pos)).unwrap_or(default_id));
            overlay.max_reading_chars = overlay.max_reading_chars.max(reading_chars);
            let list = overlay.by_reading.entry(e.reading.clone()).or_default();
            if !list.iter().any(|x| x.surface == e.surface) {
                list.push(OverlayEntry {
                    surface: e.surface.clone(),
                    lid: id,
                    rid: id,
                    cost: USER_WORD_COST,
                });
            }
        }
        overlay
    }
}
