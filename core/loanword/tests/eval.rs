//! 外来語の原語候補の評価（`bench/loanword/eval.tsv`。自作・正解つき）。
//!
//! 索引は `data/loanword/loanword.fst`（`cargo xtask fetch-loanword`）。無ければ省略する。
//! `cargo test -p nagi-loanword --test eval -- --nocapture` で内訳を表示する。
//!
//! - 取りこぼし率 = `pos` のうち、期待する原語が候補に出なかった割合
//! - 誤訳率 = `pos` のうち、候補は出たが期待する原語が無かった割合（取りこぼしの内数）
//! - 誤出力率 = `wasei` と `neg`（出してはいけない語）のうち、何か候補が出た割合

use nagi_loanword::{LoanwordIndex, LoanwordRewriter};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn evaluation_corpus() {
    let index_path = root().join("data/loanword/loanword.fst");
    let Ok(index) = LoanwordIndex::load(&index_path) else {
        eprintln!("省略: {} が無い", index_path.display());
        return;
    };
    let corpus = std::fs::read_to_string(root().join("bench/loanword/eval.tsv")).unwrap();
    let rewriter = LoanwordRewriter::new(Arc::new(index));

    let (mut pos, mut hit, mut wrong) = (0usize, 0usize, 0usize);
    let (mut banned, mut leaked) = (0usize, 0usize);
    let mut top1 = 0usize;
    for line in corpus
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let cols: Vec<&str> = line.split('\t').collect();
        let (kind, input, expected) = (cols[0], cols[1], cols.get(2).copied().unwrap_or(""));
        // 辞書が返す表記をそのまま渡す想定（読みは渡さない）
        let got = rewriter.candidates([input], "");
        match kind {
            "pos" => {
                pos += 1;
                let ok = got.iter().any(|g| g.eq_ignore_ascii_case(expected));
                if ok {
                    hit += 1;
                    if got[0].eq_ignore_ascii_case(expected) {
                        top1 += 1;
                    }
                } else {
                    if !got.is_empty() {
                        wrong += 1;
                    }
                    println!("MISS {input} 期待={expected} 出力={got:?}");
                }
            }
            _ => {
                banned += 1;
                if !got.is_empty() {
                    leaked += 1;
                    println!("LEAK {kind} {input} 出力={got:?}");
                }
            }
        }
    }
    let rate = |n: usize, d: usize| 100.0 * n as f64 / d as f64;
    println!(
        "pos {pos}: 取りこぼし {:.1}% ({}), 誤訳 {:.1}% ({wrong}), 先頭一致 {:.1}% ({top1}) / 出してはいけない {banned}: 誤出力 {:.1}% ({leaked})",
        rate(pos - hit, pos),
        pos - hit,
        rate(wrong, pos),
        rate(top1, pos),
        rate(leaked, banned)
    );
    assert!(pos >= 150 && banned >= 30, "評価語が少なすぎる");
    // 取りこぼしは索引のデータ（JMdict の版・lsource の充足度）で大きく変わるので固定しない。
    // 誤訳（出してはいけない語・違う語を出す）だけを固定する
    assert!(rate(wrong, pos) <= 5.0, "誤訳が多すぎる: {wrong}/{pos}");
    assert!(
        rate(leaked, banned) <= 5.0,
        "誤出力が多すぎる: {leaked}/{banned}"
    );
}
