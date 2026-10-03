//! 曖昧入力（F-1）の統合テスト: メモリ上の小さな辞書で `Pipeline` を作り、JSON-RPC の
//! `process_key` × n → Space の応答に Corrected 候補が出ることを検査する。

#![allow(missing_docs)]

use nagi_config::Config;
use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};
use nagi_engine::{ConversionBackend, parse_key};
use nagi_fuzzy::{FuzzyConfig, ReadingHypothesis, expand_typed};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_romaji::RomajiTable;
use nagi_server::setup::{Built, Reloadable};
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::sync::Arc;

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 記号,句点,*,*,*,*,。
4 記号,読点,*,*,*,*,、
5 記号,括弧開,*,*,*,*,*
6 記号,括弧閉,*,*,*,*,*
7 記号,一般,*,*,*,*,*
";

const WORDS: &str = "\
にほん\t1\t1\t3000\t日本
にほんご\t1\t1\t3500\t日本語
に\t2\t2\t400\tに
かんじ\t1\t1\t3000\t漢字
かんし\t1\t1\t3200\t監視
";

fn dict() -> Arc<Dictionary> {
    const N: usize = 8;
    let mut lines = vec![N.to_string()];
    lines.extend(std::iter::repeat_n("0".to_string(), N * N));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text("inline", WORDS).unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    Arc::new(b.build().unwrap())
}

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

fn runtime(config: &Config) -> Arc<Runtime> {
    Runtime::new(
        SYSTEM,
        false,
        Tunables::from_config(config),
        Learner::new(LearnConfig::default()),
        None,
    )
}

/// 辞書のみのパイプライン（LLM なし）
fn pipeline(config: &Config) -> Arc<Pipeline> {
    Arc::new(Pipeline::new(Some(dict()), None, runtime(config)))
}

/// 設定ファイルの `[fuzzy]` を読み込んだ Config で Server を作る（`setup` と同じ経路で EngineConfig になる）
fn server(config_toml: &str) -> Server {
    let (mut config, issues) = nagi_config::parse(config_toml).expect("config");
    assert!(issues.is_empty(), "{issues:?}");
    config.conversion.initial_view = nagi_config::InitialView::Segment;
    let runtime = runtime(&config);
    let pipeline = Arc::new(Pipeline::new(Some(dict()), None, runtime.clone()));
    Server::from_built(Built {
        pipeline,
        runtime,
        config,
        reload: Reloadable::detached(),
        notes: Vec::new(),
    })
}

fn send(server: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let lines = server.handle_line(&line);
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn initialize(server: &mut Server) {
    let r = send(
        server,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
    );
    assert!(r.get("error").is_none(), "{r}");
}

fn press(server: &mut Server, key: &str, alternatives: Value) -> Value {
    let k = parse_key(key).unwrap();
    let mut params = json!({"session_id":"s","keysym":k.keysym,"keycode":0,"text":k.text,
        "modifiers":{},"kind":"down"});
    if !alternatives.is_null() {
        params["alternatives"] = alternatives;
    }
    let r = send(server, "process_key", params);
    assert!(r.get("error").is_none(), "{r}");
    r["result"].clone()
}

fn type_str(server: &mut Server, keys: &str) {
    for c in keys.chars() {
        press(server, &c.to_string(), Value::Null);
    }
}

/// 候補ウィンドウの `(表記, 出所, note)`
fn items(result: &Value) -> Vec<(String, Value, Option<String>)> {
    result["candidates"]["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| {
                    (
                        c["surface"].as_str().unwrap().to_string(),
                        c["sources"].clone(),
                        c["note"].as_str().map(str::to_string),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn corrected(result: &Value) -> Vec<(String, Option<String>)> {
    items(result)
        .into_iter()
        .filter(|(_, sources, _)| {
            sources
                .as_array()
                .is_some_and(|s| s.contains(&json!("corrected")))
        })
        .map(|(s, _, n)| (s, n))
        .collect()
}

const ENABLED: &str = "[fuzzy]\nenabled = true\n";

#[test]
fn nihn_space_returns_corrected_candidate_over_rpc() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    type_str(&mut s, "nihn");
    let r = press(&mut s, "Space", Value::Null);
    // 第一仮説（打たれたとおり）の表示は変えない: に + h + ん（辞書にない h は素通し）
    assert_eq!(r["preedit"]["text"], "にhん");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(r["candidates"]["visible"], true);
    assert_eq!(
        corrected(&r),
        [("日本".to_string(), Some("にほん".to_string()))],
        "{r}"
    );
    // 補正候補を選ぶ（候補ウィンドウの次候補）と preedit が置き換わり、確定できる
    assert_eq!(r["preedit"]["text"], "日本");
    let r = press(&mut s, "Enter", Value::Null);
    assert_eq!(r["commit"], "日本");
}

#[test]
fn touch_alternatives_over_rpc_offer_correction() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    type_str(&mut s, "kan");
    // s を狙って近くの z（距離 0.2）に触れた、のような代替
    press(&mut s, "s", json!([{"text":"z","distance":0.2}]));
    type_str(&mut s, "i");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(r["preedit"]["text"], "監視");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(
        corrected(&r),
        [("漢字".to_string(), Some("かんじ".to_string()))],
        "{r}"
    );
    // 通常の候補（出所は corrected ではない）に note は無い
    assert!(
        items(&r)
            .iter()
            .filter(|(_, src, _)| !src.as_array().unwrap().contains(&json!("corrected")))
            .all(|(_, _, note)| note.is_none())
    );
}

#[test]
fn fuzzy_is_off_by_default_over_rpc() {
    let mut s = server("");
    initialize(&mut s);
    type_str(&mut s, "kan");
    press(&mut s, "s", json!([{"text":"z","distance":0.2}]));
    type_str(&mut s, "i");
    press(&mut s, "Space", Value::Null);
    let r = press(&mut s, "Space", Value::Null);
    assert!(corrected(&r).is_empty(), "{r}");
    // `note` を持つ候補も無い（JSON に note が出ない）
    assert!(items(&r).iter().all(|(_, _, n)| n.is_none()));
}

#[test]
fn invalid_alternatives_are_ignored_over_rpc() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    type_str(&mut s, "kan");
    // 2 文字・負の距離は無視される（エラーにならず、代替なしの入力として扱う）
    press(
        &mut s,
        "s",
        json!([{"text":"zz","distance":0.2},{"text":"z","distance":-1.0}]),
    );
    type_str(&mut s, "i");
    press(&mut s, "Space", Value::Null);
    let r = press(&mut s, "Space", Value::Null);
    assert!(corrected(&r).is_empty(), "{r}");
}

// ---- Pipeline の補正用 API ----

#[test]
fn pipeline_exposes_oracle_quality_and_hypothesis_conversion() {
    let config = Config::default();
    let p = pipeline(&config);
    let oracle = p.prefix_oracle().expect("辞書があれば Some");
    assert!(oracle.has_prefix("にほ"));
    assert!(oracle.contains_word("にほん"));
    assert!(!oracle.contains_word("にほ"));
    assert!(!oracle.has_prefix("ほ"));

    // 品質: 辞書に載る読みは uncovered = 0、載らない読みは割合と高いコスト
    let good = p.quality("にほん").unwrap();
    let bad = p.quality("にhん").unwrap();
    assert_eq!(good.uncovered, 0.0);
    assert!(bad.uncovered > 0.3, "{bad:?}");
    assert!(bad.conv_cost > good.conv_cost);
    assert!(p.quality("").is_none());

    // 仮説の変換: nihn → にほん
    let hyps: Vec<ReadingHypothesis> = expand_typed(
        "nihn",
        &RomajiTable::standard(),
        oracle,
        &FuzzyConfig::default(),
    );
    assert_eq!(hyps[0].reading, "にhん");
    let scored = p.convert_hypotheses(&hyps[1..], "", 2);
    assert_eq!(scored.len(), 1);
    assert_eq!(scored[0].hypothesis_index, 0);
    assert_eq!(scored[0].segments[0].surface, "日本");
    // quality と同じ尺度（良好な読みの quality と一致する）
    assert!((scored[0].conv_cost - good.conv_cost).abs() < 1e-4);
    // limit 0 は空
    assert!(p.convert_hypotheses(&hyps[1..], "", 0).is_empty());
}

#[test]
fn pipeline_without_dictionary_does_not_support_correction() {
    let config = Config::default();
    let p = Pipeline::new(None, None, runtime(&config));
    assert!(p.prefix_oracle().is_none());
    assert!(p.quality("にほん").is_none());
    assert!(p.convert_hypotheses(&[], "", 3).is_empty());
}

/// 実辞書（`cargo xtask fetch-dict && cargo xtask build-dict`）での補正の所要時間の計測。
/// 辞書が無い環境では何もせず終了する。厳密な閾値は置かず（較正は F-5）、極端な劣化だけを検出する
#[test]
fn real_dictionary_correction_latency() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dict/nagi-dict.bin");
    if !path.exists() {
        eprintln!("skip: {} が無い（cargo xtask build-dict）", path.display());
        return;
    }
    let dict = Arc::new(Dictionary::load(&path).expect("辞書の読み込み"));
    let config = Config::default();
    let p = Pipeline::new(Some(dict), None, runtime(&config));
    let oracle = p.prefix_oracle().unwrap();
    let table = RomajiTable::standard();
    for typed in ["nihn", "kanizi", "watasihamainitigakkouniikimasu"] {
        let started = std::time::Instant::now();
        let hyps = expand_typed(typed, &table, oracle, &FuzzyConfig::default());
        let expand_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        let scored = p.convert_hypotheses(&hyps[1..], "", 2);
        let convert_ms = started.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "{typed}: expand {expand_ms:.2} ms ({} hyps), convert_hypotheses {convert_ms:.2} ms ({} results)",
            hyps.len(),
            scored.len()
        );
        assert!(expand_ms + convert_ms < 500.0, "{typed}: 補正が遅すぎる");
    }
}
