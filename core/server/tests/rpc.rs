//! JSON-RPC 統合テスト: プロセス内（`Server::handle_line`）と、`imserver` バイナリを起動した状態の両方で検査する。

use nagi_engine::{EngineConfig, KanaBackend};
use nagi_server::Server;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

// ---- プロセス内 ----

fn server() -> Server {
    Server::new(Arc::new(KanaBackend), "minimal", EngineConfig::default())
}

fn call(server: &mut Server, request: Value) -> Value {
    let lines = server.handle_line(&request.to_string());
    assert_eq!(lines.len(), 1, "応答は 1 行のはず: {lines:?}");
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn init(server: &mut Server) {
    let r = call(
        server,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"test","client_version":"0","os":"test"}}),
    );
    assert_eq!(r["result"]["protocol_version"], 1);
}

fn key(id: i64, session: &str, keysym: u32, text: &str, shift: bool) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"process_key","params":{
        "session_id":session,"keysym":keysym,"keycode":0,"text":text,
        "modifiers":{"shift":shift},"kind":"down"}})
}

fn type_str(server: &mut Server, session: &str, s: &str) -> Value {
    let mut last = Value::Null;
    for (i, c) in s.chars().enumerate() {
        last = call(
            server,
            key(100 + i as i64, session, c as u32, &c.to_string(), false),
        );
    }
    last
}

#[test]
fn initialize_reports_profile_and_capabilities() {
    let mut s = server();
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":"init-1","method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"1","os":"macos"}}),
    );
    assert_eq!(r["id"], "init-1", "文字列 ID がそのまま返る");
    assert_eq!(r["result"]["profile"], "minimal");
    assert_eq!(r["result"]["mode"], "hiragana");
    assert_eq!(r["result"]["server_name"], "nagi-server");
    assert_eq!(r["result"]["capabilities"]["reconvert"], false);
    assert!(r.get("error").is_none());
}

#[test]
fn unsupported_protocol_version_is_rejected() {
    let mut s = server();
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":99,"client_name":"t","client_version":"1","os":"x"}}),
    );
    assert_eq!(r["error"]["code"], -32005);
}

#[test]
fn requests_before_initialize_are_rejected_except_ping_and_shutdown() {
    let mut s = server();
    let r = call(&mut s, key(1, "s", 'a' as u32, "a", false));
    assert_eq!(r["error"]["code"], -32002);
    let r = call(&mut s, json!({"jsonrpc":"2.0","id":2,"method":"ping"}));
    assert_eq!(r["result"], json!({}));
}

#[test]
fn malformed_input_yields_protocol_errors() {
    let mut s = server();
    let lines = s.handle_line("{not json");
    let r: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["id"], Value::Null);
    let lines = s.handle_line(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#);
    let r: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(r["error"]["code"], -32600);
    // 空行は無視
    assert!(s.handle_line("").is_empty());
    assert!(s.handle_line("   \r\n").is_empty());
}

#[test]
fn unknown_method_and_bad_params() {
    let mut s = server();
    init(&mut s);
    let r = call(&mut s, json!({"jsonrpc":"2.0","id":5,"method":"no_such"}));
    assert_eq!(r["error"]["code"], -32601);
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":6,"method":"process_key","params":{"session_id":"s"}}),
    );
    assert_eq!(r["error"]["code"], -32602);
    // 入力内容がエラーメッセージに漏れない
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":7,"method":"process_key","params":{
            "session_id":"s","keysym":"SECRET-TEXT","keycode":0,"text":"x","modifiers":{},"kind":"down"}}),
    );
    assert_eq!(r["error"]["code"], -32602);
    assert!(!r.to_string().contains("SECRET-TEXT"), "{r}");
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":8,"method":"reconvert","params":{"session_id":"s","text":"a"}}),
    );
    assert_eq!(r["error"]["code"], -32003);
}

#[test]
fn notifications_get_no_response() {
    let mut s = server();
    init(&mut s);
    let lines = s.handle_line(
        &json!({"jsonrpc":"2.0","method":"set_context","params":{
            "session_id":"s","surrounding_text":{"text":"私の名前は","cursor":5},"app_id":"x","secure_input":false}})
        .to_string(),
    );
    assert!(lines.is_empty());
    // 未知の通知も無視される
    assert!(
        s.handle_line(r#"{"jsonrpc":"2.0","method":"whatever"}"#)
            .is_empty()
    );
}

#[test]
fn typing_produces_incremental_diffs() {
    let mut s = server();
    init(&mut s);
    let r = call(&mut s, key(1, "s1", 'k' as u32, "k", false));
    assert_eq!(r["result"]["consumed"], true);
    assert_eq!(r["result"]["preedit"]["text"], "k");
    assert_eq!(
        r["result"]["preedit"]["pending"],
        json!({"start":0,"end":1})
    );
    let rev1 = r["result"]["revision"].as_u64().unwrap();
    let r = call(&mut s, key(2, "s1", 'a' as u32, "a", false));
    assert_eq!(r["result"]["preedit"]["text"], "か");
    assert_eq!(r["result"]["preedit"]["cursor"], 1);
    assert!(r["result"]["revision"].as_u64().unwrap() > rev1);
    assert!(
        r["result"].get("commit").is_none(),
        "commit なしならキー自体が無い"
    );
    // Enter で確定
    let r = call(&mut s, key(3, "s1", 0xff0d, "", false));
    assert_eq!(r["result"]["commit"], "か");
    assert_eq!(r["result"]["preedit"]["text"], "");
    // Direct の Enter は素通し（変化なし）
    let r = call(&mut s, key(4, "s1", 0xff0d, "", false));
    assert_eq!(r["result"]["consumed"], false);
    assert!(r["result"].get("preedit").is_none());
}

#[test]
fn sessions_are_independent() {
    let mut s = server();
    init(&mut s);
    type_str(&mut s, "a", "ka");
    let r = type_str(&mut s, "b", "ki");
    assert_eq!(r["result"]["preedit"]["text"], "き");
    let r = call(&mut s, key(50, "a", 0xff0d, "", false));
    assert_eq!(r["result"]["commit"], "か");
}

#[test]
fn convert_flow_with_kana_backend() {
    let mut s = server();
    init(&mut s);
    type_str(&mut s, "s", "ka");
    // Space で変換（KanaBackend は読みのまま）
    let r = call(&mut s, key(60, "s", 0x20, " ", false));
    assert_eq!(r["result"]["preedit"]["focus"], 0);
    assert_eq!(r["result"]["preedit"]["segments"][0]["kind"], "japanese");
    // 2 回目の Space で候補ウィンドウ（ひらがな → カタカナの順）
    let r = call(&mut s, key(61, "s", 0x20, " ", false));
    assert_eq!(r["result"]["candidates"]["visible"], true);
    assert_eq!(r["result"]["preedit"]["text"], "カ");
    let r = call(&mut s, key(62, "s", 0xff0d, "", false));
    assert_eq!(r["result"]["commit"], "カ");
}

#[test]
fn focus_out_commits_and_reset_discards() {
    let mut s = server();
    init(&mut s);
    type_str(&mut s, "s", "ka");
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":70,"method":"focus_out","params":{"session_id":"s"}}),
    );
    assert_eq!(r["result"]["commit"], "か");
    type_str(&mut s, "s", "ki");
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":71,"method":"reset","params":{"session_id":"s"}}),
    );
    assert!(r["result"].get("commit").is_none());
    assert_eq!(r["result"]["preedit"]["text"], "");
}

#[test]
fn restore_session_rebuilds_preedit() {
    let mut s = server();
    init(&mut s);
    let r = call(
        &mut s,
        json!({"jsonrpc":"2.0","id":80,"method":"restore_session","params":{
            "session_id":"s","segments":[{"reading":"かな","surface":"かな","kind":"japanese"},
                                          {"reading":"N","surface":"N","kind":"literal"}],
            "cursor":2,"converting":false}}),
    );
    assert_eq!(r["result"]["preedit"]["text"], "かなN");
    assert_eq!(r["result"]["preedit"]["cursor"], 2);
    // 復元後も編集を続けられる
    let r = type_str(&mut s, "s", "ki");
    assert_eq!(r["result"]["preedit"]["text"], "かなきN");
}

#[test]
fn shutdown_stops_the_loop() {
    let mut s = server();
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
        "\n"
    );
    let mut out = Vec::new();
    s.run(input.as_bytes(), &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 2, "shutdown 後は処理しない: {text}");
    assert!(s.should_shutdown());
}

// ---- 実プロセス ----

struct Proc {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Proc {
    fn spawn(args: &[&str]) -> Proc {
        let mut child = Command::new(env!("CARGO_BIN_EXE_imserver"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("imserver の起動");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Proc {
            child,
            stdin: Some(stdin),
            stdout,
            next_id: 1,
        }
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string();
        let stdin = self.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut out = String::new();
        self.stdout.read_line(&mut out).expect("応答の読み取り");
        let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out:?}"));
        assert_eq!(v["id"], id);
        v
    }

    fn initialize(&mut self) -> Value {
        self.rpc(
            "initialize",
            json!({"protocol_version":1,"client_name":"test","client_version":"0","os":"test"}),
        )
    }

    fn press(&mut self, session: &str, c: char) -> Value {
        self.rpc(
            "process_key",
            json!({"session_id":session,"keysym":c as u32,"keycode":0,"text":c.to_string(),
                   "modifiers":{},"kind":"down"}),
        )
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn process_round_trip_and_clean_shutdown() {
    let mut p = Proc::spawn(&[]);
    let r = p.initialize();
    assert_eq!(r["result"]["profile"], "minimal");
    assert_eq!(p.press("s", 'k')["result"]["preedit"]["text"], "k");
    assert_eq!(p.press("s", 'a')["result"]["preedit"]["text"], "か");
    let r = p.rpc("shutdown", json!({}));
    assert_eq!(r["result"], json!({}));
    let status = p.child.wait().unwrap();
    assert!(status.success(), "shutdown 後は正常終了する: {status:?}");
}

#[test]
fn eof_on_stdin_exits_cleanly() {
    let mut p = Proc::spawn(&[]);
    p.initialize();
    // stdin を閉じる（EOF）
    drop(p.stdin.take());
    let status = p.child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn engine_restart_with_restore_session_keeps_input() {
    // 1 つ目のプロセスで入力途中まで進める
    let mut a = Proc::spawn(&[]);
    a.initialize();
    a.press("s", 'k');
    let shown = a.press("s", 'a')["result"]["preedit"].clone();
    assert_eq!(shown["text"], "か");
    // クラッシュ（強制終了）を模擬
    a.child.kill().unwrap();
    a.child.wait().unwrap();
    drop(a);
    // 新しいプロセスに、フロントが保持していた preedit を復元させる
    let mut b = Proc::spawn(&[]);
    b.initialize();
    let r = b.rpc(
        "restore_session",
        json!({"session_id":"s","segments":[{"reading":"か","surface":"か","kind":"japanese"}],
               "cursor":1,"converting":false}),
    );
    assert_eq!(r["result"]["preedit"]["text"], "か");
    let r = b.rpc(
        "process_key",
        json!({"session_id":"s","keysym":0xff0d,"keycode":0,"text":"","modifiers":{},"kind":"down"}),
    );
    assert_eq!(r["result"]["commit"], "か");
}

fn data_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    dir.join("dict/nagi-dict.bin").exists().then_some(dir)
}

/// 実辞書での E2E（辞書が無い環境では skip）: ローマ字入力 → Space 変換 → 文節移動 → 確定
#[test]
fn dictionary_backed_conversion_end_to_end() {
    let Some(dir) = data_dir() else {
        eprintln!("skip: data/dict/nagi-dict.bin が無い（cargo xtask build-dict）");
        return;
    };
    let mut p = Proc::spawn(&["--data-dir", dir.to_str().unwrap(), "--no-llm"]);
    assert_eq!(p.initialize()["result"]["profile"], "minimal");
    for c in "kyouhaiitenkidesu".chars() {
        p.press("s", c);
    }
    let r = p.rpc(
        "process_key",
        json!({"session_id":"s","keysym":0x20,"keycode":0,"text":" ","modifiers":{},"kind":"down"}),
    );
    assert_eq!(r["result"]["preedit"]["text"], "今日はいい天気です");
    let segs: Vec<&str> = r["result"]["preedit"]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["surface"].as_str().unwrap())
        .collect();
    assert_eq!(segs, ["今日は", "いい天気です"]);
    // 2 回目の Space で候補窓（同じ文節の別候補）
    let r = p.rpc(
        "process_key",
        json!({"session_id":"s","keysym":0x20,"keycode":0,"text":" ","modifiers":{},"kind":"down"}),
    );
    assert_eq!(r["result"]["candidates"]["visible"], true);
    let items: Vec<&str> = r["result"]["candidates"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["surface"].as_str().unwrap())
        .collect();
    assert_eq!(items[0], "今日は");
    assert!(items.len() > 3, "{items:?}");
    let r = p.rpc(
        "process_key",
        json!({"session_id":"s","keysym":0xff0d,"keycode":0,"text":"","modifiers":{},"kind":"down"}),
    );
    assert!(
        r["result"]["commit"]
            .as_str()
            .unwrap()
            .ends_with("いい天気です")
    );
}

#[test]
fn missing_data_dir_falls_back_to_kana() {
    let mut p = Proc::spawn(&["--data-dir", "Z:/no/such/dir"]);
    let r = p.initialize();
    assert_eq!(r["result"]["profile"], "minimal");
    p.press("s", 'k');
    assert_eq!(p.press("s", 'a')["result"]["preedit"]["text"], "か");
}

/// 辞書 + LLM の E2E（feature `llama` でビルドし、辞書とモデルがある場合のみ。無ければ skip）
#[cfg(feature = "llama")]
#[test]
fn llm_backed_conversion_end_to_end() {
    let Some(dir) = data_dir() else {
        eprintln!("skip: 辞書が無い");
        return;
    };
    if !dir.join("models/zenz-v3.1-small-Q5_K_M.gguf").exists() {
        eprintln!("skip: モデルが無い（cargo xtask fetch-model）");
        return;
    }
    let mut p = Proc::spawn(&["--data-dir", dir.to_str().unwrap()]);
    let init = p.initialize();
    assert_eq!(init["result"]["profile"], "balanced");
    for c in "watashinonamaehanakanodesu".chars() {
        p.press("s", c);
    }
    let started = std::time::Instant::now();
    let r = p.rpc(
        "process_key",
        json!({"session_id":"s","keysym":0x20,"keycode":0,"text":" ","modifiers":{},"kind":"down"}),
    );
    eprintln!("Space -> convert: {:?}", started.elapsed());
    assert_eq!(r["result"]["preedit"]["text"], "私の名前は中野です");
    let segs: Vec<&str> = r["result"]["preedit"]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["surface"].as_str().unwrap())
        .collect();
    assert!(segs.len() >= 2, "文節に区切られる: {segs:?}");
    // 2 回目の Space で候補窓。LLM の出所が付く
    let r = p.rpc(
        "process_key",
        json!({"session_id":"s","keysym":0x20,"keycode":0,"text":" ","modifiers":{},"kind":"down"}),
    );
    assert_eq!(r["result"]["candidates"]["visible"], true);
    let r = p.rpc(
        "process_key",
        json!({"session_id":"s","keysym":0xff0d,"keycode":0,"text":"","modifiers":{},"kind":"down"}),
    );
    assert!(r["result"]["commit"].as_str().unwrap().len() > 3);
}

#[test]
fn no_llm_flag_uses_minimal_profile() {
    let Some(dir) = data_dir() else {
        return;
    };
    let mut p = Proc::spawn(&["--data-dir", dir.to_str().unwrap(), "--no-llm"]);
    assert_eq!(p.initialize()["result"]["profile"], "minimal");
}
