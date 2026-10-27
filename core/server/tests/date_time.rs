//! 日付・時刻の候補（`きょう` → `2026/10/27` など。H-72）: パイプラインの経路・設定・
//! `set_environment` のずれ・学習の除外。時計は実時間なので、日付の値そのものではなく形と差を検査する
//! （値の検査は `nagi-rewriter` の固定時刻のテスト）。

use nagi_engine::{CommitContext, CommitObserver, CommittedSegment, ConversionBackend};
use nagi_learn::{LearnConfig, Learner, context_hash};
use nagi_profile::SystemInfo;
use nagi_protocol::methods::SetEnvironmentParams;
use nagi_protocol::view::{CandidateSource, SegmentKind};
use nagi_server::learning::LearningObserver;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::sync::Arc;

mod common;
use common::{FakeModel, dict};

fn setup(config: &nagi_config::Config) -> (Pipeline, Arc<Runtime>) {
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        true,
        Tunables::from_config(config),
        Learner::new(LearnConfig::default()),
        None,
    );
    let pipeline = Pipeline::new(
        Some(dict()),
        Some(Box::new(FakeModel::new(&[]))),
        runtime.clone(),
    );
    (pipeline, runtime)
}

fn surfaces(b: &Pipeline, reading: &str) -> Vec<String> {
    b.segment_candidates(reading, "", 50)
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

fn env_offset(minutes: i32) -> SetEnvironmentParams {
    SetEnvironmentParams {
        utc_offset_minutes: Some(minutes),
        ..SetEnvironmentParams::default()
    }
}

/// `いま` の先頭候補 `HH:MM` を分に直す
fn minutes_of_day(b: &Pipeline) -> i32 {
    let s = surfaces(b, "いま")
        .into_iter()
        .find(|s| s.len() == 5 && s.as_bytes()[2] == b':')
        .expect("HH:MM の候補");
    s[..2].parse::<i32>().unwrap() * 60 + s[3..].parse::<i32>().unwrap()
}

#[test]
fn date_and_time_candidates_come_after_dictionary_with_rewriter_source() {
    let (b, _) = setup(&nagi_config::Config::default());
    let c = b.segment_candidates("きょう", "", 50);
    let s: Vec<&str> = c.iter().map(|x| x.surface.as_str()).collect();
    let slash = s
        .iter()
        .position(|x| x.len() == 10 && x.as_bytes()[4] == b'/' && x.as_bytes()[7] == b'/')
        .expect("yyyy/mm/dd");
    // 辞書など Rewriter 以外の候補の後ろ
    let first_rewriter = c
        .iter()
        .position(|x| x.sources.contains(&CandidateSource::Rewriter))
        .unwrap();
    assert!(
        c[..first_rewriter].iter().all(|x| x.surface != s[slash]),
        "{s:?}"
    );
    assert!(
        c[first_rewriter..]
            .iter()
            .all(|x| x.sources.contains(&CandidateSource::Rewriter)),
        "{s:?}"
    );
    assert!(s.iter().any(|x| x.ends_with(')') && x.contains("月")));
    let hit = &c[slash];
    assert_eq!(hit.sources, [CandidateSource::Rewriter]);
    // 時刻・年・曜日
    assert!(
        surfaces(&b, "いま")
            .iter()
            .any(|x| x.contains("時") && x.contains("分"))
    );
    assert!(surfaces(&b, "ことし")[..].iter().any(|x| x.ends_with('年')));
    assert!(surfaces(&b, "ようび").iter().any(|x| x.ends_with("曜日")));
}

#[test]
fn disabled_with_date_time_false() {
    let mut config = nagi_config::Config::default();
    config.convert.date_time = false;
    let (b, _) = setup(&config);
    for reading in ["きょう", "いま", "ことし", "ようび"] {
        assert!(
            surfaces(&b, reading)
                .iter()
                .all(|s| !s.bytes().any(|c| c.is_ascii_digit()) && !s.ends_with("曜日")),
            "{reading}"
        );
    }
}

#[test]
fn utc_offset_priority_setting_then_environment_then_default() {
    let (b, rt) = setup(&nagi_config::Config::default());
    assert_eq!(rt.utc_offset_minutes(), 540, "未受信は +09:00");
    rt.set_environment(&env_offset(-300));
    assert_eq!(rt.utc_offset_minutes(), -300);
    // 実行中に変わる: 候補にも反映される（1 時間進む）
    let before = minutes_of_day(&b);
    rt.set_environment(&env_offset(-240));
    let diff = (minutes_of_day(&b) - before).rem_euclid(1440);
    assert!((60..=61).contains(&diff), "{diff}");
    // 範囲外の報告は無視して既定
    rt.set_environment(&env_offset(100_000));
    assert_eq!(rt.utc_offset_minutes(), 540);

    // 設定の固定値が優先（UTC / 固定オフセット）。ホットリロードで切り替わる
    let mut config = nagi_config::Config::default();
    config.convert.time_zone = "UTC".into();
    rt.apply_config(&config);
    rt.set_environment(&env_offset(-300));
    assert_eq!(rt.utc_offset_minutes(), 0);
    config.convert.time_zone = "-05:30".into();
    rt.apply_config(&config);
    assert_eq!(rt.utc_offset_minutes(), -330);
    config.convert.time_zone = "auto".into();
    rt.apply_config(&config);
    assert_eq!(rt.utc_offset_minutes(), -300, "auto は報告値");
}

#[test]
fn date_time_selection_is_not_learned_but_dictionary_candidates_are() {
    let mut config = nagi_config::Config::default();
    config.predict.next_word = true;
    let (b, rt) = setup(&config);
    let observer = LearningObserver::new(rt.clone());
    let ctx = CommitContext {
        session_id: "s",
        app_id: None,
        secure_input: false,
    };
    let generated: Vec<String> = surfaces(&b, "きょう")
        .into_iter()
        .filter(|s| s.bytes().any(|c| c.is_ascii_digit()))
        .collect();
    assert!(!generated.is_empty());
    let seg = |reading: &str, surface: &str| CommittedSegment {
        reading: reading.into(),
        surface: surface.into(),
        kind: SegmentKind::Japanese,
        left_context: String::new(),
        prev_word: "前".into(),
    };
    let mut segments: Vec<CommittedSegment> = generated.iter().map(|s| seg("きょう", s)).collect();
    segments.push(seg("いま", "午後3時30分"));
    segments.push(seg("ようび", "火曜日"));
    observer.on_commit(&ctx, &segments);
    let hash = context_hash("");
    assert!(rt.learn_lookup("きょう", hash).is_empty());
    assert!(rt.learn_lookup("いま", hash).is_empty());
    assert!(rt.learn_lookup("ようび", hash).is_empty());
    assert_eq!(rt.learned_entries(), 0);
    assert!(
        rt.next_word_lookup("前", 10).is_empty(),
        "次語予測にも記録しない"
    );

    // 辞書の候補（今日）は従来どおり学習される
    observer.on_commit(&ctx, &[seg("きょう", "今日")]);
    assert_eq!(rt.learn_lookup("きょう", hash)[0].0, "今日");
    assert_eq!(rt.next_word_lookup("前", 10)[0].0, "今日");
}
