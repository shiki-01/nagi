//! 曖昧入力（タイプミス補正の「もしかして」候補、`input.fuzzy.*`）の統合テスト。

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::{Config, FuzzyStrength};
use nagi_engine::{BackendCandidate, ConversionBackend, TypingHint};
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
    assert!(
        b.fuzzy_candidates("かmじ", &TypingHint::default(), "", 5)
            .is_empty()
    );
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
    let a = b.fuzzy_candidates("かmじ", &TypingHint::default(), "", 5);
    assert_eq!(
        a,
        b.fuzzy_candidates("かmじ", &TypingHint::default(), "", 5)
    );
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
    fn json_rpc_accepts_geometry_touch_and_the_old_insert_reading_shape() {
        let mut s = server(fuzzy_config(true));
        // set_layout_geometry は空の結果を返す（英字キーが足りない配置は無視される）
        let r = send(
            &mut s,
            "set_layout_geometry",
            json!({"layout_id":"k12","mode":"kana12","width":1.0,"height":1.0,
                   "keys":[{"id":"a","label":"あ","x":0.0,"y":0.0,"w":0.3,"h":0.2}]}),
        );
        assert_eq!(r["result"], json!({}), "{r}");
        // 座標なしの従来の形（InsertReadingParams）と、座標つきの形のどちらも受け付ける
        for (i, ch) in "kamji".chars().enumerate() {
            let mut params =
                json!({"session_id":"s","input":{"type":"text","content":ch.to_string()}});
            if i == 2 {
                params["touch"] = json!({"x":0.02,"y":0.5});
            }
            let r = send(&mut s, "insert_reading", params);
            assert_eq!(r["result"]["consumed"], true, "{r}");
        }
        let r = send(
            &mut s,
            "insert_reading",
            json!({"session_id":"s","input":{"type":"key12","content":{"key":"a","gesture":"flick_left"}},
                   "flick":{"angle_deg":135.0,"distance":0.8}}),
        );
        assert_eq!(r["result"]["consumed"], true, "{r}");
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

// ---- M2: 実際の打鍵列・タッチ座標・キー矩形・補正後の読み ----

mod m2 {
    use super::*;
    use nagi_engine::{Engine, EngineConfig, parse_key};
    use nagi_protocol::methods::{LayoutKey, ReadingInput, SetLayoutGeometryParams, TouchPoint};
    use nagi_server::learning::LearningObserver;
    use std::sync::Arc;

    fn engine_with(config: Config) -> (Engine, Arc<Runtime>) {
        let (pipeline, runtime, _) = backend_parts(config);
        let mut e = Engine::new(
            Arc::new(pipeline),
            EngineConfig {
                fuzzy_input: true,
                ..EngineConfig::default()
            },
        );
        e.set_commit_observer(Arc::new(LearningObserver::new(runtime.clone())));
        (e, runtime)
    }

    fn engine() -> (Engine, Arc<Runtime>) {
        engine_with(fuzzy_config(true))
    }

    fn type_romaji(e: &mut Engine, romaji: &str) {
        for c in romaji.chars() {
            e.process_key("s", &parse_key(&c.to_string()).unwrap());
        }
    }

    /// 入力中の候補（読み、補正候補の（表記, 補正後の読み））
    fn composing(e: &mut Engine) -> Vec<(String, Option<String>)> {
        let Some(reading) = e.composing_candidate_reading("s") else {
            return Vec::new();
        };
        let Some(r) = e.refresh_composing_candidates("s", &reading) else {
            return Vec::new();
        };
        r.candidates
            .expect("candidates")
            .items
            .into_iter()
            .filter(|i| i.sources.contains(&CandidateSource::Fuzzy))
            .map(|i| (i.surface, i.reading))
            .collect()
    }

    fn key(id: &str, x: f64, y: f64) -> LayoutKey {
        LayoutKey {
            id: id.into(),
            label: id.to_uppercase(),
            x,
            y,
            w: 36.0,
            h: 54.0,
        }
    }

    /// QWERTY の 3 段（隙間つきの実寸）に、`moved` のキーだけ別の場所へ置いたもの
    fn geometry(moved: &[(&str, f64, f64)]) -> SetLayoutGeometryParams {
        let mut keys = Vec::new();
        for (row, (letters, offset)) in
            [("qwertyuiop", 0.0), ("asdfghjkl", 0.25), ("zxcvbnm", 0.75)]
                .iter()
                .enumerate()
        {
            for (col, c) in letters.chars().enumerate() {
                let id = c.to_string();
                let (x, y) = moved
                    .iter()
                    .find(|(m, _, _)| *m == id)
                    .map(|&(_, x, y)| (x, y))
                    .unwrap_or(((offset + col as f64) * 42.0, row as f64 * 64.0));
                keys.push(key(&id, x, y));
            }
        }
        // 英字以外のキーは無視される
        keys.push(LayoutKey {
            id: "shift".into(),
            label: "shift".into(),
            x: 0.0,
            y: 192.0,
            w: 40.0,
            h: 54.0,
        });
        SetLayoutGeometryParams {
            session_id: None,
            layout_id: "test".into(),
            mode: "qwerty".into(),
            keys,
            width: 420.0,
            height: 256.0,
        }
    }

    #[test]
    fn fuzzy_candidate_carries_the_corrected_reading() {
        let b = backend_with(fuzzy_config(true));
        let c = b.fuzzy_candidates("かmじ", &TypingHint::default(), "", 5);
        let kanji = c.iter().find(|c| c.surface == "感じ").expect("感じ");
        assert_eq!(kanji.reading.as_deref(), Some("かんじ"));
        let seg = b.segment_candidates("かmじ", "", 20);
        let kanji = seg.iter().find(|c| c.surface == "感じ").expect("感じ");
        assert_eq!(kanji.reading.as_deref(), Some("かんじ"));
        // 通常の候補に読みは付かない
        assert!(
            seg.iter()
                .filter(|c| c.sources != [CandidateSource::Fuzzy])
                .all(|c| c.reading.is_none())
        );
        let (mut e, _) = engine();
        type_romaji(&mut e, "kamji");
        let items = composing(&mut e);
        assert!(
            items.contains(&("感じ".to_string(), Some("かんじ".to_string()))),
            "{items:?}"
        );
    }

    #[test]
    fn selecting_a_fuzzy_candidate_commits_and_learns_the_corrected_reading() {
        let (mut e, runtime) = engine();
        type_romaji(&mut e, "kamji");
        let reading = e.composing_candidate_reading("s").unwrap();
        let r = e.refresh_composing_candidates("s", &reading).unwrap();
        let items = r.candidates.unwrap().items;
        // 火事 = m を削った「かじ」の補正（通常の変換では出ない）
        let index = items
            .iter()
            .position(|i| i.surface == "火事")
            .expect("火事");
        assert_eq!(items[index].reading.as_deref(), Some("かじ"));
        let r = e.select_candidate("s", index as u32);
        assert_eq!(r.commit.as_deref(), Some("火事"));
        // 補正後の読みで学習される（打った通りの読みではない）
        assert!(
            runtime
                .learn_lookup("かじ", 0)
                .iter()
                .any(|(s, _)| s == "火事"),
            "学習されていない"
        );
        assert!(runtime.learn_lookup("かmじ", 0).is_empty());
    }

    #[test]
    fn selecting_in_space_conversion_learns_the_corrected_reading() {
        let (mut e, runtime) = engine();
        type_romaji(&mut e, "kamji");
        let r = e.process_key("s", &parse_key("Space").unwrap());
        let items = r.candidates.unwrap().items;
        let index = items
            .iter()
            .position(|i| i.surface == "火事")
            .expect("火事");
        assert_eq!(items[index].reading.as_deref(), Some("かじ"));
        e.select_candidate("s", index as u32);
        let r = e.process_key("s", &parse_key("Enter").unwrap());
        assert_eq!(r.commit.as_deref(), Some("火事"));
        assert!(
            runtime
                .learn_lookup("かじ", 0)
                .iter()
                .any(|(s, _)| s == "火事")
        );
    }

    #[test]
    fn layout_geometry_derives_adjacency_for_non_qwerty_layouts() {
        // 内蔵の QWERTY では q は n の隣ではないので、kaqji の補正（q→n）は出ない
        let (mut e, _) = engine();
        type_romaji(&mut e, "kaqji");
        assert!(composing(&mut e).iter().all(|(s, _)| s != "感じ"));
        // q を n の真下へ置いたレイアウトを渡すと、隣接が導出されて補正される
        let (mut e, _) = engine();
        e.set_layout_geometry(&geometry(&[("q", 5.75 * 42.0, 3.0 * 64.0)]));
        type_romaji(&mut e, "kaqji");
        let items = composing(&mut e);
        assert!(items.iter().any(|(s, _)| s == "感じ"), "{items:?}");
        // 逆に m を遠くへ動かすと、kamji の m→n の補正は出なくなる
        let (mut e, _) = engine();
        e.set_layout_geometry(&geometry(&[("m", 400.0, 0.0)]));
        type_romaji(&mut e, "kamji");
        assert!(composing(&mut e).iter().all(|(s, _)| s != "感じ"));
        // 12 キーなど英字キーの少ないレイアウトは無視される（内蔵の QWERTY のまま）
        let (mut e, _) = engine();
        let mut kana = geometry(&[]);
        kana.keys.truncate(5);
        e.set_layout_geometry(&kana);
        type_romaji(&mut e, "kamji");
        assert!(composing(&mut e).iter().any(|(s, _)| s == "感じ"));
    }

    fn tap(e: &mut Engine, c: char, touch: Option<(f64, f64)>) {
        e.insert_reading_touch(
            "s",
            &ReadingInput::Text(c.to_string()),
            touch.map(|(x, y)| TouchPoint { x, y }),
            None,
        );
    }

    fn type_with_m_touch(touch: Option<(f64, f64)>) -> Vec<(String, Option<String>)> {
        let (mut e, _) = engine();
        for c in "ka".chars() {
            tap(&mut e, c, Some((0.5, 0.5)));
        }
        tap(&mut e, 'm', touch);
        for c in "ji".chars() {
            tap(&mut e, c, Some((0.5, 0.5)));
        }
        composing(&mut e)
    }

    #[test]
    fn touch_near_the_boundary_surfaces_the_neighbor_correction() {
        // m の左端（n との境界）を押した: 本来は n だった可能性が高い
        let edge = type_with_m_touch(Some((0.02, 0.5)));
        assert!(edge.iter().any(|(s, _)| s == "感じ"), "{edge:?}");
        // 座標なし（M1 の離散コスト）は補正される
        let none = type_with_m_touch(None);
        assert!(none.iter().any(|(s, _)| s == "感じ"), "{none:?}");
    }

    #[test]
    fn use_touch_coords_false_ignores_coordinates() {
        let mut config = fuzzy_config(true);
        config.input.fuzzy.use_touch_coords = false;
        let (mut e, _) = engine_with(config);
        for c in "ka".chars() {
            tap(&mut e, c, Some((0.5, 0.5)));
        }
        tap(&mut e, 'm', Some((0.5, 0.5)));
        for c in "ji".chars() {
            tap(&mut e, c, Some((0.5, 0.5)));
        }
        // 中央を押しても、座標を使わないので離散コストで補正される
        assert!(composing(&mut e).iter().any(|(s, _)| s == "感じ"));
    }

    #[test]
    fn actual_strokes_follow_edits_and_equal_physical_typing_without_coordinates() {
        // 座標なしの insert_reading と物理キーの入力は、同じ結果
        let (mut a, _) = engine();
        type_romaji(&mut a, "kamji");
        let (mut b, _) = engine();
        for c in "kamji".chars() {
            tap(&mut b, c, None);
        }
        assert_eq!(composing(&mut a), composing(&mut b));
        // BS で消して打ち直しても追従する
        let (mut c, _) = engine();
        type_romaji(&mut c, "kamjo");
        c.process_key("s", &parse_key("BackSpace").unwrap());
        type_romaji(&mut c, "i");
        assert_eq!(composing(&mut a), composing(&mut c));
    }
}
