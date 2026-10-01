//! `set_environment` / `get_status` の JSON 形の固定

use nagi_protocol::methods::method;
use nagi_protocol::{
    ClientCapabilities, InitializeParams, InputModality, MemoryPressureState, SetEnvironmentParams,
    StatusResult, ThermalState,
};
use serde_json::json;

#[test]
fn set_environment_omits_unspecified_fields_and_uses_snake_case() {
    let p = SetEnvironmentParams {
        on_battery: Some(true),
        thermal: Some(ThermalState::Serious),
        memory_pressure: Some(MemoryPressureState::Warning),
        ..Default::default()
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(
        v,
        json!({"on_battery": true, "thermal": "serious", "memory_pressure": "warning"})
    );
    let back: SetEnvironmentParams = serde_json::from_value(v).unwrap();
    assert_eq!(back, p);
    // 何も指定しなくても読める
    let empty: SetEnvironmentParams = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, SetEnvironmentParams::default());
}

#[test]
fn status_result_round_trips() {
    let s = StatusResult {
        profile: "balanced".into(),
        profile_reason: "キャリブレーション推論 30ms から balanced を選びました".into(),
        backend: "dict+llm".into(),
        llm: true,
        learning: true,
        learned_entries: 12,
        user_dict_entries: 3,
        config_path: None,
        config_error: false,
    };
    let v = serde_json::to_value(&s).unwrap();
    assert!(v.get("config_path").is_none());
    assert_eq!(serde_json::from_value::<StatusResult>(v).unwrap(), s);
}

#[test]
fn method_names() {
    assert_eq!(method::SET_ENVIRONMENT, "set_environment");
    assert_eq!(method::GET_STATUS, "get_status");
    assert_eq!(method::CLEAR_LEARNING, "clear_learning");
}

#[test]
fn initialize_capabilities_are_optional_and_round_trip() {
    // 省略した従来形式のまま読め、書き出しにも現れない
    let old = json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"macos"});
    let p: InitializeParams = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(p.client_capabilities, None);
    assert_eq!(serde_json::to_value(&p).unwrap(), old);

    let caps = ClientCapabilities {
        memory_limit_mb: Some(1024),
        environment: Some(SetEnvironmentParams {
            low_power_mode: Some(true),
            thermal: Some(ThermalState::Fair),
            ..Default::default()
        }),
        input_modality: Some(InputModality::Touch),
    };
    let v = serde_json::to_value(&caps).unwrap();
    assert_eq!(
        v,
        json!({"memory_limit_mb":1024,
               "environment":{"low_power_mode":true,"thermal":"fair"},
               "input_modality":"touch"})
    );
    assert_eq!(
        serde_json::from_value::<ClientCapabilities>(v).unwrap(),
        caps
    );
    let empty: ClientCapabilities = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, ClientCapabilities::default());
}
