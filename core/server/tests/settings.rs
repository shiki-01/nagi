//! 設定 RPC（`get_settings` / `update_settings`）の JSON-RPC 統合テスト（プロセス内）。

use nagi_server::{Server, ServerOptions, build};
use serde_json::{Value, json};
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nagi-settings-rpc-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `config.toml`（あれば初期内容つき）を持つサーバー（辞書・LLM なし）
fn server_with_config(tag: &str, initial: Option<&str>) -> (Server, PathBuf) {
    let dir = temp_dir(tag);
    let path = dir.join("config.toml");
    if let Some(s) = initial {
        std::fs::write(&path, s).unwrap();
    }
    let built = build(&ServerOptions {
        user_dir: Some(dir),
        use_llm: false,
        ..ServerOptions::default()
    });
    let mut server = Server::from_built(built);
    call(
        &mut server,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"test"}}),
    );
    (server, path)
}

/// 応答（先頭の行）を返す。後続の通知行は無視する
fn call(server: &mut Server, request: Value) -> Value {
    let lines = server.handle_line(&request.to_string());
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn get(server: &mut Server) -> Value {
    call(
        server,
        json!({"jsonrpc":"2.0","id":2,"method":"get_settings"}),
    )["result"]
        .clone()
}

fn update(server: &mut Server, changes: Value) -> Value {
    let r = call(
        server,
        json!({"jsonrpc":"2.0","id":3,"method":"update_settings","params":{"changes":changes}}),
    );
    assert!(r.get("error").is_none(), "{r}");
    r["result"].clone()
}

fn item<'a>(snapshot: &'a Value, key: &str) -> &'a Value {
    snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == key)
        .unwrap_or_else(|| panic!("項目がない: {key}"))
}

#[test]
fn get_settings_returns_defaults_and_not_explicit() {
    let (mut s, _) = server_with_config("defaults", None);
    let snap = get(&mut s);
    let items = snap["items"].as_array().unwrap();
    assert!(items.len() >= 10);
    assert!(items.iter().all(|i| i["explicit"] == false));
    let delay = item(&snap, "conversion.live_conversion_delay_ms");
    assert_eq!(delay["value"], json!({"type":"int","value":228}));
    assert_eq!(delay["default"], delay["value"]);
    assert_eq!(
        delay["kind"],
        json!({"type":"int","params":{"min":0,"max":5000,"step":10}})
    );
    assert_eq!(
        item(&snap, "conversion.live_conversion")["kind"],
        json!({"type":"bool"})
    );
    assert_eq!(item(&snap, "conversion.beam_width")["value"], Value::Null);
    assert_eq!(item(&snap, "update.auto_check")["requires_restart"], true);
    assert_eq!(item(&snap, "conversion.live_conversion")["level"], "simple");
    assert_eq!(item(&snap, "conversion.n_threads")["level"], "detail");
    assert_eq!(
        item(&snap, "conversion.n_threads")["requires_restart"],
        true
    );
    // タッチ入力のみの項目（濁点の循環順・タッチ座標の利用）以外は両方
    assert!(items.iter().all(|i| {
        let expected =
            if i["key"] == "input.dakuten_cycle" || i["key"] == "input.fuzzy.use_touch_coords" {
                "ios"
            } else {
                "both"
            };
        i["platforms"] == expected
    }));
    assert_eq!(item(&snap, "conversion.model")["value"], Value::Null);
    let mode = item(&snap, "profile.mode");
    assert_eq!(mode["value"], json!({"type":"text","value":"auto"}));
    assert_eq!(mode["kind"]["type"], "choice");
    assert_eq!(
        mode["kind"]["params"]["options"][0],
        json!({"value":"auto","label":"自動"})
    );
}

#[test]
fn update_writes_file_keeps_comments_and_reflects_in_snapshot() {
    let src = "# 私の設定\n[conversion]\n# 待ち時間\nsync_budget_ms = 150 # 行末\n";
    let (mut s, path) = server_with_config("update", Some(src));
    let r = update(
        &mut s,
        json!([
            {"key":"conversion.sync_budget_ms","value":{"type":"int","value":300}},
            {"key":"learning.enabled","value":{"type":"bool","value":false}},
            {"key":"profile.mode","value":{"type":"text","value":"light"}},
        ]),
    );
    assert_eq!(r["errors"], json!([]));
    assert_eq!(r["applied"].as_array().unwrap().len(), 3);
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("# 私の設定"));
    assert!(
        written.contains("# 待ち時間\nsync_budget_ms = 300 # 行末"),
        "{written}"
    );
    assert!(written.contains("[learning]\nenabled = false"));
    let snap = &r["snapshot"];
    let budget = item(snap, "conversion.sync_budget_ms");
    assert_eq!(budget["value"], json!({"type":"int","value":300}));
    assert_eq!(budget["explicit"], true);
    assert_eq!(budget["default"], json!({"type":"int","value":150}));
    assert_eq!(item(snap, "profile.mode")["value"]["value"], "light");
    // ホットリロードで実際に反映されている（状態に出る）
    let status = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":4,"method":"get_status"}),
    );
    assert_eq!(status["result"]["learning"], false);
    assert_eq!(
        s.runtime().tunables().sync_budget,
        std::time::Duration::from_millis(300)
    );
    assert_eq!(status["result"]["config_error"], false);
}

#[test]
fn invalid_items_are_errors_and_others_still_apply() {
    let (mut s, path) = server_with_config("invalid", None);
    let r = update(
        &mut s,
        json!([
            {"key":"conversion.sync_budget_ms","value":{"type":"int","value":5}},
            {"key":"conversion.composing_chunk_len","value":{"type":"text","value":"x"}},
            {"key":"profile.mode","value":{"type":"text","value":"turbo"}},
            {"key":"nope.unknown","value":{"type":"bool","value":true}},
            {"key":"predict.min_chars","value":{"type":"int","value":3}},
        ]),
    );
    assert_eq!(r["applied"], json!(["predict.min_chars"]));
    let keys: Vec<&str> = r["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "conversion.sync_budget_ms",
            "conversion.composing_chunk_len",
            "profile.mode",
            "nope.unknown"
        ]
    );
    assert!(r["errors"][0]["message"].as_str().unwrap().contains("10"));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("min_chars = 3"));
    assert!(!written.contains("sync_budget_ms"));
    assert_eq!(item(&r["snapshot"], "predict.min_chars")["explicit"], true);
}

#[test]
fn null_value_removes_explicit_setting() {
    let (mut s, path) = server_with_config(
        "null",
        Some("[conversion]\nbeam_width = 4\nnum_candidates = 12\n"),
    );
    let snap = get(&mut s);
    assert_eq!(item(&snap, "conversion.beam_width")["explicit"], true);
    assert_eq!(item(&snap, "conversion.beam_width")["value"]["value"], 4);
    let r = update(
        &mut s,
        json!([
            {"key":"conversion.beam_width","value":null},
            {"key":"conversion.num_candidates"},
        ]),
    );
    assert_eq!(r["errors"], json!([]));
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(!written.contains("beam_width"));
    assert!(!written.contains("num_candidates"));
    let b = item(&r["snapshot"], "conversion.beam_width");
    assert_eq!(b["explicit"], false);
    assert_eq!(b["value"], Value::Null);
    let n = item(&r["snapshot"], "conversion.num_candidates");
    assert_eq!(n["explicit"], false);
    assert_eq!(n["value"]["value"], 9);
}

#[test]
fn broken_config_is_not_overwritten() {
    let (mut s, path) = server_with_config("broken", Some("[conversion\n"));
    let snap = get(&mut s);
    assert!(snap["error"].is_string());
    assert!(
        snap["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["explicit"] == false)
    );
    let r = update(
        &mut s,
        json!([{"key":"learning.enabled","value":{"type":"bool","value":false}}]),
    );
    assert_eq!(r["applied"], json!([]));
    assert_eq!(r["errors"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[conversion\n");
}

#[test]
fn use_llm_setting_reaches_runtime_and_bad_params_are_rejected() {
    let (mut s, _) = server_with_config("llm", None);
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":5,"method":"update_settings","params":{"changes":"x"}}),
    );
    assert_eq!(r["error"]["code"], -32602);
    let r = update(
        &mut s,
        json!([{"key":"conversion.use_llm","value":{"type":"bool","value":false}}]),
    );
    assert_eq!(
        item(&r["snapshot"], "conversion.use_llm")["value"]["value"],
        false
    );
    assert!(!s.runtime().tunables().llm_enabled);
}

#[test]
fn settings_require_initialize() {
    let mut s = Server::new(
        std::sync::Arc::new(nagi_engine::KanaBackend),
        "minimal",
        nagi_engine::EngineConfig::default(),
    );
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"get_settings"}),
    );
    assert_eq!(r["error"]["code"], -32002);
}
