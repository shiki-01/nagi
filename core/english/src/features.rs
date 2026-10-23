//! ローマ字としての妥当性の特徴量。
//!
//! キー列（小文字の ASCII）を標準のローマ字テーブルで変換し、かなにならずに素通しされた文字
//! （`Emit::Raw`）の数と並びを数える。`typescript` は `t` `y` `p` `s` `c` … が素通しになるが、
//! 日本語のローマ字（`kyouha`）は 0 になる。日本語に無い子音連続（`pt` `sc` など）は
//! テーブルに当たらないので、素通しとして数えられる。

use nagi_romaji::{Emit, RomajiConverter, RomajiTable};
use std::sync::Arc;

/// 判定に使う特徴量
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Features {
    /// キー列の長さ（文字数）
    pub len: usize,
    /// かなに変換された文字数
    pub kana_chars: usize,
    /// かなにならず素通しされた文字数
    pub raw_count: usize,
    /// 素通しの連続の最大長（`pt` なら 2）
    pub max_raw_run: usize,
}

impl Features {
    /// 素通しの割合（0.0〜1.0）
    pub fn raw_ratio(&self) -> f32 {
        if self.len == 0 {
            0.0
        } else {
            self.raw_count as f32 / self.len as f32
        }
    }
}

/// キー列を変換して特徴量を求める
pub fn analyze(table: &Arc<RomajiTable>, raw: &str) -> Features {
    let mut conv = RomajiConverter::new(table.clone());
    let mut emits = Vec::new();
    for ch in raw.chars() {
        emits.extend(conv.feed(ch));
    }
    emits.extend(conv.flush());
    let (mut kana_chars, mut raw_count, mut run, mut max_run) = (0, 0, 0, 0);
    for e in &emits {
        match e {
            Emit::Converted(t) => {
                kana_chars += t.chars().count();
                run = 0;
            }
            Emit::Raw(_) => {
                raw_count += 1;
                run += 1;
                max_run = max_run.max(run);
            }
        }
    }
    Features {
        len: raw.chars().count(),
        kana_chars,
        raw_count,
        max_raw_run: max_run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Arc<RomajiTable> {
        RomajiTable::standard()
    }

    #[test]
    fn valid_romaji_leaves_nothing() {
        for w in ["kyouha", "arigatou", "nippon", "sensei", "kitte", "n"] {
            let f = analyze(&table(), w);
            assert_eq!(f.raw_count, 0, "{w}");
        }
    }

    #[test]
    fn english_leaves_ascii() {
        let f = analyze(&table(), "typescript");
        assert!(f.raw_count >= 4, "{f:?}");
        assert!(f.raw_ratio() > 0.4);
        assert!(f.max_raw_run >= 2);
        let f = analyze(&table(), "text");
        assert!(f.raw_count >= 1);
    }
}
