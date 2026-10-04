//! 文全体の N-best（上位 k 件の異なる文節分割）。`conversion.initial_view = "sentence"` 用。
//!
//! `convert.rs` の `viterbi`（1-best）を一般化したもの: 各セル（文字位置, 右ID）で
//! 上位1件だけを覚えるのではなく、上位 k 件を覚えながら格子を前に進める（k-best Viterbi）。
//! 連接コスト表・品詞・文節区切りは `convert.rs` と共通（同じ辞書データを使う）。

use crate::bunsetsu::starts_bunsetsu;
use crate::convert::ConvertedSegment;
use crate::dictionary::Dictionary;
use crate::overlay::Overlay;
use crate::pos::Pos;
use std::collections::{HashMap, HashSet};

/// 文全体の N-best の1候補（文節に区切られたセグメント列）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentenceCandidate {
    /// 文節に区切られたセグメント列
    pub segments: Vec<ConvertedSegment>,
}

#[derive(Clone, Copy)]
enum NSource {
    Entry(usize),
    Unknown,
    Extra(usize),
}

struct NNode {
    start: usize,
    end: usize,
    pos_id: u16,
    prev: usize,
    source: NSource,
}

impl Dictionary {
    /// 読み全体を Viterbi で変換し、コスト昇順で上位 `k` 件の**異なる表記**（文節分割）を返す。
    /// 表記が重複するものは除く（読み方は違っても最終的な表記が同じなら1件にまとめる）。
    /// `k == 0` または `reading` が空なら空を返す。既存の `convert`/`convert_with`（1-best）とは
    /// 別実装（そちらの性能・挙動を変えないため）
    pub fn convert_n_best(
        &self,
        reading: &str,
        k: usize,
        overlay: Option<&Overlay>,
    ) -> Vec<SentenceCandidate> {
        let chars: Vec<char> = reading.chars().collect();
        let n = chars.len();
        if n == 0 || k == 0 {
            return Vec::new();
        }

        let mut nodes: Vec<NNode> = Vec::new();
        let mut extras: Vec<String> = Vec::new();
        // best_at[p]: 右IDごとに、そこへ到達する上位k件 (total, node_idx) をコスト昇順で保持
        let mut best_at: Vec<HashMap<u16, Vec<(i32, usize)>>> = vec![HashMap::new(); n + 1];
        best_at[0].insert(0, vec![(0, usize::MAX)]);

        let bytes = reading.as_bytes();
        let mut byte_pos = Vec::with_capacity(n + 1);
        let mut char_at_byte = vec![usize::MAX; bytes.len() + 1];
        for (ci, (bi, _)) in reading.char_indices().enumerate() {
            byte_pos.push(bi);
            char_at_byte[bi] = ci;
        }
        byte_pos.push(bytes.len());
        char_at_byte[bytes.len()] = n;

        for p in 0..n {
            let mut preds: Vec<(u16, i32, usize)> = best_at[p]
                .iter()
                .flat_map(|(&rid, list)| list.iter().map(move |&(total, node)| (rid, total, node)))
                .collect();
            // HashMap の反復順に依存して同点の結果が実行ごとに変わらないよう、右 ID 順に固定する
            preds.sort_by_key(|p| p.0);

            // 辞書の共通接頭辞一致
            for (group, len) in self.common_prefix_groups(&bytes[byte_pos[p]..]) {
                let end = char_at_byte[byte_pos[p] + len];
                for idx in self.group_range(group) {
                    let lid = self.lid_of(idx);
                    let rid = self.rid_of(idx);
                    let word_cost = i32::from(self.cost_of(idx));
                    push_n_best_edge(
                        &preds,
                        lid,
                        rid,
                        self.pos_id_of(idx),
                        word_cost,
                        p,
                        end,
                        NSource::Entry(idx),
                        k,
                        &mut nodes,
                        &mut best_at,
                        |prid, plid| self.connection(prid, plid),
                    );
                }
            }
            // ユーザー辞書の語
            if let Some(ov) = overlay {
                for len in 1..=ov.max_reading_chars().min(n - p) {
                    let key: String = chars[p..p + len].iter().collect();
                    for e in ov.lookup(&key) {
                        extras.push(e.surface.clone());
                        push_n_best_edge(
                            &preds,
                            e.lid,
                            e.rid,
                            e.lid,
                            i32::from(e.cost),
                            p,
                            p + len,
                            NSource::Extra(extras.len() - 1),
                            k,
                            &mut nodes,
                            &mut best_at,
                            |prid, plid| self.connection(prid, plid),
                        );
                    }
                }
            }
            // 1文字の素通しノード（どの位置からでも経路が存在することを保証する）
            let (id, cost) = self.unknown_ids(chars[p]);
            push_n_best_edge(
                &preds,
                id,
                id,
                id,
                cost,
                p,
                p + 1,
                NSource::Unknown,
                k,
                &mut nodes,
                &mut best_at,
                |prid, plid| self.connection(prid, plid),
            );
        }

        // EOS へ渡って、上位k件の終端ノードを得る
        let mut finals: Vec<(i32, usize)> = best_at[n]
            .iter()
            .flat_map(|(&rid, list)| {
                list.iter()
                    .map(move |&(total, node)| (total + self.connection(rid, 0), node))
            })
            .collect();
        finals.sort_by_key(|&(c, node)| (c, node));
        finals.truncate(k);

        // 各終端ノードから経路をバックトラックし、文節に区切って SentenceCandidate を作る。
        // 最終的な表記が重複する経路は除く
        let mut out: Vec<SentenceCandidate> = Vec::new();
        let mut seen_surface: HashSet<String> = HashSet::new();
        for (_, last) in finals {
            let mut order: Vec<usize> = Vec::new();
            let mut cur = last;
            while cur != usize::MAX {
                order.push(cur);
                cur = nodes[cur].prev;
            }
            order.reverse();

            let mut segments: Vec<ConvertedSegment> = Vec::new();
            let mut prev_pos: Option<Pos> = None;
            for &idx in &order {
                let node = &nodes[idx];
                let pos = self.pos_of(node.pos_id);
                let text: String = chars[node.start..node.end].iter().collect();
                let surface = match node.source {
                    NSource::Entry(i) => self.surface(i).to_string(),
                    NSource::Extra(i) => extras[i].clone(),
                    NSource::Unknown => text.clone(),
                };
                if starts_bunsetsu(prev_pos, pos) || segments.is_empty() {
                    segments.push(ConvertedSegment {
                        reading: text,
                        surface,
                    });
                } else if let Some(last_seg) = segments.last_mut() {
                    last_seg.reading.push_str(&text);
                    last_seg.surface.push_str(&surface);
                }
                prev_pos = Some(pos);
            }
            let full_surface: String = segments.iter().map(|s| s.surface.as_str()).collect();
            if seen_surface.insert(full_surface) {
                out.push(SentenceCandidate { segments });
            }
        }
        out
    }
}

/// 1つの辺（lid, rid, word_cost, [start,end)）を、`preds`（この辺の開始位置に到達する上位k件）
/// 全件と組み合わせて total を計算し、上位k件だけを `nodes`・`best_at` に反映する。
/// `connection` は `Dictionary::connection` を呼ぶためのクロージャ（`self` を直接渡せないため）
#[allow(clippy::too_many_arguments)]
fn push_n_best_edge(
    preds: &[(u16, i32, usize)],
    lid: u16,
    rid: u16,
    pos_id: u16,
    word_cost: i32,
    start: usize,
    end: usize,
    source: NSource,
    k: usize,
    nodes: &mut Vec<NNode>,
    best_at: &mut [HashMap<u16, Vec<(i32, usize)>>],
    connection: impl Fn(u16, u16) -> i32,
) {
    let mut candidates: Vec<(i32, usize)> = preds
        .iter()
        .map(|&(prid, ptotal, pnode)| (ptotal + connection(prid, lid) + word_cost, pnode))
        .collect();
    candidates.sort_by_key(|&(c, _)| c);
    candidates.truncate(k);
    for (total, prev) in candidates {
        let node_idx = nodes.len();
        nodes.push(NNode {
            start,
            end,
            pos_id,
            prev,
            source,
        });
        let slot = best_at[end].entry(rid).or_default();
        slot.push((total, node_idx));
        slot.sort_by_key(|&(c, _)| c);
        slot.truncate(k);
    }
}
