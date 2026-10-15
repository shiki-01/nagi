//! 辞書で枝刈りしながらの最良優先探索（ローマ字の打鍵列 / かな）。
//!
//! ノードは「入力のどこまで読んだか・ここまでに出力した読み・累積コスト」。コストの小さい順に
//! 取り出し、展開した読みが辞書で成立し得ない（[`Frontier`]）ノードは作らない。
//! 同じ状態へより高いコストで着くものは捨てる。結果は決定的（同点は生成順）。

use crate::edit::{
    COST_DELETE, COST_INSERT, COST_TRANSPOSE, Correction, EditKind, EditOp, Params, milli,
    substitute_cost,
};
use crate::index::{Frontier, ReadingIndex};
use crate::kana::KanaTable;
use crate::layout::{Adjacency, TouchModel};
use nagi_romaji::{Emit, RomajiConverter, RomajiTable, Stroke};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::Instant;

/// 1 つの補正が含む操作の最大数（コスト 1.5 の操作で上限 6.5 まで使っても 4 回）
const MAX_OPS: usize = 4;

/// 適用した操作の列（固定長。ノードごとの割り当てを避ける）
#[derive(Clone, Copy)]
struct Ops {
    items: [Option<EditOp>; MAX_OPS],
    len: usize,
}

impl Ops {
    fn new() -> Self {
        Ops {
            items: [None; MAX_OPS],
            len: 0,
        }
    }

    /// 操作を 1 つ足した列（`None` は足さない。満杯なら `None`）
    fn with(mut self, op: Option<EditOp>) -> Option<Self> {
        if let Some(op) = op {
            if self.len == MAX_OPS {
                return None;
            }
            self.items[self.len] = Some(op);
            self.len += 1;
        }
        Some(self)
    }

    fn last_kind(&self) -> Option<EditKind> {
        self.len
            .checked_sub(1)
            .and_then(|i| self.items[i])
            .map(|op| op.kind)
    }

    fn to_vec(self) -> Vec<EditOp> {
        self.items[..self.len].iter().flatten().copied().collect()
    }
}

struct Node<S> {
    cost: u32,
    seq: u64,
    /// 入力（打鍵列 / かな）の何文字目まで読んだか
    i: usize,
    state: S,
    kana: String,
    frontier: Frontier,
    done: bool,
    ops: Ops,
}

impl<S> PartialEq for Node<S> {
    fn eq(&self, other: &Self) -> bool {
        (self.cost, self.seq) == (other.cost, other.seq)
    }
}
impl<S> Eq for Node<S> {}
impl<S> PartialOrd for Node<S> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<S> Ord for Node<S> {
    /// `BinaryHeap` は最大ヒープなので、コスト・生成順が小さいほど大きいとみなす
    fn cmp(&self, other: &Self) -> Ordering {
        (other.cost, other.seq).cmp(&(self.cost, self.seq))
    }
}

/// 状態の同一性: (入力の位置, 出力した読み, 変換器の保留)
type StateKey = (usize, String, String);

/// 探索の共通部分（ヒープ・状態ごとの最小コスト・結果）
struct Search<S> {
    heap: BinaryHeap<Node<S>>,
    seq: u64,
    max_cost: u32,
    best: HashMap<StateKey, u32>,
    results: Vec<Correction>,
    found: std::collections::HashSet<String>,
}

impl<S> Search<S> {
    fn new(max_cost: f32) -> Self {
        Search {
            heap: BinaryHeap::new(),
            seq: 0,
            max_cost: milli(max_cost),
            best: HashMap::new(),
            results: Vec::new(),
            found: std::collections::HashSet::new(),
        }
    }

    /// この状態へ `cost` で着くのが、これまでで最良か（最良なら記録する）
    fn improves(&mut self, key: StateKey, cost: u32) -> bool {
        match self.best.get_mut(&key) {
            Some(prev) if *prev <= cost => false,
            Some(prev) => {
                *prev = cost;
                true
            }
            None => {
                self.best.insert(key, cost);
                true
            }
        }
    }

    fn push(&mut self, mut node: Node<S>) {
        self.seq += 1;
        node.seq = self.seq;
        self.heap.push(node);
    }

    /// 取り出したノードが、より低いコストで既に見つかった状態の古い複製か
    fn is_stale(&self, node: &Node<S>, pending: &str) -> bool {
        self.best
            .get(&(node.i, node.kana.clone(), pending.to_string()))
            .is_some_and(|&c| c < node.cost)
    }

    /// 結果ノードを取り込む。結果が上限に達したら `true`
    fn take_result(&mut self, node: &Node<S>, max_results: usize) -> bool {
        if node.ops.len > 0 && self.found.insert(node.kana.clone()) {
            self.results.push(Correction {
                reading: node.kana.clone(),
                cost: node.cost as f32 / 1000.0,
                ops: node.ops.to_vec(),
            });
        }
        self.results.len() >= max_results
    }
}

fn append_emits(kana: &mut String, emits: Vec<Emit>) -> bool {
    for e in emits {
        match e {
            Emit::Converted(s) => kana.push_str(&s),
            // 変換できずに残った英字は、辞書の読みにならない
            Emit::Raw(_) => return false,
        }
    }
    true
}

/// ローマ字探索の設定
pub(crate) struct RomajiSearch<'a> {
    pub adjacency: &'a Adjacency,
    pub touch: &'a TouchModel,
    pub table: &'a Arc<RomajiTable>,
}

impl RomajiSearch<'_> {
    /// 打鍵列 `strokes` の補正読みを、コストの小さい順に返す。座標つきの打鍵の置換は、
    /// 離散コスト（隣接順位）の代わりにタッチ位置のガウス尤度で評価する
    pub fn run(
        &self,
        strokes: &[Stroke],
        index: &dyn ReadingIndex,
        params: &Params,
    ) -> Vec<Correction> {
        let t: Vec<char> = strokes.iter().map(|s| s.ch).collect();
        let started = Instant::now();
        let mut search: Search<RomajiConverter> = Search::new(params.max_cost);
        search.push(Node {
            cost: 0,
            seq: 0,
            i: 0,
            state: RomajiConverter::new(self.table.clone()),
            kana: String::new(),
            frontier: Frontier::new(),
            done: false,
            ops: Ops::new(),
        });
        let mut visited = 0usize;

        while let Some(node) = search.heap.pop() {
            visited += 1;
            if visited > params.max_nodes || params.deadline.is_some_and(|d| started.elapsed() >= d)
            {
                break;
            }
            if node.done {
                if search.take_result(&node, params.max_results) {
                    break;
                }
                continue;
            }
            if search.is_stale(&node, node.state.pending()) {
                continue;
            }
            self.expand(&node, &t, strokes, index, params, &mut search);
        }
        search.results
    }

    /// `node` から 1 操作ぶん進めたノードを作る
    fn expand(
        &self,
        node: &Node<RomajiConverter>,
        t: &[char],
        strokes: &[Stroke],
        index: &dyn ReadingIndex,
        params: &Params,
        search: &mut Search<RomajiConverter>,
    ) {
        let n = t.len();
        let i = node.i;
        let kinds = params.kinds;
        // `chars` を順に変換器へ入れた子ノードを作る（読み取り位置は `next_i`）
        let child = |search: &mut Search<RomajiConverter>,
                     chars: &[char],
                     next_i: usize,
                     add: f32,
                     op: Option<EditOp>| {
            let cost = node.cost + milli(add);
            if cost > search.max_cost {
                return;
            }
            let Some(ops) = node.ops.with(op) else {
                return;
            };
            let mut state = node.state.clone();
            let mut kana = node.kana.clone();
            for &c in chars {
                if !append_emits(&mut kana, state.feed(c)) {
                    return;
                }
            }
            if !search.improves((next_i, kana.clone(), state.pending().to_string()), cost) {
                return;
            }
            let frontier = if kana.len() == node.kana.len() {
                node.frontier.clone()
            } else {
                match node.frontier.extend(index, &kana) {
                    Some(f) => f,
                    None => return,
                }
            };
            search.push(Node {
                cost,
                seq: 0,
                i: next_i,
                state,
                kana,
                frontier,
                done: false,
                ops,
            });
        };

        if i == n {
            // 読み終わり: 保留を確定して、読み全体が辞書の語の連なりなら結果
            let mut state = node.state.clone();
            let mut kana = node.kana.clone();
            if append_emits(&mut kana, state.flush()) {
                let frontier = if kana.len() == node.kana.len() {
                    Some(node.frontier.clone())
                } else {
                    node.frontier.extend(index, &kana)
                };
                if let Some(frontier) = frontier.filter(Frontier::covered) {
                    search.push(Node {
                        cost: node.cost,
                        seq: 0,
                        i,
                        state,
                        kana,
                        frontier,
                        done: true,
                        ops: node.ops,
                    });
                }
            }
        } else {
            let c = t[i];
            // そのまま
            child(search, &[c], i + 1, 0.0, None);
            // 隣接キーへの置換（本来は隣のキーを打つつもりだった）
            if kinds.substitute {
                for nb in self.adjacency.neighbors(c) {
                    let cost = strokes[i]
                        .touch
                        .filter(|_| params.use_touch)
                        .and_then(|rel| {
                            self.touch
                                .substitute_cost(c, nb.label, rel, params.touch_sigma)
                        })
                        .unwrap_or_else(|| substitute_cost(nb.rank));
                    child(
                        search,
                        &[nb.label],
                        i + 1,
                        cost,
                        Some(EditOp {
                            kind: EditKind::Substitute,
                            pos: i,
                            typed: Some(c),
                            intended: Some(nb.label),
                        }),
                    );
                }
            }
            // 挿入（余計な打鍵）: 直前文字の重複、または前後のキーに隣接する文字は取り除く
            if kinds.insert {
                let dup = i > 0 && t[i - 1] == c;
                let near = (i > 0 && self.adjacency.is_adjacent(c, t[i - 1]))
                    || (i + 1 < n && self.adjacency.is_adjacent(c, t[i + 1]));
                if dup || near {
                    child(
                        search,
                        &[],
                        i + 1,
                        COST_INSERT,
                        Some(EditOp {
                            kind: EditKind::Insert,
                            pos: i,
                            typed: Some(c),
                            intended: None,
                        }),
                    );
                }
            }
            // 転置
            if kinds.transpose && i + 1 < n && t[i] != t[i + 1] {
                child(
                    search,
                    &[t[i + 1], t[i]],
                    i + 2,
                    COST_TRANSPOSE,
                    Some(EditOp {
                        kind: EditKind::Transpose,
                        pos: i,
                        typed: Some(c),
                        intended: Some(t[i + 1]),
                    }),
                );
            }
        }
        // 脱落（打ち忘れ）: ここに 1 文字足す（読みの末尾でも可）。直前に足した文字を続けては足さない
        if kinds.delete && node.ops.last_kind() != Some(EditKind::Delete) {
            for letter in 'a'..='z' {
                child(
                    search,
                    &[letter],
                    i,
                    COST_DELETE,
                    Some(EditOp {
                        kind: EditKind::Delete,
                        pos: i,
                        typed: None,
                        intended: Some(letter),
                    }),
                );
            }
        }
    }
}

/// かな探索（かな直接入力。濁点・小書きの付け忘れと、同じ行内の隣の文字）
pub(crate) struct KanaSearch<'a> {
    pub table: &'a KanaTable,
    /// 読みの各文字を出したフリックの角度・距離（読みと同じ長さのときだけ使う）
    pub flicks: Option<&'a [Option<(f32, f32)>]>,
}

impl KanaSearch<'_> {
    /// 読み `reading` の補正読みを、コストの小さい順に返す
    pub fn run(&self, reading: &str, index: &dyn ReadingIndex, params: &Params) -> Vec<Correction> {
        let t: Vec<char> = reading.chars().collect();
        let n = t.len();
        let started = Instant::now();
        let mut search: Search<()> = Search::new(params.max_cost);
        search.push(Node {
            cost: 0,
            seq: 0,
            i: 0,
            state: (),
            kana: String::new(),
            frontier: Frontier::new(),
            done: false,
            ops: Ops::new(),
        });
        let mut visited = 0usize;

        while let Some(node) = search.heap.pop() {
            visited += 1;
            if visited > params.max_nodes || params.deadline.is_some_and(|d| started.elapsed() >= d)
            {
                break;
            }
            if node.done {
                if search.take_result(&node, params.max_results) {
                    break;
                }
                continue;
            }
            if search.is_stale(&node, "") {
                continue;
            }
            let i = node.i;
            if i == n {
                if node.frontier.covered() {
                    search.push(Node { done: true, ..node });
                }
                continue;
            }
            let c = t[i];
            let mut options: Vec<(char, f32, Option<EditOp>)> = vec![(c, 0.0, None)];
            if params.kinds.voicing {
                for &v in self.table.voicing_variants(c) {
                    options.push((
                        v,
                        KanaTable::voicing_cost(),
                        Some(EditOp {
                            kind: EditKind::Voicing,
                            pos: i,
                            typed: Some(c),
                            intended: Some(v),
                        }),
                    ));
                }
            }
            if params.kinds.substitute {
                let flick = self
                    .flicks
                    .filter(|f| params.use_touch && f.len() == n)
                    .and_then(|f| f[i]);
                for &(r, cost) in self.table.row_neighbors(c) {
                    let cost = flick
                        .and_then(|fl| self.table.flick_cost(c, r, fl))
                        .unwrap_or(cost);
                    options.push((
                        r,
                        cost,
                        Some(EditOp {
                            kind: EditKind::Substitute,
                            pos: i,
                            typed: Some(c),
                            intended: Some(r),
                        }),
                    ));
                }
            }
            for (ch, add, op) in options {
                let cost = node.cost + milli(add);
                let Some(ops) = node.ops.with(op).filter(|_| cost <= search.max_cost) else {
                    continue;
                };
                let mut kana = node.kana.clone();
                kana.push(ch);
                if !search.improves((i + 1, kana.clone(), String::new()), cost) {
                    continue;
                }
                let Some(frontier) = node.frontier.extend(index, &kana) else {
                    continue;
                };
                search.push(Node {
                    cost,
                    seq: 0,
                    i: i + 1,
                    state: (),
                    kana,
                    frontier,
                    done: false,
                    ops,
                });
            }
        }
        search.results
    }
}

/// 同じ読みを複数の経路で得たとき、コストの小さいものだけ残す（並びはコスト → 読みの順）
pub(crate) fn merge(mut all: Vec<Correction>, limit: usize) -> Vec<Correction> {
    all.sort_by(|a, b| {
        a.cost
            .total_cmp(&b.cost)
            .then_with(|| a.reading.cmp(&b.reading))
    });
    let mut seen = std::collections::HashSet::new();
    all.retain(|c| seen.insert(c.reading.clone()));
    all.truncate(limit);
    all
}
