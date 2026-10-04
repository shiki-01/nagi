//! 合成ノイズ評価ループ（`nagi_server::noise_eval::evaluate`）の統合テスト: メモリ上の小さな辞書で、
//! 補正なし/ありのエンジンを比べる。

#![allow(missing_docs)]

use nagi_config::Config;
use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};
use nagi_engine::{ConversionBackend, Engine, EngineConfig};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_romaji::RomajiTable;
use nagi_server::noise_eval::{
    CountingBackend, EvalCase, EvalSettings, NoiseConfig, NoiseKind, Report, Romanizer, evaluate,
};
use nagi_server::setup::engine_config_from;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::sync::Arc;

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 記号,句点,*,*,*,*,。
";

const WORDS: &str = "\
にほん\t1\t1\t3000\t日本
にほんご\t1\t1\t3500\t日本語
に\t2\t2\t400\tに
かんじ\t1\t1\t3000\t漢字
かんし\t1\t1\t3200\t監視
";

fn dict() -> Arc<Dictionary> {
    const N: usize = 4;
    let mut lines = vec![N.to_string()];
    lines.extend(std::iter::repeat_n("0".to_string(), N * N));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text("inline", WORDS).unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    Arc::new(b.build().unwrap())
}

fn engine_config(enabled: bool) -> EngineConfig {
    let (mut config, _) = engine_config_from(&Config::default());
    config.fuzzy.enabled = enabled;
    config
}

fn run(cases: &[EvalCase], noise: NoiseConfig, seed: u64) -> Report {
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        false,
        Tunables::from_config(&Config::default()),
        Learner::new(LearnConfig::default()),
        None,
    );
    let pipeline = Arc::new(Pipeline::new(Some(dict()), None, runtime));
    let counting = Arc::new(CountingBackend::new(pipeline));
    let backend: Arc<dyn ConversionBackend> = counting.clone();
    let mut off = Engine::new(backend.clone(), engine_config(false));
    let mut on = Engine::new(backend, engine_config(true));
    let settings = EvalSettings {
        noise,
        seed,
        top_n: 5,
        verbose: true,
    };
    evaluate(
        cases,
        &Romanizer::new(RomajiTable::standard()),
        &mut off,
        &mut on,
        &counting,
        &settings,
    )
}

fn cases() -> Vec<EvalCase> {
    [
        ("にほん", "日本"),
        ("かんじ", "漢字"),
        ("にほんご", "日本語"),
        // 辞書に無い語（変換できない）と、ひらがな以外を含む読み（対象外）
        ("ふぉ", "フォ"),
        ("漢字", "漢字"),
    ]
    .into_iter()
    .map(|(r, e)| EvalCase {
        reading: r.to_string(),
        expected: e.to_string(),
    })
    .collect()
}

#[test]
fn clean_input_is_never_degraded_and_counts_are_consistent() {
    for kind in [NoiseKind::Touch, NoiseKind::Typo, NoiseKind::Flick] {
        let noise = NoiseConfig {
            kind,
            rate: 0.0,
            ..NoiseConfig::default()
        };
        let r = run(&cases(), noise, 1);
        assert_eq!(r.cases, 5);
        assert_eq!(r.skipped, 1, "ひらがな以外を含む読みだけが対象外");
        assert_eq!(r.clean_correct, 3, "{:?}", r.examples);
        // rate 0 ではノイズが入らないので、ノイズ入りの集計は空
        assert_eq!(r.noised, 0);
        assert_eq!(r.degraded, 0, "{:?}", r.examples);
        assert_eq!(r.degradation_checked, 2 * 4);
        // 代替なしのノイズ無し入力で追加仮説は走らない（読みが辞書に載る）
        assert_eq!(r.clean_hypotheses_run, 0);
    }
}

#[test]
fn typo_noise_is_recovered_by_correction_and_runs_are_reproducible() {
    let noise = NoiseConfig {
        kind: NoiseKind::Typo,
        rate: 0.5,
        max_noise: 1,
        ..NoiseConfig::default()
    };
    let (mut off_hits, mut on_hits, mut noised) = (0, 0, 0);
    for seed in 0..24 {
        let r = run(&cases(), noise, seed);
        assert_eq!(r.degraded, 0, "{:?}", r.examples);
        // 補正ありは補正なしより悪くならない（補正候補は末尾に足すだけ）
        assert!(r.on.top1 >= r.off.top1);
        assert!(r.on.top_n >= r.off.top_n);
        assert!(r.on.any >= r.off.any);
        assert_eq!(r.off.millis.len(), r.noised);
        off_hits += r.off.top_n;
        on_hits += r.on.top_n;
        noised += r.noised;
        if seed < 3 {
            let again = run(&cases(), noise, seed);
            assert_eq!(
                (r.noised, r.off.top_n, r.on.top_n, r.hypotheses_run),
                (
                    again.noised,
                    again.off.top_n,
                    again.on.top_n,
                    again.hypotheses_run
                ),
                "同じ seed なら同じ結果"
            );
        }
    }
    assert!(noised > 0);
    assert!(
        on_hits > off_hits,
        "off={off_hits} on={on_hits} noised={noised}"
    );
}

#[test]
fn touch_noise_is_recovered_through_alternatives() {
    let noise = NoiseConfig {
        kind: NoiseKind::Touch,
        rate: 0.5,
        max_noise: 1,
        ..NoiseConfig::default()
    };
    let (mut off_hits, mut on_hits, mut ran) = (0, 0, 0);
    for seed in 0..24 {
        let r = run(&cases(), noise, seed);
        assert_eq!(r.degraded, 0, "{:?}", r.examples);
        assert!(r.on.top_n >= r.off.top_n);
        off_hits += r.off.top_n;
        on_hits += r.on.top_n;
        ran += r.hypotheses_run;
    }
    assert!(ran > 0);
    assert!(on_hits > off_hits, "off={off_hits} on={on_hits}");
}

/// フリックの評価データ: 「小゛゜」を使う読み（かんじ）・記号（、）・漢字・辞書にない語を含む
fn flick_cases() -> Vec<EvalCase> {
    [
        ("にほん", "日本"),
        ("かんじ", "漢字"),
        ("かんし", "監視"),
        ("にほんご", "日本語"),
        ("ふぉ", "フォ"),
        ("かんじ、", "漢字、"),
        ("漢字", "漢字"),
        ("abc", "abc"),
    ]
    .into_iter()
    .map(|(r, e)| EvalCase {
        reading: r.to_string(),
        expected: e.to_string(),
    })
    .collect()
}

#[test]
fn flick_evaluation_excludes_unsupported_readings_and_never_degrades_clean_input() {
    let noise = NoiseConfig {
        kind: NoiseKind::Flick,
        rate: 0.0,
        ..NoiseConfig::default()
    };
    let r = run(&flick_cases(), noise, 1);
    assert_eq!(r.cases, 8);
    // 対象外: 、 を含む読み・漢字・ASCII（最初の対応できない文字で数える）
    assert_eq!(r.skipped, 3);
    assert_eq!(r.unsupported.get(&'、'), Some(&1));
    assert_eq!(r.unsupported.get(&'漢'), Some(&1));
    assert_eq!(r.unsupported.get(&'a'), Some(&1));
    // 「小゛゜」を使う読み: かんじ・にほんご・ふぉ
    assert_eq!(r.with_presses, 3);
    assert_eq!(r.degraded, 0, "{:?}", r.examples);
    assert_eq!(r.degradation_checked, 2 * 5);
    assert_eq!(r.noised, 0);
    // 代替なしの入力で追加仮説は走らない
    assert_eq!(r.clean_hypotheses_run, 0);
}

#[test]
fn flick_noise_is_recovered_through_alternatives_and_runs_are_reproducible() {
    let noise = NoiseConfig {
        kind: NoiseKind::Flick,
        rate: 0.5,
        max_noise: 1,
        tap_sigma: 0.15,
        ..NoiseConfig::default()
    };
    let (mut off_hits, mut on_hits, mut noised) = (0, 0, 0);
    for seed in 0..40 {
        let r = run(&flick_cases(), noise, seed);
        assert_eq!(r.degraded, 0, "{:?}", r.examples);
        assert!(r.on.top_n >= r.off.top_n);
        assert!(r.on.any >= r.off.any);
        off_hits += r.off.top_n;
        on_hits += r.on.top_n;
        noised += r.noised;
        if seed < 3 {
            let again = run(&flick_cases(), noise, seed);
            assert_eq!(
                (r.noised, r.off.top_n, r.on.top_n, r.hypotheses_run),
                (
                    again.noised,
                    again.off.top_n,
                    again.on.top_n,
                    again.hypotheses_run
                ),
                "同じ seed なら同じ結果"
            );
        }
    }
    assert!(noised > 0);
    assert!(
        on_hits > off_hits,
        "off={off_hits} on={on_hits} noised={noised}"
    );
}
