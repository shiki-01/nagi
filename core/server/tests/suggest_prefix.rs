//! 入力中の候補（サジェスト）の接頭辞の候補（H-58）の、実際の `Pipeline` + JSON-RPC 経由の確認:
//! `initialize` の `capabilities.partial_suggestions` を宣言したときだけ出ること、辞書のみで引くこと、
//! 所要時間の概算。カタカナ語の原語（小さな自作索引）が候補バーに出ること

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_engine::EngineConfig;
use nagi_learn::{LearnConfig, Learner};
use nagi_loanword::{Entry, IndexBuilder, LoanwordIndex};
use nagi_profile::SystemInfo;
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const EXTRA: &str = "\
きょう\t1\t1\t3000\t今日
きょう\t1\t1\t3500\t京
きょ\t1\t1\t3000\t巨
きょうは\t1\t1\t3000\t今日は
いい\t1\t1\t3000\t良い
てん\t1\t1\t3000\t天
てんき\t1\t1\t3000\t天気
てんき\t1\t1\t3500\t転機
き\t1\t1\t3000\t木
き\t1\t1\t3200\t気
こんぴゅーたー\t1\t1\t3000\tコンピューター
";

/// 自作の小さな原語の索引（computer のみ）
fn fixture_loanword() -> Arc<LoanwordIndex> {
    let mut b = IndexBuilder::new();
    b.add(&Entry {
        headword: "コンピューター".into(),
        lang: "eng".into(),
        spelling: "computer".into(),
        wasei: false,
        rank: 0,
    });
    Arc::new(LoanwordIndex::from_vec(b.build().unwrap()).unwrap())
}

fn server(partial: Option<bool>, loanword: bool) -> Server {
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
    if loanword {
        pipeline.set_loanword_index(Some(fixture_loanword()));
    }
    let mut s = Server::new(Arc::new(pipeline), "balanced", EngineConfig::default());
    let mut params = json!({
        "protocol_version":1,"client_name":"t","client_version":"0","os":"ios"});
    if let Some(p) = partial {
        params["capabilities"] = json!({"input_method":"touch","partial_suggestions":p});
    }
    s.handle_line(
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":params}).to_string(),
    );
    s
}

/// かなを 1 文字ずつ送り（キーボードの `insertReading` と同じ）、最後に届いた候補バーを返す。
/// 同期の応答には候補の再計算が載らないこと（`insert_reading` の応答の `candidates` が空）も確認する
fn typed(s: &mut Server, text: &str) -> Vec<Value> {
    let mut last_sync: Option<Value> = None;
    for (i, ch) in text.chars().enumerate() {
        let lines = s.handle_line(
            &json!({"jsonrpc":"2.0","id":10 + i as i64,"method":"insert_reading","params":{
                "session_id":"s","input":{"type":"text","content":ch.to_string()}}})
            .to_string(),
        );
        let r: Value = serde_json::from_str(&lines[0]).unwrap();
        last_sync = Some(r["result"]["candidates"].clone());
    }
    // 同期の応答には、拡充した候補（接頭辞）を載せない
    if let Some(c) = last_sync.filter(|c| !c.is_null()) {
        assert!(
            c["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i.get("covers").is_none()),
            "{c}"
        );
    }
    std::thread::sleep(Duration::from_millis(300));
    s.poll_notifications()
        .iter()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|l| l["method"] == "update_candidates")
        .next_back()
        .and_then(|l| l["params"]["candidates"]["items"].as_array().cloned())
        .unwrap_or_default()
}

fn surfaces(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .map(|i| i["surface"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn declared_client_gets_whole_then_prefixes_then_single_characters() {
    let mut s = server(Some(true), false);
    let items = typed(&mut s, "きょうはいいてんき");
    let covers: Vec<u64> = items
        .iter()
        .map(|i| i["covers"].as_u64().unwrap_or(9))
        .collect();
    assert!(covers.windows(2).all(|w| w[0] >= w[1]), "{covers:?}");
    assert_eq!(covers.last(), Some(&1), "{:?}", surfaces(&items));
    let all = surfaces(&items);
    for expected in ["今日は", "今日", "京", "巨", "木", "気"] {
        assert!(all.iter().any(|x| x == expected), "{expected}: {all:?}");
    }
    // 1 文字の読み（き）の漢字は最後
    assert_eq!(all.last().map(String::as_str), Some("気"), "{all:?}");
}

#[test]
fn undeclared_or_false_clients_get_no_prefix_only_candidates() {
    for partial in [None, Some(false)] {
        let mut s = server(partial, false);
        let items = typed(&mut s, "きょうはいいてんき");
        assert!(!items.is_empty());
        assert!(
            items.iter().all(|i| i.get("covers").is_none()),
            "{partial:?}: {:?}",
            surfaces(&items)
        );
    }
}

#[test]
fn selecting_a_prefix_candidate_over_rpc_keeps_the_rest_composing() {
    let mut s = server(Some(true), false);
    let items = typed(&mut s, "きょうはいいてんき");
    let index = items
        .iter()
        .position(|i| i["covers"] == 3 && i["surface"] == "今日")
        .unwrap_or_else(|| panic!("{:?}", surfaces(&items)));
    let lines = s.handle_line(
        &json!({"jsonrpc":"2.0","id":100,"method":"select_candidate","params":{
            "session_id":"s","index":index}})
        .to_string(),
    );
    let r: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(r["result"]["commit"], "今日", "{r}");
    assert_eq!(r["result"]["preedit"]["text"], "はいいてんき", "{r}");
}

#[test]
fn loanword_spelling_appears_next_to_the_katakana_candidate() {
    for partial in [None, Some(true)] {
        let mut s = server(partial, true);
        let items = typed(&mut s, "こんぴゅーたー");
        let all = surfaces(&items);
        let kata = all.iter().position(|x| x == "コンピューター").unwrap();
        assert_eq!(
            all.get(kata + 1).map(String::as_str),
            Some("computer"),
            "{all:?}"
        );
    }
}

#[test]
fn without_loanword_data_only_the_katakana_appears() {
    let mut s = server(Some(true), false);
    let all = surfaces(&typed(&mut s, "こんぴゅーたー"));
    assert!(all.iter().any(|x| x == "コンピューター"), "{all:?}");
    assert!(all.iter().all(|x| x != "computer"), "{all:?}");
}
