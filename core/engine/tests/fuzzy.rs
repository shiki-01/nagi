//! 曖昧入力（F-1）の結合テスト: 辞書付きのモックバックエンドで、Space 変換のときに補正候補
//! （`CandidateSource::Corrected`）が並ぶこと、第一仮説が変わらないこと、打鍵ログが無効化される
//! ケースでは補正が出ないことを検査する（設計: `docs/fuzzy-input-design.md`）。

use nagi_engine::{
    BackendCandidate, BackendSegment, BackendSentence, CommitContext, CommitObserver,
    CommittedSegment, ConversionBackend, Engine, EngineConfig, ReadingQuality, ScoredSentence,
    parse_key,
};
use nagi_fuzzy::{PrefixOracle, ReadingHypothesis};
use nagi_protocol::methods::{InputAlternative, RestoreSegment, RestoreSessionParams};
use nagi_protocol::view::{CandidateSource, SegmentKind};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const S: &str = "s";

struct MockOracle {
    words: BTreeSet<String>,
    calls: AtomicUsize,
}

impl PrefixOracle for MockOracle {
    fn has_prefix(&self, p: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.words.iter().any(|w| w.starts_with(p))
    }

    fn contains_word(&self, w: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.words.contains(w)
    }
}

/// 読み → 表記の小さな辞書を持つバックエンド。辞書に載らない文字は連続を 1 文節にまとめる
struct DictMock {
    words: BTreeMap<String, String>,
    oracle: MockOracle,
    /// `sentence_candidates` で文全体の N-best（最良 + 読みそのまま）を返すか
    sentence_nbest: bool,
    hypotheses_calls: AtomicUsize,
}

impl DictMock {
    fn new(words: &[(&str, &str)]) -> Self {
        DictMock {
            words: words
                .iter()
                .map(|(r, s)| (r.to_string(), s.to_string()))
                .collect(),
            oracle: MockOracle {
                words: words.iter().map(|(r, _)| r.to_string()).collect(),
                calls: AtomicUsize::new(0),
            },
            sentence_nbest: false,
            hypotheses_calls: AtomicUsize::new(0),
        }
    }

    fn with_sentence_nbest(mut self) -> Self {
        self.sentence_nbest = true;
        self
    }

    /// 最長一致で区切る。`(読み, 表記, 辞書にあるか)`
    fn split(&self, reading: &str) -> Vec<(String, String, bool)> {
        let chars: Vec<char> = reading.chars().collect();
        let mut out: Vec<(String, String, bool)> = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let found = (i + 1..=chars.len()).rev().find_map(|j| {
                let r: String = chars[i..j].iter().collect();
                self.words.get(&r).map(|s| (j, r, s.clone()))
            });
            match found {
                Some((j, r, s)) => {
                    out.push((r, s, true));
                    i = j;
                }
                None => {
                    match out.last_mut() {
                        Some(last) if !last.2 => {
                            last.0.push(chars[i]);
                            last.1.push(chars[i]);
                        }
                        _ => out.push((chars[i].to_string(), chars[i].to_string(), false)),
                    }
                    i += 1;
                }
            }
        }
        out
    }

    fn segments(&self, reading: &str) -> Vec<BackendSegment> {
        self.split(reading)
            .into_iter()
            .map(|(reading, surface, _)| BackendSegment {
                reading,
                surface,
                sources: vec![CandidateSource::SystemDict],
            })
            .collect()
    }

    /// 文節数 + 未知文字 × 10（nat 相当の単純なコスト）
    fn cost_of(&self, reading: &str) -> (f32, f32) {
        let parts = self.split(reading);
        let total = reading.chars().count().max(1) as f32;
        let unknown: usize = parts
            .iter()
            .filter(|p| !p.2)
            .map(|p| p.0.chars().count())
            .sum();
        (
            unknown as f32 / total,
            parts.len() as f32 + unknown as f32 * 10.0,
        )
    }
}

impl ConversionBackend for DictMock {
    fn name(&self) -> &str {
        "dict-mock"
    }

    fn convert_sentence(&self, reading: &str, _left: &str) -> Vec<BackendSegment> {
        self.segments(reading)
    }

    fn segment_candidates(&self, reading: &str, _: &str, limit: usize) -> Vec<BackendCandidate> {
        self.words
            .get(reading)
            .into_iter()
            .take(limit)
            .map(|s| BackendCandidate {
                surface: s.clone(),
                sources: vec![CandidateSource::SystemDict],
            })
            .collect()
    }

    fn sentence_candidates(&self, reading: &str, _: &str, limit: usize) -> Vec<BackendSentence> {
        if !self.sentence_nbest || limit == 0 {
            return Vec::new();
        }
        vec![
            BackendSentence {
                segments: self.segments(reading),
            },
            BackendSentence {
                segments: vec![BackendSegment {
                    reading: reading.to_string(),
                    surface: reading.to_string(),
                    sources: vec![CandidateSource::Kana],
                }],
            },
        ]
    }

    fn prefix_oracle(&self) -> Option<&dyn PrefixOracle> {
        Some(&self.oracle)
    }

    fn quality(&self, reading: &str) -> Option<ReadingQuality> {
        let (uncovered, conv_cost) = self.cost_of(reading);
        Some(ReadingQuality {
            uncovered,
            conv_cost,
        })
    }

    fn convert_hypotheses(
        &self,
        hyps: &[ReadingHypothesis],
        _: &str,
        limit: usize,
    ) -> Vec<ScoredSentence> {
        self.hypotheses_calls.fetch_add(1, Ordering::SeqCst);
        hyps.iter()
            .enumerate()
            .take(limit)
            .map(|(i, h)| ScoredSentence {
                segments: self.segments(&h.reading),
                conv_cost: self.cost_of(&h.reading).1,
                hypothesis_index: i,
            })
            .collect()
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, String)>>);

impl CommitObserver for Recorder {
    fn on_commit(&self, _: &CommitContext<'_>, segments: &[CommittedSegment]) {
        let mut v = self.0.lock().unwrap();
        for s in segments {
            v.push((s.reading.clone(), s.surface.clone()));
        }
    }

    fn on_forget(&self, _: &CommitContext<'_>, _: &str, _: &str) -> bool {
        false
    }
}

fn config(enabled: bool) -> EngineConfig {
    let mut c = EngineConfig::default();
    c.fuzzy.enabled = enabled;
    c
}

fn make(backend: Arc<DictMock>, cfg: EngineConfig) -> (Engine, Arc<Recorder>) {
    let mut e = Engine::new(backend, cfg);
    let rec = Arc::new(Recorder::default());
    e.set_commit_observer(rec.clone());
    (e, rec)
}

fn press(e: &mut Engine, key: &str) -> nagi_protocol::ProcessKeyResult {
    e.process_key(S, &parse_key(key).unwrap())
}

fn type_keys(e: &mut Engine, keys: &str) {
    for c in keys.chars() {
        press(e, &c.to_string());
    }
}

fn alt(text: &str, distance: f32) -> InputAlternative {
    InputAlternative {
        text: text.to_string(),
        distance,
    }
}

/// 代替付きで 1 キー打つ
fn tap(e: &mut Engine, key: &str, alts: &[InputAlternative]) {
    e.process_key_with_alternatives(S, &parse_key(key).unwrap(), alts);
}

/// `kansi`（「かんし」）を、s のキーに近い z を代替にして打つ。z ならば「かんじ」
fn type_kansi_with_alt(e: &mut Engine) {
    type_keys(e, "kan");
    tap(e, "s", &[alt("z", 0.2)]);
    type_keys(e, "i");
}

fn kanji_dict() -> Arc<DictMock> {
    Arc::new(DictMock::new(&[("かんじ", "漢字"), ("かんし", "監視")]))
}

fn candidate_surfaces(e: &Engine) -> Vec<String> {
    e.snapshot(S)
        .unwrap()
        .candidates
        .items
        .iter()
        .map(|c| c.surface.clone())
        .collect()
}

fn corrected(e: &Engine) -> Vec<(String, Option<String>)> {
    e.snapshot(S)
        .unwrap()
        .candidates
        .items
        .iter()
        .filter(|c| c.sources.contains(&CandidateSource::Corrected))
        .map(|c| (c.surface.clone(), c.note.clone()))
        .collect()
}

fn preedit_readings(e: &Engine) -> String {
    e.snapshot(S)
        .unwrap()
        .preedit
        .segments
        .iter()
        .map(|s| s.reading.clone())
        .collect()
}

fn preedit_text(e: &Engine) -> String {
    e.snapshot(S).unwrap().preedit.text
}

// ---- 補正候補が出る ----

#[test]
fn touch_alternative_offers_corrected_candidate_in_one_segment() {
    let (mut e, rec) = make(kanji_dict(), config(true));
    type_kansi_with_alt(&mut e);
    assert_eq!(preedit_text(&e), "かんし");
    press(&mut e, "Space");
    // 第一仮説（打たれたとおり）の変換が先頭のまま
    assert_eq!(preedit_text(&e), "監視");
    assert_eq!(preedit_readings(&e), "かんし");
    // 2 度目の Space で候補ウィンドウ。補正候補は通常の候補（かな・カナ含む）の後ろ
    press(&mut e, "Space");
    assert_eq!(candidate_surfaces(&e), ["監視", "かんし", "カンシ", "漢字"]);
    assert_eq!(corrected(&e), [("漢字".to_string(), Some("かんじ".into()))]);
    // 通常の候補の note は無い
    assert!(
        e.snapshot(S).unwrap().candidates.items[..3]
            .iter()
            .all(|c| c.note.is_none())
    );

    // 補正候補を選ぶと reading が書き換わる。通常の候補に戻すと元に戻る
    e.select_candidate(S, 3);
    assert_eq!(preedit_text(&e), "漢字");
    assert_eq!(preedit_readings(&e), "かんじ");
    e.select_candidate(S, 0);
    assert_eq!(preedit_text(&e), "監視");
    assert_eq!(preedit_readings(&e), "かんし");

    // 補正候補で確定すると、書き換えた reading で確定・通知される
    e.select_candidate(S, 3);
    let r = press(&mut e, "Enter");
    assert_eq!(r.commit.as_deref(), Some("漢字"));
    assert_eq!(
        rec.0.lock().unwrap().as_slice(),
        [("かんじ".to_string(), "漢字".to_string())]
    );
    // 確定取消で、書き換え後の reading が入力に戻る（元の reading への Undo は未対応）
    press(&mut e, "Ctrl+Backspace");
    assert_eq!(preedit_readings(&e), "かんじ");
}

#[test]
fn nihn_offers_nihon_when_composition_has_raw_literal() {
    // `nihn` の `h` は不一致なので Literal として残り、入力は 3 つの piece になる。
    // 複数文節なので、補正は文全体の候補（sentence_alternatives）に載る
    let backend = Arc::new(DictMock::new(&[("にほん", "日本"), ("に", "二")]));
    let (mut e, rec) = make(backend.clone(), config(true));
    let (mut off, _) = make(backend, config(false));
    type_keys(&mut e, "nihn");
    type_keys(&mut off, "nihn");
    // 末尾の n は保留（Space で「ん」に確定する）
    assert_eq!(preedit_text(&e), "にhn");
    press(&mut e, "Space");
    press(&mut off, "Space");
    // 第一仮説の表示は fuzzy 無効と同じ
    assert_eq!(preedit_text(&e), "二hん");
    assert_eq!(preedit_text(&e), off.snapshot(S).unwrap().preedit.text);
    assert!(corrected(&off).is_empty());

    press(&mut e, "Space");
    assert_eq!(corrected(&e), [("日本".to_string(), Some("にほん".into()))]);
    assert_eq!(candidate_surfaces(&e), ["二hん", "日本"]);
    assert_eq!(preedit_text(&e), "日本");
    let r = press(&mut e, "Enter");
    assert_eq!(r.commit.as_deref(), Some("日本"));
    assert_eq!(
        rec.0.lock().unwrap().as_slice(),
        [("にほん".to_string(), "日本".to_string())]
    );
    press(&mut e, "Ctrl+Backspace");
    assert_eq!(preedit_readings(&e), "にほん");
}

#[test]
fn first_hypothesis_commit_is_unchanged_when_not_choosing_correction() {
    let backend = Arc::new(DictMock::new(&[("にほん", "日本"), ("に", "二")]));
    let (mut e, rec) = make(backend.clone(), config(true));
    let (mut off, off_rec) = make(backend, config(false));
    for engine in [&mut e, &mut off] {
        type_keys(engine, "nihn");
        press(engine, "Space");
    }
    let (a, b) = (press(&mut e, "Enter"), press(&mut off, "Enter"));
    assert_eq!(a.commit, b.commit);
    assert_eq!(a.commit.as_deref(), Some("二hん"));
    assert_eq!(
        *rec.0.lock().unwrap(),
        *off_rec.0.lock().unwrap(),
        "学習に渡る読み・表記も同じ"
    );
}

#[test]
fn sentence_mode_adds_corrected_to_sentence_candidates() {
    let backend =
        Arc::new(DictMock::new(&[("かんじ", "漢字"), ("かんし", "監視")]).with_sentence_nbest());
    let (mut e, rec) = make(backend, config(true));
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    // 文全体の候補（既定の sentence 表示）。第一仮説が先頭、補正は末尾
    let snap = e.snapshot(S).unwrap();
    assert!(snap.candidates.visible);
    assert_eq!(candidate_surfaces(&e), ["監視", "かんし", "漢字"]);
    assert_eq!(corrected(&e), [("漢字".to_string(), Some("かんじ".into()))]);
    assert_eq!(snap.candidates.selected, 0);
    e.select_candidate(S, 2);
    let r = press(&mut e, "Enter");
    assert_eq!(r.commit.as_deref(), Some("漢字"));
    assert_eq!(
        rec.0.lock().unwrap().as_slice(),
        [("かんじ".to_string(), "漢字".to_string())]
    );
}

#[test]
fn kana_insert_with_alternative_offers_correction() {
    let backend = Arc::new(DictMock::new(&[("かん", "缶"), ("けん", "県")]));
    let (mut e, _) = make(backend, config(true));
    e.insert_reading(S, "か", &[alt("け", 0.2)]);
    e.insert_reading(S, "ん", &[]);
    press(&mut e, "Space");
    assert_eq!(preedit_text(&e), "缶");
    press(&mut e, "Space");
    assert_eq!(corrected(&e), [("県".to_string(), Some("けん".into()))]);
    e.select_candidate(S, 3);
    assert_eq!(preedit_readings(&e), "けん");
}

#[test]
fn promote_corrected_only_when_first_hypothesis_is_poor() {
    // 「かんし」は辞書に載らない（uncovered = 1.0）ので promote される
    let dict = || Arc::new(DictMock::new(&[("かんじ", "漢字")]));
    let mut cfg = config(true);
    cfg.fuzzy.promote_corrected = true;
    let (mut e, _) = make(dict(), cfg);
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    assert_eq!(preedit_text(&e), "漢字");
    assert_eq!(preedit_readings(&e), "かんじ");
    press(&mut e, "Space");
    assert_eq!(candidate_surfaces(&e)[0], "漢字");
    assert_eq!(corrected(&e).len(), 1);

    // promote が無効なら先頭は打たれたとおり
    let (mut e, _) = make(dict(), config(true));
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    assert_eq!(preedit_text(&e), "かんし");
    press(&mut e, "Space");
    assert_eq!(candidate_surfaces(&e).last().unwrap(), "漢字");

    // 第一仮説が良好なら promote でも先頭にしない（曖昧さだけで出た補正）
    let mut cfg = config(true);
    cfg.fuzzy.promote_corrected = true;
    let (mut e, _) = make(kanji_dict(), cfg);
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    assert_eq!(preedit_text(&e), "監視");
}

// ---- 走らせる条件 ----

#[test]
fn expand_is_not_called_for_good_unambiguous_input() {
    let backend = kanji_dict();
    let (mut e, _) = make(backend.clone(), config(true));
    type_keys(&mut e, "kanzi");
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert_eq!(
        backend.oracle.calls.load(Ordering::SeqCst),
        0,
        "expand 未呼び出し"
    );
    assert_eq!(backend.hypotheses_calls.load(Ordering::SeqCst), 0);
    assert!(corrected(&e).is_empty());
    press(&mut e, "Enter");

    // 代替が max_distance（既定 1.0）を超えるなら、曖昧さなしとして扱う
    type_keys(&mut e, "kan");
    tap(&mut e, "z", &[alt("s", 2.0)]);
    type_keys(&mut e, "i");
    press(&mut e, "Space");
    assert_eq!(backend.oracle.calls.load(Ordering::SeqCst), 0);
    assert!(corrected(&e).is_empty());
}

#[test]
fn ambiguity_alone_triggers_expand_even_if_first_is_good() {
    let backend = kanji_dict();
    let (mut e, _) = make(backend.clone(), config(true));
    // 「かんじ」（良好）を、z の代替 s で打つ。s の代替は近い（曖昧）ので「かんし」を補正候補に出す
    type_keys(&mut e, "kan");
    tap(&mut e, "z", &[alt("s", 0.2)]);
    type_keys(&mut e, "i");
    press(&mut e, "Space");
    assert_eq!(preedit_text(&e), "漢字");
    press(&mut e, "Space");
    assert!(backend.oracle.calls.load(Ordering::SeqCst) > 0);
    assert_eq!(backend.hypotheses_calls.load(Ordering::SeqCst), 1);
    assert_eq!(corrected(&e), [("監視".to_string(), Some("かんし".into()))]);
}

#[test]
fn corrections_far_worse_than_first_are_dropped_unless_first_is_poor() {
    // 補正仮説の変換コストが第一仮説より margin（1.5）を超えて悪いなら出さない
    let backend = Arc::new(DictMock::new(&[
        ("かんじ", "漢字"),
        ("か", "蚊"),
        ("ん", "ん"),
        ("し", "市"),
    ]));
    // 第一仮説 かんじ は 1 文節（cost 1）、補正 かんし は 3 文節（cost 3）で total が超える
    let run = |margin: f32| {
        let mut cfg = config(true);
        cfg.fuzzy.ambiguity_margin = margin;
        let (mut e, _) = make(backend.clone(), cfg);
        type_keys(&mut e, "kan");
        tap(&mut e, "z", &[alt("s", 0.2)]);
        type_keys(&mut e, "i");
        press(&mut e, "Space");
        press(&mut e, "Space");
        corrected(&e)
    };
    assert!(run(1.5).is_empty());
    // margin を広げれば出る（フィルタが効いていることの確認）
    assert_eq!(run(10.0).len(), 1);
}

#[test]
fn fuzzy_disabled_never_records_or_corrects() {
    let backend = kanji_dict();
    let (mut e, _) = make(backend.clone(), config(false));
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert_eq!(candidate_surfaces(&e), ["監視", "かんし", "カンシ"]);
    assert_eq!(backend.oracle.calls.load(Ordering::SeqCst), 0);
    assert_eq!(backend.hypotheses_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn backend_without_oracle_never_corrects() {
    use nagi_engine::MockBackend;
    let mut e = Engine::new(
        Arc::new(MockBackend::new().with_sentence("かんし", vec![("かんし", vec!["監視"])])),
        config(true),
    );
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    assert_eq!(preedit_text(&e), "監視");
    press(&mut e, "Space");
    assert!(corrected(&e).is_empty());
}

// ---- 打鍵ログの無効化 ----

/// 無効化のあとに `kansi`（z 代替付き）の s と i を打ち、補正が出ないことを確かめる
fn assert_no_correction_after(prefix: impl FnOnce(&mut Engine)) {
    // 大文字の直後の小文字もローマ字として扱う設定（literal モードに入らない）
    let mut cfg = config(true);
    cfg.shift_letter = nagi_romaji::ShiftLetterMode::SingleUpper;
    let (mut e, _) = make(kanji_dict(), cfg);
    prefix(&mut e);
    tap(&mut e, "s", &[alt("z", 0.2)]);
    type_keys(&mut e, "i");
    assert_eq!(preedit_text(&e).chars().last(), Some('し'));
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert!(corrected(&e).is_empty(), "{:?}", candidate_surfaces(&e));
}

#[test]
fn key_log_invalidated_by_cursor_move() {
    assert_no_correction_after(|e| {
        type_keys(e, "kan");
        press(e, "Left");
        press(e, "Right");
    });
}

#[test]
fn key_log_invalidated_by_mid_composition_edit() {
    // 文中への挿入（かんし → か[ん]し の位置に挿入）
    let (mut e, _) = make(kanji_dict(), config(true));
    type_keys(&mut e, "kai");
    press(&mut e, "Left");
    tap(&mut e, "n", &[]);
    tap(&mut e, "s", &[alt("z", 0.2)]);
    press(&mut e, "End");
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert!(corrected(&e).is_empty());
}

#[test]
fn key_log_invalidated_by_literal_mixture() {
    assert_no_correction_after(|e| {
        type_keys(e, "kan");
        press(e, "Shift+a");
    });
}

#[test]
fn key_log_invalidated_by_digit() {
    assert_no_correction_after(|e| {
        type_keys(e, "kan1");
    });
}

#[test]
fn key_log_invalidated_by_restore_session() {
    assert_no_correction_after(|e| {
        e.restore_session(
            S,
            &RestoreSessionParams {
                session_id: S.to_string(),
                segments: vec![RestoreSegment {
                    reading: "かん".into(),
                    surface: "かん".into(),
                    kind: SegmentKind::Japanese,
                }],
                cursor: 2,
                focus: None,
                converting: false,
            },
        );
    });
}

#[test]
fn key_log_invalidated_by_cancel_back_to_composing() {
    // 変換を取り消して Composing へ戻ると、入力は reading から再構築されるので打鍵ログは無い
    let (mut e, _) = make(kanji_dict(), config(true));
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    press(&mut e, "Escape");
    assert_eq!(preedit_text(&e), "かんし");
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert!(corrected(&e).is_empty());
}

#[test]
fn key_log_survives_backspace_and_rollback() {
    // 末尾の Backspace では有効のまま
    let (mut e, _) = make(kanji_dict(), config(true));
    type_kansi_with_alt(&mut e);
    type_keys(&mut e, "u");
    press(&mut e, "BackSpace");
    assert_eq!(preedit_text(&e), "かんし");
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert_eq!(corrected(&e).len(), 1);

    // G-49 の巻き戻し（`nihnz` → Backspace で z を取り消す）でも nihn の補正が出る
    let backend = Arc::new(DictMock::new(&[("にほん", "日本"), ("に", "二")]));
    let (mut e, _) = make(backend, config(true));
    type_keys(&mut e, "nihnz");
    press(&mut e, "BackSpace");
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert_eq!(corrected(&e).len(), 1, "{:?}", candidate_surfaces(&e));
}

#[test]
fn enabling_fuzzy_midway_waits_for_a_fresh_composition() {
    let (mut e, _) = make(kanji_dict(), config(false));
    type_keys(&mut e, "kan");
    e.update_config(config(true));
    tap(&mut e, "s", &[alt("z", 0.2)]);
    type_keys(&mut e, "i");
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert!(corrected(&e).is_empty());
    press(&mut e, "Enter");
    // 次の入力からは有効
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert_eq!(corrected(&e).len(), 1);
}

#[test]
fn hypotheses_limit_bounds_the_number_of_corrections() {
    let backend = Arc::new(DictMock::new(&[
        ("かんじ", "漢字"),
        ("かんし", "監視"),
        ("かんじー", "感じー"),
    ]));
    let mut cfg = config(true);
    cfg.fuzzy.search.max_hypotheses = 1; // 第一仮説のみ = 補正なし
    let (mut e, _) = make(backend, cfg);
    type_kansi_with_alt(&mut e);
    press(&mut e, "Space");
    press(&mut e, "Space");
    assert!(corrected(&e).is_empty());
}

// ---- 不変性（補正が出ない入力。quality 等の補正用 API が走る経路でも結果が変わらない） ----

#[test]
fn inputs_without_corrections_are_identical_with_and_without_fuzzy() {
    // 第一仮説が良好で曖昧さの無い入力（expand は呼ばれない）と、代替が付いても補正が
    // 辞書にない入力（expand は走るが補正は 0 件）。表示・確定は fuzzy 無効と同じ
    let inputs: Vec<Vec<(&str, Vec<InputAlternative>)>> = vec![
        vec![
            ("k", vec![]),
            ("a", vec![]),
            ("n", vec![]),
            ("z", vec![]),
            ("i", vec![]),
        ],
        vec![
            ("k", vec![alt("l", 0.3)]),
            ("a", vec![alt("s", 0.5)]),
            ("n", vec![]),
            ("z", vec![alt("x", 0.4)]),
            ("i", vec![]),
        ],
        vec![
            ("k", vec![]),
            ("a", vec![]),
            ("n", vec![]),
            ("BackSpace", vec![]),
            ("n", vec![]),
            ("z", vec![]),
            ("i", vec![]),
        ],
    ];
    for typed in inputs {
        let mut traces: Vec<Vec<String>> = Vec::new();
        for enabled in [false, true] {
            let (mut e, rec) = make(kanji_dict(), config(enabled));
            let mut trace = Vec::new();
            for (k, alts) in &typed {
                let r = e.process_key_with_alternatives(S, &parse_key(k).unwrap(), alts);
                trace.push(format!("{r:?}"));
            }
            for k in ["Space", "Space", "Enter"] {
                let r = press(&mut e, k);
                trace.push(format!("{r:?} {:?}", e.snapshot(S)));
            }
            trace.push(format!("{:?}", rec.0.lock().unwrap()));
            traces.push(trace);
        }
        assert_eq!(traces[0], traces[1]);
    }
}
