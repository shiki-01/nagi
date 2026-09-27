//! `Engine` の公開 API（差分・revision・セッション・文脈）のテスト

use nagi_engine::{
    BackendCandidate, BackendSegment, Completion, ConversionBackend, Engine, EngineConfig,
    KanaBackend, StateKind, parse_key,
};
use nagi_protocol::methods::{SetContextParams, SurroundingText};
use nagi_protocol::view::CandidateSource;
use nagi_protocol::view::InputMode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

// ---- 確定の通知（学習）と設定の差し替え ----

use nagi_engine::{CommitContext, CommitObserver, CommittedSegment, MockBackend};
use nagi_protocol::view::SegmentKind;
use std::sync::Mutex;

/// (セッション ID, アプリ ID, secure input, 確定した文節)
type CommitRecord = (String, Option<String>, bool, Vec<CommittedSegment>);

#[derive(Default)]
struct Recorder {
    commits: Mutex<Vec<CommitRecord>>,
    forgets: Mutex<Vec<(String, String)>>,
    forget_result: Mutex<bool>,
}

impl CommitObserver for Recorder {
    fn on_commit(&self, ctx: &CommitContext<'_>, segments: &[CommittedSegment]) {
        self.commits.lock().unwrap().push((
            ctx.session_id.to_string(),
            ctx.app_id.map(str::to_string),
            ctx.secure_input,
            segments.to_vec(),
        ));
    }

    fn on_forget(&self, _ctx: &CommitContext<'_>, reading: &str, surface: &str) -> bool {
        self.forgets
            .lock()
            .unwrap()
            .push((reading.to_string(), surface.to_string()));
        *self.forget_result.lock().unwrap()
    }
}

fn mock_engine() -> (Engine, Arc<Recorder>) {
    let backend = MockBackend::new().with_sentence(
        "きょうはいい",
        vec![
            ("きょうは", vec!["今日は", "京は"]),
            ("いい", vec!["いい", "良い"]),
        ],
    );
    let mut e = Engine::new(Arc::new(backend), EngineConfig::default());
    let rec = Arc::new(Recorder::default());
    e.set_commit_observer(rec.clone());
    (e, rec)
}

fn type_str(e: &mut Engine, session: &str, keys: &str) {
    for c in keys.chars() {
        press(e, session, &c.to_string());
    }
}

#[test]
fn observer_receives_converted_segments_with_left_context() {
    let (mut e, rec) = mock_engine();
    e.set_context(
        "s",
        &SetContextParams {
            session_id: "s".into(),
            surrounding_text: Some(SurroundingText {
                text: "私は".into(),
                cursor: 2,
            }),
            app_id: Some("com.example.editor".into()),
            secure_input: None,
        },
    );
    type_str(&mut e, "s", "kyouhaii");
    press(&mut e, "s", "Space");
    press(&mut e, "s", "Space"); // 候補窓で「京は」を選ぶ
    assert_eq!(e.snapshot("s").unwrap().preedit.segments[0].surface, "京は");
    press(&mut e, "s", "Enter");
    let commits = rec.commits.lock().unwrap();
    assert_eq!(commits.len(), 1);
    let (session, app, secure, segs) = &commits[0];
    assert_eq!(
        (session.as_str(), app.as_deref(), *secure),
        ("s", Some("com.example.editor"), false)
    );
    assert_eq!(segs.len(), 2);
    assert_eq!(
        (segs[0].reading.as_str(), segs[0].surface.as_str()),
        ("きょうは", "京は")
    );
    assert_eq!(segs[0].left_context, "私は");
    assert_eq!(
        segs[1].left_context, "私は京は",
        "前の文節の確定表記が文脈に入る"
    );
    assert_eq!(segs[0].kind, SegmentKind::Japanese);
}

#[test]
fn observer_is_not_called_without_conversion() {
    let (mut e, rec) = mock_engine();
    type_str(&mut e, "s", "ka");
    press(&mut e, "s", "Enter"); // 変換せずに確定
    type_str(&mut e, "s", "ka");
    e.focus_out("s");
    type_str(&mut e, "s", "ka");
    press(&mut e, "s", "Escape");
    assert!(rec.commits.lock().unwrap().is_empty());
}

#[test]
fn observer_sees_secure_input_flag() {
    let (mut e, rec) = mock_engine();
    e.set_context(
        "s",
        &SetContextParams {
            session_id: "s".into(),
            surrounding_text: None,
            app_id: None,
            secure_input: Some(true),
        },
    );
    type_str(&mut e, "s", "ka");
    press(&mut e, "s", "Space");
    press(&mut e, "s", "Enter");
    let commits = rec.commits.lock().unwrap();
    assert_eq!(commits.len(), 1);
    assert!(
        commits[0].2,
        "secure input 中であることが observer に伝わる"
    );
}

#[test]
fn ctrl_delete_asks_observer_to_forget_and_rebuilds_candidates() {
    let (mut e, rec) = mock_engine();
    type_str(&mut e, "s", "kyouhaii");
    press(&mut e, "s", "Space");
    press(&mut e, "s", "Space"); // 候補窓を出す（選択は「京は」）
    // 忘れられなければ候補は変わらない
    let before = e.snapshot("s").unwrap().candidates;
    press(&mut e, "s", "Ctrl+Delete");
    assert_eq!(
        *rec.forgets.lock().unwrap(),
        [("きょうは".to_string(), "京は".to_string())]
    );
    assert_eq!(e.snapshot("s").unwrap().candidates, before);
    // 忘れられたら候補一覧を作り直し、窓は開いたまま・選択表記は維持
    *rec.forget_result.lock().unwrap() = true;
    let r = press(&mut e, "s", "Ctrl+Delete");
    assert!(r.consumed);
    let snap = e.snapshot("s").unwrap();
    assert!(snap.candidates.visible);
    assert_eq!(snap.candidates.items[0].surface, "京は");
    assert_eq!(snap.candidates.selected, 0);
    // Composing / Direct では何も起きない
    press(&mut e, "s", "Escape");
    let n = rec.forgets.lock().unwrap().len();
    press(&mut e, "s", "Ctrl+Delete");
    assert_eq!(rec.forgets.lock().unwrap().len(), n);
}

#[test]
fn update_config_applies_to_existing_sessions() {
    use nagi_romaji::{PunctuationStyle, ShiftLetterMode};
    let mut e = engine();
    press(&mut e, "s", "k");
    press(&mut e, "s", "a");
    assert_eq!(press(&mut e, "s", ",").preedit.unwrap().text, "か、");
    let config = EngineConfig {
        punctuation: PunctuationStyle::Western,
        shift_letter: ShiftLetterMode::SingleUpper,
        page_size: 5,
        ..EngineConfig::default()
    };
    e.update_config(config);
    assert_eq!(e.config().page_size, 5);
    assert_eq!(press(&mut e, "s", ",").preedit.unwrap().text, "か、，");
    // 新しいセッションでも新設定
    assert_eq!(press(&mut e, "t", ",").preedit.unwrap().text, "，");
    assert_eq!(e.snapshot("t").unwrap().candidates.page_size, 5);
}

// ---- 非同期の変換結果の差し替え ----

/// `ready` になるまでは読みそのまま、なったあとは「きょう → 今日」を返す
#[derive(Default)]
struct Deferred {
    ready: AtomicBool,
}

impl ConversionBackend for Deferred {
    fn name(&self) -> &str {
        "deferred"
    }

    fn convert_sentence(&self, reading: &str, _left_context: &str) -> Vec<BackendSegment> {
        let surface = if self.ready.load(Ordering::SeqCst) && reading == "きょう" {
            "今日"
        } else {
            reading
        };
        vec![BackendSegment {
            reading: reading.to_string(),
            surface: surface.to_string(),
            sources: vec![CandidateSource::SystemDict],
        }]
    }

    fn segment_candidates(&self, _: &str, _: &str, _: usize) -> Vec<BackendCandidate> {
        Vec::new()
    }
}

fn done(reading: &str, left: &str) -> [Completion; 1] {
    [Completion {
        reading: reading.to_string(),
        left_context: left.to_string(),
    }]
}

fn deferred_engine() -> (Engine, Arc<Deferred>) {
    let backend = Arc::new(Deferred::default());
    (
        Engine::new(backend.clone(), EngineConfig::default()),
        backend,
    )
}

fn type_and_convert(e: &mut Engine) -> nagi_protocol::ProcessKeyResult {
    for k in ["k", "y", "o", "u"] {
        press(e, "s", k);
    }
    press(e, "s", "Space")
}

#[test]
fn completion_replaces_untouched_converting_preedit() {
    let (mut e, backend) = deferred_engine();
    let space = type_and_convert(&mut e);
    assert_eq!(space.preedit.as_ref().unwrap().text, "きょう");

    backend.ready.store(true, Ordering::SeqCst);
    let updates = e.apply_completions(&done("きょう", ""));
    assert_eq!(updates.len(), 1);
    let (id, result) = &updates[0];
    assert_eq!(id, "s");
    assert_eq!(result.preedit.as_ref().unwrap().text, "今日");
    assert!(result.revision > space.revision, "revision が進む");
    assert_eq!(e.snapshot("s").unwrap().preedit.text, "今日");
    assert_eq!(e.snapshot("s").unwrap().state, StateKind::Converting);

    // 同じ結果の再通知は何もしない（revision も進まない）
    assert!(e.apply_completions(&done("きょう", "")).is_empty());
    assert_eq!(e.snapshot("s").unwrap().revision, result.revision);
}

#[test]
fn completion_is_ignored_after_user_operation() {
    let (mut e, backend) = deferred_engine();
    type_and_convert(&mut e);
    press(&mut e, "s", "Space"); // 候補送り（ユーザー操作）
    backend.ready.store(true, Ordering::SeqCst);
    assert!(e.apply_completions(&done("きょう", "")).is_empty());
}

#[test]
fn completion_with_other_key_is_ignored() {
    let (mut e, backend) = deferred_engine();
    type_and_convert(&mut e);
    backend.ready.store(true, Ordering::SeqCst);
    assert!(e.apply_completions(&done("きょうは", "")).is_empty());
    assert!(e.apply_completions(&done("きょう", "別の文脈")).is_empty());
    assert_eq!(e.snapshot("s").unwrap().preedit.text, "きょう");
}

#[test]
fn completion_is_ignored_while_composing() {
    let (mut e, backend) = deferred_engine();
    for k in ["k", "y", "o", "u"] {
        press(&mut e, "s", k);
    }
    backend.ready.store(true, Ordering::SeqCst);
    assert!(e.apply_completions(&done("きょう", "")).is_empty());
}
