//! 入力中の候補（サジェスト）の拡充（H-58）: 読み全体 → 接頭辞を 1 文字ずつ短く → 1 文字の順に並ぶこと、
//! 総数が長さに応じて増えること、接頭辞の候補を選ぶと残りの読みが入力中に残ること、
//! 対応を宣言しないクライアントには出ないこと、上限

use nagi_engine::{
    BackendCandidate, BackendSegment, ConversionBackend, Engine, EngineConfig, KanaBackend,
    parse_key,
};
use nagi_protocol::methods::ReadingInput;
use nagi_protocol::view::{CandidateSource, CandidateView};
use std::sync::Arc;

/// どんな読みにも 2 件の候補（`<読み>甲` `<読み>乙`）を返すバックエンド
struct Gen;

impl ConversionBackend for Gen {
    fn name(&self) -> &str {
        "gen"
    }
    fn convert_sentence(&self, reading: &str, left: &str) -> Vec<BackendSegment> {
        KanaBackend.convert_sentence(reading, left)
    }
    fn segment_candidates(&self, reading: &str, _: &str, _: usize) -> Vec<BackendCandidate> {
        ["甲", "乙"]
            .iter()
            .map(|s| BackendCandidate {
                surface: format!("{reading}{s}"),
                sources: vec![CandidateSource::SystemDict],
                reading: None,
            })
            .collect()
    }
}

fn engine(backend: Arc<dyn ConversionBackend>, partial: bool, suggest_max: usize) -> Engine {
    let mut e = Engine::new(
        backend,
        EngineConfig {
            suggest_max,
            ..EngineConfig::default()
        },
    );
    e.set_partial_suggestions(partial);
    e
}

fn type_kana(e: &mut Engine, text: &str) {
    for c in text.chars() {
        e.insert_reading("s", &ReadingInput::Text(c.to_string()));
    }
}

/// 入力中の候補を作って返す（サーバーがデバウンス後にやること）
fn suggestions(e: &mut Engine) -> Vec<CandidateView> {
    let reading = e.composing_candidate_reading("s").expect("対象の読み");
    e.refresh_composing_candidates("s", &reading)
        .and_then(|r| r.candidates)
        .map(|c| c.items)
        .unwrap_or_default()
}

const READING: &str = "きょうはいいてんき";

#[test]
fn whole_then_shorter_prefixes_then_single_characters() {
    let mut e = engine(Arc::new(Gen), true, 120);
    type_kana(&mut e, READING);
    let items = suggestions(&mut e);
    // 読み全体 → 長い接頭辞から短い接頭辞へ（covers は単調に減る）→ 最後は 1 文字
    let covers: Vec<u32> = items
        .iter()
        .map(|i| i.covers.unwrap_or(READING.chars().count() as u32))
        .collect();
    assert!(covers.windows(2).all(|w| w[0] >= w[1]), "{covers:?}");
    assert_eq!(covers.first(), Some(&9));
    assert_eq!(covers.last(), Some(&1));
    for len in 1..=8u32 {
        assert!(covers.contains(&len), "len {len}: {covers:?}");
    }
    // 接頭辞の候補は、その接頭辞の候補
    let prefix8: String = READING.chars().take(8).collect();
    assert!(
        items
            .iter()
            .any(|i| i.covers == Some(8) && i.surface == format!("{prefix8}甲")),
        "{items:?}"
    );
    // 1 文字の読みの候補が末尾（き + 甲）
    assert_eq!(items.last().map(|i| i.surface.as_str()), Some("き乙"));
    // 同じ (表記, covers) は 1 度だけ
    for (i, a) in items.iter().enumerate() {
        assert!(
            items[i + 1..]
                .iter()
                .all(|b| !(a.surface == b.surface && a.covers == b.covers)),
            "重複: {}",
            a.surface
        );
    }
}

#[test]
fn count_grows_with_the_reading_length() {
    let count = |text: &str| {
        let mut e = engine(Arc::new(Gen), true, 120);
        type_kana(&mut e, text);
        suggestions(&mut e).len()
    };
    let (short, mid, long) = (count("きょう"), count("きょうは"), count(READING));
    assert!(short < mid && mid < long, "{short} {mid} {long}");
    // 宣言しないクライアントの候補は長さで増えない（従来どおりの少数）
    let mut legacy = engine(Arc::new(Gen), false, 120);
    type_kana(&mut legacy, READING);
    assert!(suggestions(&mut legacy).len() < long);
}

#[test]
fn clients_that_do_not_declare_support_get_no_prefix_only_candidates() {
    let mut e = engine(Arc::new(Gen), false, 120);
    type_kana(&mut e, READING);
    let items = suggestions(&mut e);
    assert!(!items.is_empty());
    assert!(items.iter().all(|i| i.covers.is_none()), "{items:?}");
}

#[test]
fn suggest_max_caps_the_total_and_keeps_the_single_character_tail() {
    let mut e = engine(Arc::new(Gen), true, 15);
    type_kana(&mut e, READING);
    let items = suggestions(&mut e);
    assert!(items.len() <= 15, "{}", items.len());
    // 途中の接頭辞で埋まっても、1 文字の候補は押し出されない
    assert_eq!(items.last().and_then(|i| i.covers), Some(1), "{items:?}");
}

#[test]
fn selecting_a_prefix_candidate_commits_it_and_keeps_the_rest_composing() {
    let mut e = engine(Arc::new(Gen), true, 120);
    type_kana(&mut e, READING);
    let items = suggestions(&mut e);
    let index = items
        .iter()
        .position(|i| i.covers == Some(4))
        .expect("4 文字の接頭辞の候補");
    let surface = items[index].surface.clone();
    let r = e.select_candidate("s", index as u32);
    assert_eq!(r.commit.as_deref(), Some(surface.as_str()));
    // 残りの読みは入力中のまま
    let rest: String = READING.chars().skip(4).collect();
    assert_eq!(e.snapshot("s").unwrap().preedit.text, rest);
    assert_eq!(
        e.snapshot("s").unwrap().state,
        nagi_engine::StateKind::Composing
    );
    // 残りの読みの候補を作り直せる（先頭は残りの読み全体）
    let next = suggestions(&mut e);
    assert!(next.iter().all(|i| i.covers.is_none_or(|c| c < 5)));
    // 続けて 1 文字の候補を選ぶと、さらに 1 文字ぶん確定して残りは減る
    let index = next.iter().position(|i| i.covers == Some(1)).unwrap();
    let r = e.select_candidate("s", index as u32);
    assert!(r.commit.is_some());
    let rest: String = READING.chars().skip(5).collect();
    assert_eq!(e.snapshot("s").unwrap().preedit.text, rest);
}

#[test]
fn selecting_the_whole_candidate_commits_everything() {
    let mut e = engine(Arc::new(Gen), true, 120);
    type_kana(&mut e, "きょうは");
    let items = suggestions(&mut e);
    let index = items
        .iter()
        .position(|i| i.covers.is_none() && i.surface == "きょうは甲")
        .unwrap();
    let r = e.select_candidate("s", index as u32);
    assert_eq!(r.commit.as_deref(), Some("きょうは甲"));
    assert_eq!(
        e.snapshot("s").unwrap().state,
        nagi_engine::StateKind::Direct
    );
}

#[test]
fn a_single_character_reading_has_no_prefix_items() {
    let mut e = engine(Arc::new(Gen), true, 120);
    type_kana(&mut e, "き");
    assert!(suggestions(&mut e).iter().all(|i| i.covers.is_none()));
}

#[test]
fn keys_still_work_with_the_expanded_list() {
    // 同期の応答（process_key）には候補の再計算を載せない（バックグラウンドで作る）
    let mut e = engine(Arc::new(Gen), true, 120);
    let r = e.process_key("s", &parse_key("k").unwrap());
    assert!(r.candidates.is_none());
}
