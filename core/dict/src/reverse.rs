//! 確定済みの表記（surface）から読みを推定する逆変換（`reconvert` 用）。
//!
//! `convert.rs` の Viterbi（読み → 表記）を反転させたもの: 辞書の語を表記で逆引きし、
//! 表記の文字列を辞書語の並びに分割して、各語の読みを連結する。連接コスト表・品詞に
//! よる文節区切りは `convert.rs` と共通（同じ辞書データを使う）。

use crate::bunsetsu::starts_bunsetsu;
use crate::convert::ConvertedSegment;
use crate::dictionary::Dictionary;
use crate::pos::Pos;
use std::collections::HashMap;

/// 表記 → [(読み, 語 index)] の逆引き索引
pub(crate) struct ReverseIndex {
    by_surface: HashMap<String, Vec<(String, usize)>>,
    /// 索引に登録されている表記の最大文字数（前方一致探索の上限に使う）
    max_surface_chars: usize,
}

struct RNode {
    end: usize,
    lid: u16,
    prev: usize,
    reading: String,
}

impl Dictionary {
    /// 逆引き索引を返す（初回だけ辞書全体を舐めて構築し、以降はキャッシュを返す）
    fn reverse_index(&self) -> &ReverseIndex {
        self.reverse_index_cell().get_or_init(|| {
            use fst::Streamer;
            let mut by_surface: HashMap<String, Vec<(String, usize)>> = HashMap::new();
            let mut max_surface_chars = 1usize;
            let mut stream = self.trie_stream();
            while let Some((reading_bytes, group)) = stream.next() {
                let reading = String::from_utf8_lossy(reading_bytes).into_owned();
                for idx in self.group_range(group as u32) {
                    let surface = self.surface(idx);
                    max_surface_chars = max_surface_chars.max(surface.chars().count());
                    by_surface
                        .entry(surface.to_string())
                        .or_default()
                        .push((reading.clone(), idx));
                }
            }
            ReverseIndex {
                by_surface,
                max_surface_chars,
            }
        })
    }

    /// 確定済みの表記（surface）から読みを推定し、文節に区切って返す（`reconvert` 用）。
    /// 辞書の逆引きによる Viterbi。辞書に登録が無い区間（未知語）は、その文字をそのまま
    /// 読みとして通す(正しい読みではない可能性があるが、クラッシュせず編集を続けられる。
    /// 「不能区間はLLMで補う」という設計書の記述の、LLM 部分は今回のスコープ外)
    pub fn reverse_convert(&self, surface: &str) -> Vec<ConvertedSegment> {
        let chars: Vec<char> = surface.chars().collect();
        let n = chars.len();
        if n == 0 {
            return Vec::new();
        }
        let index = self.reverse_index();

        let mut nodes: Vec<RNode> = Vec::new();
        let mut best_at: Vec<HashMap<u16, (i32, usize)>> = vec![HashMap::new(); n + 1];
        best_at[0].insert(0, (0, usize::MAX));

        for p in 0..n {
            let preds: Vec<(u16, i32, usize)> = best_at[p]
                .iter()
                .map(|(&rid, &(total, node))| (rid, total, node))
                .collect();
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

            let max_len = index.max_surface_chars.min(n - p);
            for len in 1..=max_len {
                let span: String = chars[p..p + len].iter().collect();
                let Some(candidates) = index.by_surface.get(&span) else {
                    continue;
                };
                let end = p + len;
                for (reading, idx) in candidates {
                    let lid = self.lid_of(*idx);
                    let rid = self.rid_of(*idx);
                    let word_cost = i32::from(self.cost_of(*idx));
                    let (pc, prev) = best_pred(lid);
                    let node_idx = nodes.len();
                    let total = pc + word_cost;
                    nodes.push(RNode {
                        end,
                        lid,
                        prev,
                        reading: reading.clone(),
                    });
                    let slot = best_at[end].entry(rid).or_insert((i32::MAX, usize::MAX));
                    if total < slot.0 {
                        *slot = (total, node_idx);
                    }
                }
            }
            // 1文字の素通しノード(どの位置からでも経路が存在することを保証する。
            // convert.rs の viterbi の同名の処理と同じ考え方)
            let (id, cost) = self.unknown_ids(chars[p]);
            let (pc, prev) = best_pred(id);
            let ch: String = chars[p..p + 1].iter().collect();
            let node_idx = nodes.len();
            let total = pc + cost;
            nodes.push(RNode {
                end: p + 1,
                lid: id,
                prev,
                reading: ch,
            });
            let slot = best_at[p + 1].entry(id).or_insert((i32::MAX, usize::MAX));
            if total < slot.0 {
                *slot = (total, node_idx);
            }
        }

        let (_, last) = best_at[n]
            .iter()
            .map(|(&rid, &(total, node))| (total + self.connection(rid, 0), node))
            .min_by_key(|&(c, _)| c)
            .unwrap_or((0, usize::MAX));

        let mut order: Vec<usize> = Vec::new();
        let mut cur = last;
        while cur != usize::MAX {
            order.push(cur);
            cur = nodes[cur].prev;
        }
        order.reverse();

        let mut segments: Vec<ConvertedSegment> = Vec::new();
        let mut prev_pos: Option<Pos> = None;
        let mut pos_cursor = 0usize;
        for &idx in &order {
            let node = &nodes[idx];
            let surface_text: String = chars[pos_cursor..node.end].iter().collect();
            pos_cursor = node.end;
            let pos = self.pos_of(node.lid);
            if starts_bunsetsu(prev_pos, pos) || segments.is_empty() {
                segments.push(ConvertedSegment {
                    reading: node.reading.clone(),
                    surface: surface_text,
                });
            } else if let Some(last_seg) = segments.last_mut() {
                last_seg.reading.push_str(&node.reading);
                last_seg.surface.push_str(&surface_text);
            }
            prev_pos = Some(pos);
        }
        segments
    }
}
