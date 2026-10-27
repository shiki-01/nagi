//! `Pipeline::segment_candidates` に Rewriter（全角/半角変換）の候補が
//! `CandidateSource::Rewriter` として出所付きで含まれることの統合テスト。

use nagi_engine::ConversionBackend;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::view::CandidateSource;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::time::Duration;

mod common;
use common::{FakeModel, dict};

fn backend(model: FakeModel) -> Pipeline {
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        true,
        Tunables {
            sync_budget: Duration::from_millis(1000),
            ..Tunables::default()
        },
        Learner::new(LearnConfig::default()),
        None,
    );
    Pipeline::new(Some(dict()), Some(Box::new(model)), runtime)
}

#[test]
fn halfwidth_ascii_reading_gets_fullwidth_rewriter_candidate() {
    let b = backend(FakeModel::new(&[]));
    let c = b.segment_candidates("abc123", "", 10);
    let hit = c
        .iter()
        .find(|cand| cand.surface == "ａｂｃ１２３")
        .expect("全角変換候補が含まれるはず");
    assert!(
        hit.sources.contains(&CandidateSource::Rewriter),
        "出所は Rewriter であるはず: {:?}",
        hit.sources
    );
}

fn pipeline_with(config: nagi_config::Config, user_tsv: &str) -> Pipeline {
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        true,
        Tunables::from_config(&config),
        Learner::new(LearnConfig::default()),
        None,
    );
    let d = dict();
    let mut user = nagi_dict::UserDictionary::default();
    user.add_tsv("t.tsv", user_tsv, &mut Vec::new());
    runtime.set_overlay(d.make_overlay(&user));
    Pipeline::new(Some(d), Some(Box::new(FakeModel::new(&[]))), runtime)
}

fn surfaces(c: &[nagi_engine::BackendCandidate]) -> Vec<&str> {
    c.iter().map(|x| x.surface.as_str()).collect()
}

#[test]
fn emoji_candidates_follow_dictionary_candidates_with_rewriter_source() {
    let b = pipeline_with(nagi_config::Config::default(), "");
    // 表に無い読みには付かない
    let c = b.segment_candidates("こうえん", "", 20);
    assert!(!surfaces(&c).contains(&"😄"));
    // 辞書に無い読み: 読みそのまま（kana）が先頭の辞書側、絵文字は Rewriter の出所で続く
    let c = b.segment_candidates("わらう", "", 20);
    let s = surfaces(&c);
    let at = s.iter().position(|x| *x == "😄").expect("絵文字が含まれる");
    assert_eq!(&s[at..at + 3], ["😄", "😆", "🤣"]);
    assert!(
        s[..at].iter().all(|x| !x.contains('😄')),
        "絵文字は他の候補の後ろ"
    );
    assert!(
        c.iter()
            .filter(|x| ["😄", "😆", "🤣"].contains(&x.surface.as_str()))
            .all(|x| x.sources == [CandidateSource::Rewriter])
    );
    // 絵文字 → 顔文字の順
    let c = b.segment_candidates("えがお", "", 20);
    let s = surfaces(&c);
    let first_emoji = s.iter().position(|x| *x == "😊").unwrap();
    let first_kao = s.iter().position(|x| *x == "(^_^)").unwrap();
    assert!(first_emoji < first_kao);
}

#[test]
fn emoji_and_kaomoji_can_be_disabled() {
    let mut config = nagi_config::Config::default();
    config.rewriter.emoji = false;
    let b = pipeline_with(config.clone(), "");
    let s = surfaces(&b.segment_candidates("えがお", "", 20)).join(" ");
    assert!(!s.contains('😊') && s.contains("(^_^)"));
    config.rewriter.kaomoji = false;
    let b = pipeline_with(config, "");
    let s = surfaces(&b.segment_candidates("えがお", "", 20)).join(" ");
    assert!(!s.contains('😊') && !s.contains("(^_^)"));
}

#[test]
fn calculator_candidate_is_on_by_default_and_can_be_disabled() {
    let b = pipeline_with(nagi_config::Config::default(), "");
    let c = b.segment_candidates("1+2", "", 20);
    let hit = c.iter().find(|x| x.surface == "3").expect("電卓候補");
    assert!(hit.sources.contains(&CandidateSource::Rewriter));
    let mut config = nagi_config::Config::default();
    config.convert.calculator = false;
    let b = pipeline_with(config, "");
    let c = b.segment_candidates("1+2", "", 20);
    assert!(!surfaces(&c).contains(&"3"), "無効にすると電卓候補は出ない");
    assert!(!surfaces(&b.literal_candidates("1+2", 20)).contains(&"3"));
}

#[test]
fn date_candidate_is_on_by_default_and_can_be_disabled() {
    let b = pipeline_with(nagi_config::Config::default(), "");
    let c = b.segment_candidates("20260927", "", 20);
    let hit = c
        .iter()
        .find(|x| x.surface == "2026/09/27")
        .expect("日付候補");
    assert!(hit.sources.contains(&CandidateSource::Rewriter));
    let mut config = nagi_config::Config::default();
    config.convert.date_time = false;
    let b = pipeline_with(config, "");
    let c = b.segment_candidates("20260927", "", 20);
    assert!(
        !surfaces(&c).contains(&"2026/09/27"),
        "無効にすると日付候補は出ない"
    );
    assert!(!surfaces(&c).contains(&"2026年9月27日"));
    assert!(!surfaces(&b.literal_candidates("20260927", 20)).contains(&"2026/09/27"));
}

#[test]
fn literal_candidates_are_rewriter_only_with_width_forms_last() {
    let b = pipeline_with(nagi_config::Config::default(), "");
    let c = b.literal_candidates("1+2", 20);
    assert_eq!(surfaces(&c), ["3", "１＋２"]);
    assert!(c.iter().all(|x| x.sources == [CandidateSource::Rewriter]));
    // 複数項の式・末尾の `=`・全角
    assert_eq!(surfaces(&b.literal_candidates("(1+2)*3=", 20))[0], "9");
    assert_eq!(surfaces(&b.literal_candidates("１＋２", 20))[0], "3");
    // 記号: 装飾 → 全角化の重複は除く
    let paren = b.literal_candidates("(", 20);
    let s = surfaces(&paren);
    assert_eq!(&s[..3], ["（", "「", "『"]);
    let mut dedup = s.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), s.len());
    // 辞書・LLM の候補（漢字）は混ざらない
    let c = b.literal_candidates("abc", 20);
    assert_eq!(surfaces(&c), ["ａｂｃ"]);
}

#[test]
fn symbol_reading_candidates_come_from_the_rewriter_position() {
    let b = pipeline_with(nagi_config::Config::default(), "");
    let c = b.segment_candidates("かっこ", "", 50);
    let hit = c.iter().find(|x| x.surface == "「」").expect("記号の候補");
    assert!(hit.sources.contains(&CandidateSource::Rewriter));
    let c = b.segment_candidates("まる", "", 50);
    assert!(surfaces(&c).contains(&"○"));
}

#[test]
fn user_dictionary_wins_over_emoji_for_the_same_reading() {
    let b = pipeline_with(nagi_config::Config::default(), "わらう\t笑顔の人\t名詞\t\n");
    let c = b.segment_candidates("わらう", "", 20);
    let s = surfaces(&c);
    let user = s.iter().position(|x| *x == "笑顔の人").unwrap();
    let emoji = s.iter().position(|x| *x == "😄").unwrap();
    assert!(user < emoji, "{s:?}");
    // ユーザー辞書が絵文字そのものを登録していれば、その語が先に出て、出所は両方付く
    let b = pipeline_with(nagi_config::Config::default(), "わらう\t🤣\t名詞\t\n");
    let c = b.segment_candidates("わらう", "", 20);
    let s = surfaces(&c);
    assert!(
        s.iter().position(|x| *x == "🤣").unwrap() < s.iter().position(|x| *x == "😄").unwrap()
    );
    let hit = c.iter().find(|x| x.surface == "🤣").unwrap();
    assert!(hit.sources.contains(&CandidateSource::UserDict));
    assert!(hit.sources.contains(&CandidateSource::Rewriter));
}

/// 入力中の候補（composing candidates）にも、読みが表に一致するときだけ出る
#[test]
fn composing_candidates_include_emoji_only_for_matching_reading() {
    use nagi_engine::EngineConfig;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn composing_items(reading: &str) -> Vec<String> {
        let backend = pipeline_with(nagi_config::Config::default(), "");
        let mut s =
            nagi_server::Server::new(Arc::new(backend), "balanced", EngineConfig::default());
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let tx = std::sync::Mutex::new(tx);
        s.set_wake_callback(Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        }));
        let call = |s: &mut nagi_server::Server, req: Value| s.handle_line(&req.to_string());
        call(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocol_version":1,"client_name":"t","client_version":"0","os":"ios"}}),
        );
        call(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"insert_reading","params":{
                "session_id":"s","input":{"type":"text","content":reading}}}),
        );
        rx.recv_timeout(Duration::from_secs(5))
            .expect("通知で起こされる");
        let lines: Vec<Value> = s
            .poll_notifications()
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let update = lines
            .iter()
            .find(|l| l["method"] == "update_candidates")
            .expect("update_candidates が届く");
        update["params"]["candidates"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["surface"].as_str().unwrap().to_string())
            .collect()
    }

    let hit = composing_items("わらう");
    assert!(hit.iter().any(|s| s == "😄"), "{hit:?}");
    let miss = composing_items("こうえん");
    assert!(miss.iter().all(|s| s != "😄"), "{miss:?}");
}
