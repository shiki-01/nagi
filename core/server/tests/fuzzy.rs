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
    // 構造は第一仮説のまま（注目文節の候補が進むだけ）。補正候補まで進むと文全体が置き換わり、確定できる
    assert_ne!(r["preedit"]["text"], "日本");
    let mut r = r;
    for _ in 0..items(&r).len() {
        if r["preedit"]["text"] == "日本" {
            break;
        }
        r = press(&mut s, "Space", Value::Null);
    }
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

/// キー種別だけで送るフロント（`text` が空。keysym は ASCII）のタップでも、代替が打鍵ログに届く
#[test]
fn touch_alternatives_work_with_empty_text_over_rpc() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    for c in "kansi".chars() {
        let alts = if c == 's' {
            json!([{"text":"z","distance":0.2}])
        } else {
            Value::Null
        };
        let mut params = json!({"session_id":"s","keysym":c as u32,"keycode":0,"text":"",
            "modifiers":{},"kind":"down"});
        if !alts.is_null() {
            params["alternatives"] = alts;
        }
        let r = send(&mut s, "process_key", params);
        assert_eq!(r["result"]["consumed"], true, "{r}");
    }
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(r["preedit"]["text"], "監視");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(
        corrected(&r),
        [("漢字".to_string(), Some("かんじ".to_string()))],
        "{r}"
    );
}

/// フロントが打たれたキー自身の距離も代替に含めると、代替のコストはその距離を基準にした相対値になる
/// （タップ位置が 2 つのキーの境界に近いとき、遠い方への代替も十分あり得るとして扱う）
#[test]
fn typed_key_distance_is_the_cost_baseline_over_rpc() {
    let run = |s_alts: Value| {
        let mut s = server(ENABLED);
        initialize(&mut s);
        type_str(&mut s, "kan");
        press(&mut s, "s", s_alts);
        type_str(&mut s, "i");
        press(&mut s, "Space", Value::Null);
        corrected(&press(&mut s, "Space", Value::Null))
    };
    // z だけが距離 0.9: 絶対コスト 0.5 * (0.9 / 0.35)^2 ≒ 3.3 は曖昧さの閾値 1.5 を超えるので補正なし
    assert!(run(json!([{"text":"z","distance":0.9}])).is_empty());
    // s 自身が 0.8、z が 0.9: 相対コスト ≒ 0.7 なので補正候補が出る
    assert_eq!(
        run(json!([{"text":"s","distance":0.8},{"text":"z","distance":0.9}])),
        [("漢字".to_string(), Some("かんじ".to_string()))]
    );
}

/// フリック: `insert_reading` の `alternatives`（ひらがな）が打鍵ログ → 補正候補まで届く
#[test]
fn flick_alternatives_over_insert_reading_offer_correction() {
    let insert = |s: &mut Server, reading: &str, alternatives: Value| {
        let mut params = json!({"session_id":"s","reading":reading});
        if !alternatives.is_null() {
            params["alternatives"] = alternatives;
        }
        let r = send(s, "insert_reading", params);
        assert!(r.get("error").is_none(), "{r}");
        r["result"].clone()
    };
    let mut s = server(ENABLED);
    initialize(&mut s);
    insert(&mut s, "か", Value::Null);
    insert(&mut s, "ん", Value::Null);
    // 「じ」を狙って「し」に判定された、のような代替
    let r = insert(&mut s, "し", json!([{"text":"じ","distance":0.2}]));
    assert_eq!(r["preedit"]["text"], "かんし");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(r["preedit"]["text"], "監視");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(
        corrected(&r),
        [("漢字".to_string(), Some("かんじ".to_string()))],
        "{r}"
    );
}

/// フリックで複数文字を一度に挿入するときの代替は 1 打鍵のものではないので無視される
#[test]
fn flick_alternatives_ignored_for_multi_char_insert() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    let r = send(
        &mut s,
        "insert_reading",
        json!({"session_id":"s","reading":"かんし","alternatives":[{"text":"じ","distance":0.2}]}),
    );
    assert!(r.get("error").is_none(), "{r}");
    press(&mut s, "Space", Value::Null);
    let r = press(&mut s, "Space", Value::Null);
    assert!(corrected(&r).is_empty(), "{r}");
}

/// `insert_reading`（フリックの 1 文字）。`alternatives` が null なら省略する
fn flick(s: &mut Server, reading: &str, alternatives: Value) -> Value {
    let mut params = json!({"session_id":"s","reading":reading});
    if !alternatives.is_null() {
        params["alternatives"] = alternatives;
    }
    let r = send(s, "insert_reading", params);
    assert!(r.get("error").is_none(), "{r}");
    r["result"].clone()
}

/// Space を 2 回押した（候補ウィンドウが開いた）あとの補正候補
fn corrected_after_space(s: &mut Server) -> Vec<(String, Option<String>)> {
    press(s, "Space", Value::Null);
    corrected(&press(s, "Space", Value::Null))
}

fn kanji_correction() -> Vec<(String, Option<String>)> {
    vec![("漢字".to_string(), Some("かんじ".to_string()))]
}

/// かなだけの composition（フリック列）では、先頭・途中・末尾のどの位置の誤りも補正候補になり、
/// 第一仮説（打たれたとおり）の表示は変わらない
#[test]
fn flick_only_composition_corrects_any_position() {
    // 末尾: かんし（じ を狙った）
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "か", Value::Null);
    flick(&mut s, "ん", Value::Null);
    let r = flick(&mut s, "し", json!([{"text":"じ","distance":0.2}]));
    assert_eq!(r["preedit"]["text"], "かんし");
    assert_eq!(corrected_after_space(&mut s), kanji_correction());

    // 途中: にへん（ほ を狙った。「へ」は右フリックの隣のゾーン）
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "に", Value::Null);
    let r = flick(
        &mut s,
        "へ",
        json!([{"text":"ほ","distance":0.3},{"text":"ふ","distance":0.5}]),
    );
    assert_eq!(r["preedit"]["text"], "にへ");
    let r = flick(&mut s, "ん", Value::Null);
    assert_eq!(r["preedit"]["text"], "にへん");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(r["preedit"]["text"], "にへん", "第一仮説の表示は変わらない");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(
        corrected(&r),
        [("日本".to_string(), Some("にほん".to_string()))]
    );
}

/// ローマ字キーとかなキー（フリック）が混在した composition でも、双方の代替が効く
#[test]
fn flick_and_romaji_keys_can_be_mixed() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    // ni（ローマ字）+ へ（フリック。ほ の代替）+ n（ローマ字）
    type_str(&mut s, "ni");
    let r = flick(&mut s, "へ", json!([{"text":"ほ","distance":0.3}]));
    assert_eq!(r["preedit"]["text"], "にへ");
    press(&mut s, "n", Value::Null);
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(r["preedit"]["text"], "にへん");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(
        corrected(&r),
        [("日本".to_string(), Some("にほん".to_string()))],
        "{r}"
    );

    // かん（フリック）+ s（ローマ字。z の代替）+ i（ローマ字）。かなの後ろのローマ字の代替
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "か", Value::Null);
    flick(&mut s, "ん", Value::Null);
    press(&mut s, "s", json!([{"text":"z","distance":0.2}]));
    press(&mut s, "i", Value::Null);
    assert_eq!(corrected_after_space(&mut s), kanji_correction());
}

/// 「小゛゜」: フロントは Backspace + `insert_reading`（代替なし）で濁点・小書きを実現する。
/// Backspace で打鍵ログが 1 件戻り、直後の `insert_reading` の代替が記録される（G-49 の巻き戻しと整合する）
#[test]
fn flick_dakuten_via_backspace_keeps_the_key_log_consistent() {
    // 戻した打鍵の代替は残らない: し（じ の代替つき）→ Backspace → す（別の代替）。
    // 「かんす」は辞書に無く、戻した「し」の代替 じ で補正されてはならない
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "か", Value::Null);
    flick(&mut s, "ん", Value::Null);
    flick(&mut s, "し", json!([{"text":"じ","distance":0.2}]));
    let r = press(&mut s, "BackSpace", Value::Null);
    assert_eq!(r["preedit"]["text"], "かん");
    let r = flick(&mut s, "す", json!([{"text":"ず","distance":0.2}]));
    assert_eq!(r["preedit"]["text"], "かんす");
    assert!(corrected_after_space(&mut s).is_empty());

    // 直後の `insert_reading` の代替が記録される: し → Backspace → し（じ の代替つき）で、補正される
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "か", Value::Null);
    flick(&mut s, "ん", Value::Null);
    flick(&mut s, "し", json!([{"text":"せ","distance":0.2}]));
    press(&mut s, "BackSpace", Value::Null);
    let r = flick(&mut s, "し", json!([{"text":"じ","distance":0.2}]));
    assert_eq!(r["preedit"]["text"], "かんし");
    assert_eq!(corrected_after_space(&mut s), kanji_correction());

    // 実際の「小゛゜」: し（代替つき）→ Backspace → じ（代替なし）。表示は かんじ で、打鍵ログは
    // [か, ん, じ]（代替なし）。補正の元になる代替は無く、通常の変換結果（漢字）が出る
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "か", Value::Null);
    flick(&mut s, "ん", Value::Null);
    flick(&mut s, "し", json!([{"text":"じ","distance":0.2}]));
    press(&mut s, "BackSpace", Value::Null);
    let r = flick(&mut s, "じ", Value::Null);
    assert_eq!(r["preedit"]["text"], "かんじ");
    let r = press(&mut s, "Space", Value::Null);
    assert_eq!(r["preedit"]["text"], "漢字");
    let r = press(&mut s, "Space", Value::Null);
    assert!(corrected(&r).is_empty(), "{r}");
}

/// 1 文字の composition を Backspace で空にしてから次のフリックを始めても、打鍵ログは新しく始まる
/// （「小゛゜」で先頭のかなを変えるとき）
#[test]
fn flick_log_restarts_after_backspace_empties_the_composition() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "き", json!([{"text":"か","distance":0.2}]));
    let r = press(&mut s, "BackSpace", Value::Null);
    assert_eq!(r["preedit"]["text"], "");
    flick(&mut s, "か", Value::Null);
    flick(&mut s, "ん", Value::Null);
    flick(&mut s, "し", json!([{"text":"じ","distance":0.2}]));
    assert_eq!(corrected_after_space(&mut s), kanji_correction());
}

/// 複数文字の `insert_reading`（ペーストなど）: その代替は無視される。かなだけの挿入は打鍵ログを保つ
/// （それ以前の 1 打鍵の代替は有効なまま）が、ひらがな以外を含む挿入はログを無効にする
#[test]
fn flick_multi_char_insert_ignores_its_alternatives_and_non_kana_invalidates_the_log() {
    // ペーストした文字（んし）への代替（じ）は 1 打鍵のものではないので使われない
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "か", Value::Null);
    let r = flick(&mut s, "んし", json!([{"text":"じ","distance":0.2}]));
    assert_eq!(r["preedit"]["text"], "かんし");
    assert!(corrected_after_space(&mut s).is_empty());

    // 先頭のフリックの代替は、あとのかなのペーストを挟んでも有効
    let mut s2 = server(ENABLED);
    initialize(&mut s2);
    flick(&mut s2, "に", Value::Null);
    flick(&mut s2, "へ", json!([{"text":"ほ","distance":0.3}]));
    let r = flick(&mut s2, "んご", json!([{"text":"ぐ","distance":0.1}]));
    assert_eq!(r["preedit"]["text"], "にへんご");
    press(&mut s2, "Space", Value::Null);
    let r = press(&mut s2, "Space", Value::Null);
    assert_eq!(
        corrected(&r),
        [("日本語".to_string(), Some("にほんご".to_string()))],
        "{r}"
    );

    // ひらがな以外（漢字）を含む挿入でログは無効になり、補正は出ない
    let mut s = server(ENABLED);
    initialize(&mut s);
    flick(&mut s, "か", Value::Null);
    flick(&mut s, "ん", Value::Null);
    flick(&mut s, "し", json!([{"text":"じ","distance":0.2}]));
    flick(&mut s, "字", Value::Null);
    press(&mut s, "Space", Value::Null);
    let r = press(&mut s, "Space", Value::Null);
    assert!(corrected(&r).is_empty(), "{r}");
}

/// 確定したかな自身の距離も代替に含めると、コアは相対コストで換算する（iOS フロントは現状含めない）
#[test]
fn flick_typed_distance_is_the_cost_baseline_over_rpc() {
    let run = |alts: Value| {
        let mut s = server(ENABLED);
        initialize(&mut s);
        flick(&mut s, "か", Value::Null);
        flick(&mut s, "ん", Value::Null);
        flick(&mut s, "し", alts);
        corrected_after_space(&mut s)
    };
    // じ だけが 0.9: 絶対コスト 0.5 * (0.9 / 0.35)^2 ≒ 3.3 は曖昧さの閾値 1.5 を超えるので補正なし
    assert!(run(json!([{"text":"じ","distance":0.9}])).is_empty());
    // し 自身が 0.8、じ が 0.9: 相対コスト ≒ 0.7 なので補正候補が出る
    assert_eq!(
        run(json!([{"text":"し","distance":0.8},{"text":"じ","distance":0.9}])),
        kanji_correction()
    );
}

/// かなだけの入力で、各打鍵に多くの代替があっても、補正候補は辞書の語として成立するものだけ
/// （`expand` の枝刈り。かなに ASCII の欠落補完を足さない）
#[test]
fn flick_corrections_are_pruned_by_the_dictionary() {
    let mut s = server(ENABLED);
    initialize(&mut s);
    let kana_row = |c: &str| {
        // その打鍵の代替として、同じ行の他のかな（距離 0.3〜0.5）を 3 つ
        let all = ["か", "き", "く", "け", "こ"];
        let alts: Vec<Value> = all
            .iter()
            .filter(|k| **k != c)
            .take(3)
            .enumerate()
            .map(|(i, k)| json!({"text":k,"distance":0.3 + 0.1 * i as f64}))
            .collect();
        Value::Array(alts)
    };
    flick(&mut s, "か", kana_row("か"));
    flick(&mut s, "ん", Value::Null);
    flick(
        &mut s,
        "し",
        json!([{"text":"じ","distance":0.2},{"text":"す","distance":0.3},{"text":"せ","distance":0.4}]),
    );
    press(&mut s, "Space", Value::Null);
    let r = press(&mut s, "Space", Value::Null);
    let found = corrected(&r);
    assert_eq!(found, kanji_correction(), "{r}");
    // 候補の総数も少ない（辞書にない読みの仮説は変換されない）
    assert!(items(&r).len() <= 12, "{:?}", items(&r));
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
        // 最適化なしのビルドは数倍遅いので、極端な劣化の検出に留める
        let limit_ms = if cfg!(debug_assertions) {
            5000.0
        } else {
            500.0
        };
        assert!(expand_ms + convert_ms < limit_ms, "{typed}: 補正が遅すぎる");
    }
}

/// 照会回数と時間を数える `PrefixOracle`（実辞書での `expand` のプロファイル用）
struct CountingOracle<'a> {
    inner: &'a dyn nagi_fuzzy::PrefixOracle,
    prefix: std::cell::Cell<(u64, std::time::Duration)>,
    word: std::cell::Cell<(u64, std::time::Duration)>,
}

impl nagi_fuzzy::PrefixOracle for CountingOracle<'_> {
    fn has_prefix(&self, s: &str) -> bool {
        let t = std::time::Instant::now();
        let r = self.inner.has_prefix(s);
        let (n, d) = self.prefix.get();
        self.prefix.set((n + 1, d + t.elapsed()));
        r
    }
    fn contains_word(&self, s: &str) -> bool {
        let t = std::time::Instant::now();
        let r = self.inner.contains_word(s);
        let (n, d) = self.word.get();
        self.word.set((n + 1, d + t.elapsed()));
        r
    }
}

/// 実辞書での `expand` 単体の計測（辞書が無ければ skip）。典型的なタッチ入力（全打鍵に隣接キーの代替つき）。
/// `cargo test -p nagi-server --release --test fuzzy -- --ignored --nocapture real_dictionary_expand_profile`
#[test]
#[ignore = "計測用（実辞書が必要）"]
fn real_dictionary_expand_profile() {
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
    let cfg = FuzzyConfig::default();
    for typed in [
        "nihn",
        "kanizi",
        "kyouhaiitenkidesune",
        "watasihamainitigakkouniikimasu",
    ] {
        let keys: Vec<nagi_fuzzy::KeyAlternatives> = typed
            .chars()
            .map(|c| {
                // 近傍キーを 2 つ（距離 0.5・0.7）。打たれたキー自身の距離は 0.3
                let n1 = neighbor(c, 1);
                let n2 = neighbor(c, 2);
                nagi_fuzzy::KeyAlternatives::from_distances(
                    c,
                    &[(c, 0.3), (n1, 0.5), (n2, 0.7)],
                    &cfg,
                )
            })
            .collect();
        let counting = CountingOracle {
            inner: oracle,
            prefix: Default::default(),
            word: Default::default(),
        };
        let started = std::time::Instant::now();
        let (hyps, stats) = nagi_fuzzy::expand_with_stats(&keys, &table, &counting, &cfg);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let (pn, pd) = counting.prefix.get();
        let (wn, wd) = counting.word.get();
        eprintln!(
            "{typed} ({} keys): expand {ms:.2} ms, {} hyps, {} states; has_prefix {pn} calls {:.2} ms; contains_word {wn} calls {:.2} ms",
            typed.len(),
            hyps.len(),
            stats.states_expanded,
            pd.as_secs_f64() * 1000.0,
            wd.as_secs_f64() * 1000.0,
        );
    }
}

/// QWERTY の左右隣のキー（計測用の代替）
fn neighbor(c: char, k: usize) -> char {
    const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    for row in ROWS {
        if let Some(i) = row.find(c) {
            let chars: Vec<char> = row.chars().collect();
            let j = if k == 1 { i + 1 } else { i.wrapping_sub(1) };
            return chars
                .get(j)
                .copied()
                .unwrap_or(chars[(i + 2) % chars.len()]);
        }
    }
    c
}
