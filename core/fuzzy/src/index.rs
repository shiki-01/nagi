//! 辞書による枝刈り。
//!
//! 展開途中の読みが辞書で意味を持ち得るかを、辞書の trie への問い合わせだけで判定する
//! （`ReadingIndex`）。辞書の実体（`nagi-dict`）には依存せず、呼び出し側が実装を渡す。

/// 読みの索引（辞書の trie）
pub trait ReadingIndex {
    /// `text` の先頭から始まる辞書の読みのバイト長（短い順）
    fn word_prefix_lens(&self, text: &str) -> Vec<usize>;
    /// 読みが `prefix` で始まる語（`prefix` 自身を含む）があるか
    fn has_reading_prefix(&self, prefix: &str) -> bool;
}

/// 1 文字で語として数える仮名（助詞・活用語尾など）。辞書には 1 文字の読みがほぼ全部あるため、
/// 何でも許すと「どんな仮名列も語の連なり」になり枝刈りが効かない。そこで 2 文字以上の語か、
/// この集合の 1 文字だけを「語として成立する片」とみなす（ヒューリスティック。評価で調整する）
const SINGLE_KANA_PIECES: &str = "はがをにのへでともやかねよなたてだしいくるればずんぬっ";

fn piece_ok(piece: &str) -> bool {
    let mut chars = piece.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => SINGLE_KANA_PIECES.contains(c),
        (Some(_), Some(_)) => true,
        _ => false,
    }
}

/// 展開途中の読み（`kana`）が辞書の語の連なりとして成立し得るかを、読みを後ろへ伸ばすたびに
/// 差分だけ更新して調べるための状態。
///
/// 語の境界（バイト位置）のうち、そこから先が「辞書の語の接頭辞」か「空（読みの終わり）」で、
/// まだ語が続き得るものを `alive` に持つ。読みを伸ばしても、すでに閉じた語の境界は変わらない
/// （先頭側は固定で、末尾にだけ文字が付く）ので、前の状態から続きを計算できる。
/// 全体を毎回走査すると、探索の 1 ノードごとに読みの長さ分の trie 問い合わせが要る
#[derive(Debug, Clone)]
pub struct Frontier {
    alive: Vec<usize>,
    /// この状態の読みのバイト長
    len: usize,
}

impl Frontier {
    /// 空の読みの状態
    pub fn new() -> Self {
        Frontier {
            alive: vec![0],
            len: 0,
        }
    }

    /// 読みを `kana`（これまでの読みに文字を足したもの）に伸ばした状態。成立し得なければ `None`
    pub fn extend(&self, index: &dyn ReadingIndex, kana: &str) -> Option<Frontier> {
        debug_assert!(kana.len() >= self.len);
        let mut alive: Vec<usize> = Vec::with_capacity(self.alive.len() + 2);
        let mut work: Vec<usize> = self.alive.clone();
        let mut done: Vec<usize> = Vec::new();
        while let Some(b) = work.pop() {
            if done.contains(&b) {
                continue;
            }
            done.push(b);
            let rest = &kana[b..];
            if rest.is_empty() || index.has_reading_prefix(rest) {
                alive.push(b);
            }
            if rest.is_empty() {
                continue;
            }
            for l in index.word_prefix_lens(rest) {
                let end = b + l;
                // 古い境界で閉じた語は、前の状態で処理済み（生き残っていればそこにいる）
                if end > self.len && piece_ok(&rest[..l]) {
                    work.push(end);
                }
            }
        }
        if alive.is_empty() {
            return None;
        }
        alive.sort_unstable();
        Some(Frontier {
            alive,
            len: kana.len(),
        })
    }

    /// 読み全体が語の連なりに分けられるか（読みの終わりが境界に含まれる。空の読みは含まない）
    pub fn covered(&self) -> bool {
        self.len > 0 && self.alive.last() == Some(&self.len)
    }
}

impl Default for Frontier {
    fn default() -> Self {
        Self::new()
    }
}

/// `kana` の途中までが、辞書の語の連なり（＋最後は語の接頭辞）として成立し得るか。
/// 空は成立。展開の各ノードで使う枝刈りの、差分なしの版（[`Frontier`] が本体）
pub fn viable_prefix(index: &dyn ReadingIndex, kana: &str) -> bool {
    Frontier::new().extend(index, kana).is_some()
}

/// `kana` 全体が、辞書の語の連なりに分けられるか
pub fn covered(index: &dyn ReadingIndex, kana: &str) -> bool {
    Frontier::new()
        .extend(index, kana)
        .is_some_and(|f| f.covered())
}

/// テスト用: 語の集合から作る索引
#[derive(Debug, Clone, Default)]
pub struct WordSet {
    words: std::collections::BTreeSet<String>,
}

impl WordSet {
    /// 読みの一覧から作る
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(words: I) -> Self {
        WordSet {
            words: words.into_iter().map(Into::into).collect(),
        }
    }
}

impl ReadingIndex for WordSet {
    fn word_prefix_lens(&self, text: &str) -> Vec<usize> {
        text.char_indices()
            .map(|(i, c)| i + c.len_utf8())
            .filter(|&end| self.words.contains(&text[..end]))
            .collect()
    }

    fn has_reading_prefix(&self, prefix: &str) -> bool {
        self.words
            .range::<String, _>(prefix.to_string()..)
            .next()
            .is_some_and(|w| w.starts_with(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viable_and_covered() {
        let idx = WordSet::new(["かんじ", "かん", "じ", "きょう", "は"]);
        assert!(viable_prefix(&idx, ""));
        assert!(viable_prefix(&idx, "かん"));
        assert!(viable_prefix(&idx, "かんじきょ"), "語 + 接頭辞");
        assert!(!viable_prefix(&idx, "かmじ"), "辞書に無い文字");
        assert!(!viable_prefix(&idx, "くんじ"));
        assert!(covered(&idx, "かんじ"));
        assert!(covered(&idx, "きょうは"));
        assert!(
            !covered(&idx, "きょう"[..6].to_string().as_str()),
            "語の途中"
        );
        assert!(!covered(&idx, ""));
    }

    #[test]
    fn single_kana_only_for_function_words() {
        // 「あ」は辞書にあっても片として数えない。「は」は数える
        let idx = WordSet::new(["あ", "は", "かん"]);
        assert!(!covered(&idx, "かんあ"));
        assert!(covered(&idx, "かんは"));
    }
}
