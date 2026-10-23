//! 状態機械のゴールデンテスト: `tests/golden/*.yaml` のキー列 → 期待する状態を検査する。
//!
//! 形式は `tests/golden/README.md` を参照。

use nagi_engine::{
    Engine, EngineConfig, KanaBackend, MockBackend, SessionSnapshot, StateKind, key_from_text,
    parse_key,
};
use nagi_protocol::methods::{ProcessKeyResult, RestoreSegment, RestoreSessionParams};
use nagi_protocol::view::{InputMode, SegmentKind};
use nagi_romaji::{PunctuationStyle, ShiftLetterMode};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    mock: Option<MockSpec>,
    #[serde(default)]
    config: Option<ConfigSpec>,
    #[serde(default = "default_session")]
    session: String,
    steps: Vec<Step>,
}

fn default_session() -> String {
    "s1".to_string()
}

#[derive(Debug, Default, Deserialize)]
struct MockSpec {
    #[serde(default)]
    sentences: BTreeMap<String, Vec<MockSegment>>,
    #[serde(default)]
    segments: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct MockSegment {
    reading: String,
    candidates: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ConfigSpec {
    shift_letter: Option<String>,
    punctuation: Option<String>,
    page_size: Option<usize>,
    mode: Option<String>,
    romaji_carryover: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KeySpec {
    Name(String),
    Text { text: String },
}

#[derive(Debug, Deserialize)]
struct RestoreSpec {
    segments: Vec<RestoreSegSpec>,
    #[serde(default)]
    cursor: u32,
    #[serde(default)]
    focus: Option<u32>,
    #[serde(default)]
    converting: bool,
}

#[derive(Debug, Deserialize)]
struct RestoreSegSpec {
    reading: String,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Step {
    #[serde(default)]
    key: Option<KeySpec>,
    /// `focus_in` | `focus_out` | `reset` | `restore` | `live_convert` | `refresh_suggest`
    #[serde(default)]
    action: Option<String>,
    /// `action: live_convert` の期待 reading（サーバーがデバウンス後に渡す入力全体の読み）
    #[serde(default)]
    reading: Option<String>,
    #[serde(default)]
    restore: Option<RestoreSpec>,
    #[serde(default)]
    expect: Option<Expect>,
}

#[derive(Debug, Default, Deserialize)]
struct Expect {
    consumed: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    commit: Option<Option<String>>,
    /// このステップの応答に `delete_before` が含まれるか、その値。
    /// `null` は「含まれないこと」を期待、キー自体を省略すれば検査しない
    #[serde(default, deserialize_with = "double_option")]
    delete_before: Option<Option<u32>>,
    preedit: Option<String>,
    pending: Option<String>,
    segments: Option<Vec<String>>,
    readings: Option<Vec<String>>,
    kinds: Option<Vec<String>>,
    cursor: Option<u32>,
    #[serde(default, deserialize_with = "double_option")]
    focus: Option<Option<u32>>,
    /// 選択範囲（UTF-16 コード単位の [start, end)）。`null` で「選択なし」を期待、
    /// キー自体を省略すれば検査しない
    #[serde(default, deserialize_with = "double_option")]
    selection: Option<Option<(u32, u32)>>,
    state: Option<String>,
    mode: Option<String>,
    candidates: Option<Vec<String>>,
    candidates_visible: Option<bool>,
    selected: Option<u32>,
    /// このステップで `ProcessKeyResult.mode` が返ること（None は返らないこと）
    #[serde(default, deserialize_with = "double_option")]
    result_mode: Option<Option<String>>,
    /// このステップの応答に preedit が含まれるか（差分通知の検査）
    result_preedit_present: Option<bool>,
    /// このステップの応答に candidates が含まれるか
    result_candidates_present: Option<bool>,
    /// このステップで revision が増えるか
    revision_increased: Option<bool>,
}

fn parse_mode(s: &str) -> InputMode {
    match s {
        "hiragana" => InputMode::Hiragana,
        "alphanumeric" => InputMode::Alphanumeric,
        other => panic!("不明な mode: {other}"),
    }
}

fn mode_name(m: InputMode) -> &'static str {
    match m {
        InputMode::Hiragana => "hiragana",
        InputMode::Alphanumeric => "alphanumeric",
    }
}

fn kind_name(k: SegmentKind) -> &'static str {
    match k {
        SegmentKind::Japanese => "japanese",
        SegmentKind::Literal => "literal",
        SegmentKind::Fixed => "fixed",
        SegmentKind::Styled => "styled",
    }
}

fn parse_kind(s: &str) -> SegmentKind {
    match s {
        "japanese" => SegmentKind::Japanese,
        "literal" => SegmentKind::Literal,
        "fixed" => SegmentKind::Fixed,
        "styled" => SegmentKind::Styled,
        other => panic!("不明な kind: {other}"),
    }
}

fn state_name(s: StateKind) -> &'static str {
    match s {
        StateKind::Direct => "direct",
        StateKind::Composing => "composing",
        StateKind::Converting => "converting",
        StateKind::RangeSelecting => "range_selecting",
        StateKind::StyleAction => "style_action",
    }
}

fn build_engine(case: &Case) -> Engine {
    let mut config = EngineConfig::default();
    if let Some(c) = &case.config {
        if let Some(s) = &c.shift_letter {
            config.shift_letter = match s.as_str() {
                "literal_continue" => ShiftLetterMode::LiteralContinue,
                "single_upper" => ShiftLetterMode::SingleUpper,
                other => panic!("不明な shift_letter: {other}"),
            };
        }
        if let Some(p) = &c.punctuation {
            config.punctuation = PunctuationStyle::from_config_str(p)
                .unwrap_or_else(|| panic!("不明な punctuation: {p}"));
        }
        if let Some(n) = c.page_size {
            config.page_size = n;
        }
        if let Some(m) = &c.mode {
            config.initial_mode = parse_mode(m);
        }
        if let Some(b) = c.romaji_carryover {
            config.romaji_carryover = b;
        }
    }
    let backend: Arc<dyn nagi_engine::ConversionBackend> = match case.backend.as_deref() {
        None | Some("kana") => Arc::new(KanaBackend),
        Some("mock") => {
            let spec = case.mock.as_ref().expect("backend: mock には mock: が必要");
            let mut mock = MockBackend::new();
            for (reading, segs) in &spec.sentences {
                mock = mock.with_sentence(
                    reading,
                    segs.iter()
                        .map(|s| {
                            (
                                s.reading.as_str(),
                                s.candidates.iter().map(String::as_str).collect(),
                            )
                        })
                        .collect(),
                );
            }
            for (reading, cands) in &spec.segments {
                mock = mock.with_segments(reading, cands.iter().map(String::as_str).collect());
            }
            Arc::new(mock)
        }
        Some(other) => panic!("不明な backend: {other}"),
    };
    Engine::new(backend, config)
}

fn check(
    expect: &Expect,
    result: &ProcessKeyResult,
    snap: &SessionSnapshot,
    prev_revision: u32,
) -> Vec<String> {
    let mut errors = Vec::new();
    macro_rules! eq {
        ($name:expr, $actual:expr, $expected:expr) => {
            if ($actual) != ($expected) {
                errors.push(format!(
                    "{}: 期待 {:?} / 実際 {:?}",
                    $name, $expected, $actual
                ));
            }
        };
    }
    if let Some(v) = expect.consumed {
        eq!("consumed", result.consumed, v);
    }
    if let Some(v) = &expect.commit {
        eq!("commit", &result.commit, v);
    }
    if let Some(v) = &expect.delete_before {
        eq!("delete_before", &result.delete_before, v);
    }
    if let Some(v) = &expect.preedit {
        eq!("preedit", &snap.preedit.text, v);
    }
    if let Some(v) = &expect.pending {
        let actual = snap
            .preedit
            .pending
            .map(|r| {
                let units: Vec<u16> = snap.preedit.text.encode_utf16().collect();
                String::from_utf16_lossy(&units[r.start as usize..r.end as usize])
            })
            .unwrap_or_default();
        eq!("pending", &actual, v);
    }
    if let Some(v) = &expect.segments {
        let actual: Vec<String> = snap
            .preedit
            .segments
            .iter()
            .map(|s| s.surface.clone())
            .collect();
        eq!("segments", &actual, v);
    }
    if let Some(v) = &expect.readings {
        let actual: Vec<String> = snap
            .preedit
            .segments
            .iter()
            .map(|s| s.reading.clone())
            .collect();
        eq!("readings", &actual, v);
    }
    if let Some(v) = &expect.kinds {
        let actual: Vec<String> = snap
            .preedit
            .segments
            .iter()
            .map(|s| kind_name(s.kind).to_string())
            .collect();
        eq!("kinds", &actual, v);
    }
    if let Some(v) = expect.cursor {
        eq!("cursor", snap.preedit.cursor, v);
    }
    if let Some(v) = &expect.focus {
        eq!("focus", &snap.preedit.focus, v);
    }
    if let Some(v) = &expect.selection {
        let actual = snap.preedit.selection.map(|r| (r.start, r.end));
        eq!("selection", &actual, v);
    }
    if let Some(v) = &expect.state {
        eq!("state", state_name(snap.state), v.as_str());
    }
    if let Some(v) = &expect.mode {
        eq!("mode", mode_name(snap.mode), v.as_str());
    }
    if let Some(v) = &expect.candidates {
        let actual: Vec<String> = snap
            .candidates
            .items
            .iter()
            .map(|c| c.surface.clone())
            .collect();
        eq!("candidates", &actual, v);
    }
    if let Some(v) = expect.candidates_visible {
        eq!("candidates_visible", snap.candidates.visible, v);
    }
    if let Some(v) = expect.selected {
        eq!("selected", snap.candidates.selected, v);
    }
    if let Some(v) = &expect.result_mode {
        let actual = result.mode.map(|m| mode_name(m).to_string());
        eq!("result_mode", &actual, v);
    }
    if let Some(v) = expect.result_preedit_present {
        eq!("result_preedit_present", result.preedit.is_some(), v);
    }
    if let Some(v) = expect.result_candidates_present {
        eq!("result_candidates_present", result.candidates.is_some(), v);
    }
    if let Some(v) = expect.revision_increased {
        eq!("revision_increased", result.revision != prev_revision, v);
    }
    errors
}

fn run_case(case: &Case) -> Vec<String> {
    let mut failures = Vec::new();
    let mut engine = build_engine(case);
    let mut prev_revision = 0u32;
    for (i, step) in case.steps.iter().enumerate() {
        let label = format!("[{}] step {}", case.name, i + 1);
        let result = match (&step.key, step.action.as_deref()) {
            (Some(KeySpec::Name(name)), None) => match parse_key(name) {
                Ok(key) => engine.process_key(&case.session, &key),
                Err(e) => {
                    failures.push(format!("{label}: キー {name:?} を解釈できない: {e}"));
                    continue;
                }
            },
            (Some(KeySpec::Text { text }), None) => {
                engine.process_key(&case.session, &key_from_text(text))
            }
            (None, Some("focus_in")) => engine.focus_in(&case.session),
            (None, Some("focus_out")) => engine.focus_out(&case.session),
            (None, Some("reset")) => engine.reset(&case.session),
            (None, Some("live_convert")) => {
                let reading = step
                    .reading
                    .as_deref()
                    .expect("action: live_convert には reading: が必要");
                // 反映されなかった（読みが古い等）場合は何も返らない
                engine
                    .live_convert(&case.session, reading)
                    .unwrap_or_default()
            }
            (None, Some("refresh_suggest")) => {
                // サーバーがデバウンス後に行う入力中の候補（サジェスト）の作成。対象が無ければ何も返らない
                engine
                    .composing_candidate_reading(&case.session)
                    .and_then(|reading| {
                        engine.refresh_composing_candidates(&case.session, &reading)
                    })
                    .unwrap_or_default()
            }
            (None, Some("restore")) => {
                let spec = step
                    .restore
                    .as_ref()
                    .expect("action: restore には restore: が必要");
                let params = RestoreSessionParams {
                    session_id: case.session.clone(),
                    segments: spec
                        .segments
                        .iter()
                        .map(|s| RestoreSegment {
                            reading: s.reading.clone(),
                            surface: s.surface.clone().unwrap_or_else(|| s.reading.clone()),
                            kind: s.kind.as_deref().map_or(SegmentKind::Japanese, parse_kind),
                        })
                        .collect(),
                    cursor: spec.cursor,
                    focus: spec.focus,
                    converting: spec.converting,
                };
                engine.restore_session(&case.session, &params)
            }
            _ => {
                failures.push(format!("{label}: key か action のどちらか一方が必要"));
                continue;
            }
        };
        let snap = engine
            .snapshot(&case.session)
            .expect("処理後はセッションが存在する");
        if let Some(expect) = &step.expect {
            for e in check(expect, &result, &snap, prev_revision) {
                failures.push(format!("{label} ({:?}): {e}", step.key));
            }
        }
        prev_revision = result.revision;
    }
    failures
}

#[test]
fn golden() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/golden がある")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "ゴールデンテストの YAML が無い");

    let mut total = 0usize;
    let mut names = std::collections::HashSet::new();
    let mut failures = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("読み込み");
        let cases: Vec<Case> = serde_yaml_ng::from_str(&text)
            .unwrap_or_else(|e| panic!("{}: YAML の解釈に失敗: {e}", file.display()));
        for case in cases {
            total += 1;
            if !names.insert(case.name.clone()) {
                failures.push(format!("ケース名が重複: {}", case.name));
            }
            failures.extend(run_case(&case));
        }
    }
    eprintln!("golden cases: {total}");
    assert!(
        failures.is_empty(),
        "{} 件の失敗（{total} ケース中）:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
