//! 英語の自動判定（段階 1）: Space の候補に、打鍵列の ASCII を足す・上位に置く

use nagi_engine::{
    AutoEnglishMode, BackendCandidate, BackendSegment, ConversionBackend, Engine, EngineConfig,
    InitialView, KanaBackend, MockBackend, parse_key,
};
use nagi_protocol::ProcessKeyResult;
use std::sync::Arc;

/// `english_level` を固定できるバックエンド（残りは Mock に任せる）
struct Fixed {
    inner: MockBackend,
    level: Option<nagi_english::Level>,
    spellings: Vec<String>,
}

impl ConversionBackend for Fixed {
    fn name(&self) -> &str {
        "fixed"
    }
    fn convert_sentence(&self, reading: &str, left: &str) -> Vec<BackendSegment> {
        self.inner.convert_sentence(reading, left)
    }
    fn segment_candidates(&self, reading: &str, left: &str, limit: usize) -> Vec<BackendCandidate> {
        self.inner.segment_candidates(reading, left, limit)
    }
    fn english_level(&self, raw: &str, reading: &str) -> nagi_english::Level {
        match self.level {
            Some(l) => l,
            None => KanaBackend.english_level(raw, reading),
        }
    }
    fn english_spellings(&self, _raw: &str, _reading: &str, limit: usize) -> Vec<String> {
        self.spellings.iter().take(limit).cloned().collect()
    }
}

fn engine(
    level: Option<nagi_english::Level>,
    mode: AutoEnglishMode,
    view: InitialView,
    reading: &str,
) -> Engine {
    engine_with_spellings(level, mode, view, reading, &[])
}

fn engine_with_spellings(
    level: Option<nagi_english::Level>,
    mode: AutoEnglishMode,
    view: InitialView,
    reading: &str,
    spellings: &[&str],
) -> Engine {
    let inner = MockBackend::new()
        .with_segments(
            reading,
            vec!["漢字甲", "漢字乙", "漢字丙", "漢字丁", "漢字戊"],
        )
        .with_sentence(
            reading,
            vec![(reading, vec!["漢字甲", "漢字乙", "漢字丙", "漢字丁"])],
        );
    Engine::new(
        Arc::new(Fixed {
            inner,
            level,
            spellings: spellings.iter().map(|s| s.to_string()).collect(),
        }),
        EngineConfig {
            auto_english: mode,
            initial_view: view,
            apps: nagi_engine::AppsConfig {
                passthrough_chars: vec!['/'],
                ..Default::default()
            },
            ..EngineConfig::default()
        },
    )
}

fn type_keys(e: &mut Engine, keys: &str) -> ProcessKeyResult {
    let mut last = None;
    for c in keys.chars() {
        last = Some(e.process_key("s", &parse_key(&c.to_string()).unwrap()));
    }
    last.unwrap()
}

/// 入力して Space を押し、候補の表記の並びを返す（窓が出るまで Space を重ねる。出なければ `None`）
fn try_candidates_after_space(e: &mut Engine, keys: &str) -> Option<Vec<String>> {
    type_keys(e, keys);
    for _ in 0..2 {
        let r = e.process_key("s", &parse_key("Space").unwrap());
        if let Some(c) = r.candidates
            && c.visible
        {
            return Some(c.items.into_iter().map(|i| i.surface).collect());
        }
    }
    None
}

fn candidates_after_space(e: &mut Engine, keys: &str) -> Vec<String> {
    try_candidates_after_space(e, keys).expect("候補ウィンドウが出ませんでした")
}

fn position(items: &[String], s: &str) -> usize {
    items
        .iter()
        .position(|i| i == s)
        .unwrap_or_else(|| panic!("{s} が無い: {items:?}"))
}

// `typescript` を打ったときの読み（ローマ字で変換できなかった ASCII が混ざる）
fn reading_of(keys: &str) -> String {
    let mut e = Engine::new(Arc::new(KanaBackend), EngineConfig::default());
    type_keys(&mut e, keys);
    // 確定すると、入力中の読み（保留のローマ字を含む）がそのまま出る
    e.process_key("s", &parse_key("Enter").unwrap())
        .commit
        .unwrap()
}

#[test]
fn high_confidence_puts_ascii_second_in_segment_view() {
    let reading = reading_of("typescript");
    let mut e = engine(
        Some(nagi_english::Level::High),
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
    );
    let items = candidates_after_space(&mut e, "typescript");
    // 先頭は今の変換結果のまま（自動では変えない）。ASCII はその次
    assert_eq!(position(&items, "typescript"), 1, "{items:?}");
    assert_ne!(items[0], "typescript");
}

#[test]
fn high_confidence_puts_ascii_second_in_sentence_view() {
    let reading = reading_of("typescript");
    let mut e = engine(
        Some(nagi_english::Level::High),
        AutoEnglishMode::Conservative,
        InitialView::Sentence,
        &reading,
    );
    let items = candidates_after_space(&mut e, "typescript");
    assert_eq!(position(&items, "typescript"), 1, "{items:?}");
    assert_ne!(items[0], "typescript");
}

#[test]
fn sentence_alternatives_stay_aligned_after_moving() {
    // 動かした候補を選ぶと、その表記で確定する（並びの対応がずれていない）
    let reading = reading_of("typescript");
    let mut e = engine(
        Some(nagi_english::Level::High),
        AutoEnglishMode::Conservative,
        InitialView::Sentence,
        &reading,
    );
    candidates_after_space(&mut e, "typescript");
    e.process_key("s", &parse_key("Space").unwrap()); // 2 番目 = typescript
    let r = e.process_key("s", &parse_key("Enter").unwrap());
    assert_eq!(r.commit.as_deref(), Some("typescript"));
}

#[test]
fn mid_confidence_places_ascii_behind_main_candidates() {
    let reading = reading_of("typescript");
    let mut e = engine(
        Some(nagi_english::Level::Mid),
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
    );
    let items = candidates_after_space(&mut e, "typescript");
    assert_eq!(position(&items, "typescript"), 3, "{items:?}");
}

#[test]
fn low_confidence_and_off_do_not_promote_ascii() {
    let reading = reading_of("typescript");
    for (level, mode) in [
        (nagi_english::Level::Low, AutoEnglishMode::Conservative),
        (nagi_english::Level::None, AutoEnglishMode::Conservative),
        (nagi_english::Level::High, AutoEnglishMode::Off),
    ] {
        let mut e = engine(Some(level), mode, InitialView::Segment, &reading);
        // 従来どおりの変換（文節に割れる）。全体の ASCII が上位の候補に出ることはない
        let items = try_candidates_after_space(&mut e, "typescript").unwrap_or_default();
        assert!(
            items
                .iter()
                .position(|i| i == "typescript")
                .is_none_or(|p| p >= 5),
            "{level:?} {mode:?} {items:?}"
        );
    }
}

/// 有効なローマ字になる英単語（`house` → ほうせ）。1 つの日本語の文節なので、候補の末尾の
/// 「形式の変換」にあった ASCII を動かす
#[test]
fn valid_romaji_word_is_promoted_in_both_views() {
    let reading = reading_of("house");
    assert_eq!(reading, "ほうせ");
    for view in [InitialView::Segment, InitialView::Sentence] {
        let mut e = engine(
            Some(nagi_english::Level::High),
            AutoEnglishMode::Conservative,
            view,
            &reading,
        );
        let items = candidates_after_space(&mut e, "house");
        assert_eq!(position(&items, "house"), 1, "{view:?} {items:?}");
        assert_ne!(items[0], "house");
    }
    // 判定が弱ければ末尾のまま
    let mut e = engine(
        Some(nagi_english::Level::None),
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
    );
    let items = candidates_after_space(&mut e, "house");
    assert!(position(&items, "house") >= 5, "{items:?}");
}

#[test]
fn reserved_modes_behave_like_conservative() {
    let reading = reading_of("typescript");
    for mode in [AutoEnglishMode::Standard, AutoEnglishMode::Aggressive] {
        let mut e = engine(
            Some(nagi_english::Level::High),
            mode,
            InitialView::Segment,
            &reading,
        );
        let items = candidates_after_space(&mut e, "typescript");
        assert_eq!(position(&items, "typescript"), 1, "{mode:?}");
    }
}

#[test]
fn default_backend_without_lexicon_uses_features_only() {
    // 語彙なしの既定判定でも、かなにならない ASCII が多い入力は上位に出る
    let reading = reading_of("typescript");
    let mut e = engine(
        None,
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
    );
    let items = candidates_after_space(&mut e, "typescript");
    assert!(position(&items, "typescript") <= 3, "{items:?}");
    // 日本語のローマ字は動かさない
    let reading = reading_of("kyouha");
    let mut e = engine(
        None,
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
    );
    let items = candidates_after_space(&mut e, "kyouha");
    assert!(position(&items, "kyouha") >= 5, "{items:?}");
}

#[test]
fn passthrough_literal_is_not_touched() {
    // `/help`（素通し）は Literal。判定の対象外で、Space は語の確定
    let reading = reading_of("typescript");
    let mut e = engine(
        Some(nagi_english::Level::High),
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
    );
    type_keys(&mut e, "/help");
    let r = e.process_key("s", &parse_key("Space").unwrap());
    assert_eq!(r.commit.as_deref(), Some("/help"));
    assert!(r.candidates.is_none());
}

#[test]
fn spelling_suggestions_follow_main_candidates_as_fuzzy() {
    // 綴りの「もしかして」: 英語らしさが中程度なら、主要な候補の後ろ・形式の変換の前に出る
    let reading = reading_of("typscript");
    let mut e = engine_with_spellings(
        Some(nagi_english::Level::Mid),
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
        &["typescript"],
    );
    let items = candidates_after_space(&mut e, "typscript");
    let fix = position(&items, "typescript");
    assert!(fix >= 1, "{items:?}");
    // 形式の変換（カタカナ・全角英数）より前
    assert!(fix < position(&items, "ｔｙｐｓｃｒｉｐｔ"), "{items:?}");
    assert_ne!(items[0], "typescript");
    // 選ぶと綴りを直した語で確定する
    for _ in 0..fix {
        e.process_key("s", &parse_key("Space").unwrap());
    }
    let r = e.process_key("s", &parse_key("Enter").unwrap());
    assert_eq!(r.commit.as_deref(), Some("typescript"));
}

#[test]
fn spelling_suggestions_open_the_window_even_when_english_is_unlikely() {
    // 英語らしさが弱くても、綴りの候補があれば 1 文節にして出す。先頭は変えない
    let reading = reading_of("wprld");
    let mut e = engine_with_spellings(
        Some(nagi_english::Level::None),
        AutoEnglishMode::Conservative,
        InitialView::Segment,
        &reading,
        &["world", "would"],
    );
    let items = candidates_after_space(&mut e, "wprld");
    assert_ne!(items[0], "world", "{items:?}");
    assert!(position(&items, "world") < position(&items, "would"));
}

#[test]
fn spelling_suggestions_are_off_with_auto_english_off() {
    let reading = reading_of("typscript");
    let mut e = engine_with_spellings(
        Some(nagi_english::Level::Mid),
        AutoEnglishMode::Off,
        InitialView::Segment,
        &reading,
        &["typescript"],
    );
    let items = try_candidates_after_space(&mut e, "typscript").unwrap_or_default();
    assert!(!items.iter().any(|i| i == "typescript"), "{items:?}");
}
