//! 曖昧入力（タイプミス補正の「もしかして」候補、`input.fuzzy.*`）の統合テスト。

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::{Config, FuzzyStrength};
use nagi_engine::{BackendCandidate, ConversionBackend};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::view::CandidateSource;
use nagi_server::{Pipeline, Runtime, Tunables};

const EXTRA: &str = "\
かんじ\t1\t1\t3000\t感じ
かんじ\t1\t1\t3500\t漢字
がっこう\t1\t1\t3000\t学校
しんぶん\t1\t1\t3000\t新聞
かじ\t1\t1\t3200\t火事
ありがとう\t1\t1\t3000\tありがとう
";

fn backend_with(config: Config) -> Pipeline {
    backend_parts(config).0
}

fn backend_parts(
    config: Config,
) -> (
    Pipeline,
    std::sync::Arc<Runtime>,
    std::sync::Arc<nagi_dict::Dictionary>,
) {
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        false,
        Tunables::from_config(&config),
        Learner::new(LearnConfig::default()),
        None,
    );
    let dict = dict_with_extra(EXTRA);
    let pipeline = Pipeline::new(
        Some(dict.clone()),
        Some(Box::new(FakeModel::new(&[]))),
        runtime.clone(),
    );
    (pipeline, runtime, dict)
}

fn fuzzy_config(enabled: bool) -> Config {
    let mut config = Config::default();
    config.input.fuzzy.enabled = enabled;
    config
}

fn fuzzy_of(c: &[BackendCandidate]) -> Vec<&str> {
    c.iter()
        .filter(|c| c.sources.contains(&CandidateSource::Fuzzy))
        .map(|c| c.surface.as_str())
        .collect()
}

#[test]
fn typo_reading_gets_fuzzy_candidates_after_normal_ones() {
    let b = backend_with(fuzzy_config(true));
    // kamji: m は n の隣。「かんじ」の変換結果が Fuzzy の出所で、通常の候補の後ろに出る
    let c = b.segment_candidates("かmじ", "", 20);
    let fuzzy = fuzzy_of(&c);
    assert!(fuzzy.contains(&"感じ"), "{c:?}");
    let first_fuzzy = c
        .iter()
        .position(|c| c.sources.contains(&CandidateSource::Fuzzy))
        .unwrap();
    assert!(
        c[first_fuzzy..]
            .iter()
            .all(|c| c.sources == [CandidateSource::Fuzzy]),
        "補正候補は末尾にまとまる: {c:?}"
    );
    // 補正候補の表記は通常の候補と重ならない
    let mut surfaces: Vec<&str> = c.iter().map(|c| c.surface.as_str()).collect();
    surfaces.sort_unstable();
    surfaces.dedup();
    assert_eq!(surfaces.len(), c.len(), "{c:?}");
}

#[test]
fn clean_input_gets_no_fuzzy_candidates() {
    let b = backend_with(fuzzy_config(true));
    for reading in ["かんじ", "ありがとう", "がっこう", "しんぶん", "きょう"] {
        let c = b.segment_candidates(reading, "", 20);
        assert!(fuzzy_of(&c).is_empty(), "{reading}: {c:?}");
    }
}

#[test]
fn disabled_by_default_and_by_setting() {
    assert!(!Config::default().input.fuzzy.enabled);
    let b = backend_with(Config::default());
    assert!(fuzzy_of(&b.segment_candidates("かmじ", "", 20)).is_empty());
    assert!(b.fuzzy_candidates("かmじ", "", 5).is_empty());
    let b = backend_with(fuzzy_config(false));
    assert!(fuzzy_of(&b.segment_candidates("かmじ", "", 20)).is_empty());
}

#[test]
fn max_candidates_limits_the_count() {
    let mut config = fuzzy_config(true);
    config.input.fuzzy.max_candidates = 5;
    let b = backend_with(config.clone());
    // kamji → かんじ（置換）と かじ（余計な m の削除）
    let many = b.segment_candidates("かmじ", "", 20);
    assert!(fuzzy_of(&many).len() >= 2, "{many:?}");
    config.input.fuzzy.max_candidates = 1;
    let b = backend_with(config);
    let one = b.segment_candidates("かmじ", "", 20);
    assert_eq!(fuzzy_of(&one), ["感じ"], "{one:?}");
}

#[test]
fn strength_maps_to_the_cost_limit() {
    // gsjkou: s→a と j→k の 2 回の置換で gakkou（学校）。標準は届き、控えめは届かない
    let reading = "gsjこう";
    let normal = backend_with(fuzzy_config(true));
    assert!(
        fuzzy_of(&normal.segment_candidates(reading, "", 20)).contains(&"学校"),
        "{:?}",
        normal.segment_candidates(reading, "", 20)
    );
    let mut config = fuzzy_config(true);
    config.input.fuzzy.strength = FuzzyStrength::Gentle;
    let gentle = backend_with(config);
    assert!(fuzzy_of(&gentle.segment_candidates(reading, "", 20)).is_empty());
    // 1 回の置換なら控えめでも届く
    assert!(fuzzy_of(&gentle.segment_candidates("かmじ", "", 20)).contains(&"感じ"));
}

#[test]
fn kinds_restrict_the_edits() {
    let mut config = fuzzy_config(true);
    config.input.fuzzy.kinds = "transpose".to_string();
    let b = backend_with(config);
    assert!(fuzzy_of(&b.segment_candidates("かmじ", "", 20)).is_empty());
    // 転置: kajni → kanji
    let c = b.segment_candidates("かjに", "", 20);
    assert!(fuzzy_of(&c).contains(&"感じ"), "{c:?}");
}

#[test]
fn fuzzy_survives_the_candidate_limit_and_is_deterministic() {
    let b = backend_with(fuzzy_config(true));
    let c = b.segment_candidates("かmじ", "", 1);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].sources, [CandidateSource::Fuzzy], "{c:?}");
    let again = b.segment_candidates("かmじ", "", 1);
    assert_eq!(c, again);
    let a = b.fuzzy_candidates("かmじ", "", 5);
    assert_eq!(a, b.fuzzy_candidates("かmじ", "", 5));
    assert!(a.iter().all(|c| c.sources == [CandidateSource::Fuzzy]));
    assert!(!a.is_empty());
}

#[test]
fn user_dictionary_reading_is_not_corrected() {
    let (b, runtime, dict) = backend_parts(fuzzy_config(true));
    assert!(!fuzzy_of(&b.segment_candidates("かmじ", "", 20)).is_empty());
    let mut user = nagi_dict::UserDictionary::default();
    let mut issues = Vec::new();
    user.add_tsv("t", "かmじ\tケーエム\t名詞\t\n", &mut issues);
    assert!(issues.is_empty(), "{issues:?}");
    runtime.set_overlay(dict.make_overlay(&user));
    // 利用者が登録した読みは意図した入力とみなす
    let c = b.segment_candidates("かmじ", "", 20);
    assert!(fuzzy_of(&c).is_empty(), "{c:?}");
    assert!(c.iter().any(|c| c.surface == "ケーエム"));
}

// ---- エンジン経由（入力中の候補・Space 変換） ----

mod engine_flow {
    use super::*;
    use nagi_engine::parse_key;
    use nagi_server::Server;
    use nagi_server::setup::{Built, Reloadable};
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn send(server: &mut Server, method: &str, params: Value) -> Value {
        let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
        let mut lines = server.handle_line(&line).into_iter();
        serde_json::from_str(&lines.next().expect("応答")).unwrap()
    }

    fn press(server: &mut Server, key: &str) -> Value {
        let k = parse_key(key).unwrap();
        send(
            server,
            "process_key",
            json!({"session_id":"s","keysym":k.keysym,"keycode":0,"text":k.text,
                   "modifiers":{"shift":k.modifiers.shift,"ctrl":k.modifiers.ctrl,"alt":k.modifiers.alt,"meta":k.modifiers.meta},
                   "kind":"down"}),
        )["result"]
            .clone()
    }

    fn server(config: Config) -> Server {
        let (pipeline, runtime, dict) = backend_parts(config.clone());
        let mut reload = Reloadable::detached();
        reload.dict = Some(dict);
        let mut s = Server::from_built(Built {
            pipeline: Arc::new(pipeline),
            runtime,
            config,
            reload,
            notes: Vec::new(),
        });
        send(
            &mut s,
            "initialize",
            json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
        );
        s
    }

    fn sources_of(r: &Value) -> Vec<(String, Vec<String>)> {
        r["candidates"]["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|i| {
                        (
                            i["surface"].as_str().unwrap().to_string(),
                            i["sources"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|s| s.as_str().unwrap().to_string())
                                .collect(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn space_conversion_lists_the_fuzzy_candidate() {
        let mut s = server(fuzzy_config(true));
        for c in "kamji".chars() {
            press(&mut s, &c.to_string());
        }
        let r = press(&mut s, "Space");
        let items = sources_of(&r);
        eprintln!("{items:?} preedit={}", r["preedit"]);
        assert!(
            items
                .iter()
                .any(|(surface, src)| surface == "感じ" && src == &["fuzzy"]),
            "{items:?}"
        );
    }

    #[test]
    fn typing_keeps_the_typed_reading() {
        // 補正は候補にだけ混ざり、入力中の読みは打った通りのまま
        let mut s = server(fuzzy_config(true));
        let mut last = Value::Null;
        for c in "kamji".chars() {
            last = press(&mut s, &c.to_string());
        }
        assert_eq!(last["preedit"]["text"], "かmじ", "{last}");
    }
}

mod composing {
    use super::*;
    use nagi_engine::{Engine, EngineConfig, parse_key};
    use nagi_protocol::view::CandidateSource;
    use std::sync::Arc;

    fn engine(fuzzy_input: bool) -> Engine {
        let pipeline = backend_with(fuzzy_config(true));
        Engine::new(
            Arc::new(pipeline),
            EngineConfig {
                fuzzy_input,
                ..EngineConfig::default()
            },
        )
    }

    fn type_romaji(e: &mut Engine, romaji: &str) {
        for c in romaji.chars() {
            e.process_key("s", &parse_key(&c.to_string()).unwrap());
        }
    }

    #[test]
    fn typo_input_gets_composing_candidates_with_fuzzy() {
        let mut e = engine(true);
        type_romaji(&mut e, "kamji");
        assert_eq!(e.composing_candidate_reading("s").as_deref(), Some("かmじ"));
        let r = e.refresh_composing_candidates("s", "かmじ").expect("候補");
        let items = r.candidates.expect("candidates").items;
        let fuzzy = items
            .iter()
            .find(|i| i.surface == "感じ")
            .unwrap_or_else(|| panic!("{items:?}"));
        assert_eq!(fuzzy.sources, [CandidateSource::Fuzzy]);
        assert_eq!(fuzzy.covers, None, "読み全体を置き換える");
        // 入力中の読みは打った通りのまま
        assert_eq!(e.snapshot("s").unwrap().preedit.text, "かmじ");
    }

    #[test]
    fn typo_input_without_fuzzy_has_no_composing_candidates() {
        let mut e = engine(false);
        type_romaji(&mut e, "kamji");
        assert_eq!(e.composing_candidate_reading("s"), None);
    }

    #[test]
    fn clean_input_has_no_fuzzy_in_composing_candidates() {
        let mut e = engine(true);
        type_romaji(&mut e, "kanji");
        let reading = e.composing_candidate_reading("s").expect("読み");
        assert_eq!(reading, "かんじ");
        let r = e.refresh_composing_candidates("s", &reading).expect("候補");
        let items = r.candidates.expect("candidates").items;
        assert!(
            items
                .iter()
                .all(|i| !i.sources.contains(&CandidateSource::Fuzzy)),
            "{items:?}"
        );
    }
}
