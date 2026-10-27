//! 英単語の綴りの「もしかして」（`docs/plan-next-features.md` C）: 語彙に無い打鍵列に近い語彙の語を、
//! 重みつきの編集距離（キーの近接で置換を安くする）で探す。
//!
//! 探索は語彙の fst を深さ優先でたどり、編集距離の DP の行を 1 文字ずつ伸ばす。行の最小値が上限を
//! 超えた枝は刈る（語彙全体を総当たりしない）。隣り合う 2 文字の入れ替え（`teh` → `the`）は
//! 1 回の編集として数える（制限つき Damerau–Levenshtein）。
//!
//! キーの近さは呼び出し側が [`SpellCosts::substitute`] で渡す（配置の知識はこのクレートに持たない）。

use crate::lexicon::Lexicon;
use fst::raw::{Fst, Node, Output};

/// 綴りの候補を探す最小の文字数。短い語は 1 文字の違いで別の語になりやすく、誤補正が増える
pub const MIN_SPELL_LEN: usize = 4;
/// 綴りの候補を探す最大の文字数（長い入力は探索が重く、打ち間違いより別の語の連なりが多い）
pub const MAX_SPELL_LEN: usize = 24;

/// 編集の重み
pub struct SpellCosts<'a> {
    /// 置換 `typed`（打った文字）→ `intended`（語彙の文字）の重み。近いキーほど安くする
    pub substitute: &'a dyn Fn(u8, u8) -> f32,
    /// 挿入（語彙の文字が抜けていた）の重み
    pub insert: f32,
    /// 削除（余分な文字を打った）の重み
    pub delete: f32,
    /// 直前と同じ文字の挿入・削除（`adress` → `address`、`helllo` → `hello`）の重み
    pub double: f32,
    /// 隣り合う 2 文字の入れ替えの重み
    pub transpose: f32,
}

/// 既定の重み（置換は一律 1.0。キーの近さを使うなら `substitute` を差し替える）
pub fn uniform_substitute(_: u8, _: u8) -> f32 {
    1.0
}

impl Default for SpellCosts<'_> {
    fn default() -> Self {
        SpellCosts {
            substitute: &uniform_substitute,
            insert: 1.0,
            delete: 1.0,
            double: 0.5,
            transpose: 0.7,
        }
    }
}

/// 綴りの候補
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// 語彙の語
    pub word: String,
    /// 頻度の段階（小さいほど一般的）
    pub tier: u8,
    /// 重みつきの編集距離
    pub cost: f32,
}

impl Suggestion {
    /// 並べ替えの得点（小さいほど良い）。編集距離を主に、同じくらいなら一般的な語を先にする
    pub fn score(&self) -> f32 {
        self.cost + f32::from(self.tier) * 0.01
    }
}

/// 文字数に応じた編集距離の上限。短い語ほど厳しくする（4〜5 文字は 1 回、6〜8 文字は 1.6、それ以上は 2 回）
pub fn max_cost_for(len: usize) -> f32 {
    match len {
        0..=5 => 1.0,
        6..=8 => 1.6,
        _ => 2.0,
    }
}

/// `word`（小文字の ASCII 英字）に近い語彙の語を、良い順に最大 `limit` 件返す。`word` 自体が語彙に
/// あれば（打ち間違いではないので）空。文字数が [`MIN_SPELL_LEN`]〜[`MAX_SPELL_LEN`] の外・英小文字以外を
/// 含むときも空
pub fn suggest(
    lexicon: &Lexicon,
    word: &str,
    costs: &SpellCosts<'_>,
    limit: usize,
) -> Vec<Suggestion> {
    let len = word.len();
    if limit == 0
        || !(MIN_SPELL_LEN..=MAX_SPELL_LEN).contains(&len)
        || !word.bytes().all(|b| b.is_ascii_lowercase())
        || lexicon.tier(word).is_some()
    {
        return Vec::new();
    }
    let typed = word.as_bytes();
    let max_cost = max_cost_for(len);
    let fst = lexicon.fst();
    let mut search = Search {
        fst,
        typed,
        costs,
        max_cost,
        prefix: Vec::with_capacity(len + 3),
        rows: vec![first_row(typed, costs)],
        found: Vec::new(),
    };
    search.walk(fst.root(), Output::zero());
    let mut found = search.found;
    found.sort_by(|a, b| a.score().total_cmp(&b.score()).then(a.word.cmp(&b.word)));
    found.truncate(limit);
    found
}

/// 語彙の語が空のときの行: 打った文字を i 文字消す重み
fn first_row(typed: &[u8], costs: &SpellCosts<'_>) -> Vec<f32> {
    let mut row = Vec::with_capacity(typed.len() + 1);
    row.push(0.0);
    for i in 0..typed.len() {
        let prev = row[i];
        row.push(prev + delete_cost(typed, i, costs));
    }
    row
}

/// 打った文字 `typed[i]` を消す重み（直前と同じ文字なら安い）
fn delete_cost(typed: &[u8], i: usize, costs: &SpellCosts<'_>) -> f32 {
    if i > 0 && typed[i - 1] == typed[i] {
        costs.double
    } else {
        costs.delete
    }
}

struct Search<'a, 'c, D: AsRef<[u8]>> {
    fst: &'a Fst<D>,
    typed: &'a [u8],
    costs: &'a SpellCosts<'c>,
    max_cost: f32,
    /// 語彙の語のうち、いまたどっている接頭辞
    prefix: Vec<u8>,
    /// 接頭辞の各長さに対する DP の行（`rows[k][i]` = 接頭辞 k 文字と打った i 文字の距離）
    rows: Vec<Vec<f32>>,
    found: Vec<Suggestion>,
}

impl<D: AsRef<[u8]>> Search<'_, '_, D> {
    fn walk(&mut self, node: Node<'_>, out: Output) {
        let n = self.typed.len();
        if node.is_final() {
            let cost = self.rows.last().map_or(f32::INFINITY, |r| r[n]);
            if cost <= self.max_cost + 1e-4 && !self.prefix.is_empty() {
                let value = out.cat(node.final_output()).value();
                self.found.push(Suggestion {
                    // 語彙は ASCII 英小文字だけ（`LexiconBuilder::add`）
                    word: String::from_utf8_lossy(&self.prefix).into_owned(),
                    tier: value as u8,
                    cost,
                });
            }
        }
        for t in node.transitions() {
            let row = self.next_row(t.inp);
            if row.iter().copied().fold(f32::INFINITY, f32::min) > self.max_cost + 1e-4 {
                continue;
            }
            self.prefix.push(t.inp);
            self.rows.push(row);
            self.walk(self.fst.node(t.addr), out.cat(t.out));
            self.rows.pop();
            self.prefix.pop();
        }
    }

    /// 接頭辞に語彙の文字 `c` を足したときの DP の行
    fn next_row(&self, c: u8) -> Vec<f32> {
        let typed = self.typed;
        let costs = self.costs;
        let prev = self.rows.last().expect("先頭の行は常にある");
        let prev2 = self.rows.len().checked_sub(2).map(|k| &self.rows[k]);
        let last = self.prefix.last().copied();
        let insert = if last == Some(c) {
            costs.double
        } else {
            costs.insert
        };
        let mut row = Vec::with_capacity(typed.len() + 1);
        row.push(prev[0] + insert);
        for i in 1..=typed.len() {
            let t = typed[i - 1];
            let sub = if t == c {
                0.0
            } else {
                (costs.substitute)(t, c)
            };
            let mut best = (prev[i - 1] + sub)
                .min(prev[i] + insert)
                .min(row[i - 1] + delete_cost(typed, i - 1, costs));
            if let (Some(prev2), Some(last)) = (prev2, last)
                && i >= 2
                && t == last
                && typed[i - 2] == c
                && t != c
            {
                best = best.min(prev2[i - 2] + costs.transpose);
            }
            row.push(best);
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::LexiconBuilder;

    fn lexicon(words: &[(&str, u8)]) -> Lexicon {
        let mut b = LexiconBuilder::new();
        for (w, t) in words {
            b.add(w, *t);
        }
        Lexicon::from_vec(b.build().unwrap()).unwrap()
    }

    fn words(found: &[Suggestion]) -> Vec<&str> {
        found.iter().map(|s| s.word.as_str()).collect()
    }

    #[test]
    fn finds_missing_letter_and_ranks_by_cost() {
        let lex = lexicon(&[("typescript", 70), ("javascript", 50), ("type", 10)]);
        let found = suggest(&lex, "typscript", &SpellCosts::default(), 5);
        assert_eq!(words(&found), ["typescript"]);
        assert!((found[0].cost - 1.0).abs() < 1e-4);
        assert_eq!(found[0].tier, 70);
    }

    #[test]
    fn transposition_and_doubled_letters_are_cheap() {
        let lex = lexicon(&[("receive", 20), ("address", 20), ("hello", 10)]);
        let c = SpellCosts::default();
        let found = suggest(&lex, "recieve", &c, 3);
        assert_eq!(words(&found), ["receive"]);
        assert!((found[0].cost - 0.7).abs() < 1e-4);
        let found = suggest(&lex, "adress", &c, 3);
        assert_eq!(words(&found), ["address"]);
        assert!((found[0].cost - 0.5).abs() < 1e-4);
        let found = suggest(&lex, "helllo", &c, 3);
        assert_eq!(words(&found), ["hello"]);
        assert!((found[0].cost - 0.5).abs() < 1e-4);
    }

    #[test]
    fn substitution_cost_prefers_near_keys() {
        let lex = lexicon(&[("world", 30), ("would", 10)]);
        // wprld: p は o の隣（安い）。would は 2 回の置換が要る
        let near = |a: u8, b: u8| if (a, b) == (b'p', b'o') { 0.6 } else { 1.0 };
        let c = SpellCosts {
            substitute: &near,
            ..SpellCosts::default()
        };
        let found = suggest(&lex, "wprld", &c, 3);
        assert_eq!(words(&found), ["world"]);
        assert!((found[0].cost - 0.6).abs() < 1e-4);
    }

    #[test]
    fn known_word_short_or_far_input_gives_nothing() {
        let lex = lexicon(&[("hello", 10), ("help", 10), ("typescript", 70)]);
        let c = SpellCosts::default();
        assert!(suggest(&lex, "hello", &c, 5).is_empty());
        assert!(suggest(&lex, "hel", &c, 5).is_empty());
        assert!(suggest(&lex, "Hello", &c, 5).is_empty());
        assert!(suggest(&lex, "kyouha", &c, 5).is_empty());
        // 5 文字以下は 1 回まで: hlep は入れ替え 1 回で help
        assert_eq!(words(&suggest(&lex, "hlep", &c, 5)), ["help"]);
        assert!(suggest(&lex, "hxlpo", &c, 5).is_empty());
        assert!(suggest(&lex, "helo", &c, 0).is_empty());
    }

    #[test]
    fn ties_prefer_common_words_and_limit_applies() {
        let lex = lexicon(&[("helo", 90), ("hello", 10), ("help", 10), ("hell", 35)]);
        // helo 自体は語彙にあるので空。heli で比べる
        let c = SpellCosts::default();
        let found = suggest(&lex, "heli", &c, 2);
        assert_eq!(found.len(), 2);
        // どれも 1 回の置換。一般的な語（段階が小さい）が先
        assert_eq!(found[0].word, "help");
        assert_eq!(found[1].word, "hell");
    }
}
