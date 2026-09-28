//! Viterbi による最小変換（`minimal` 経路）と、文節区切り・文節候補の生成。

use crate::bunsetsu::starts_bunsetsu;
use crate::dictionary::Dictionary;
use crate::overlay::Overlay;
use crate::pos::{Pos, PosClass};
use std::collections::HashMap;

/// 辞書に無い 1 文字を素通しするときの語コスト
pub(crate) const UNKNOWN_COST: i32 = 10_000;
/// 句読点・括弧の素通しコスト（変換の邪魔をしないよう低め）
const PUNCT_COST: i32 = 1_000;
/// 記号の素通しコスト
const SYMBOL_COST: i32 = 4_000;

/// 変換結果の 1 文節
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedSegment {
    /// 読み（ひらがな）
    pub reading: String,
    /// 表記
    pub surface: String,
}

/// 文節 1 つ分の候補
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentCandidate {
    /// 表記
    pub surface: String,
    /// コスト（小さいほど良い。経路の総コスト、または語コスト）
    pub cost: i32,
}

#[derive(Clone, Copy)]
enum Source {
    /// 辞書の語（全体通しの語 index）
    Entry(usize),
    /// 辞書に無い 1 文字
    Unknown,
    /// ユーザー辞書（オーバーレイ）の語（`Path::extras` の index）
    Extra(usize),
}

struct Node {
    start: usize,
    end: usize,
    rid: u16,
    pos_id: u16,
    word_cost: i32,
    total: i32,
    prev: usize,
    source: Source,
}

/// Viterbi 経路上の 1 語
struct PathWord {
    start: usize,
    end: usize,
    pos_id: u16,
    word_cost: i32,
    source: Source,
}

struct Path {
    words: Vec<PathWord>,
    total: i32,
    /// オーバーレイの語の表記
    extras: Vec<String>,
}

impl Dictionary {
    /// 読み（ひらがな）を Viterbi で最良経路に変換し、文節に区切って返す
    pub fn convert(&self, reading: &str) -> Vec<ConvertedSegment> {
        self.convert_with(reading, None)
    }

    /// [`Dictionary::convert`] にユーザー辞書（オーバーレイ）を加えたもの
    pub fn convert_with(&self, reading: &str, overlay: Option<&Overlay>) -> Vec<ConvertedSegment> {
        let chars: Vec<char> = reading.chars().collect();
        if chars.is_empty() {
            return Vec::new();
        }
        let path = self.viterbi(reading, &chars, overlay);
        let mut segments: Vec<ConvertedSegment> = Vec::new();
        let mut prev_pos: Option<Pos> = None;
        for w in &path.words {
            let pos = self.pos_of(w.pos_id);
            let text: String = chars[w.start..w.end].iter().collect();
            let surface = self.word_surface(w, &text, &path.extras);
            if starts_bunsetsu(prev_pos, pos) || segments.is_empty() {
                segments.push(ConvertedSegment {
                    reading: text,
                    surface,
                });
            } else if let Some(last) = segments.last_mut() {
                last.reading.push_str(&text);
                last.surface.push_str(&surface);
            }
            prev_pos = Some(pos);
        }
        segments
    }

    /// 1 文節分の候補を返す（良い順、表記重複なし、最大 `limit` 件）
    ///
    /// 順序: (1) その読み全体の Viterbi 最良経路 →(2) 先頭の語だけを同じ読みの別の語に
    /// 置き換えたもの（付属語は据え置き）→(3) その読み全体に一致する辞書語。(2)(3) はコスト昇順。
    pub fn segment_candidates(&self, reading: &str, limit: usize) -> Vec<SegmentCandidate> {
        self.segment_candidates_with(reading, limit, None)
    }

    /// [`Dictionary::segment_candidates`] にユーザー辞書（オーバーレイ）を加えたもの
    pub fn segment_candidates_with(
        &self,
        reading: &str,
        limit: usize,
        overlay: Option<&Overlay>,
    ) -> Vec<SegmentCandidate> {
        let chars: Vec<char> = reading.chars().collect();
        if chars.is_empty() || limit == 0 {
            return Vec::new();
        }
        let path = self.viterbi(reading, &chars, overlay);
        let mut out: Vec<SegmentCandidate> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut push = |out: &mut Vec<SegmentCandidate>, surface: String, cost: i32| {
            if seen.insert(surface.clone()) {
                out.push(SegmentCandidate { surface, cost });
            }
        };
        let best: String = path
            .words
            .iter()
            .map(|w| {
                let text: String = chars[w.start..w.end].iter().collect();
                self.word_surface(w, &text, &path.extras)
            })
            .collect();
        push(&mut out, best, path.total);

        let mut extras: Vec<SegmentCandidate> = Vec::new();
        let head_is_content = path.words.first().is_some_and(|w| {
            matches!(
                self.pos_of(w.pos_id).class,
                PosClass::Noun | PosClass::Verb | PosClass::Adjective | PosClass::Adverb
            )
        });
        if path.words.len() >= 2 && head_is_content {
            let head = &path.words[0];
            let head_reading: String = chars[head.start..head.end].iter().collect();
            let tail: String = path.words[1..]
                .iter()
                .map(|w| {
                    let text: String = chars[w.start..w.end].iter().collect();
                    self.word_surface(w, &text, &path.extras)
                })
                .collect();
            for e in self.lookup(&head_reading).into_iter().take(6) {
                extras.push(SegmentCandidate {
                    surface: format!("{}{}", e.surface, tail),
                    cost: path.total - head.word_cost + i32::from(e.cost),
                });
            }
        }
        extras.sort_by_key(|c| c.cost);
        let mut exact: Vec<SegmentCandidate> = self
            .lookup(reading)
            .into_iter()
            .map(|e| SegmentCandidate {
                surface: e.surface,
                cost: i32::from(e.cost),
            })
            .collect();
        exact.sort_by_key(|c| c.cost);
        for c in extras.into_iter().chain(exact) {
            push(&mut out, c.surface, c.cost);
        }
        out.truncate(limit);
        out
    }

    fn word_surface(&self, w: &PathWord, reading_text: &str, extras: &[String]) -> String {
        match w.source {
            Source::Entry(i) => self.surface(i).to_string(),
            Source::Extra(i) => extras[i].clone(),
            Source::Unknown => reading_text.to_string(),
        }
    }

    /// 辞書に無い文字を素通しするノードの (品詞 ID, コスト)
    pub(crate) fn unknown_ids(&self, c: char) -> (u16, i32) {
        let f = self.fallback_ids();
        match c {
            '、' | '，' | ',' => (f.comma, PUNCT_COST),
            '。' | '．' | '.' => (f.period, PUNCT_COST),
            '「' | '『' | '（' | '(' | '［' | '【' | '〈' | '《' => {
                (f.bracket_open, PUNCT_COST)
            }
            '」' | '』' | '）' | ')' | '］' | '】' | '〉' | '》' => {
                (f.bracket_close, PUNCT_COST)
            }
            c if c.is_alphanumeric() || ('\u{3040}'..='\u{30ff}').contains(&c) => {
                (f.noun, UNKNOWN_COST)
            }
            _ => (f.symbol, SYMBOL_COST),
        }
    }

    fn viterbi(&self, reading: &str, chars: &[char], overlay: Option<&Overlay>) -> Path {
        let n = chars.len();
        let bytes = reading.as_bytes();
        // 文字位置 → バイト位置、バイト位置 → 文字位置
        let mut byte_pos = Vec::with_capacity(n + 1);
        let mut char_at_byte = vec![usize::MAX; bytes.len() + 1];
        for (ci, (bi, _)) in reading.char_indices().enumerate() {
            byte_pos.push(bi);
            char_at_byte[bi] = ci;
        }
        byte_pos.push(bytes.len());
        char_at_byte[bytes.len()] = n;

        let mut nodes: Vec<Node> = Vec::new();
        let mut extras: Vec<String> = Vec::new();
        // best_at[p]: 位置 p で終わるノードのうち、右 ID ごとの最小総コスト (総コスト, ノード index)
        let mut best_at: Vec<HashMap<u16, (i32, usize)>> = vec![HashMap::new(); n + 1];
        best_at[0].insert(0, (0, usize::MAX));

        for p in 0..n {
            let preds: Vec<(u16, i32, usize)> = best_at[p]
                .iter()
                .map(|(&rid, &(total, node))| (rid, total, node))
                .collect();
            // 左 ID ごとに「最良の前ノード + 連接コスト」を使い回す
            let mut memo: HashMap<u16, (i32, usize)> = HashMap::new();
            let mut best_pred = |lid: u16| -> (i32, usize) {
                *memo.entry(lid).or_insert_with(|| {
                    preds
                        .iter()
                        .map(|&(rid, total, node)| (total + self.connection(rid, lid), node))
                        .min_by_key(|&(c, _)| c)
                        .unwrap_or((i32::MAX / 2, usize::MAX))
                })
            };

            let mut new_nodes: Vec<Node> = Vec::new();
            for (group, len) in self.common_prefix_groups(&bytes[byte_pos[p]..]) {
                let end = char_at_byte[byte_pos[p] + len];
                for idx in self.group_range(group) {
                    let lid = self.lid_of(idx);
                    let word_cost = i32::from(self.cost_of(idx));
                    let (pc, prev) = best_pred(lid);
                    new_nodes.push(Node {
                        start: p,
                        end,
                        rid: self.rid_of(idx),
                        pos_id: self.pos_id_of(idx),
                        word_cost,
                        total: pc + word_cost,
                        prev,
                        source: Source::Entry(idx),
                    });
                }
            }
            // ユーザー辞書の語
            if let Some(ov) = overlay {
                for len in 1..=ov.max_reading_chars().min(n - p) {
                    let key: String = chars[p..p + len].iter().collect();
                    for e in ov.lookup(&key) {
                        let (pc, prev) = best_pred(e.lid);
                        extras.push(e.surface.clone());
                        new_nodes.push(Node {
                            start: p,
                            end: p + len,
                            rid: e.rid,
                            pos_id: e.lid,
                            word_cost: i32::from(e.cost),
                            total: pc + i32::from(e.cost),
                            prev,
                            source: Source::Extra(extras.len() - 1),
                        });
                    }
                }
            }
            // 1 文字の素通しノード（どの位置からでも経路が存在することを保証する）
            let (id, cost) = self.unknown_ids(chars[p]);
            let (pc, prev) = best_pred(id);
            new_nodes.push(Node {
                start: p,
                end: p + 1,
                rid: id,
                pos_id: id,
                word_cost: cost,
                total: pc + cost,
                prev,
                source: Source::Unknown,
            });

            for node in new_nodes {
                let idx = nodes.len();
                let slot = best_at[node.end]
                    .entry(node.rid)
                    .or_insert((i32::MAX, usize::MAX));
                if node.total < slot.0 {
                    *slot = (node.total, idx);
                }
                nodes.push(node);
            }
        }

        // EOS（ID 0）へ
        let (total, last) = best_at[n]
            .iter()
            .map(|(&rid, &(total, node))| (total + self.connection(rid, 0), node))
            .min_by_key(|&(c, _)| c)
            .unwrap_or((0, usize::MAX));
        let mut words = Vec::new();
        let mut cur = last;
        while cur != usize::MAX {
            let node = &nodes[cur];
            words.push(PathWord {
                start: node.start,
                end: node.end,
                pos_id: node.pos_id,
                word_cost: node.word_cost,
                source: node.source,
            });
            cur = node.prev;
        }
        words.reverse();
        Path {
            words,
            total,
            extras,
        }
    }
}
