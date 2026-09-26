//! `Engine` の公開 API（差分・revision・セッション・文脈）のテスト

use nagi_engine::{Engine, EngineConfig, KanaBackend, StateKind, parse_key};
use nagi_protocol::methods::{SetContextParams, SurroundingText};
use nagi_protocol::view::InputMode;
use std::sync::Arc;

fn engine() -> Engine {
    Engine::new(Arc::new(KanaBackend), EngineConfig::default())
}

fn press(e: &mut Engine, session: &str, key: &str) -> nagi_protocol::ProcessKeyResult {
    e.process_key(session, &parse_key(key).unwrap())
}

#[test]
fn revision_increases_only_when_something_changes() {
    let mut e = engine();
    let r0 = press(&mut e, "s", "Space"); // Direct の Space は素通し・変化なし
    assert!(!r0.consumed);
    assert!(r0.preedit.is_none() && r0.candidates.is_none() && r0.mode.is_none());
    let r1 = press(&mut e, "s", "k");
    let r2 = press(&mut e, "s", "a");
    assert!(r1.revision > r0.revision);
    assert!(r2.revision > r1.revision);
    // 何も変わらないキー（Composing 中の Up は消費のみ）
    let r3 = press(&mut e, "s", "Up");
    assert!(r3.consumed);
    assert_eq!(r3.revision, r2.revision);
    assert!(r3.preedit.is_none());
    // 確定だけでも revision は進む
    let r4 = press(&mut e, "s", "Enter");
    assert_eq!(r4.commit.as_deref(), Some("か"));
    assert!(r4.revision > r3.revision);
}

#[test]
fn flags_changed_never_changes_state() {
    let mut e = engine();
    press(&mut e, "s", "k");
    let mut key = parse_key("Eisu").unwrap();
    key.kind = nagi_protocol::KeyKind::FlagsChanged;
    let before = e.snapshot("s").unwrap();
    let r = e.process_key("s", &key);
    assert!(!r.consumed);
    assert_eq!(e.snapshot("s").unwrap(), before);
}

#[test]
fn sessions_are_isolated_and_can_end() {
    let mut e = engine();
    press(&mut e, "a", "k");
    press(&mut e, "b", "Eisu");
    assert_eq!(e.snapshot("a").unwrap().state, StateKind::Composing);
    assert_eq!(e.snapshot("b").unwrap().mode, InputMode::Alphanumeric);
    assert_eq!(e.snapshot("a").unwrap().mode, InputMode::Hiragana);
    assert!(e.snapshot("c").is_none());
    e.end_session("a");
    assert!(e.snapshot("a").is_none());
    // 終了後に同じ ID を使うと新しいセッションになる
    let r = press(&mut e, "a", "k");
    assert_eq!(r.preedit.unwrap().text, "k");
}

#[test]
fn focus_out_commits_pending_n_as_kana() {
    let mut e = engine();
    press(&mut e, "s", "k");
    press(&mut e, "s", "a");
    press(&mut e, "s", "n");
    let r = e.focus_out("s");
    assert!(r.consumed);
    assert_eq!(r.commit.as_deref(), Some("かん"));
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Direct);
    // Direct での focus_out は何もしない
    let r = e.focus_out("s");
    assert!(r.commit.is_none() && r.preedit.is_none());
}

#[test]
fn surrounding_text_feeds_left_context_unless_secure() {
    use std::sync::Mutex;
    struct Spy(Mutex<Vec<String>>);
    impl nagi_engine::ConversionBackend for Spy {
        fn name(&self) -> &str {
            "spy"
        }
        fn convert_sentence(&self, reading: &str, left: &str) -> Vec<nagi_engine::BackendSegment> {
            self.0.lock().unwrap().push(left.to_string());
            KanaBackend.convert_sentence(reading, left)
        }
        fn segment_candidates(
            &self,
            _: &str,
            _: &str,
            _: usize,
        ) -> Vec<nagi_engine::BackendCandidate> {
            Vec::new()
        }
    }
    let spy = Arc::new(Spy(Mutex::new(Vec::new())));
    let mut e = Engine::new(spy.clone(), EngineConfig::default());
    let ctx = |text: &str, cursor: u32, secure: Option<bool>| SetContextParams {
        session_id: "s".into(),
        surrounding_text: Some(SurroundingText {
            text: text.into(),
            cursor,
        }),
        app_id: None,
        secure_input: secure,
    };
    // カーソル手前（UTF-16 の 5 単位 = 「私の名前は」）だけが左文脈になる
    e.set_context("s", &ctx("私の名前はほげ", 5, None));
    press(&mut e, "s", "a");
    press(&mut e, "s", "Space");
    assert_eq!(spy.0.lock().unwrap().last().unwrap(), "私の名前は");
    press(&mut e, "s", "Escape");
    press(&mut e, "s", "Escape");
    // secure input 中は文脈を持たない（以降の文脈も保存しない）
    e.set_context("s", &ctx("秘密", 2, Some(true)));
    press(&mut e, "s", "a");
    press(&mut e, "s", "Space");
    assert_eq!(spy.0.lock().unwrap().last().unwrap(), "");
    press(&mut e, "s", "Escape");
    press(&mut e, "s", "Escape");
    e.set_context("s", &ctx("別の文", 3, None));
    press(&mut e, "s", "a");
    press(&mut e, "s", "Space");
    assert_eq!(spy.0.lock().unwrap().last().unwrap(), "");
}

#[test]
fn multi_scalar_text_is_inserted_whole() {
    let mut e = engine();
    let mut key = parse_key("a").unwrap();
    key.keysym = 0;
    key.text = "👨\u{200d}👩".to_string();
    let r = e.process_key("s", &key);
    assert!(r.consumed);
    assert_eq!(r.preedit.unwrap().text, "👨\u{200d}👩");
}

#[test]
fn engine_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Engine>();
}
