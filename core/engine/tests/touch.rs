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

#[test]
fn space_shows_the_candidate_list_immediately() {
    let mut e = engine();
    e.insert_reading("s", &ReadingInput::Text("かな".into()));
    let r = e.perform_action("s", &TouchAction::Space);
    let candidates = r.candidates.expect("変換を始めたら候補が出る");
    assert!(candidates.visible);
    assert!(!candidates.items.is_empty());
    // 変換中の ←/→ は注目文節の移動（カーソルではない）として使える
    let r = e.perform_action("s", &TouchAction::MoveFocus(TouchDirection::Right));
    assert!(r.consumed);
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Converting);
    // 注目文節を動かしても、候補の一覧は出たまま（閉じない）
    let shown = e.snapshot("s").unwrap().candidates;
    assert!(shown.visible);
}

// ---- 入力中（変換前）の候補と部分変換 ----

fn mock_engine() -> Engine {
    use nagi_engine::MockBackend;
    let backend = MockBackend::new().with_sentence(
        "きょうはあめ",
        vec![
            ("きょう", vec!["今日", "京"]),
            ("は", vec!["は"]),
            ("あめ", vec!["雨", "飴"]),
        ],
    );
    Engine::new(Arc::new(backend), EngineConfig::default())
}

fn type_text(e: &mut Engine, text: &str) {
    e.insert_reading("s", &ReadingInput::Text(text.into()));
}

/// 入力中の候補を作る（サーバーがバックグラウンドで行うのと同じ）。候補の表記と covers を返す
fn refresh(e: &mut Engine) -> Vec<(String, Option<u32>)> {
    let reading = e
        .composing_candidate_reading("s")
        .expect("候補を作る対象がある");
    let r = e
        .refresh_composing_candidates("s", &reading)
        .expect("候補が作られる");
    r.candidates
        .expect("候補の差分")
        .items
        .into_iter()
        .map(|c| (c.surface, c.covers))
        .collect()
}

#[test]
fn composing_candidates_list_whole_prefix_and_kana() {
    let mut e = mock_engine();
    type_text(&mut e, "きょうはあめ");
    let items = refresh(&mut e);
    // 読み全体の最良、先頭の文節の候補（部分変換）、ひらがな・カタカナ
    assert_eq!(items[0], ("今日は雨".into(), None));
    assert!(items.contains(&("今日".into(), Some(3))));
    assert!(items.contains(&("京".into(), Some(3))));
    assert!(items.contains(&("きょうはあめ".into(), None)));
    assert!(items.contains(&("キョウハアメ".into(), None)));
}

#[test]
fn selecting_a_prefix_candidate_commits_only_that_part() {
    let mut e = mock_engine();
    type_text(&mut e, "きょうはあめ");
    let items = refresh(&mut e);
    let index = items.iter().position(|i| i.0 == "京").unwrap();
    let r = e.select_candidate("s", index as u32);
    assert_eq!(r.commit.as_deref(), Some("京"));
    // 残りの読みは入力中のまま
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Composing);
    assert_eq!(text(&mut e), "はあめ");
}

#[test]
fn selecting_the_whole_candidate_commits_everything() {
    let mut e = mock_engine();
    type_text(&mut e, "きょうはあめ");
    refresh(&mut e);
    let r = e.select_candidate("s", 0);
    assert_eq!(r.commit.as_deref(), Some("今日は雨"));
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Direct);
}

#[test]
fn composing_candidates_go_stale_when_the_input_changes() {
    let mut e = mock_engine();
    type_text(&mut e, "きょうは");
    refresh(&mut e);
    // 入力が変わったら、古い候補は表示にも選択にも使わない
    let r = e.insert_reading("s", &ReadingInput::Text("あ".into()));
    assert!(r.candidates.is_some_and(|c| c.items.is_empty()));
    let r = e.select_candidate("s", 0);
    assert!(r.commit.is_none());
    assert_eq!(text(&mut e), "きょうはあ");
    // 古い読みでの更新は無視される
    assert!(e.refresh_composing_candidates("s", "きょうは").is_none());
}

#[test]
fn no_composing_candidates_while_romaji_is_pending_or_for_literals() {
    let mut e = engine();
    e.process_key("s", &parse_key("k").unwrap());
    assert!(e.composing_candidate_reading("s").is_none());
    let mut e = engine();
    e.process_key("s", &parse_key("1").unwrap());
    assert!(e.composing_candidate_reading("s").is_none());
}

#[test]
fn dakuten_order_decides_tsu_and_u_cycles() {
    use nagi_engine::touch::DakutenOrder;
    let run = |order, key, gesture, expected: &[&str]| {
        let mut e = Engine::new(
            Arc::new(KanaBackend),
            EngineConfig {
                dakuten_order: order,
                ..EngineConfig::default()
            },
        );
        key12(&mut e, key, gesture);
        for want in expected {
            e.insert_reading("s", &ReadingInput::Variant);
            assert_eq!(text(&mut e), *want, "{order:?}");
        }
    };
    // 既定（小書き先）: つ→っ→づ→つ
    run(
        DakutenOrder::SmallFirst,
        KanaKey::Ta,
        KeyGesture::FlickUp,
        &["っ", "づ", "つ"],
    );
    // 濁点先: つ→づ→っ→つ、う→ゔ→ぅ
    run(
        DakutenOrder::DakutenFirst,
        KanaKey::Ta,
        KeyGesture::FlickUp,
        &["づ", "っ", "つ"],
    );
    run(
        DakutenOrder::DakutenFirst,
        KanaKey::A,
        KeyGesture::FlickUp,
        &["ゔ", "ぅ", "う"],
    );
    // 小書きのない文字は同じ
    run(
        DakutenOrder::DakutenFirst,
        KanaKey::Ha,
        KeyGesture::Tap,
        &["ば", "ぱ", "は"],
    );
    assert_eq!(
        EngineConfig::default().dakuten_order,
        DakutenOrder::SmallFirst
    );
}
