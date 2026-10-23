//! Literal（数字・英字・記号）の Space 変換の候補と、候補末尾の「形式の変換」のテスト

use nagi_engine::{Engine, EngineConfig, KanaBackend, MockBackend, key_from_text, parse_key};
use nagi_protocol::ProcessKeyResult;
use nagi_protocol::view::CandidateSource;
use std::sync::Arc;

fn press(e: &mut Engine, key: &str) -> ProcessKeyResult {
    // 記号・かななど、キー名にならない 1 文字はテキストとして送る
    let event = parse_key(key).unwrap_or_else(|_| key_from_text(key));
    e.process_key("s", &event)
}

fn type_str(e: &mut Engine, keys: &str) {
    for c in keys.chars() {
        press(e, &c.to_string());
    }
}

fn engine_with(backend: MockBackend) -> Engine {
    Engine::new(Arc::new(backend), EngineConfig::default())
}

fn surfaces(r: &ProcessKeyResult) -> Vec<String> {
    r.candidates
        .as_ref()
        .expect("候補の差分")
        .items
        .iter()
        .map(|c| c.surface.clone())
        .collect()
}

#[test]
fn literal_space_opens_window_with_rewriter_candidates() {
    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "1+2");
    let r = press(&mut e, "Space");
    let c = r.candidates.as_ref().expect("1 回目の Space で窓が開く");
    assert!(c.visible);
    // 元の表記が先頭、続けて電卓、末尾に全角
    assert_eq!(surfaces(&r), ["1+2", "3", "１＋２"]);
    assert_eq!(c.items[1].sources, [CandidateSource::Rewriter]);
    assert_eq!(r.preedit.as_ref().unwrap().text, "1+2");
    // 2 回目の Space で最初の Rewriter 候補を選ぶ
    let r = press(&mut e, "Space");
    assert_eq!(r.preedit.as_ref().unwrap().text, "3");
    let r = press(&mut e, "Enter");
    assert_eq!(r.commit.as_deref(), Some("3"));
}

#[test]
fn literal_candidates_for_symbol_and_alpha() {
    // ローマ字表にない ASCII 記号は Literal のまま入る: 記号の装飾が先、全角化は末尾（重複なし）
    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "$");
    let r = press(&mut e, "Space");
    assert!(r.candidates.as_ref().unwrap().visible);
    assert_eq!(surfaces(&r), ["$", "＄", "¥", "￥"]);

    // Shift+英字のあとの記号も Literal。複数文字には記号の装飾を出さない
    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "A(");
    let r = press(&mut e, "Space");
    assert_eq!(surfaces(&r), ["A(", "Ａ（"]);

    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "Ab");
    let r = press(&mut e, "Space");
    assert_eq!(surfaces(&r), ["Ab", "Ａｂ"]);
}

#[test]
fn romaji_paren_is_japanese_and_still_gets_symbol_candidates() {
    // ローマ字入力の `(` は全角の `（` になる（日本語扱い）。2 回目の Space で候補が開き、
    // 半角化と記号の装飾が続く
    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "(");
    press(&mut e, "Space");
    let r = press(&mut e, "Space");
    let s = surfaces(&r);
    assert_eq!(s[0], "（");
    assert_eq!(&s[1..3], ["(", "「"]);
}

#[test]
fn reading_kakko_offers_decorated_brackets() {
    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "kakko");
    press(&mut e, "Space");
    let r = press(&mut e, "Space");
    let s = surfaces(&r);
    assert_eq!(s[0], "かっこ");
    assert_eq!(&s[1..4], ["「」", "『』", "（）"]);
    // 末尾は形式の変換（既出のひらがなは重ねない）
    assert_eq!(&s[s.len() - 3..], ["カッコ", "ｋａｋｋｏ", "kakko"]);
}

#[test]
fn literal_without_rewriter_candidates_keeps_old_behavior() {
    // Rewriter が 1 件も出さなければ、窓は開かない
    let mut e = Engine::new(Arc::new(KanaBackend), EngineConfig::default());
    type_str(&mut e, "1+2");
    let r = press(&mut e, "Space");
    assert!(r.candidates.is_none() || !r.candidates.as_ref().unwrap().visible);
    assert_eq!(r.preedit.as_ref().unwrap().text, "1+2");
}

#[test]
fn mixed_input_offers_literal_candidates_on_focus() {
    let backend = MockBackend::new()
        .with_rewriter()
        .with_sentence("きょう", vec![("きょう", vec!["今日"])]);
    let mut e = engine_with(backend);
    type_str(&mut e, "kyou1+2");
    let r = press(&mut e, "Space");
    // 先頭の日本語文節では窓が開かない or 開いても Literal の候補ではない
    assert_eq!(r.preedit.as_ref().unwrap().text, "今日1+2");
    press(&mut e, "Right");
    let r = press(&mut e, "Space");
    assert_eq!(surfaces(&r), ["1+2", "3", "１＋２"]);
}

#[test]
fn japanese_segment_ends_with_hiragana_katakana_and_width_forms() {
    let backend = MockBackend::new().with_sentence("きょう", vec![("きょう", vec!["今日", "京"])]);
    let mut e = engine_with(backend);
    type_str(&mut e, "kyou");
    let r = press(&mut e, "Space");
    let r = if r.candidates.is_some() {
        r
    } else {
        press(&mut e, "Space")
    };
    let s = surfaces(&r);
    assert_eq!(s, ["今日", "京", "きょう", "キョウ", "ｋｙｏｕ", "kyou"]);
    let items = &r.candidates.as_ref().unwrap().items;
    assert_eq!(items[2].sources, [CandidateSource::Kana]);
    assert_eq!(items[4].sources, [CandidateSource::Rewriter]);
}

#[test]
fn width_forms_follow_the_romaji_of_each_segment() {
    // 文節に分かれても、その文節に対応するローマ字が使われる
    let backend = MockBackend::new().with_sentence(
        "きょうは",
        vec![("きょう", vec!["今日"]), ("は", vec!["は"])],
    );
    let mut e = Engine::new(
        Arc::new(backend),
        EngineConfig {
            initial_view: nagi_engine::InitialView::Segment,
            ..EngineConfig::default()
        },
    );
    type_str(&mut e, "kyouha");
    let r = press(&mut e, "Space");
    let r = if r.candidates.is_some() {
        r
    } else {
        press(&mut e, "Space")
    };
    let s = surfaces(&r);
    assert_eq!(&s[s.len() - 2..], ["ｋｙｏｕ", "kyou"]);
    press(&mut e, "Right");
    let r = press(&mut e, "Space");
    let s = surfaces(&r);
    assert_eq!(&s[s.len() - 2..], ["ｈａ", "ha"]);
}

#[test]
fn width_forms_are_omitted_when_romaji_is_unknown() {
    // かな直接入力など、ローマ字が分からない文節には全角英数・半角英数を出さない
    let backend = MockBackend::new().with_segments("きょう", vec!["今日"]);
    let mut e = engine_with(backend);
    type_str(&mut e, "きょう");
    let r = press(&mut e, "Space");
    let r = if r.candidates.is_some() {
        r
    } else {
        press(&mut e, "Space")
    };
    assert_eq!(surfaces(&r), ["きょう", "今日", "キョウ"]);
}

#[test]
fn form_candidates_survive_the_candidate_limit() {
    let many: Vec<String> = (0..80).map(|i| format!("候補{i}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let mut e = engine_with(MockBackend::new().with_segments("きょう", refs));
    type_str(&mut e, "kyou");
    let r = press(&mut e, "Space");
    let r = if r.candidates.is_some() {
        r
    } else {
        press(&mut e, "Space")
    };
    let s = surfaces(&r);
    assert!(s.len() <= 50, "{}", s.len());
    // 現在の表記（読みそのまま）は先頭にあるので、末尾に足されるのは残りの 3 つ
    assert_eq!(s[0], "きょう");
    assert_eq!(&s[s.len() - 3..], ["キョウ", "ｋｙｏｕ", "kyou"]);
}

#[test]
fn width_forms_are_found_in_mixed_input() {
    // 数字を挟んだ入力でも、日本語の文節にはその部分のローマ字が対応づく
    let backend = MockBackend::new().with_sentence("きょう", vec![("きょう", vec!["今日"])]);
    let mut e = engine_with(backend);
    type_str(&mut e, "1kyou");
    press(&mut e, "Space");
    press(&mut e, "Right");
    let r = press(&mut e, "Space");
    let s = surfaces(&r);
    assert_eq!(&s[s.len() - 2..], ["ｋｙｏｕ", "kyou"]);
}

#[test]
fn composing_suggestions_do_not_include_form_candidates() {
    let backend = MockBackend::new().with_segments("きょう", vec!["今日"]);
    let mut e = engine_with(backend);
    type_str(&mut e, "kyou");
    let r = press(&mut e, "Right");
    if let Some(c) = r.candidates {
        assert!(c.items.iter().all(|i| i.surface != "ｋｙｏｕ"));
    }
}

#[test]
fn paren_expression_is_one_literal_segment_with_calc_candidate() {
    // `(` `)` は全角（日本語扱い）になるが、数字と隣接する数式は 1 つの Literal にまとまり、電卓候補が出る
    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "(1+2)*3");
    let r = press(&mut e, "Space");
    assert!(r.candidates.as_ref().expect("候補").visible);
    let s = surfaces(&r);
    assert_eq!(s[0], "（1+2）*3");
    assert!(s.contains(&"9".to_string()), "{s:?}");
    assert_eq!(r.preedit.as_ref().unwrap().segments.len(), 1);
}

#[test]
fn paren_expression_commits_typed_text() {
    let mut e = engine_with(MockBackend::new().with_rewriter());
    type_str(&mut e, "(1+2)");
    press(&mut e, "Space");
    let r = press(&mut e, "Enter");
    assert_eq!(r.commit.as_deref(), Some("（1+2）"));
}
