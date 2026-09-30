//! モデルの非同期読み込み（`ServerOptions::async_model_load`）のテスト。ローダーは差し替えた偽物なので、
//! 推論ライブラリ（feature `llama`）もモデルファイルも要らない。

use nagi_engine::ConversionBackend;
use nagi_infer::ConversionModel;
use nagi_protocol::view::CandidateSource;
use nagi_server::{Built, ModelLoader, Server, ServerOptions, build_with_loader};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod common;
use common::{FakeModel, dict};

/// テストごとの一時ディレクトリ（辞書と、更新確認を切った設定を置く）
fn temp_dirs(tag: &str) -> ServerOptions {
    let root: PathBuf =
        std::env::temp_dir().join(format!("nagi-async-load-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/dict")).unwrap();
    std::fs::create_dir_all(root.join("user")).unwrap();
    std::fs::write(root.join("data/dict/nagi-dict.bin"), dict().to_bytes()).unwrap();
    std::fs::write(
        root.join("user/config.toml"),
        "[update]\nauto_check = false\n",
    )
    .unwrap();
    ServerOptions {
        data_dir: Some(root.join("data")),
        user_dir: Some(root.join("user")),
        async_model_load: true,
        ..ServerOptions::default()
    }
}

/// `release` に送るまで待つローダー
fn gated_loader(release: mpsc::Receiver<()>, fail: bool) -> ModelLoader {
    Box::new(move || {
        let _ = release.recv();
        if fail {
            Err("テスト用の読み込み失敗".to_string())
        } else {
            Ok(
                Box::new(FakeModel::new(&[("こうえんにいく", "公園に行く")]))
                    as Box<dyn ConversionModel>,
            )
        }
    })
}

fn call(server: &mut Server, request: Value) -> Value {
    let lines = server.handle_line(&request.to_string());
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn init_and_key(server: &mut Server) {
    let r = call(
        server,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocol_version":1,"client_name":"t","client_version":"0","os":"test"}}),
    );
    assert_eq!(r["result"]["protocol_version"], 1);
    let r = call(
        server,
        json!({"jsonrpc":"2.0","id":2,"method":"process_key","params":{
            "session_id":"s","keysym":97,"keycode":0,"text":"a",
            "modifiers":{},"kind":"down"}}),
    );
    assert_eq!(r["result"]["consumed"], true);
}

fn uses_llm(built: &Built) -> bool {
    built
        .pipeline
        .convert_sentence("こうえんにいく", "")
        .iter()
        .any(|s| s.sources.contains(&CandidateSource::Llm))
}

fn wait_until(mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn slow_loader_does_not_block_initialize_or_process_key() {
    let options = temp_dirs("slow");
    let (release, gate) = mpsc::channel();
    let started = Instant::now();
    let built = build_with_loader(&options, Some(gated_loader(gate, false)));
    let runtime = built.runtime.clone();
    let mut server = Server::from_built(built);
    init_and_key(&mut server);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "ローダーが未完了でも起動と応答は即座に返る: {:?}",
        started.elapsed()
    );
    assert_eq!(runtime.profile(), nagi_profile::Profile::Minimal);
    assert!(!runtime.effective().use_llm);
    let _ = release.send(());
}

#[test]
fn llm_path_and_profile_notification_activate_after_loader_completes() {
    let options = temp_dirs("done");
    let (release, gate) = mpsc::channel();
    let built = build_with_loader(&options, Some(gated_loader(gate, false)));
    assert!(!uses_llm(&built), "読み込み中は辞書のみ");
    assert!(
        built
            .runtime
            .drain_events()
            .iter()
            .all(|e| e.code != "profile_changed")
    );

    release.send(()).unwrap();
    assert!(
        wait_until(|| uses_llm(&built)),
        "ローダー完了後に LLM 経路が有効になる"
    );
    assert_ne!(built.runtime.profile(), nagi_profile::Profile::Minimal);
    assert!(built.runtime.effective().use_llm);
    let mut seen = false;
    assert!(wait_until(|| {
        seen |= built
            .runtime
            .drain_events()
            .iter()
            .any(|e| e.code == "profile_changed");
        seen
    }));
    assert_eq!(built.pipeline.name(), "dict+llm");
}

#[test]
fn loader_failure_keeps_dictionary_only() {
    let options = temp_dirs("fail");
    let (release, gate) = mpsc::channel();
    let built = build_with_loader(&options, Some(gated_loader(gate, true)));
    release.send(()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let mut server = Server::from_built(built);
    init_and_key(&mut server);
    assert_eq!(server.runtime().profile(), nagi_profile::Profile::Minimal);
    assert!(!server.runtime().effective().use_llm);
}

#[test]
fn sync_mode_loads_before_build_returns() {
    let mut options = temp_dirs("sync");
    options.async_model_load = false;
    let (release, gate) = mpsc::channel();
    release.send(()).unwrap();
    let built = build_with_loader(&options, Some(gated_loader(gate, false)));
    assert_ne!(built.runtime.profile(), nagi_profile::Profile::Minimal);
    assert!(wait_until(|| uses_llm(&built)));
}
