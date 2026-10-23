//! 設定の再読み込み・環境プロファイルの自動切替・学習・ユーザー辞書の統合テスト（プロセス内）。
//! LLM は偽のモデル、辞書は小さなインライン辞書を使う。

mod common;

use common::{FakeModel, dict};
use nagi_config::Config;
use nagi_engine::parse_key;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::setup::{Built, Reloadable};
use nagi_server::{Pipeline, Runtime, Server, ServerOptions, Tunables, build};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nagi-features-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 応答 1 行 + 通知（`notify`）の行
struct Reply {
    response: Value,
    notes: Vec<Value>,
}

fn send(server: &mut Server, method: &str, params: Value) -> Reply {
    let line = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
    let mut lines = server.handle_line(&line).into_iter();
    let response: Value = serde_json::from_str(&lines.next().expect("応答")).unwrap();
    let notes = lines.map(|l| serde_json::from_str(&l).unwrap()).collect();
    Reply { response, notes }
}

fn initialize(server: &mut Server) -> Reply {
    send(
        server,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
    )
}

fn press(server: &mut Server, session: &str, key: &str) -> Value {
    let k = parse_key(key).unwrap();
    send(
        server,
        "process_key",
        json!({"session_id":session,"keysym":k.keysym,"keycode":0,"text":k.text,
               "modifiers":{"shift":k.modifiers.shift,"ctrl":k.modifiers.ctrl,"alt":k.modifiers.alt,"meta":k.modifiers.meta},
               "kind":"down"}),
    )
    .response["result"]
        .clone()
}

/// 文字を順に入力し、最後に表示された preedit の文字列（差分通知に含まれた最後の値）を返す
fn type_str(server: &mut Server, session: &str, s: &str) -> String {
    let mut last = String::new();
    for c in s.chars() {
        let r = press(server, session, &c.to_string());
        if let Some(t) = r["preedit"]["text"].as_str() {
            last = t.to_string();
        }
    }
    last
}

fn set_context(server: &mut Server, session: &str, extra: Value) {
    let mut params = json!({"session_id": session});
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let r = send(server, "set_context", params);
    assert!(r.response.get("error").is_none());
}

fn status(server: &mut Server) -> Value {
    send(server, "get_status", json!({})).response["result"].clone()
}

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

/// 偽モデル + インライン辞書 + 一時ディレクトリの学習・ユーザー辞書・設定で Server を作る
fn server_in(user_dir: &Path) -> Server {
    let config_path = user_dir.join("config.toml");
    let mut config = match nagi_config::load_file(&config_path) {
        Ok((c, _)) => c,
        Err(_) => Config::default(),
    };
    // これらのテストは LLM 由来の再ランキング・学習・ユーザー辞書優先度を検証するものであり、
    // 文全体N-best（`initial_view = "sentence"`、既定）の対象外にする。sentence モードでは
    // `Pipeline::sentence_candidates`（辞書のみ、LLM再採点なし）が優先されてしまい、
    // これらのテストが期待する LLM 由来の並び順を検証できなくなるため
    config.conversion.initial_view = nagi_config::InitialView::Segment;
    let learn_path = user_dir.join("learning.tsv");
    let (learner, _) = Learner::load(&learn_path, LearnConfig::default()).unwrap();
    let runtime = Runtime::new(
        SYSTEM,
        true,
        Tunables::from_config(&config),
        learner,
        Some(learn_path),
    );
    runtime.set_config_state(Some(config_path.clone()), false);
    let dict = dict();
    let user_dict_dir = user_dir.join("user_dict");
    let (user, _) = nagi_dict::UserDictionary::load_dir(&user_dict_dir);
    runtime.set_overlay(dict.make_overlay(&user));
    let model = FakeModel::new(&[("こうえん", "公園")]);
    let pipeline = Arc::new(Pipeline::new(
        Some(dict.clone()),
        Some(Box::new(model)),
        runtime.clone(),
    ));
    let mut reload = Reloadable::detached();
    reload.config_path = Some(config_path);
    reload.user_dict_dir = Some(user_dict_dir);
    reload.dict = Some(dict);
    reload.mark_read();
    Server::from_built(Built {
        pipeline,
        runtime,
        config,
        reload,
        notes: Vec::new(),
    })
}

/// 変換して候補窓を出し、`steps` の分だけ Space（次候補）を押す
fn convert_kouen(server: &mut Server, session: &str) -> String {
    type_str(server, session, "kouen");
    press(server, session, "Space")["preedit"]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

// ---- 環境プロファイル ----

#[test]
fn profile_follows_environment_reports_and_notifies() {
    let dir = temp_dir("profile");
    let mut s = server_in(&dir);
    let init = initialize(&mut s);
    assert_eq!(init.response["result"]["profile"], "balanced");
    let st = status(&mut s);
    assert_eq!(st["profile"], "balanced");
    assert_eq!(st["llm"], true);
    assert_eq!(st["backend"], "dict+llm");

    // バッテリー駆動 → 1 段軽く（応答のあとに notify 通知が付く）
    let r = send(&mut s, "set_environment", json!({"on_battery": true}));
    assert!(r.response.get("error").is_none());
    assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
    assert_eq!(r.notes[0]["method"], "notify");
    assert_eq!(r.notes[0]["params"]["code"], "profile_changed");
    assert!(
        r.notes[0]["params"]["message"]
            .as_str()
            .unwrap()
            .contains("バッテリー")
    );
    let st = status(&mut s);
    assert_eq!(st["profile"], "light");
    assert!(
        st["profile_reason"]
            .as_str()
            .unwrap()
            .contains("バッテリー")
    );

    // メモリ逼迫（危険）→ minimal。LLM を使わなくなる
    send(
        &mut s,
        "set_environment",
        json!({"memory_pressure": "critical"}),
    );
    let st = status(&mut s);
    assert_eq!(st["profile"], "minimal");
    assert_eq!(st["llm"], false);
    // minimal では辞書の結果（公園 は辞書の最良でもある。LLM を使わないことを確認するため別の読みを使う）
    type_str(&mut s, "p", "kouen");
    let r = press(&mut s, "p", "Space");
    assert_eq!(r["preedit"]["text"], "公園");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fixed_profile_in_config_is_not_switched_automatically() {
    let dir = temp_dir("fixed");
    std::fs::write(dir.join("config.toml"), "[profile]\nmode = \"quality\"\n").unwrap();
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(status(&mut s)["profile"], "quality");
    let r = send(
        &mut s,
        "set_environment",
        json!({"on_battery": true, "thermal": "critical"}),
    );
    assert!(r.notes.is_empty(), "固定なら切り替えない");
    assert_eq!(status(&mut s)["profile"], "quality");
    // auto に戻すと環境に従って下がる
    std::fs::write(dir.join("config.toml"), "[profile]\nmode = \"auto\"\n").unwrap();
    let r = send(&mut s, "reload_config", json!({}));
    assert_eq!(r.response["result"]["ok"], true);
    assert_eq!(
        status(&mut s)["profile"],
        "minimal",
        "危険なサーマル + バッテリーで balanced から 2 段"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn notifications_wait_until_initialize() {
    let dir = temp_dir("notify-order");
    std::fs::write(
        dir.join("config.toml"),
        "[input]\nshift_letter = \"bogus\"\n",
    )
    .unwrap();
    let mut s = server_in(&dir);
    // 壊れた設定でも起動できる。起動時の問題は initialize の応答の後に通知される
    let init = initialize(&mut s);
    assert_eq!(init.response["result"]["server_name"], "nagi-server");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 設定の再読み込み ----

fn shift_letter_behaviour(server: &mut Server, session: &str) -> String {
    let text = type_str(server, session, "Na");
    press(server, session, "Escape");
    text
}

#[test]
fn reload_config_applies_and_keeps_old_settings_on_errors() {
    let dir = temp_dir("reload");
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, "[input]\nshift_letter = \"single_upper\"\n").unwrap();
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(shift_letter_behaviour(&mut s, "a"), "Nあ");

    // 有効な変更が反映される（既存のセッションにも）
    std::fs::write(&cfg, "[input]\nshift_letter = \"literal_continue\"\n").unwrap();
    let r = send(&mut s, "reload_config", json!({}));
    assert_eq!(r.response["result"]["ok"], true);
    assert_eq!(shift_letter_behaviour(&mut s, "a"), "Na");

    // 壊れた設定 → 旧設定を維持し、行番号付きで通知
    std::fs::write(
        &cfg,
        "[conversion]\nnum_candidates = 5\n\n[input]\nshift_letter = \"bogus\"\n",
    )
    .unwrap();
    let r = send(&mut s, "reload_config", json!({}));
    assert_eq!(r.response["result"]["ok"], false);
    assert_eq!(r.response["result"]["errors"][0]["line"], 5);
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.notes[0]["params"]["level"], "error");
    assert_eq!(r.notes[0]["params"]["code"], "config_error");
    assert!(
        r.notes[0]["params"]["message"]
            .as_str()
            .unwrap()
            .contains("5 行目")
    );
    assert_eq!(shift_letter_behaviour(&mut s, "a"), "Na", "旧設定のまま");
    assert_eq!(status(&mut s)["config_error"], true);

    // 直すと戻る。不明なキーは警告だけ
    std::fs::write(
        &cfg,
        "[input]\nshift_letter = \"single_upper\"\nshift_leter = 1\n",
    )
    .unwrap();
    let r = send(&mut s, "reload_config", json!({}));
    assert_eq!(r.response["result"]["ok"], true);
    assert_eq!(r.response["result"]["errors"][0]["line"], 3);
    assert_eq!(r.notes[0]["params"]["level"], "warning");
    assert_eq!(status(&mut s)["config_error"], false);
    assert_eq!(shift_letter_behaviour(&mut s, "a"), "Nあ");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_file_changes_are_picked_up_automatically() {
    let dir = temp_dir("autoreload");
    let cfg = dir.join("config.toml");
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(shift_letter_behaviour(&mut s, "a"), "Na");
    std::fs::write(&cfg, "[input]\nshift_letter = \"single_upper\"\n").unwrap();
    std::thread::sleep(Duration::from_millis(1200)); // 確認の最短間隔（1 秒）を超える
    let r = send(&mut s, "ping", json!({}));
    assert!(r.response.get("error").is_none());
    assert_eq!(
        shift_letter_behaviour(&mut s, "a"),
        "Nあ",
        "ファイル監視で自動反映"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_starts_with_defaults_when_config_is_broken() {
    let dir = temp_dir("build-broken");
    std::fs::write(dir.join("config.toml"), "this is = = not toml\n").unwrap();
    let built = build(&ServerOptions {
        user_dir: Some(dir.clone()),
        use_llm: false,
        ..ServerOptions::default()
    });
    let events = built.runtime.drain_events();
    assert!(
        events.iter().any(|e| e.code == "config_error"),
        "{events:?}"
    );
    let mut s = Server::from_built(built);
    initialize(&mut s);
    assert_eq!(status(&mut s)["config_error"], true);
    // 既定値で入力できる
    assert_eq!(type_str(&mut s, "a", "ka"), "か");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- 学習 ----

#[test]
fn confirmed_candidates_are_learned_and_preferred_next_time() {
    let dir = temp_dir("learn");
    let mut s = server_in(&dir);
    initialize(&mut s);
    // 最初は LLM の 公園。2 回目の Space で次の候補（再採点で 後援）を選んで確定
    assert_eq!(convert_kouen(&mut s, "a"), "公園");
    let r = press(&mut s, "a", "Space");
    assert_eq!(r["preedit"]["text"], "後援");
    let r = press(&mut s, "a", "Enter");
    assert_eq!(r["commit"], "後援");
    assert_eq!(status(&mut s)["learned_entries"], 1);
    // 別のセッションでも、次は学習した表記が最初の変換結果になる
    assert_eq!(convert_kouen(&mut s, "b"), "後援");
    let r = press(&mut s, "b", "Space");
    let items = r["candidates"]["items"].as_array().unwrap();
    assert_eq!(items[0]["surface"], "後援");
    assert!(
        items[0]["sources"].to_string().contains("learned"),
        "学習由来の出所が付く: {items:?}"
    );
    press(&mut s, "b", "Escape");
    press(&mut s, "b", "Escape");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ctrl_delete_forgets_a_learned_candidate() {
    let dir = temp_dir("forget");
    let mut s = server_in(&dir);
    initialize(&mut s);
    convert_kouen(&mut s, "a");
    press(&mut s, "a", "Space");
    press(&mut s, "a", "Enter"); // 後援 を学習
    assert_eq!(convert_kouen(&mut s, "b"), "後援");
    // 候補窓を出して、現在の候補（後援）に戻してから Ctrl+Delete
    press(&mut s, "b", "Space");
    press(&mut s, "b", "Shift+Space");
    let r = press(&mut s, "b", "Ctrl+Delete");
    assert_eq!(r["consumed"], true);
    assert_eq!(status(&mut s)["learned_entries"], 0);
    press(&mut s, "b", "Escape");
    press(&mut s, "b", "Escape");
    assert_eq!(
        convert_kouen(&mut s, "c"),
        "公園",
        "忘れたので LLM の結果に戻る"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn learning_is_suspended_for_secure_input_and_password_managers() {
    let dir = temp_dir("secure");
    let mut s = server_in(&dir);
    initialize(&mut s);
    // secure input 中
    set_context(&mut s, "a", json!({"secure_input": true}));
    convert_kouen(&mut s, "a");
    press(&mut s, "a", "Space");
    press(&mut s, "a", "Enter");
    assert_eq!(status(&mut s)["learned_entries"], 0);
    // パスワードマネージャのアプリ
    set_context(&mut s, "b", json!({"app_id": "com.1password.1password"}));
    convert_kouen(&mut s, "b");
    press(&mut s, "b", "Space");
    press(&mut s, "b", "Enter");
    assert_eq!(status(&mut s)["learned_entries"], 0);
    // 通常のアプリでは学習する
    set_context(&mut s, "c", json!({"app_id": "com.example.editor"}));
    convert_kouen(&mut s, "c");
    press(&mut s, "c", "Space");
    press(&mut s, "c", "Enter");
    assert_eq!(status(&mut s)["learned_entries"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn learning_can_be_disabled_by_config_and_cleared() {
    let dir = temp_dir("learn-off");
    std::fs::write(dir.join("config.toml"), "[learning]\nenabled = false\n").unwrap();
    let mut s = server_in(&dir);
    initialize(&mut s);
    convert_kouen(&mut s, "a");
    press(&mut s, "a", "Space");
    press(&mut s, "a", "Enter");
    assert_eq!(status(&mut s)["learned_entries"], 0);
    assert_eq!(status(&mut s)["learning"], false);
    // 有効にして学習 → 全削除
    std::fs::write(dir.join("config.toml"), "[learning]\nenabled = true\n").unwrap();
    send(&mut s, "reload_config", json!({}));
    convert_kouen(&mut s, "a");
    press(&mut s, "a", "Space");
    press(&mut s, "a", "Enter");
    assert_eq!(status(&mut s)["learned_entries"], 1);
    let r = send(&mut s, "clear_learning", json!({}));
    assert!(r.response.get("error").is_none());
    assert_eq!(status(&mut s)["learned_entries"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn learning_survives_restart_via_atomic_file() {
    let dir = temp_dir("persist");
    {
        let mut s = server_in(&dir);
        initialize(&mut s);
        convert_kouen(&mut s, "a");
        press(&mut s, "a", "Space");
        press(&mut s, "a", "Enter");
        // フォーカス喪失で保存される
        send(&mut s, "focus_out", json!({"session_id": "a"}));
    }
    let file = std::fs::read_to_string(dir.join("learning.tsv")).unwrap();
    assert!(file.starts_with("#nagi-learn\tv1"), "{file}");
    assert!(file.contains("こうえん\t後援"), "{file}");
    assert!(!dir.join("learning.tsv.tmp").exists());
    // 再起動後も学習が効く
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(status(&mut s)["learned_entries"], 1);
    assert_eq!(convert_kouen(&mut s, "z"), "後援");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- ユーザー辞書・ランキング ----

#[test]
fn user_dictionary_words_and_snippets() {
    let dir = temp_dir("userdict");
    let ud = dir.join("user_dict");
    std::fs::create_dir_all(&ud).unwrap();
    std::fs::write(
        ud.join("mine.txt"),
        "こうえん\t公苑\t名詞\t自宅の最寄り\nひなた\t拝啓\\n敬具\tスニペット\n",
    )
    .unwrap();
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(status(&mut s)["user_dict_entries"], 2);
    // ユーザー辞書は LLM より優先（既定の優先順）。候補窓でも先頭に、出所は user_dict
    assert_eq!(convert_kouen(&mut s, "a"), "公苑");
    let r = press(&mut s, "a", "Space");
    let items = r["candidates"]["items"].as_array().unwrap();
    let pos = items.iter().position(|i| i["surface"] == "公苑").unwrap();
    assert!(
        items[pos]["sources"].to_string().contains("user_dict"),
        "{items:?}"
    );
    assert!(
        items.iter().any(|i| i["surface"] == "公園"),
        "LLM の結果も候補に残る"
    );
    press(&mut s, "a", "Escape");
    press(&mut s, "a", "Escape");

    // スニペット: 読みが一致する候補として出る（変換結果は置き換えない）
    type_str(&mut s, "b", "hinata");
    let r = press(&mut s, "b", "Space");
    assert_eq!(r["preedit"]["text"], "ひなた");
    let r = press(&mut s, "b", "Space");
    let items = r["candidates"]["items"].as_array().unwrap();
    assert!(
        items.iter().any(|i| i["surface"] == "拝啓\n敬具"),
        "{items:?}"
    );
    press(&mut s, "b", "Escape");
    press(&mut s, "b", "Escape");

    // ファイルを消して再読み込み → 反映される
    std::fs::remove_file(ud.join("mine.txt")).unwrap();
    let r = send(&mut s, "reload_config", json!({}));
    assert_eq!(r.response["result"]["ok"], true);
    assert_eq!(status(&mut s)["user_dict_entries"], 0);
    assert_eq!(convert_kouen(&mut s, "c"), "公園");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ranking_order_in_config_decides_who_wins() {
    let dir = temp_dir("ranking");
    let ud = dir.join("user_dict");
    std::fs::create_dir_all(&ud).unwrap();
    std::fs::write(ud.join("mine.txt"), "こうえん\t公苑\t名詞\n").unwrap();
    // llm を user_dict より前にすると、変換結果は LLM のまま（ユーザー辞書は候補に回る）
    std::fs::write(
        dir.join("config.toml"),
        "[ranking]\nsource_order = [\"llm\", \"user_dict\", \"learned\", \"system_dict\", \"kana\", \"rewriter\"]\n",
    )
    .unwrap();
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(convert_kouen(&mut s, "a"), "公園");
    let r = press(&mut s, "a", "Space");
    let items: Vec<&str> = r["candidates"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["surface"].as_str().unwrap())
        .collect();
    assert!(items.contains(&"公苑"), "{items:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn broken_user_dictionary_lines_are_reported_but_do_not_break_loading() {
    let dir = temp_dir("userdict-broken");
    let ud = dir.join("user_dict");
    std::fs::create_dir_all(&ud).unwrap();
    std::fs::write(ud.join("mine.txt"), "こうえん\t公苑\t名詞\nこわれた行\n").unwrap();
    let mut s = server_in(&dir);
    initialize(&mut s);
    std::fs::write(ud.join("mine.txt"), "こうえん\t公苑\t名詞\nこわれた行\n\n").unwrap();
    let r = send(&mut s, "reload_config", json!({}));
    assert_eq!(r.response["result"]["ok"], true);
    assert!(
        r.notes
            .iter()
            .any(|n| n["params"]["code"] == "user_dict_warning"
                && n["params"]["message"].as_str().unwrap().contains("2 行目")),
        "{:?}",
        r.notes
    );
    assert_eq!(status(&mut s)["user_dict_entries"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 他 IME のユーザー辞書の取り込み: MS-IME（UTF-16 LE・BOM）を `imported.tsv` に追記して即座に反映し、
/// 重複は既存優先でスキップ、不正な入力は INVALID_PARAMS
#[test]
fn import_user_dict_appends_dedupes_and_reloads() {
    use base64::Engine as _;
    let dir = temp_dir("userdict-import");
    let ud = dir.join("user_dict");
    std::fs::create_dir_all(&ud).unwrap();
    std::fs::write(ud.join("mine.txt"), "こうえん\t公苑\t名詞\n").unwrap();
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(status(&mut s)["user_dict_entries"], 1);

    let text = "!Microsoft IME Dictionary Tool\r\nこうえん\t公苑\t名詞\r\nこうえん\t後援\t名詞\r\n漢字\t不正\t名詞\r\nひなた\t日向\t地名\r\n";
    let mut bytes = vec![0xff, 0xfe];
    for u in text.encode_utf16() {
        bytes.extend(u.to_le_bytes());
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let r = send(&mut s, "import_user_dict", json!({"data_base64": data}));
    let result = &r.response["result"];
    assert_eq!(result["format"], "ms_ime", "{:?}", r.response);
    assert_eq!(result["total"], 4);
    assert_eq!(result["imported"], 2);
    assert_eq!(result["skipped"], 2);
    let reasons: Vec<&str> = result["skipped_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons, ["duplicate_existing", "invalid_reading"]);
    let written = std::fs::read_to_string(ud.join("imported.tsv")).unwrap();
    assert_eq!(written, "こうえん\t後援\t名詞\t\nひなた\t日向\t地名\t\n");
    // 即座に反映される
    assert_eq!(status(&mut s)["user_dict_entries"], 3);

    // 同じ内容をもう一度: すべて既存と重複で、何も書き足さない
    let r = send(
        &mut s,
        "import_user_dict",
        json!({"data_base64": data, "format": "ms_ime"}),
    );
    assert_eq!(r.response["result"]["imported"], 0);
    assert_eq!(
        std::fs::read_to_string(ud.join("imported.tsv")).unwrap(),
        written
    );

    // 不正な入力
    let r = send(&mut s, "import_user_dict", json!({"data_base64": "!!!"}));
    assert_eq!(r.response["error"]["code"], -32602);
    let r = send(&mut s, "import_user_dict", json!({"data_base64": ""}));
    assert_eq!(r.response["error"]["code"], -32602);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `userdict.enabled` / `snippet.enabled` を設定で切り替える（登録内容は消さない）
#[test]
fn userdict_and_snippet_can_be_disabled_by_config() {
    let dir = temp_dir("userdict-off");
    let ud = dir.join("user_dict");
    std::fs::create_dir_all(&ud).unwrap();
    std::fs::write(
        ud.join("mine.txt"),
        "こうえん\t公苑\t名詞\t自宅の最寄り\nひなた\t拝啓\\n敬具\tスニペット\n",
    )
    .unwrap();
    let snippet_shown = |s: &mut Server, session: &str| {
        type_str(s, session, "hinata");
        press(s, session, "Space");
        let r = press(s, session, "Space");
        let shown = r["candidates"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|i| i["surface"] == "拝啓\n敬具"));
        press(s, session, "Escape");
        press(s, session, "Escape");
        shown
    };
    let mut s = server_in(&dir);
    initialize(&mut s);
    assert_eq!(convert_kouen(&mut s, "a"), "公苑");
    assert!(snippet_shown(&mut s, "a2"));

    // 語だけ無効: 変換は LLM 結果に戻り、スニペットは残る。登録件数の表示は変わらない
    std::fs::write(dir.join("config.toml"), "[userdict]\nenabled = false\n").unwrap();
    let r = send(&mut s, "reload_config", json!({}));
    assert_eq!(r.response["result"]["ok"], true);
    assert_eq!(convert_kouen(&mut s, "b"), "公園");
    assert!(snippet_shown(&mut s, "b2"));
    assert_eq!(status(&mut s)["user_dict_entries"], 2);

    // スニペットだけ無効
    std::fs::write(dir.join("config.toml"), "[snippet]\nenabled = false\n").unwrap();
    send(&mut s, "reload_config", json!({}));
    assert_eq!(convert_kouen(&mut s, "c"), "公苑");
    assert!(!snippet_shown(&mut s, "c2"));

    // 設定を消せば元に戻る
    std::fs::write(dir.join("config.toml"), "").unwrap();
    send(&mut s, "reload_config", json!({}));
    assert!(snippet_shown(&mut s, "d"));
    let _ = std::fs::remove_dir_all(&dir);
}
