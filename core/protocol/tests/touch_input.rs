//! タッチ入力（`insert_reading` / `perform_action` / `alternatives`）の JSON 形の固定

use nagi_protocol::methods::method;
use nagi_protocol::{
    InputAlternative, InsertReadingParams, PerformActionParams, ProcessKeyParams,
    ServerCapabilities,
};
use serde_json::json;

#[test]
fn method_names() {
    assert_eq!(method::INSERT_READING, "insert_reading");
    assert_eq!(method::PERFORM_ACTION, "perform_action");
}

#[test]
fn insert_reading_alternatives_are_optional() {
    let p: InsertReadingParams =
        serde_json::from_value(json!({"session_id": "s", "reading": "か"})).unwrap();
    assert!(p.alternatives.is_empty());
    // 空なら出力でも省略する
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        json!({"session_id": "s", "reading": "か"})
    );
}

#[test]
fn insert_reading_round_trips_with_alternatives() {
    let p = InsertReadingParams {
        session_id: "s".into(),
        reading: "か".into(),
        alternatives: vec![InputAlternative {
            text: "け".into(),
            distance: 0.5,
        }],
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(
        v,
        json!({"session_id": "s", "reading": "か", "alternatives": [{"text": "け", "distance": 0.5}]})
    );
    let back: InsertReadingParams = serde_json::from_value(v).unwrap();
    assert_eq!(back, p);
}

#[test]
fn perform_action_round_trips() {
    let v = json!({"session_id": "s", "action": "MoveCursor:Left"});
    let p: PerformActionParams = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(p.action, "MoveCursor:Left");
    assert_eq!(serde_json::to_value(&p).unwrap(), v);
}

#[test]
fn process_key_params_accept_old_json_and_alternatives() {
    // alternatives の無い既存クライアントの JSON
    let old = json!({
        "session_id": "s", "keysym": 97, "keycode": 0, "text": "a",
        "modifiers": {"shift": false, "ctrl": false, "alt": false, "meta": false, "caps_lock": false},
        "kind": "down"
    });
    let p: ProcessKeyParams = serde_json::from_value(old.clone()).unwrap();
    assert!(p.alternatives.is_empty());
    assert_eq!(serde_json::to_value(&p).unwrap(), old);

    let mut with_alt = p.clone();
    with_alt.alternatives.push(InputAlternative {
        text: "s".into(),
        distance: 0.25,
    });
    let v = serde_json::to_value(&with_alt).unwrap();
    assert_eq!(v["alternatives"][0]["text"], "s");
    let back: ProcessKeyParams = serde_json::from_value(v).unwrap();
    assert_eq!(back, with_alt);
    // key_event は alternatives の影響を受けない
    assert_eq!(with_alt.key_event(), p.key_event());
}

#[test]
fn capabilities_read_json_without_touch_input() {
    let c: ServerCapabilities = serde_json::from_value(
        json!({"reconvert": true, "live_conversion": false, "style_conversion": false}),
    )
    .unwrap();
    assert!(!c.touch_input);
}
