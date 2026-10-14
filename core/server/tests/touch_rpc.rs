//! タッチ入力の JSON-RPC（`insert_reading` / `perform_action`）。フロント（Swift）が送る JSON の形で呼ぶ

use nagi_engine::{EngineConfig, KanaBackend};
use nagi_server::Server;
use serde_json::{Value, json};
use std::sync::Arc;

fn server() -> Server {
    let mut s = Server::new(Arc::new(KanaBackend), "balanced", EngineConfig::default());
    call(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"ios"}}),
    );
    s
}

fn call(server: &mut Server, request: Value) -> Value {
    let lines = server.handle_line(&request.to_string());
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn insert(server: &mut Server, id: i64, input: Value) -> Value {
    call(
        server,
        json!({"jsonrpc":"2.0","id":id,"method":"insert_reading","params":{
            "session_id":"s","input":input}}),
    )
}

fn action(server: &mut Server, id: i64, action: Value) -> Value {
    call(
        server,
        json!({"jsonrpc":"2.0","id":id,"method":"perform_action","params":{
            "session_id":"s","action":action}}),
    )
}

#[test]
fn insert_reading_accepts_text_key12_and_variant() {
    let mut s = server();
    let r = insert(&mut s, 2, json!({"type":"text","content":"きょ"}));
    assert_eq!(r["result"]["consumed"], true);
    assert_eq!(r["result"]["preedit"]["text"], "きょ");

    let r = insert(
        &mut s,
        3,
        json!({"type":"key12","content":{"key":"ka","gesture":"toggle"}}),
    );
    assert_eq!(r["result"]["preedit"]["text"], "きょか");
    let r = insert(
        &mut s,
        4,
        json!({"type":"key12","content":{"key":"ka","gesture":"toggle"}}),
    );
    assert_eq!(r["result"]["preedit"]["text"], "きょき");

    let r = insert(&mut s, 5, json!({"type":"variant"}));
    assert_eq!(r["result"]["preedit"]["text"], "きょぎ");
}

#[test]
fn perform_action_drives_conversion_and_commit() {
    let mut s = server();
    insert(&mut s, 2, json!({"type":"text","content":"かな"}));
    let r = action(&mut s, 3, json!({"type":"space"}));
    assert_eq!(r["result"]["consumed"], true);
    assert!(r["result"]["candidates"].is_object() || r["result"]["preedit"].is_object());
    let r = action(&mut s, 4, json!({"type":"enter"}));
    assert_eq!(r["result"]["consumed"], true);
    assert!(r["result"]["commit"].is_string());
    // 未入力の Backspace / Enter は消費されない（フロントが既定の処理をする）
    let r = action(&mut s, 5, json!({"type":"backspace"}));
    assert_eq!(r["result"]["consumed"], false);
    let r = action(&mut s, 6, json!({"type":"enter"}));
    assert_eq!(r["result"]["consumed"], false);
}

#[test]
fn actions_with_direction_and_mode_payloads_parse() {
    let mut s = server();
    insert(&mut s, 2, json!({"type":"text","content":"あい"}));
    let r = action(&mut s, 3, json!({"type":"move_cursor","content":"left"}));
    assert_eq!(r["result"]["consumed"], true);
    let r = action(
        &mut s,
        4,
        json!({"type":"set_mode","content":"alphanumeric"}),
    );
    assert_eq!(r["result"]["consumed"], true);
}

#[test]
fn malformed_touch_params_are_rejected() {
    let mut s = server();
    let r = insert(
        &mut s,
        2,
        json!({"type":"key12","content":{"key":"zz","gesture":"tap"}}),
    );
    assert!(r["error"].is_object(), "{r}");
    let r = action(&mut s, 3, json!({"type":"no_such_action"}));
    assert!(r["error"].is_object(), "{r}");
}

#[test]
fn touch_methods_require_initialize() {
    let mut s = Server::new(Arc::new(KanaBackend), "balanced", EngineConfig::default());
    let r = insert(&mut s, 1, json!({"type":"variant"}));
    assert!(r["error"].is_object());
    let r = action(&mut s, 2, json!({"type":"space"}));
    assert!(r["error"].is_object());
}

#[test]
fn composing_candidates_are_pushed_without_further_input() {
    use nagi_engine::MockBackend;
    use std::time::Duration;

    let backend = MockBackend::new().with_sentence(
        "きょうは",
        vec![("きょう", vec!["今日", "京"]), ("は", vec!["は"])],
    );
    let mut s = Server::new(Arc::new(backend), "balanced", EngineConfig::default());
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let tx = std::sync::Mutex::new(tx);
    s.set_wake_callback(Arc::new(move || {
        let _ = tx.lock().unwrap().send(());
    }));
    call(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"ios"}}),
    );
    insert(&mut s, 2, json!({"type":"text","content":"きょうは"}));

    // 入力のあと、リクエストを送らなくても候補が届く
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
    let items = update["params"]["candidates"]["items"].as_array().unwrap();
    assert_eq!(items[0]["surface"], "今日は");
    assert!(
        items
            .iter()
            .any(|i| i["surface"] == "京" && i["covers"] == 3),
        "{items:?}"
    );

    // 部分変換: 「京」を選ぶと「京」だけが確定し、「は」が入力中に残る
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":3,"method":"select_candidate","params":{
            "session_id":"s","index":items.iter().position(|i| i["surface"] == "京").unwrap()}}),
    );
    assert_eq!(r["result"]["commit"], "京");
    assert_eq!(r["result"]["preedit"]["text"], "は");
}
