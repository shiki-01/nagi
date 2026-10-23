//! 英語の自動判定（段階 1）の入力中の候補バー: iOS と同じ呼び出し（`insert_reading` で 1 文字ずつ）で
//! 打鍵列の ASCII（`computer`）が候補に出ること、日本語の入力では出ないこと

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_engine::EngineConfig;
use nagi_english::{Lexicon, LexiconBuilder};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const EXTRA: &str = "\
きょう\t1\t1\t3000\t今日
ばなな\t1\t1\t3000\tバナナ
";

/// `core/english/tests/fixtures/words.txt`（自作の小さな語彙。computer を含む）
fn fixture_lexicon() -> Arc<Lexicon> {
    let text = include_str!("../../english/tests/fixtures/words.txt");
    let mut b = LexiconBuilder::new();
    for l in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let (w, t) = l.split_once('\t').unwrap();
        b.add(w, t.parse().unwrap());
    }
    Arc::new(Lexicon::from_vec(b.build().unwrap()).unwrap())
}

fn server(lexicon: Option<Arc<Lexicon>>) -> Server {
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
    let mut pipeline = Pipeline::new(
        Some(dict_with_extra(EXTRA)),
        Some(Box::new(FakeModel::new(&[]))),
        runtime,
    );
    pipeline.set_english_lexicon(lexicon);
    let mut s = Server::new(Arc::new(pipeline), "balanced", EngineConfig::default());
    s.handle_line(
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"ios"}})
        .to_string(),
    );
    s
}

/// 1 文字ずつ送り（キーボードの `insertReading` と同じ）、最後に届いた候補バーの表記を返す
fn typed_candidates(s: &mut Server, keys: &str, first_id: i64) -> Vec<String> {
    for (i, ch) in keys.chars().enumerate() {
        s.handle_line(
            &json!({"jsonrpc":"2.0","id":first_id + i as i64,"method":"insert_reading","params":{
                "session_id":"s","input":{"type":"text","content":ch.to_string()}}})
            .to_string(),
        );
    }
    std::thread::sleep(Duration::from_millis(250));
    let updates: Vec<Value> = s
        .poll_notifications()
        .iter()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|l| l["method"] == "update_candidates")
        .map(|l| l["params"]["candidates"].clone())
        .collect();
    updates
        .last()
        .and_then(|c| c["items"].as_array())
        .map(|items| {
            items
                .iter()
                .map(|i| i["surface"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn typed_english_word_appears_in_the_composing_bar() {
    let mut s = server(Some(fixture_lexicon()));
    let items = typed_candidates(&mut s, "computer", 2);
    // 先頭は打った通り（動かさない）。ASCII は 2 番目
    assert_eq!(
        items.first().map(String::as_str),
        Some("こmぷてr"),
        "{items:?}"
    );
    assert_eq!(
        items.get(1).map(String::as_str),
        Some("computer"),
        "{items:?}"
    );
}

#[test]
fn selecting_the_english_candidate_commits_ascii() {
    let mut s = server(Some(fixture_lexicon()));
    let items = typed_candidates(&mut s, "computer", 2);
    let index = items.iter().position(|i| i == "computer").unwrap();
    let r = s.handle_line(
        &json!({"jsonrpc":"2.0","id":100,"method":"select_candidate","params":{
            "session_id":"s","index":index}})
        .to_string(),
    );
    let r: Value = serde_json::from_str(&r[0]).unwrap();
    assert_eq!(r["result"]["commit"], "computer", "{r}");
}

#[test]
fn japanese_typing_never_shows_ascii_candidates() {
    for keys in [
        "kyouhagoogle",
        "kyouhagooglenisearchsuru",
        "kyouhaiitenkida",
        "arigatougozaimasu",
        "banana",
    ] {
        // 打鍵のたびの状態（接頭辞）すべてで見る
        for end in 3..=keys.len() {
            let mut s = server(Some(fixture_lexicon()));
            let items = typed_candidates(&mut s, &keys[..end], 2);
            assert!(
                items
                    .iter()
                    .all(|i| !i.bytes().all(|b| b.is_ascii_lowercase())),
                "{} -> {items:?}",
                &keys[..end]
            );
        }
    }
}

#[test]
fn without_a_lexicon_computer_is_not_judged_english() {
    // 設計上の限界: 語彙が無いと computer は特徴量だけで Low になり、候補バーに出ない
    // （語彙は本体アプリの「追加データ」で入れる。H-56）
    let mut s = server(None);
    let items = typed_candidates(&mut s, "computer", 2);
    assert!(!items.iter().any(|i| i == "computer"), "{items:?}");
}
