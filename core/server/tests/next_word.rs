//! 確定直後の次語予測（`predict.next_word`）の統合テスト（プロセス内）。
//! 学習したバイグラム（直前の語 → 次の語）が、確定直後の Direct 状態の候補として届くことを確認する。

mod common;

use common::{FakeModel, dict};
use nagi_config::Config;
use nagi_engine::parse_key;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::setup::{Built, Reloadable};
use nagi_server::{Pipeline, Runtime, Server, Tunables};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nagi-nextword-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn send(server: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let mut lines = server.handle_line(&line).into_iter();
    serde_json::from_str(&lines.next().expect("応答")).unwrap()
}

fn press(server: &mut Server, session: &str, key: &str) -> Value {
    let k = parse_key(key).unwrap();
    send(
        server,
        "process_key",
        json!({"session_id":session,"keysym":k.keysym,"keycode":0,"text":k.text,
               "modifiers":{"shift":k.modifiers.shift,"ctrl":k.modifiers.ctrl,"alt":k.modifiers.alt,"meta":k.modifiers.meta},
               "kind":"down"}),
    )["result"]
        .clone()
}

fn type_str(server: &mut Server, session: &str, s: &str) {
    for c in s.chars() {
        press(server, session, &c.to_string());
    }
}

/// 読みを打って Space で変換し、Enter で確定する。確定時の応答を返す
fn commit_word(server: &mut Server, session: &str, romaji: &str) -> Value {
    type_str(server, session, romaji);
    press(server, session, "Space");
    press(server, session, "Enter")
}

fn server_in(dir: &Path, next_word: bool) -> Server {
    let config_path = dir.join("config.toml");
    let mut config = Config::default();
    config.predict.next_word = next_word;
    config.predict.next_word_count = 5;
    server_with(dir, config)
}

fn server_with(dir: &Path, config: Config) -> Server {
    let learn_path = dir.join("learning.tsv");
    let (learner, _) = Learner::load(&learn_path, LearnConfig::default()).unwrap();
    let runtime = Runtime::new(
        SYSTEM,
        true,
        Tunables::from_config(&config),
        learner,
        Some(learn_path),
    );
    let dict = dict();
    runtime.set_overlay(dict.make_overlay(&nagi_dict::UserDictionary::default()));
    let model = FakeModel::new(&[]);
    let pipeline = Arc::new(Pipeline::new(
        Some(dict.clone()),
        Some(Box::new(model)),
        runtime.clone(),
    ));
    let mut reload = Reloadable::detached();
    reload.dict = Some(dict);
    let mut s = Server::from_built(Built {
        pipeline,
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

fn surfaces(r: &Value) -> Vec<String> {
    r["candidates"]["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|i| i["surface"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 「今日 → は」を 1 回学習させる
fn teach_kyou_wa(s: &mut Server, session: &str) {
    let r = commit_word(s, session, "kyou");
    assert_eq!(r["commit"], "今日");
    let r = commit_word(s, session, "ha");
    assert_eq!(r["commit"], "は");
}

#[test]
fn learned_bigram_is_offered_right_after_commit() {
    let dir = temp_dir("basic");
    let mut s = server_in(&dir, true);
    // 学習前は出ない
    let r = commit_word(&mut s, "a", "kyou");
    assert_eq!(r["commit"], "今日");
    assert!(surfaces(&r).is_empty());
    let r = commit_word(&mut s, "a", "ha");
    assert_eq!(r["commit"], "は");
    // 別のセッションで「今日」を確定すると、次語として「は」が Learned の出所で出る
    let r = commit_word(&mut s, "b", "kyou");
    assert_eq!(surfaces(&r), ["は"]);
    assert_eq!(r["candidates"]["visible"], true);
    assert_eq!(r["candidates"]["items"][0]["sources"], json!(["learned"]));
    assert_eq!(r["candidates"]["items"][0]["covers"], Value::Null);
    // 入力が始まったら消える
    let r = press(&mut s, "b", "h");
    assert_eq!(r["candidates"]["visible"], false);
    assert!(surfaces(&r).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn selecting_a_next_word_inserts_it_and_learns_the_pair() {
    let dir = temp_dir("select");
    let mut s = server_in(&dir, true);
    teach_kyou_wa(&mut s, "a");
    let r = commit_word(&mut s, "b", "kyou");
    assert_eq!(surfaces(&r), ["は"]);
    let r = send(
        &mut s,
        "select_candidate",
        json!({"session_id":"b","index":0}),
    )["result"]
        .clone();
    assert_eq!(r["commit"], "は");
    // 選んだ語が次の「直前の語」になる（まだ「は」の次語は学習していないので空 = 候補が消える）
    assert_eq!(r["candidates"]["visible"], false);
    // 「今日 → は」の回数が増えている
    let next = std::fs::read_to_string({
        send(&mut s, "focus_out", json!({"session_id":"b"}));
        dir.join("learning-next.tsv")
    })
    .unwrap();
    assert!(next.contains("今日\tは\t3\t"), "{next}");
    // 範囲外の index は何もしない
    let r = commit_word(&mut s, "c", "kyou");
    assert!(!surfaces(&r).is_empty());
    let r = send(
        &mut s,
        "select_candidate",
        json!({"session_id":"c","index":9}),
    )["result"]
        .clone();
    assert!(r.get("commit").is_none_or(Value::is_null));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_by_default_records_and_shows_nothing() {
    let dir = temp_dir("off");
    assert!(!Config::default().predict.next_word);
    let mut s = server_in(&dir, false);
    teach_kyou_wa(&mut s, "a");
    let r = commit_word(&mut s, "b", "kyou");
    assert!(surfaces(&r).is_empty());
    send(&mut s, "focus_out", json!({"session_id":"b"}));
    // 無効のあいだはバイグラムを記録しない（ファイルも作らない）
    let path = dir.join("learning-next.tsv");
    assert!(
        !path.exists() || !std::fs::read_to_string(path).unwrap().contains("今日"),
        "無効なら記録しない"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_is_recorded_or_shown_while_learning_is_suspended() {
    let dir = temp_dir("suspend");
    let mut s = server_in(&dir, true);
    // secure input 中: 記録しない
    send(
        &mut s,
        "set_context",
        json!({"session_id":"p","secure_input":true}),
    );
    teach_kyou_wa(&mut s, "p");
    // 通常のセッション: 何も学習されていないので出ない
    let r = commit_word(&mut s, "q", "kyou");
    assert!(surfaces(&r).is_empty());
    // 学習済みにしてから、停止中のアプリ・secure input では出ない
    teach_kyou_wa(&mut s, "q");
    let r = commit_word(&mut s, "r", "kyou");
    assert_eq!(surfaces(&r), ["は"]);
    send(
        &mut s,
        "set_context",
        json!({"session_id":"pw","app_id":"com.1password.1password"}),
    );
    let r = commit_word(&mut s, "pw", "kyou");
    assert!(surfaces(&r).is_empty(), "パスワードマネージャでは出さない");
    send(
        &mut s,
        "set_context",
        json!({"session_id":"sec","secure_input":true}),
    );
    let r = commit_word(&mut s, "sec", "kyou");
    assert!(surfaces(&r).is_empty(), "secure input 中は出さない");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn learning_disabled_by_config_hides_next_words() {
    let dir = temp_dir("learning-off");
    let mut config = Config::default();
    config.predict.next_word = true;
    config.learning.enabled = false;
    let mut s = server_with(&dir, config);
    teach_kyou_wa(&mut s, "a");
    let r = commit_word(&mut s, "b", "kyou");
    assert!(surfaces(&r).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clear_learning_removes_bigrams_even_after_restart() {
    let dir = temp_dir("clear");
    let mut s = server_in(&dir, true);
    teach_kyou_wa(&mut s, "a");
    let r = commit_word(&mut s, "b", "kyou");
    assert_eq!(surfaces(&r), ["は"]);
    let r = send(&mut s, "clear_learning", json!({}));
    assert!(r.get("error").is_none());
    let r = commit_word(&mut s, "c", "kyou");
    assert!(surfaces(&r).is_empty());
    drop(s);
    let file = std::fs::read_to_string(dir.join("learning-next.tsv")).unwrap();
    assert!(!file.contains("今日\tは"), "{file}");
    let mut s = server_in(&dir, true);
    let r = commit_word(&mut s, "d", "kyou");
    assert!(surfaces(&r).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bigrams_survive_restart_and_read_old_format_files() {
    let dir = temp_dir("old");
    // 旧版のファイル: 次語ファイルは無く、learning.tsv は世代なしの v1
    std::fs::write(
        dir.join("learning.tsv"),
        "#nagi-learn\tv1\nきょう\t今日\t0000000000000000\t2\t1700000000\tconv\n",
    )
    .unwrap();
    {
        let mut s = server_in(&dir, true);
        let r = commit_word(&mut s, "a", "kyou");
        assert!(surfaces(&r).is_empty(), "次語ファイルが無くても動く");
        let r = commit_word(&mut s, "a", "ha");
        assert_eq!(r["commit"], "は");
        send(&mut s, "focus_out", json!({"session_id":"a"}));
    }
    // 通常の学習ファイルは従来のスキーマのまま（次語は混ざらない）
    let main = std::fs::read_to_string(dir.join("learning.tsv")).unwrap();
    assert!(main.starts_with("#nagi-learn\tv1"), "{main}");
    assert!(!main.contains("\tは\t0000") || !main.contains("今日\tは"), "{main}");
    let next = std::fs::read_to_string(dir.join("learning-next.tsv")).unwrap();
    assert!(next.starts_with("#nagi-learn-next\tv1"), "{next}");
    let mut s = server_in(&dir, true);
    let r = commit_word(&mut s, "b", "kyou");
    assert_eq!(surfaces(&r), ["は"]);
    drop(s);
    // 世代なしの次語ファイルも読める
    std::fs::write(
        dir.join("learning-next.tsv"),
        "#nagi-learn-next\tv1\n今日\tも\t4\t1700000000\n",
    )
    .unwrap();
    let mut s = server_in(&dir, true);
    let r = commit_word(&mut s, "c", "kyou");
    assert_eq!(surfaces(&r), ["も"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_instances_merge_their_bigrams() {
    let dir = temp_dir("merge");
    {
        let mut a = server_in(&dir, true);
        let mut b = server_in(&dir, true);
        // a: 今日 → は、b: 今日 → に（同時に動いている 2 つのインスタンス）
        commit_word(&mut a, "x", "kyou");
        commit_word(&mut a, "x", "ha");
        commit_word(&mut b, "y", "kyou");
        commit_word(&mut b, "y", "ni");
        send(&mut a, "focus_out", json!({"session_id":"x"}));
        send(&mut b, "focus_out", json!({"session_id":"y"}));
    }
    let mut c = server_in(&dir, true);
    let r = commit_word(&mut c, "z", "kyou");
    let got = surfaces(&r);
    assert!(got.contains(&"は".to_string()) && got.contains(&"に".to_string()), "{got:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
