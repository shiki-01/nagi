//! 入力中の候補（接頭辞の拡充。H-58）の再計算の所要時間の概算。実データ（`data/dict/nagi-dict.bin`、
//! あれば `data/loanword/loanword.fst`）が無ければ省略する。LLM は使わない（接頭辞の候補は辞書のみ）。
//! 数字を見るには `cargo test --release -p nagi-server --test suggest_latency -- --nocapture`

use nagi_config::Config;
use nagi_engine::{Engine, EngineConfig};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::methods::ReadingInput;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn engine(partial: bool) -> Option<Engine> {
    let dict = nagi_server::setup::load_dictionary(&data_dir()).ok()?;
    let config = Config::default();
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        false,
        Tunables::from_config(&config),
        Learner::new(LearnConfig::default()),
        None,
    );
    let mut pipeline = Pipeline::new(Some(dict), None, runtime);
    if let Some(Ok(index)) = nagi_server::setup::load_loanword_index(Some(&data_dir())) {
        pipeline.set_loanword_index(Some(Arc::new(index)));
    }
    let mut e = Engine::new(Arc::new(pipeline), EngineConfig::default());
    e.set_partial_suggestions(partial);
    Some(e)
}

#[test]
fn prefix_suggestions_are_cheap_enough_for_the_background() {
    for reading in [
        "きょうはいいてんき",
        "きょうはいいてんきなのでさんぽにいこう",
        "こんぴゅーたーをつかってしごとをするためにあたらしいぱそこんをかいました",
    ] {
        let mut timings = Vec::new();
        for partial in [false, true] {
            let Some(mut e) = engine(partial) else {
                eprintln!("省略: {} に辞書が無い", data_dir().display());
                return;
            };
            for c in reading.chars() {
                e.insert_reading("s", &ReadingInput::Text(c.to_string()));
            }
            let target = e.composing_candidate_reading("s").unwrap();
            let mut samples = Vec::new();
            let mut count = 0;
            for _ in 0..7 {
                let start = Instant::now();
                let r = e.refresh_composing_candidates("s", &target);
                samples.push(start.elapsed());
                let count_before = count;
                if let Some(c) = r.and_then(|r| r.candidates) {
                    count = c.items.len();
                    if partial && count_before == 0 {
                        let shown: Vec<String> = c
                            .items
                            .iter()
                            .map(|i| format!("{}:{}", i.covers.unwrap_or(0), i.surface))
                            .collect();
                        println!("  {}", shown.join(" "));
                    }
                }
                // 次の回も計算させる（同じ入力では再計算しないので、文脈を変えて無効にする）
                e.set_context(
                    "s",
                    &nagi_protocol::methods::SetContextParams {
                        session_id: "s".into(),
                        surrounding_text: Some(nagi_protocol::methods::SurroundingText {
                            text: format!("x{}", samples.len()),
                            cursor: 2,
                        }),
                        app_id: None,
                        secure_input: None,
                    },
                );
            }
            samples.sort();
            let median = samples[samples.len() / 2];
            println!(
                "{} 文字 partial={partial}: 中央値 {:.1} ms（候補 {count} 件）",
                reading.chars().count(),
                median.as_secs_f64() * 1000.0
            );
            timings.push(median);
        }
        // 緩い上限（debug ビルドでも十分余裕がある値）
        assert!(timings[1].as_millis() < 3000, "{timings:?}");
    }
}
