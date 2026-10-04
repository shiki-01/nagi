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
//!
//! # 合成ノイズ評価（曖昧入力。`docs/fuzzy-input-design.md` §10-3）
//!
//! `--noise touch|typo` を付けると、上の評価の代わりに曖昧入力の評価を行う（`Conversion Expected`
//! 系の行の読みからローマ字の打鍵列を作り、ノイズを混ぜて入力 → Space 変換）:
//!
//! ```text
//! cargo run --release -p nagi-server --bin nagi-eval -- --data-dir ../data --no-llm \
//!     --noise touch --noise-rate 0.15 --seed 1 [--max-noise 1] [--tap-sigma 0.25] [--top-n 5] [--verbose]
//! ```
//!
//! - `--noise-rate`: 1 打鍵あたりのノイズ確率（既定 0.1）、`--max-noise`: 1 入力に混ぜる最大数（既定 1）
//! - `--noise-tail N`: ノイズを末尾 N 打鍵にだけ混ぜる（既定 0 = 全体。補正の `window` は 8 なので、
//!   機能そのものの効果を見るには 8 以下にする）
//! - `--seed`: 乱数の種（既定 1。同じ値なら再現する）、`--tap-sigma`: タップ位置誤差の標準偏差（キー幅比）
//! - `--top-n`: 「候補に出た」とみなす上位件数（既定 5）、`--limit`: 評価データの先頭 N 行
//! - 補正ありの設定の上書き（較正用）: `--fuzzy-sigma` `--input-weight` `--ambiguity-margin` `--poor-coverage`
//!   `--fuzzy-max-edits` `--fuzzy-hypotheses`
//!
//! 補正あり/なしで、ノイズ無し入力の第 1 候補・補正由来を除く候補リストが 1 件でも変わったら
//! （劣化）、非ゼロで終了する。

use nagi_engine::{ConversionBackend, Engine};
use nagi_romaji::RomajiTable;
use nagi_server::noise_eval::{
    CountingBackend, EvalCase, EvalSettings, NoiseConfig, NoiseKind, Romanizer, evaluate,
    percentile,
};
use nagi_server::setup::engine_config_from;
use nagi_server::{ServerOptions, build};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
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
    let mut noise: Option<NoiseKind> = None;
    let mut noise_cfg = NoiseConfig {
        rate: 0.1,
        ..NoiseConfig::default()
    };
    let mut seed = 1u64;
    let mut top_n = 5usize;
    let mut tune = Tuning::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => options.data_dir = args.next().map(PathBuf::from),
            "--no-llm" => options.use_llm = false,
            "--user-dir" => options.user_dir = args.next().map(PathBuf::from),
            "--sync-budget-ms" => {
                options.sync_budget_ms = args.next().and_then(|v| v.parse().ok());
            }
            "--limit" => {
                limit = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(usize::MAX)
            }
            "--verbose" => verbose = true,
            "--noise" => {
                noise = Some(
                    args.next()
                        .as_deref()
                        .and_then(NoiseKind::parse)
                        .unwrap_or_else(|| usage("--noise は touch か typo")),
                );
            }
            "--noise-rate" => noise_cfg.rate = number(&mut args, "--noise-rate"),
            "--max-noise" => noise_cfg.max_noise = number(&mut args, "--max-noise"),
            "--noise-tail" => noise_cfg.tail = number(&mut args, "--noise-tail"),
            "--tap-sigma" => noise_cfg.tap_sigma = number(&mut args, "--tap-sigma"),
            "--seed" => seed = number(&mut args, "--seed"),
            "--top-n" => top_n = number(&mut args, "--top-n"),
            "--fuzzy-sigma" => tune.sigma = Some(number(&mut args, "--fuzzy-sigma")),
            "--input-weight" => tune.input_weight = Some(number(&mut args, "--input-weight")),
            "--ambiguity-margin" => {
                tune.ambiguity_margin = Some(number(&mut args, "--ambiguity-margin"));
            }
            "--poor-coverage" => tune.poor_coverage = Some(number(&mut args, "--poor-coverage")),
            "--fuzzy-hypotheses" => {
                tune.max_hypotheses = Some(number(&mut args, "--fuzzy-hypotheses"));
            }
            "--fuzzy-max-edits" => tune.max_edits = Some(number(&mut args, "--fuzzy-max-edits")),
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
    if let Some(kind) = noise {
        noise_cfg.kind = kind;
        if !(0.0..=1.0).contains(&noise_cfg.rate) {
            usage("--noise-rate は 0..1");
        }
        run_noise_eval(
            &text, &options, noise_cfg, seed, top_n, tune, limit, verbose,
        );
        return;
    }
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

    let built = build(&options);
    for note in &built.notes {
        eprintln!("{note}");
    }
    let backend = built.pipeline.clone();
    eprintln!(
        "backend={} profile={}",
        backend.name(),
        built.runtime.profile().as_str()
    );

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

/// 補正ありの設定の上書き（較正用）
#[derive(Default, Clone, Copy)]
struct Tuning {
    sigma: Option<f32>,
    input_weight: Option<f32>,
    ambiguity_margin: Option<f32>,
    poor_coverage: Option<f32>,
    max_edits: Option<u8>,
    max_hypotheses: Option<usize>,
}

fn usage(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn number<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> T {
    args.next()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| usage(&format!("{name} の値が不正です")))
}

/// 合成ノイズ評価（`--noise`）。劣化があれば非ゼロで終了する
#[allow(clippy::too_many_arguments)]
fn run_noise_eval(
    tsv: &str,
    options: &ServerOptions,
    noise: NoiseConfig,
    seed: u64,
    top_n: usize,
    tune: Tuning,
    limit: usize,
    verbose: bool,
) {
    let cases: Vec<EvalCase> = tsv
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 5 && c[3].starts_with("Conversion Expected")).then(|| EvalCase {
                reading: c[1].to_string(),
                expected: c[4].to_string(),
            })
        })
        .take(limit)
        .collect();
    let built = build(options);
    for note in &built.notes {
        eprintln!("{note}");
    }
    let counting = Arc::new(CountingBackend::new(built.pipeline.clone()));
    let backend: Arc<dyn ConversionBackend> = counting.clone();
    eprintln!(
        "backend={} profile={}",
        backend.name(),
        built.runtime.profile().as_str()
    );
    let (mut config, _) = engine_config_from(&built.config);
    // 評価は全文の候補リスト（sentence 表示）で見る。補正の設定は `[fuzzy]` の値を使い、
    // 上書き指定があれば補正ありの側だけに反映する
    config.fuzzy.enabled = false;
    let mut off = Engine::new(backend.clone(), config.clone());
    config.fuzzy.enabled = true;
    if let Some(v) = tune.sigma {
        config.fuzzy.search.sigma = v;
    }
    if let Some(v) = tune.input_weight {
        config.fuzzy.input_weight = v;
    }
    if let Some(v) = tune.ambiguity_margin {
        config.fuzzy.ambiguity_margin = v;
    }
    if let Some(v) = tune.poor_coverage {
        config.fuzzy.poor_coverage = v;
    }
    if let Some(v) = tune.max_hypotheses {
        config.fuzzy.search.max_hypotheses = v;
    }
    if let Some(v) = tune.max_edits {
        config.fuzzy.search.max_edits = v;
    }
    let fz = config.fuzzy.clone();
    let mut on = Engine::new(backend, config);
    let romanizer = Romanizer::new(RomajiTable::standard());
    let settings = EvalSettings {
        noise,
        seed,
        top_n,
        verbose,
    };
    let started = Instant::now();
    let report = evaluate(&cases, &romanizer, &mut off, &mut on, &counting, &settings);
    for line in &report.examples {
        println!("{line}");
    }
    let typeable = report.cases - report.skipped;
    let pct = |n: usize, d: usize| 100.0 * n as f64 / d.max(1) as f64;
    println!(
        "noise={:?} rate={} max_noise={} tail={} tap_sigma={} seed={} top_n={top_n}",
        noise.kind, noise.rate, noise.max_noise, noise.tail, noise.tap_sigma, seed
    );
    println!(
        "fuzzy: sigma={} input_weight={} ambiguity_margin={} poor_coverage={} max_edits={} window={} max_cost={} hypotheses={}",
        fz.search.sigma,
        fz.input_weight,
        fz.ambiguity_margin,
        fz.poor_coverage,
        fz.search.max_edits,
        fz.search.window,
        fz.search.max_cost,
        fz.search.max_hypotheses
    );
    println!(
        "cases={} skipped(not typeable)={} clean_top1_correct={} noised={}",
        report.cases, report.skipped, report.clean_correct, report.noised
    );
    for (label, t) in [("fuzzy off", &report.off), ("fuzzy on ", &report.on)] {
        println!(
            "{label}: top1 {:>4} / {:<4} ({:.1}%)  top{top_n} {:>4} ({:.1}%)  any {:>4} ({:.1}%)  space latency p50={:.1}ms p95={:.1}ms",
            t.top1,
            report.noised,
            pct(t.top1, report.noised),
            t.top_n,
            pct(t.top_n, report.noised),
            t.any,
            pct(t.any, report.noised),
            percentile(&t.millis, 0.5),
            percentile(&t.millis, 0.95),
        );
    }
    println!(
        "additional hypotheses run: noisy {} / {} ({:.1}%), clean(no alternatives) {} / {} ({:.1}%), clean(accurate taps) {} / {} ({:.1}%)",
        report.hypotheses_run,
        report.noised,
        pct(report.hypotheses_run, report.noised),
        report.clean_hypotheses_run,
        typeable,
        pct(report.clean_hypotheses_run, typeable),
        report.tap_hypotheses_run,
        typeable,
        pct(report.tap_hypotheses_run, typeable),
    );
    println!(
        "degradation (clean input, on vs off): {} / {} (unstable baseline cases: {})",
        report.degraded, report.degradation_checked, report.unstable_baseline
    );
    eprintln!("elapsed {:.1}s", started.elapsed().as_secs_f64());
    if report.degraded > 0 {
        eprintln!("劣化あり（補正ありでノイズ無し入力の結果が変わった）");
        std::process::exit(1);
    }
}
