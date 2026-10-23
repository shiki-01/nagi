//! 評価コーパス（`bench/auto-english/corpus.tsv`）で、英語の自動判定の誤判定率と取りこぼし率を測る。
//!
//! - 誤判定率: 日本語（`ja`）と日英混在（`mixed`）のうち、英語と判定（Mid 以上 / High）されたものの割合
//! - 取りこぼし率: 英語（`en`）のうち、Mid 以上 / High に届かなかったものの割合
//! - 曖昧（`amb`）は率に含めず、分布だけを出す
//!
//! 語彙は (a) なし（劣化動作） (b) 自作の小さな語彙（`tests/fixtures/words.txt`） (c) 実データ
//! （`data/english/words.fst`。`cargo xtask fetch-english` で取得。無ければ省略。環境変数
//! `NAGI_ENGLISH_LEXICON` で場所を上書きできる）の 3 通りで測る。`cargo test -p nagi-english
//! --test corpus -- --nocapture` で表が出る。

use nagi_english::{Judge, Level, Lexicon, LexiconBuilder};
use std::path::PathBuf;
use std::sync::Arc;

struct Row {
    label: String,
    raw: String,
    dict_known: bool,
}

fn corpus() -> Vec<Row> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/auto-english/corpus.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert_eq!(c.len(), 3, "{l}");
            Row {
                label: c[0].to_string(),
                raw: c[1].to_string(),
                dict_known: c[2] == "1",
            }
        })
        .collect()
}

fn fixture_lexicon() -> Arc<Lexicon> {
    let text = include_str!("fixtures/words.txt");
    let mut b = LexiconBuilder::new();
    for l in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let (w, t) = l.split_once('\t').unwrap();
        assert!(b.add(w, t.parse().unwrap()), "{w}");
    }
    Arc::new(Lexicon::from_vec(b.build().unwrap()).unwrap())
}

fn real_lexicon() -> Option<Arc<Lexicon>> {
    let path = std::env::var_os("NAGI_ENGLISH_LEXICON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/english/words.fst")
        });
    Lexicon::load(&path).ok().map(Arc::new)
}

#[derive(Debug, Default)]
struct Report {
    negatives: usize,
    false_mid: Vec<String>,
    false_high: Vec<String>,
    positives: usize,
    missed_mid: Vec<String>,
    missed_high: Vec<String>,
    amb: [usize; 4],
}

impl Report {
    fn rate(n: usize, d: usize) -> f64 {
        100.0 * n as f64 / d as f64
    }
}

fn evaluate(judge: &Judge, rows: &[Row]) -> Report {
    let mut r = Report::default();
    for row in rows {
        let level = judge.judge(&row.raw, row.dict_known).level;
        match row.label.as_str() {
            "ja" | "mixed" => {
                r.negatives += 1;
                if level >= Level::Mid {
                    r.false_mid.push(row.raw.clone());
                }
                if level >= Level::High {
                    r.false_high.push(row.raw.clone());
                }
            }
            "en" => {
                r.positives += 1;
                if level < Level::Mid {
                    r.missed_mid.push(row.raw.clone());
                }
                if level < Level::High {
                    r.missed_high.push(row.raw.clone());
                }
            }
            "amb" => r.amb[level as usize] += 1,
            other => panic!("不明なラベル: {other}"),
        }
    }
    r
}

fn print(name: &str, r: &Report) {
    println!(
        "[{name}] 誤判定率 Mid+ {:.2}% ({}/{}) High {:.2}% ({}/{}) / 取りこぼし率 Mid+ {:.1}% ({}/{}) High {:.1}% ({}/{})",
        Report::rate(r.false_mid.len(), r.negatives),
        r.false_mid.len(),
        r.negatives,
        Report::rate(r.false_high.len(), r.negatives),
        r.false_high.len(),
        r.negatives,
        Report::rate(r.missed_mid.len(), r.positives),
        r.missed_mid.len(),
        r.positives,
        Report::rate(r.missed_high.len(), r.positives),
        r.missed_high.len(),
        r.positives,
    );
    println!("  曖昧の分布 None/Low/Mid/High = {:?}", r.amb);
    println!("  誤判定(Mid+): {:?}", r.false_mid);
    println!("  取りこぼし(Mid+): {:?}", r.missed_mid);
}

#[test]
fn corpus_is_well_formed() {
    let rows = corpus();
    let count = |l: &str| rows.iter().filter(|r| r.label == l).count();
    assert!(rows.len() >= 300, "{}", rows.len());
    assert!(count("ja") >= 100 && count("en") >= 100 && count("mixed") >= 20 && count("amb") >= 10);
    for r in &rows {
        assert!(r.raw.bytes().all(|b| b.is_ascii_lowercase()), "{}", r.raw);
    }
}

#[test]
fn degraded_mode_without_lexicon() {
    let rows = corpus();
    let r = evaluate(&Judge::standard(None), &rows);
    print("語彙なし", &r);
    // 日本語を英語と判定しない（High は 0、Mid 以上も 1% 以下）
    assert!(r.false_high.is_empty(), "{:?}", r.false_high);
    assert!(Report::rate(r.false_mid.len(), r.negatives) <= 1.0);
    // 有効なローマ字になる英語（house など）は特徴量だけでは見つからないが、残る英語は拾う
    assert!(Report::rate(r.missed_mid.len(), r.positives) <= 65.0);
}

#[test]
fn fixture_lexicon_quality() {
    let rows = corpus();
    let r = evaluate(&Judge::standard(Some(fixture_lexicon())), &rows);
    print("自作の小さな語彙", &r);
    assert!(r.false_high.is_empty(), "{:?}", r.false_high);
    assert!(Report::rate(r.false_mid.len(), r.negatives) <= 1.0);
    assert!(Report::rate(r.missed_mid.len(), r.positives) <= 30.0);
}

#[test]
fn real_lexicon_quality_if_available() {
    let Some(lexicon) = real_lexicon() else {
        eprintln!("実データの語彙が無いので省略（cargo xtask fetch-english）");
        return;
    };
    let rows = corpus();
    let r = evaluate(&Judge::standard(Some(lexicon)), &rows);
    print("SCOWL", &r);
    assert!(r.false_high.is_empty(), "{:?}", r.false_high);
    assert!(Report::rate(r.false_mid.len(), r.negatives) <= 1.0);
    assert!(Report::rate(r.missed_mid.len(), r.positives) <= 12.0);
}
