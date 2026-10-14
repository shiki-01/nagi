//! タッチ入力（`insert_reading` / `perform_action`）のテスト

use nagi_engine::{Engine, EngineConfig, KanaBackend, StateKind, parse_key};
use nagi_protocol::ProcessKeyResult;
use nagi_protocol::methods::{KanaKey, KeyGesture, ReadingInput, TouchAction, TouchDirection};
use nagi_protocol::view::InputMode;
use std::sync::Arc;

fn engine() -> Engine {
    Engine::new(Arc::new(KanaBackend), EngineConfig::default())
}

fn key12(e: &mut Engine, key: KanaKey, gesture: KeyGesture) -> ProcessKeyResult {
    e.insert_reading("s", &ReadingInput::Key12 { key, gesture })
}

fn text(e: &mut Engine) -> String {
    e.snapshot("s").map(|s| s.preedit.text).unwrap_or_default()
}

#[test]
fn text_input_goes_straight_to_the_reading() {
    let mut e = engine();
    let r = e.insert_reading("s", &ReadingInput::Text("かな".into()));
    assert!(r.consumed);
    assert_eq!(text(&mut e), "かな");
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Composing);
    // 空文字は何もしない
    assert!(
        !e.insert_reading("s", &ReadingInput::Text(String::new()))
            .consumed
    );
}

#[test]
fn flick_picks_the_character_for_each_direction() {
    let mut e = engine();
    for (gesture, expected) in [
        (KeyGesture::Tap, "か"),
        (KeyGesture::FlickLeft, "き"),
        (KeyGesture::FlickUp, "く"),
        (KeyGesture::FlickRight, "け"),
        (KeyGesture::FlickDown, "こ"),
    ] {
        let mut e2 = engine();
        key12(&mut e2, KanaKey::Ka, gesture);
        assert_eq!(text(&mut e2), expected, "{gesture:?}");
    }
    // 連続したタップはトグルしない（フリック入力）
    key12(&mut e, KanaKey::A, KeyGesture::Tap);
    key12(&mut e, KanaKey::A, KeyGesture::Tap);
    assert_eq!(text(&mut e), "ああ");
}

#[test]
fn toggle_cycles_through_the_row_and_wraps() {
    let mut e = engine();
    for expected in ["あ", "い", "う", "え", "お", "あ"] {
        key12(&mut e, KanaKey::A, KeyGesture::Toggle);
        assert_eq!(text(&mut e), expected);
    }
}

#[test]
fn toggle_starts_a_new_character_after_another_key_or_edit() {
    let mut e = engine();
    key12(&mut e, KanaKey::A, KeyGesture::Toggle);
    key12(&mut e, KanaKey::Ka, KeyGesture::Toggle); // 別のキー
    key12(&mut e, KanaKey::Ka, KeyGesture::Toggle); // か → き
    assert_eq!(text(&mut e), "あき");
    // フリックを挟むとトグルは続かない
    key12(&mut e, KanaKey::Sa, KeyGesture::FlickUp);
    key12(&mut e, KanaKey::Sa, KeyGesture::Toggle);
    assert_eq!(text(&mut e), "あきすさ");
    // カーソル移動のあとは新しい文字（トグルで置き換えない）
    key12(&mut e, KanaKey::Ta, KeyGesture::Toggle);
    e.perform_action("s", &TouchAction::MoveCursor(TouchDirection::Left));
    key12(&mut e, KanaKey::Ta, KeyGesture::Toggle);
    assert_eq!(text(&mut e), "あきすさたた");
}

#[test]
fn toggle_does_not_continue_after_a_physical_key() {
    let mut e = engine();
    key12(&mut e, KanaKey::A, KeyGesture::Toggle);
    e.process_key("s", &parse_key("Right").unwrap());
    key12(&mut e, KanaKey::A, KeyGesture::Toggle);
    assert_eq!(text(&mut e), "ああ");
}

#[test]
fn variant_cycles_dakuten_handakuten_and_small_kana() {
    let mut e = engine();
    key12(&mut e, KanaKey::Ha, KeyGesture::Tap);
    for expected in ["ば", "ぱ", "は"] {
        assert!(e.insert_reading("s", &ReadingInput::Variant).consumed);
        assert_eq!(text(&mut e), expected);
    }
    // 循環に無い文字は変えない（消費はする）
    key12(&mut e, KanaKey::Wa, KeyGesture::FlickUp); // ん
    e.insert_reading("s", &ReadingInput::Variant);
    assert_eq!(text(&mut e), "はん");
    // つ → っ
    key12(&mut e, KanaKey::Ta, KeyGesture::FlickUp);
    e.insert_reading("s", &ReadingInput::Variant);
    assert_eq!(text(&mut e), "はんっ");
}

#[test]
fn variant_and_actions_pass_through_when_nothing_is_being_composed() {
    let mut e = engine();
    assert!(!e.insert_reading("s", &ReadingInput::Variant).consumed);
    for action in [
        TouchAction::Space,
        TouchAction::Enter,
        TouchAction::Backspace,
        TouchAction::Cancel,
        TouchAction::MoveCursor(TouchDirection::Left),
    ] {
        assert!(!e.perform_action("s", &action).consumed, "{action:?}");
    }
}

#[test]
fn space_converts_then_picks_the_next_candidate_and_enter_commits() {
    let mut e = engine();
    e.insert_reading("s", &ReadingInput::Text("かな".into()));
    let r = e.perform_action("s", &TouchAction::Space);
    assert!(r.consumed);
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Converting);
    let r = e.perform_action("s", &TouchAction::Enter);
    assert!(r.consumed);
    assert!(r.commit.is_some());
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Direct);
}

#[test]
fn backspace_and_cancel_work_while_composing() {
    let mut e = engine();
    e.insert_reading("s", &ReadingInput::Text("かな".into()));
    assert!(e.perform_action("s", &TouchAction::Backspace).consumed);
    assert_eq!(text(&mut e), "か");
    assert!(e.perform_action("s", &TouchAction::Cancel).consumed);
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Direct);
}

#[test]
fn typing_during_conversion_commits_and_starts_a_new_reading() {
    let mut e = engine();
    e.insert_reading("s", &ReadingInput::Text("かな".into()));
    e.perform_action("s", &TouchAction::Space);
    let r = key12(&mut e, KanaKey::A, KeyGesture::Tap);
    assert!(r.commit.is_some(), "変換中の入力は直前を確定する");
    assert_eq!(text(&mut e), "あ");
}

#[test]
fn mode_actions_work_in_direct_state() {
    let mut e = engine();
    let r = e.perform_action("s", &TouchAction::SetMode(InputMode::Alphanumeric));
    assert!(r.consumed);
    assert_eq!(e.snapshot("s").unwrap().mode, InputMode::Alphanumeric);
    e.perform_action("s", &TouchAction::ToggleMode);
    assert_eq!(e.snapshot("s").unwrap().mode, InputMode::Hiragana);
}
