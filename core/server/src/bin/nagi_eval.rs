//! `nagi-eval`: Mozc の評価データ（`evaluation.tsv`）での変換精度・レイテンシの計測。
//!
//! `cargo xtask fetch-dict` で `data/dict/mozc/evaluation.tsv` を取得しておく。使い方:
//!
//! ```text
//! cargo run --release -p nagi-server --features llama --bin nagi-eval -- \
//!     --data-dir ../data [--no-llm] [--limit N] [--verbose]
//! ```
//!
//! 評価する行（Mozc の `status` 列は使わず、全行を Nagi の変換結果で判定する）:
//! - `Conversion Expected`: 全文の最良変換が期待値と完全一致
//! - `Conversion Expected N`: 読み全体を 1 文節としたときの候補の上位 N 件に期待値がある
//! - `Conversion Match` / `Conversion Not Match`: 全文の最良変換が期待文字列を含む / 含まない
//!
//! `Prediction` / `Suggestion` は対象外（予測変換は Phase 2）。

use nagi_server::{ServerOptions, load_backend};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

struct Case {
    reading: String,
    command: String,
    argument: String,
}

fn main() {
    let mut options = ServerOptions::default();
    let mut limit = usize::MAX;
    let mut verbose = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => options.data_dir = args.next().map(PathBuf::from),
            "--no-llm" => options.use_llm = false,
            "--sync-budget-ms" => {
                options.sync_budget_ms = args.next().and_then(|v| v.parse().ok()).unwrap_or(150);
            }
            "--limit" => {
                limit = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(usize::MAX)
            }
            "--verbose" => verbose = true,
            other => {
                eprintln!("不明な引数: {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(dir) = options.data_dir.clone() else {
        eprintln!("--data-dir が必要です");
        std::process::exit(2);
    };
    let tsv = dir.join("dict/mozc/evaluation.tsv");
    let text = std::fs::read_to_string(&tsv).unwrap_or_else(|e| {
        panic!(
            "{} を読めません（cargo xtask fetch-dict）: {e}",
            tsv.display()
        )
    });
    let cases: Vec<Case> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 5 && c[3].starts_with("Conversion")).then(|| Case {
                reading: c[1].to_string(),
                command: c[3].to_string(),
                argument: c[4].to_string(),
            })
        })
        .take(limit)
        .collect();

    let (backend, profile, note) = load_backend(&options);
    eprintln!("backend={} profile={profile}: {note}", backend.name());

    let mut stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut latencies_ms: Vec<f64> = Vec::new();
    for case in &cases {
        let started = Instant::now();
        let segments = backend.convert_sentence(&case.reading, "");
        latencies_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        let top: String = segments.iter().map(|s| s.surface.as_str()).collect();
        let (passed, detail) = match case.command.as_str() {
            "Conversion Expected" => (top == case.argument, format!("top={top}")),
            "Conversion Match" => (top.contains(&case.argument), format!("top={top}")),
            "Conversion Not Match" => (!top.contains(&case.argument), format!("top={top}")),
            c if c.starts_with("Conversion Expected ") => {
                let n: usize = c["Conversion Expected ".len()..].parse().unwrap_or(1);
                let candidates = backend.segment_candidates(&case.reading, "", 1000);
                let rank = candidates.iter().position(|c| c.surface == case.argument);
                (
                    rank.is_some_and(|r| r < n),
                    format!(
                        "rank={rank:?} top3={:?}",
                        candidates
                            .iter()
                            .take(3)
                            .map(|c| &c.surface)
                            .collect::<Vec<_>>()
                    ),
                )
            }
            _ => continue,
        };
        let key = if case.command.starts_with("Conversion Expected ") {
            "Conversion Expected N".to_string()
        } else {
            case.command.clone()
        };
        let entry = stats.entry(key).or_default();
        entry.1 += 1;
        entry.0 += usize::from(passed);
        if verbose && !passed {
            println!(
                "NG [{}] {} -> want {:?}; {detail}",
                case.command, case.reading, case.argument
            );
        }
    }

    let (mut ok, mut total) = (0, 0);
    for (command, (passed, count)) in &stats {
        println!(
            "{command:<24} {passed:>4} / {count:<4} ({:.1}%)",
            100.0 * *passed as f64 / *count as f64
        );
        ok += passed;
        total += count;
    }
    println!(
        "{:<24} {ok:>4} / {total:<4} ({:.1}%)",
        "TOTAL",
        100.0 * ok as f64 / total.max(1) as f64
    );
    latencies_ms.sort_by(f64::total_cmp);
    let pct = |p: f64| latencies_ms[((latencies_ms.len() - 1) as f64 * p) as usize];
    println!(
        "convert_sentence latency: p50={:.1}ms p95={:.1}ms max={:.1}ms (n={})",
        pct(0.5),
        pct(0.95),
        latencies_ms.last().copied().unwrap_or(0.0),
        latencies_ms.len()
    );
}
