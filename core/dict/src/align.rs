//! 強制アライメント: 読み（ひらがな）と、別の変換器（LLM など）が返した表記から、文節ごとの
//! 読みと表記の対応を求める（設計書 §4.2、決定事項 G-6）。
//!
//! 辞書の格子のうち「表記が与えられた表記の続きに一致する語」だけを辿る制約付き Viterbi:
//!
//! 1. 辞書語（読みが reading の続きに、表記が surface の続きに一致するもの）: 語コスト + 連接コスト
//! 2. 仮名の素通し（読みと表記の 1 文字が同じ、またはひらがな → カタカナ）: 送り仮名・助詞・記号
//! 3. 未知の連続部分（表記側の「ひらがなでない連続」を読みの任意の長さに対応づける）: 辞書に
//!    無い固有名詞・当て字など。高コストにして、辞書で説明できる経路を優先する
//!
//! 経路が求まれば、辞書の品詞から文節に区切る（[`crate::bunsetsu`]）。経路が無ければ `None`。

use crate::bunsetsu::starts_bunsetsu;
use crate::convert::ConvertedSegment;
use crate::dictionary::Dictionary;
use crate::pos::Pos;
use std::collections::{BTreeMap, HashMap};

/// 仮名の素通し（同一文字）のコスト。辞書語より高くして、語として説明できる経路を優先する
const KANA_PASS_COST: i32 = 6_000;
/// ひらがな → カタカナの素通しのコスト
const KATAKANA_PASS_COST: i32 = 7_000;
/// 未知の連続部分の基本コストと、読み 1 文字あたりの加算
const GAP_BASE_COST: i32 = 15_000;
const GAP_PER_CHAR_COST: i32 = 2_000;
/// 未知の連続部分に割り当てる読みの最大長（文字）
const GAP_MAX_READING: usize = 16;

fn is_hiragana(c: char) -> bool {
    ('\u{3041}'..='\u{309f}').contains(&c)
}

fn to_katakana_char(c: char) -> char {
    if ('\u{3041}'..='\u{3096}').contains(&c) {
        char::from_u32(c as u32 + 0x60).unwrap_or(c)
    } else {
        c
    }
}

/// 位置ごとの状態: 表記の位置 j → (右 ID → (総コスト, ノード index))
type Layer = BTreeMap<usize, HashMap<u16, (i32, usize)>>;

/// 前のノード候補: (右 ID, 総コスト, ノード index)
type Preds = Vec<(u16, i32, usize)>;

struct Node {
    /// この語の読み（文字位置の半開区間）と表記（文字位置の半開区間）
    r: (usize, usize),
    s: (usize, usize),
    lid: u16,
    prev: usize,
}

impl Dictionary {
    /// `reading` と `surface`（変換結果）から、文節ごとの (読み, 表記) を求める。
    ///
    /// 返した文節の読みを連結すると `reading`、表記を連結すると `surface` に一致する。
    /// 辞書と仮名の素通しでは説明できない部分（表記側のひらがな以外の連続）は、読みの適当な
    /// 長さに対応づけて 1 語として扱う。それでも経路が無い場合（表記のひらがなが読みに
    /// 存在しないなど）は `None`。
    pub fn align(&self, reading: &str, surface: &str) -> Option<Vec<ConvertedSegment>> {
        let r: Vec<char> = reading.chars().collect();
        let s: Vec<char> = surface.chars().collect();
        if r.is_empty() || s.is_empty() {
            return None;
        }
        let (n, m) = (r.len(), s.len());
        // バイト位置（辞書引きと表記の前方一致に使う）
        let r_byte: Vec<usize> = reading
            .char_indices()
            .map(|(b, _)| b)
            .chain([reading.len()])
            .collect();
        let s_byte: Vec<usize> = surface
            .char_indices()
            .map(|(b, _)| b)
            .chain([surface.len()])
            .collect();
        let r_char_at: HashMap<usize, usize> =
            r_byte.iter().enumerate().map(|(c, &b)| (b, c)).collect();
        let s_char_at: HashMap<usize, usize> =
            s_byte.iter().enumerate().map(|(c, &b)| (b, c)).collect();
        // 表記の位置 j から始まる「ひらがなでない連続」の終端（j がひらがななら j）
        let mut run_end = vec![0usize; m + 1];
        run_end[m] = m;
        for j in (0..m).rev() {
            run_end[j] = if is_hiragana(s[j]) {
                j
            } else {
                run_end[j + 1].max(j + 1)
            };
        }
        let fb = self.fallback_ids();

        let mut nodes: Vec<Node> = Vec::new();
        // best[i]: 読みの位置 i で終わるノードの (表記の位置 j) → (右 ID → (総コスト, ノード index))
        let mut best: Vec<Layer> = vec![BTreeMap::new(); n + 1];
        best[0].entry(0).or_default().insert(0, (0, usize::MAX));

        for i in 0..n {
            let states: Vec<(usize, Preds)> = best[i]
                .iter()
                .map(|(&j, preds)| {
                    (
                        j,
                        preds
                            .iter()
                            .map(|(&rid, &(c, node))| (rid, c, node))
                            .collect(),
                    )
                })
                .collect();
            for (j, preds) in states {
                if j >= m {
                    continue;
                }
                let mut memo: HashMap<u16, (i32, usize)> = HashMap::new();
                let mut connect = |lid: u16| -> (i32, usize) {
                    *memo.entry(lid).or_insert_with(|| {
                        preds
                            .iter()
                            .map(|&(rid, c, node)| (c + self.connection(rid, lid), node))
                            .min_by_key(|&(c, _)| c)
                            .unwrap_or((i32::MAX / 2, usize::MAX))
                    })
                };
                let mut edges: Vec<(usize, usize, u16, u16, i32)> = Vec::new(); // (i2, j2, lid, rid, 語コスト)

                // 1. 辞書語
                let rest_surface = &surface[s_byte[j]..];
                for (group, len) in self.common_prefix_groups(&reading.as_bytes()[r_byte[i]..]) {
                    let Some(&i2) = r_char_at.get(&(r_byte[i] + len)) else {
                        continue;
                    };
                    for idx in self.group_range(group) {
                        let word = self.surface(idx);
                        if rest_surface.starts_with(word)
                            && let Some(&j2) = s_char_at.get(&(s_byte[j] + word.len()))
                        {
                            edges.push((
                                i2,
                                j2,
                                self.lid_of(idx),
                                self.rid_of(idx),
                                i32::from(self.cost_of(idx)),
                            ));
                        }
                    }
                }
                // 2. 仮名の素通し
                if r[i] == s[j] {
                    edges.push((i + 1, j + 1, fb.noun, fb.noun, KANA_PASS_COST));
                } else if to_katakana_char(r[i]) == s[j] && is_hiragana(r[i]) {
                    edges.push((i + 1, j + 1, fb.noun, fb.noun, KATAKANA_PASS_COST));
                }
                // 3. 未知の連続部分
                if !is_hiragana(s[j]) {
                    let j2 = run_end[j];
                    for k in 1..=GAP_MAX_READING.min(n - i) {
                        edges.push((
                            i + k,
                            j2,
                            fb.noun,
                            fb.noun,
                            GAP_BASE_COST + GAP_PER_CHAR_COST * k as i32,
                        ));
                    }
                }

                for (i2, j2, lid, rid, word_cost) in edges {
                    let (pc, prev) = connect(lid);
                    let total = pc + word_cost;
                    let idx = nodes.len();
                    nodes.push(Node {
                        r: (i, i2),
                        s: (j, j2),
                        lid,
                        prev,
                    });
                    let slot = best[i2]
                        .entry(j2)
                        .or_default()
                        .entry(rid)
                        .or_insert((i32::MAX, usize::MAX));
                    if total < slot.0 {
                        *slot = (total, idx);
                    }
                }
            }
        }

        // 終端 (n, m) から EOS へ
        let finals = best[n].get(&m)?;
        let (_, last) = finals
            .iter()
            .map(|(&rid, &(c, node))| (c + self.connection(rid, 0), node))
            .min_by_key(|&(c, _)| c)?;
        let mut path = Vec::new();
        let mut cur = last;
        while cur != usize::MAX {
            path.push(&nodes[cur]);
            cur = nodes[cur].prev;
        }
        path.reverse();

        let mut segments: Vec<ConvertedSegment> = Vec::new();
        let mut prev_pos: Option<Pos> = None;
        for node in path {
            let pos = self.pos_of(node.lid);
            let rd: String = r[node.r.0..node.r.1].iter().collect();
            let sf: String = s[node.s.0..node.s.1].iter().collect();
            if starts_bunsetsu(prev_pos, pos) || segments.is_empty() {
                segments.push(ConvertedSegment {
                    reading: rd,
                    surface: sf,
                });
            } else if let Some(last) = segments.last_mut() {
                last.reading.push_str(&rd);
                last.surface.push_str(&sf);
            }
            prev_pos = Some(pos);
        }
        Some(segments)
    }
}
