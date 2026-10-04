//! 読み仮説のビーム探索。

use crate::config::{CONSONANT_CONFUSIONS, FuzzyConfig};
use crate::keys::KeyAlternatives;
use nagi_romaji::{Emit, RomajiConverter, RomajiTable};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// 語として分割可能な境界を調べる最大の語長（文字数）
const MAX_WORD_CHARS: usize = 10;
/// 保持する境界の範囲（文字数）。これより古い境界は捨てる
const KEEP_BOUNDARY_CHARS: usize = 16;

/// 辞書の照会（枝刈り用）。実装はエンジン側で `Dictionary` に結線する
pub trait PrefixOracle {
    /// 読みが辞書のいずれかの読みの接頭辞か（完全一致を含む）
    fn has_prefix(&self, reading_prefix: &str) -> bool;
    /// 読みが辞書の語として完全一致で存在するか
    fn contains_word(&self, word: &str) -> bool;
}

/// 呼び出し単位（1 回の `expand`）でだけ生きる `PrefixOracle` の照会結果のメモ化。
///
/// 探索では同じ文字列（状態間で共通する接尾辞）を何度も照会するため、辞書（FST）への照会を
/// 1 回にする。`expand` の終了で破棄する（メモリは探索中の状態数に比例する範囲）。結果は元の oracle と同じ
struct MemoOracle<'a> {
    inner: &'a dyn PrefixOracle,
    prefix: RefCell<HashMap<String, bool>>,
    word: RefCell<HashMap<String, bool>>,
}

impl<'a> MemoOracle<'a> {
    fn new(inner: &'a dyn PrefixOracle) -> Self {
        Self {
            inner,
            prefix: RefCell::new(HashMap::new()),
            word: RefCell::new(HashMap::new()),
        }
    }
}

impl PrefixOracle for MemoOracle<'_> {
    fn has_prefix(&self, reading_prefix: &str) -> bool {
        if let Some(&r) = self.prefix.borrow().get(reading_prefix) {
            return r;
        }
        // 接頭辞が辞書の読みの接頭辞でなければ、それを伸ばした文字列もそうではない。
        // 直前の文字を除いた文字列が既に false と分かっていれば照会しない
        let shorter = reading_prefix
            .char_indices()
            .next_back()
            .map(|(i, _)| &reading_prefix[..i]);
        let known_false = shorter
            .is_some_and(|sh| !sh.is_empty() && self.prefix.borrow().get(sh).is_some_and(|&r| !r));
        let r = !known_false && self.inner.has_prefix(reading_prefix);
        self.prefix
            .borrow_mut()
            .insert(reading_prefix.to_string(), r);
        r
    }

    fn contains_word(&self, word: &str) -> bool {
        if let Some(&r) = self.word.borrow().get(word) {
            return r;
        }
        let r = self.inner.contains_word(word);
        self.word.borrow_mut().insert(word.to_string(), r);
        r
    }
}

/// 適用した編集の内訳
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditSummary {
    /// 代替キーへの置換（タッチ・フリックの曖昧さ）
    pub substitutions: u8,
    /// 欠落補完（1 文字挿入）
    pub insertions: u8,
    /// 余剰削除
    pub deletions: u8,
    /// 入れ替え
    pub transpositions: u8,
    /// 子音の紛れ
    pub confusions: u8,
}

impl EditSummary {
    /// `max_edits` の対象になる編集の数（代替キーへの置換は含めない）
    fn edit_ops(&self) -> u8 {
        self.insertions + self.deletions + self.transpositions + self.confusions
    }
}

/// 読みの仮説。cost は小さいほど尤もらしい（第一仮説は 0）
#[derive(Debug, Clone, PartialEq)]
pub struct ReadingHypothesis {
    /// 読み（ひらがな）
    pub reading: String,
    /// 入力モデル由来のコスト（nat）
    pub cost: f32,
    /// 編集の内訳
    pub edits: EditSummary,
}

/// 探索の統計（テスト・計測用）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExpandStats {
    /// 遷移を展開した状態の総数
    pub states_expanded: usize,
}

#[derive(Clone)]
struct State {
    /// 確定済みの文字列
    kana: String,
    /// 変換器の保留
    pending: String,
    cost: f32,
    edits: EditSummary,
    /// 語の境界（`kana` のバイト位置, 辞書語に由来するか。false は未知語区間の終端）
    boundaries: Vec<(usize, bool)>,
}

impl State {
    fn key(&self) -> (String, String, u8) {
        (
            self.kana.clone(),
            self.pending.clone(),
            self.edits.edit_ops(),
        )
    }
}

/// 打たれたとおりの入力（代替なし）から仮説を作る
pub fn expand_typed(
    typed: &str,
    table: &Arc<RomajiTable>,
    oracle: &dyn PrefixOracle,
    cfg: &FuzzyConfig,
) -> Vec<ReadingHypothesis> {
    let keys: Vec<KeyAlternatives> = typed.chars().map(KeyAlternatives::exact).collect();
    expand(&keys, table, oracle, cfg)
}

/// 読み仮説を cost 昇順で返す。先頭は常に第一仮説（各位置の打たれたとおり。cost 0。枝刈り対象外）
pub fn expand(
    keys: &[KeyAlternatives],
    table: &Arc<RomajiTable>,
    oracle: &dyn PrefixOracle,
    cfg: &FuzzyConfig,
) -> Vec<ReadingHypothesis> {
    expand_with_stats(keys, table, oracle, cfg).0
}

/// [`expand`] と探索の統計を返す
pub fn expand_with_stats(
    keys: &[KeyAlternatives],
    table: &Arc<RomajiTable>,
    oracle: &dyn PrefixOracle,
    cfg: &FuzzyConfig,
) -> (Vec<ReadingHypothesis>, ExpandStats) {
    let steps = Steps::new(table);
    let memo = MemoOracle::new(oracle);
    let oracle: &dyn PrefixOracle = &memo;
    let mut stats = ExpandStats::default();
    let first = ReadingHypothesis {
        reading: typed_reading(keys, table),
        cost: 0.0,
        edits: EditSummary::default(),
    };
    let mut out = vec![first];
    if keys.is_empty() || cfg.max_hypotheses <= 1 {
        return (out, stats);
    }

    let n = keys.len();
    let max_edits = match cfg.max_edits {
        0 => 0,
        m if n >= cfg.long_input_len => m.max(2),
        m => m,
    };
    let w_start = n.saturating_sub(cfg.window);
    let beam = cfg.beam.max(1);

    let mut layers: Vec<Vec<State>> = vec![Vec::new(); n + 1];
    layers[0].push(State {
        kana: String::new(),
        pending: String::new(),
        cost: 0.0,
        edits: EditSummary::default(),
        boundaries: vec![(0, true)],
    });

    for i in 0..=n {
        let mut cur = std::mem::take(&mut layers[i]);
        // 欠落補完は同じ位置に留まるので、編集回数の層ごとに展開する
        if i >= w_start && max_edits > 0 && romaji_adjacent(keys, i) {
            let mut frontier = cur.clone();
            for _ in 0..max_edits {
                let mut next = Vec::new();
                for s in &frontier {
                    if s.edits.edit_ops() >= max_edits {
                        continue;
                    }
                    insertions(s, &steps, oracle, cfg, &mut next);
                }
                cur.extend(next.iter().cloned());
                frontier = next;
            }
        }
        select(&mut cur, beam);
        if i == n {
            layers[n] = cur;
            break;
        }
        stats.states_expanded += cur.len();
        let mut produced: Vec<(usize, State)> = Vec::new();
        for s in &cur {
            transitions(
                s,
                keys,
                i,
                i >= w_start,
                max_edits,
                &steps,
                oracle,
                cfg,
                &mut produced,
            );
        }
        for (pos, s) in produced {
            layers[pos].push(s);
        }
    }

    // 終端: 保留を確定し、全体が辞書語に分割できるものだけ採用する
    let mut best: HashMap<String, ReadingHypothesis> = HashMap::new();
    for s in std::mem::take(&mut layers[n]) {
        let Some(s) = finish(s, &steps, oracle, cfg) else {
            continue;
        };
        let end = s.kana.len();
        if !s.boundaries.iter().any(|&(q, _)| q == end) {
            continue;
        }
        let h = ReadingHypothesis {
            reading: s.kana,
            cost: s.cost,
            edits: s.edits,
        };
        match best.get_mut(&h.reading) {
            Some(old) if old.cost <= h.cost => {}
            Some(old) => *old = h,
            None => {
                best.insert(h.reading.clone(), h);
            }
        }
    }
    best.remove(&out[0].reading);
    let mut rest: Vec<ReadingHypothesis> = best.into_values().collect();
    rest.sort_by(|a, b| {
        a.cost
            .total_cmp(&b.cost)
            .then(a.edits.edit_ops().cmp(&b.edits.edit_ops()))
            .then_with(|| a.reading.cmp(&b.reading))
    });
    rest.truncate(cfg.max_hypotheses - 1);
    out.extend(rest);
    (out, stats)
}

/// 打たれたとおりの読み（Raw は素通し）。ひらがなのキー（フリック等）は、保留を確定してから
/// そのまま挿入する（`Composer::insert_reading_char` と同じ）
fn typed_reading(keys: &[KeyAlternatives], table: &Arc<RomajiTable>) -> String {
    let mut conv = RomajiConverter::new(Arc::clone(table));
    let mut s = String::new();
    let mut push = |emits: Vec<Emit>| {
        for e in emits {
            match e {
                Emit::Converted(t) => s.push_str(&t),
                Emit::Raw(c) => s.push(c),
            }
        }
    };
    for k in keys {
        if is_kana_char(k.typed()) {
            push(conv.flush());
            push(vec![Emit::Raw(k.typed())]);
        } else {
            push(conv.feed(k.typed()));
        }
    }
    push(conv.flush());
    s
}

/// 変換器の遷移（保留, 入力文字列）→ 結果のメモ。変換器は決定的なので、同じ入力は 1 回だけ計算する
/// （`RomajiConverter` の生成・保留の再入力・テーブル照会を状態ごとにやり直さないため）。`expand` の
/// 呼び出し単位で破棄する
struct Steps {
    table: Arc<RomajiTable>,
    cache: RefCell<HashMap<(String, String), StepResult>>,
}

/// [`step`] の結果（出力文字列, 新しい保留）。不正な連続は None
type StepResult = Option<(String, String)>;

impl Steps {
    fn new(table: &Arc<RomajiTable>) -> Self {
        Self {
            table: Arc::clone(table),
            cache: RefCell::new(HashMap::new()),
        }
    }
}

/// 保留を復元した変換器に `chars` を順に入力する。Raw が出たら（不正な連続として）None。
/// 戻り値は（出力文字列, 新しい保留）
fn step(steps: &Steps, pending: &str, chars: &[char]) -> StepResult {
    let key = (pending.to_string(), chars.iter().collect::<String>());
    if let Some(r) = steps.cache.borrow().get(&key) {
        return r.clone();
    }
    let r = step_uncached(&steps.table, pending, chars);
    steps.cache.borrow_mut().insert(key, r.clone());
    r
}

fn step_uncached(table: &Arc<RomajiTable>, pending: &str, chars: &[char]) -> StepResult {
    let mut conv = RomajiConverter::new(Arc::clone(table));
    for c in pending.chars() {
        // 保留は待機状態の文字列なので、再入力しても何も出ないはず
        if !conv.feed(c).is_empty() {
            return None;
        }
    }
    let mut out = String::new();
    for &c in chars {
        if is_kana_char(c) {
            // ひらがなのキー（フリック等）は、保留を確定してから素通しで挿入する
            for e in conv.flush() {
                match e {
                    Emit::Converted(t) => out.push_str(&t),
                    Emit::Raw(_) => return None,
                }
            }
            out.push(c);
            continue;
        }
        for e in conv.feed(c) {
            match e {
                Emit::Converted(t) => out.push_str(&t),
                Emit::Raw(_) => return None,
            }
        }
    }
    Some((out, conv.pending().to_string()))
}

/// `state` に `chars` を入力した新しい状態を作る（cost 加算・辞書枝刈りを含む）。生き残らなければ None
fn advance(
    state: &State,
    chars: &[char],
    add_cost: f32,
    edits: EditSummary,
    table: &Steps,
    oracle: &dyn PrefixOracle,
    cfg: &FuzzyConfig,
) -> Option<State> {
    let cost = state.cost + add_cost;
    if cost > cfg.max_cost {
        return None;
    }
    let (out, pending) = step(table, &state.pending, chars)?;
    let mut s = State {
        kana: state.kana.clone(),
        pending,
        cost,
        edits,
        boundaries: state.boundaries.clone(),
    };
    if !out.is_empty() {
        append(&mut s, &out, oracle, cfg)?;
    }
    Some(s)
}

fn is_kana_char(c: char) -> bool {
    ('ぁ'..='ゖ').contains(&c) || c == 'ー'
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// 確定文字列を追加して境界を更新し、辞書の接頭辞として成立しなければ None
fn append(s: &mut State, text: &str, oracle: &dyn PrefixOracle, cfg: &FuzzyConfig) -> Option<()> {
    for ch in text.chars() {
        let start = s.kana.len();
        s.kana.push(ch);
        let p = s.kana.len();
        if !is_kana_char(ch) {
            // 記号等は区切りとして扱う。直前までが語に分割できているときだけ続行
            if !s.boundaries.iter().any(|&(q, _)| q == start) {
                return None;
            }
            s.boundaries = vec![(p, true)];
            continue;
        }
        let is_word = s.boundaries.iter().any(|&(q, _)| {
            char_count(&s.kana[q..p]) <= MAX_WORD_CHARS && oracle.contains_word(&s.kana[q..p])
        });
        if is_word {
            s.boundaries.push((p, true));
        } else if cfg.unknown_budget > 0
            && s.boundaries
                .iter()
                .any(|&(q, genuine)| genuine && char_count(&s.kana[q..p]) <= cfg.unknown_budget)
        {
            s.boundaries.push((p, false));
        }
        let kana = &s.kana;
        s.boundaries
            .retain(|&(q, _)| char_count(&kana[q..p]) <= KEEP_BOUNDARY_CHARS);
    }
    let end = s.kana.len();
    // 末尾が境界なら照会しない。そうでなければ新しい境界から（成立しやすい順に）照会する
    (s.boundaries.iter().any(|&(q, _)| q == end)
        || s.boundaries
            .iter()
            .rev()
            .any(|&(q, _)| oracle.has_prefix(&s.kana[q..])))
    .then_some(())
}

/// 保留を確定する。不正（Raw）や枝刈りで None
fn finish(s: State, table: &Steps, oracle: &dyn PrefixOracle, cfg: &FuzzyConfig) -> Option<State> {
    if s.pending.is_empty() {
        return Some(s);
    }
    let mut conv = RomajiConverter::new(Arc::clone(&table.table));
    for c in s.pending.chars() {
        conv.feed(c);
    }
    let mut out = String::new();
    for e in conv.flush() {
        match e {
            Emit::Converted(t) => out.push_str(&t),
            Emit::Raw(_) => return None,
        }
    }
    let mut s = s;
    s.pending.clear();
    append(&mut s, &out, oracle, cfg)?;
    Some(s)
}

fn lower(c: char) -> bool {
    c.is_ascii_lowercase()
}

/// 位置 `i`（`keys[i]` の直前）が小文字 ASCII のローマ字キーに隣接しているか。
/// 欠落補完はローマ字の綴りの欠落を補うものなので、ひらがなのキー（フリック等）だけが並ぶ位置では行わない
/// （かなだけの入力に ASCII 1 文字を挿入しても、打ち間違いとして意味を持たず、探索を無駄に広げるだけ）
fn romaji_adjacent(keys: &[KeyAlternatives], i: usize) -> bool {
    (i > 0 && lower(keys[i - 1].typed())) || keys.get(i).is_some_and(|k| lower(k.typed()))
}

/// この位置に 1 文字挿入する（テーブル上合法な連続になる a-z だけ）
fn insertions(
    s: &State,
    table: &Steps,
    oracle: &dyn PrefixOracle,
    cfg: &FuzzyConfig,
    out: &mut Vec<State>,
) {
    for c in 'a'..='z' {
        let mut edits = s.edits;
        edits.insertions += 1;
        if let Some(n) = advance(s, &[c], cfg.cost_insertion, edits, table, oracle, cfg) {
            out.push(n);
        }
    }
}

/// 位置 `i` の打鍵から出る遷移を `out`（遷移先位置, 状態）に積む。欠落補完は呼び出し側で展開済み
#[allow(clippy::too_many_arguments)]
fn transitions(
    s: &State,
    keys: &[KeyAlternatives],
    i: usize,
    in_window: bool,
    max_edits: u8,
    table: &Steps,
    oracle: &dyn PrefixOracle,
    cfg: &FuzzyConfig,
    out: &mut Vec<(usize, State)>,
) {
    let key = &keys[i];
    for (k, alt) in key.items().iter().enumerate() {
        let mut edits = s.edits;
        if k > 0 {
            edits.substitutions = edits.substitutions.saturating_add(1);
        }
        if let Some(n) = advance(s, &[alt.ch], alt.cost, edits, table, oracle, cfg) {
            out.push((i + 1, n));
        }
    }
    if !in_window || s.edits.edit_ops() >= max_edits {
        return;
    }
    let typed = key.typed();
    if !lower(typed) {
        return;
    }
    // 余剰削除
    let mut edits = s.edits;
    edits.deletions += 1;
    if let Some(n) = advance(s, &[], cfg.cost_deletion, edits, table, oracle, cfg) {
        out.push((i + 1, n));
    }
    // 入れ替え
    if let Some(next) = keys.get(i + 1) {
        let t2 = next.typed();
        if lower(t2) && t2 != typed {
            let mut edits = s.edits;
            edits.transpositions += 1;
            if let Some(n) = advance(
                s,
                &[t2, typed],
                cfg.cost_transposition,
                edits,
                table,
                oracle,
                cfg,
            ) {
                out.push((i + 2, n));
            }
        }
    }
    // 子音の紛れ
    if cfg.enable_confusions {
        for &(from, to) in CONSONANT_CONFUSIONS {
            if from != typed {
                continue;
            }
            let mut edits = s.edits;
            edits.confusions += 1;
            if let Some(n) = advance(s, &[to], cfg.cost_confusion, edits, table, oracle, cfg) {
                out.push((i + 1, n));
            }
        }
    }
}

/// 重複（同じ確定・保留・編集数）を除き、cost 順に上位 `beam` 件へ絞る
fn select(states: &mut Vec<State>, beam: usize) {
    let mut best: HashMap<(String, String, u8), State> = HashMap::new();
    for s in states.drain(..) {
        match best.get_mut(&s.key()) {
            Some(old) if old.cost <= s.cost => {}
            Some(old) => *old = s,
            None => {
                best.insert(s.key(), s);
            }
        }
    }
    let mut v: Vec<State> = best.into_values().collect();
    v.sort_by(|a, b| {
        a.cost
            .total_cmp(&b.cost)
            .then_with(|| a.kana.cmp(&b.kana))
            .then_with(|| a.pending.cmp(&b.pending))
    });
    v.truncate(beam);
    *states = v;
}
