//! 変換モデルだけの解放 / 再読み込み（`unload_model` / `reload_model`、メモリ逼迫・メモリ上限による自動解放）の
//! テスト。ローダーは差し替えた偽物なので、推論ライブラリ（feature `llama`）もモデルファイルも要らない。

use nagi_engine::ConversionBackend;
use nagi_infer::{ConversionModel, ConvertRequest, Hypothesis, InferError};
use nagi_protocol::view::CandidateSource;
use nagi_server::{ModelLoader, Server, ServerOptions, build_with_loader};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

mod common;
use common::{FakeModel, dict};

/// `set_active` と、解放中に推論が走っていないかを記録する偽モデル
struct Probe {
    inner: FakeModel,
    active: Arc<AtomicBool>,
    /// 解放中（`set_active(false)` 後）に `convert` / `score` が呼ばれた回数（0 でなければバグ）
    ran_inactive: Arc<AtomicUsize>,
    /// `set_active(true)` を失敗させる
    fail_reload: Arc<AtomicBool>,
    /// 指定されていれば、`convert` はここに送られるまで待つ
    gate: Option<Mutex<mpsc::Receiver<()>>>,
    started: Arc<AtomicUsize>,
    /// 設定されていれば、`set_active(true)` はここに送られるまで待つ
    reload_gate: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    /// `set_active(true)` に入った回数
    reload_entered: Arc<AtomicUsize>,
}

/// テスト側が持つ `Probe` の操作口
#[derive(Clone)]
struct Handles {
    active: Arc<AtomicBool>,
    ran_inactive: Arc<AtomicUsize>,
    fail_reload: Arc<AtomicBool>,
    started: Arc<AtomicUsize>,
    reload_gate: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    reload_entered: Arc<AtomicUsize>,
}

fn probe(gate: Option<mpsc::Receiver<()>>) -> (Probe, Handles) {
    let handles = Handles {
        active: Arc::new(AtomicBool::new(true)),
        ran_inactive: Arc::new(AtomicUsize::new(0)),
        fail_reload: Arc::new(AtomicBool::new(false)),
        started: Arc::new(AtomicUsize::new(0)),
        reload_gate: Arc::new(Mutex::new(None)),
        reload_entered: Arc::new(AtomicUsize::new(0)),
    };
    let p = Probe {
        inner: FakeModel::new(&[("こうえんにいく", "公園に行く")]),
        active: handles.active.clone(),
        ran_inactive: handles.ran_inactive.clone(),
        fail_reload: handles.fail_reload.clone(),
        gate: gate.map(Mutex::new),
        started: handles.started.clone(),
        reload_gate: handles.reload_gate.clone(),
        reload_entered: handles.reload_entered.clone(),
    };
    (p, handles)
}

impl ConversionModel for Probe {
    fn id(&self) -> &str {
        "probe"
    }

    fn convert(&mut self, req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError> {
        if !self.active.load(Ordering::SeqCst) {
            self.ran_inactive.fetch_add(1, Ordering::SeqCst);
        }
        self.started.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            let _ = gate.lock().unwrap().recv();
        }
        self.inner.convert(req)
    }

    fn score(&mut self, req: &ConvertRequest, c: &[String]) -> Result<Vec<f32>, InferError> {
        if !self.active.load(Ordering::SeqCst) {
            self.ran_inactive.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.score(req, c)
    }

    fn set_active(&mut self, active: bool) -> Result<(), InferError> {
        if active {
            self.reload_entered.fetch_add(1, Ordering::SeqCst);
            let gate = self.reload_gate.lock().unwrap().take();
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
        }
        if active && self.fail_reload.load(Ordering::SeqCst) {
            return Err(InferError::Other("テスト用の再読み込み失敗".into()));
        }
        self.active.store(active, Ordering::SeqCst);
        Ok(())
    }
}

fn options(tag: &str) -> ServerOptions {
    let root: PathBuf =
        std::env::temp_dir().join(format!("nagi-model-unload-{}-{tag}", std::process::id()));
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
        sync_budget_ms: Some(20),
        ..ServerOptions::default()
    }
}

fn call(server: &mut Server, request: Value) -> Value {
    let lines = server.handle_line(&request.to_string());
    serde_json::from_str(&lines[0]).expect("応答は JSON")
}

fn rpc(server: &mut Server, method: &str, params: Value) -> Value {
    call(
        server,
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    )
}

fn uses_llm(pipeline: &dyn ConversionBackend) -> bool {
    pipeline
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

struct Fixture {
    server: Server,
    pipeline: Arc<nagi_server::Pipeline>,
    runtime: Arc<nagi_server::Runtime>,
    handles: Handles,
}

fn fixture(
    tag: &str,
    gate: Option<mpsc::Receiver<()>>,
    mutate: impl FnOnce(&mut ServerOptions),
) -> Fixture {
    let mut options = options(tag);
    mutate(&mut options);
    let (model, handles) = probe(gate);
    let loader: ModelLoader = Box::new(move || Ok(Box::new(model) as Box<dyn ConversionModel>));
    let built = build_with_loader(&options, Some(loader));
    let (pipeline, runtime) = (built.pipeline.clone(), built.runtime.clone());
    let mut server = Server::from_built(built);
    let r = rpc(
        &mut server,
        "initialize",
        json!({"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}),
    );
    assert_eq!(r["result"]["protocol_version"], 1);
    Fixture {
        server,
        pipeline,
        runtime,
        handles,
    }
}

#[test]
fn unload_and_reload_methods_switch_between_dictionary_only_and_llm() {
    let mut f = fixture("rpc", None, |_| {});
    assert!(wait_until(|| uses_llm(&*f.pipeline)), "初期は LLM が使える");

    let r = rpc(&mut f.server, "unload_model", json!({}));
    assert_eq!(r["result"], json!({}), "{r}");
    assert!(!f.runtime.effective().use_llm);
    assert_eq!(f.runtime.profile(), nagi_profile::Profile::Minimal);
    assert!(!uses_llm(&*f.pipeline), "解放中は辞書のみ");
    assert!(
        wait_until(|| !f.handles.active.load(Ordering::SeqCst)),
        "モデルの重みが解放される"
    );
    let status = rpc(&mut f.server, "get_status", json!({}));
    assert_eq!(status["result"]["llm"], false);
    // 辞書のみでも入力は続けられる
    let key = rpc(
        &mut f.server,
        "process_key",
        json!({"session_id":"s","keysym":97,"keycode":0,"text":"a","modifiers":{},"kind":"down"}),
    );
    assert_eq!(key["result"]["consumed"], true);
    // 明示の解放は、メモリ逼迫の解消などでは戻らない（reload_model まで）
    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_pressure":"normal"}),
    );
    std::thread::sleep(Duration::from_millis(100));
    assert!(!f.runtime.effective().use_llm);

    let r = rpc(&mut f.server, "reload_model", json!({}));
    assert_eq!(r["result"], json!({}), "{r}");
    assert!(wait_until(|| f.runtime.effective().use_llm));
    assert!(f.handles.active.load(Ordering::SeqCst));
    assert!(wait_until(|| uses_llm(&*f.pipeline)));
    assert_eq!(f.handles.ran_inactive.load(Ordering::SeqCst), 0);
    let status = rpc(&mut f.server, "get_status", json!({}));
    assert_eq!(status["result"]["llm"], true);
}

#[test]
fn critical_memory_pressure_unloads_and_normal_reloads_after_delay() {
    let mut f = fixture("pressure", None, |_| {});
    f.runtime.set_model_reload_delay(Duration::from_millis(300));
    assert!(wait_until(|| uses_llm(&*f.pipeline)));

    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_pressure":"critical"}),
    );
    assert!(!f.runtime.effective().use_llm);
    assert!(f.runtime.model_unloaded());
    assert!(wait_until(|| !f.handles.active.load(Ordering::SeqCst)));
    assert!(!uses_llm(&*f.pipeline));

    // 警告に下がっただけでは戻さない
    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_pressure":"warning"}),
    );
    std::thread::sleep(Duration::from_millis(400));
    rpc(&mut f.server, "ping", json!({}));
    assert!(f.runtime.model_unloaded(), "warning の間は解放したまま");

    // normal に戻っても、待ち時間が経つまでは戻さない
    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_pressure":"normal"}),
    );
    assert!(f.runtime.model_unloaded());
    assert!(!f.runtime.effective().use_llm);
    // 時間が経ち、次のリクエストで再読み込みされる
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        wait_until(|| {
            rpc(&mut f.server, "ping", json!({}));
            f.runtime.effective().use_llm
        }),
        "normal から待ち時間後に再読み込みされる"
    );
    assert!(f.handles.active.load(Ordering::SeqCst));
    assert!(wait_until(|| uses_llm(&*f.pipeline)));
    assert_eq!(f.handles.ran_inactive.load(Ordering::SeqCst), 0);
}

#[test]
fn reload_failure_continues_dictionary_only_without_retrying() {
    let mut f = fixture("fail", None, |_| {});
    f.runtime.set_model_reload_delay(Duration::ZERO);
    assert!(wait_until(|| uses_llm(&*f.pipeline)));
    f.handles.fail_reload.store(true, Ordering::SeqCst);

    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_pressure":"critical"}),
    );
    assert!(wait_until(|| !f.handles.active.load(Ordering::SeqCst)));
    // 通知は set_environment 自身の応答の行に混ざることもあるため、その行も調べる
    let mut warned = false;
    let lines = f.server.handle_line(
        &json!({"jsonrpc":"2.0","id":3,"method":"set_environment","params":{"memory_pressure":"normal"}})
            .to_string(),
    );
    warned |= lines.iter().any(|l| l.contains("llm_reload_failed"));
    // 再読み込みを試みて失敗し、辞書のみで継続する（警告の通知が出る）
    assert!(wait_until(|| {
        let lines = f
            .server
            .handle_line(&json!({"jsonrpc":"2.0","id":2,"method":"ping"}).to_string());
        warned |= lines.iter().any(|l| l.contains("llm_reload_failed"));
        warned
    }));
    assert!(!f.runtime.effective().use_llm);
    assert_eq!(f.runtime.profile(), nagi_profile::Profile::Minimal);
    assert!(!uses_llm(&*f.pipeline));
    assert!(!f.handles.active.load(Ordering::SeqCst));
    let r = rpc(
        &mut f.server,
        "process_key",
        json!({"session_id":"s","keysym":97,"keycode":0,"text":"a","modifiers":{},"kind":"down"}),
    );
    assert_eq!(r["result"]["consumed"], true);

    // 自動では再試行しない
    std::thread::sleep(Duration::from_millis(200));
    rpc(&mut f.server, "ping", json!({}));
    assert!(!f.runtime.effective().use_llm);

    // 原因が解消したあとの明示の再読み込みで戻る
    f.handles.fail_reload.store(false, Ordering::SeqCst);
    rpc(&mut f.server, "reload_model", json!({}));
    assert!(wait_until(|| f.runtime.effective().use_llm));
    assert!(wait_until(|| uses_llm(&*f.pipeline)));
}

#[test]
fn unload_does_not_wait_for_inflight_inference_and_discards_its_result() {
    let (release, gate) = mpsc::channel();
    let mut f = fixture("inflight", Some(gate), |_| {});
    // 起動時のウォームアップ推論（偽モデルは試験用の読みを知らないので、ページインと最初の 1 回で
    // 打ち切られて計 2 回）はゲートが開くまで止まっている。ちょうどその分だけ開ける
    assert!(wait_until(|| f.handles.started.load(Ordering::SeqCst) >= 1));
    for _ in 0..2 {
        release.send(()).unwrap();
    }
    assert!(wait_until(|| f.handles.started.load(Ordering::SeqCst) >= 2));
    std::thread::sleep(Duration::from_millis(100));
    assert!(f.runtime.effective().use_llm);
    let _ = f.pipeline.take_completions();
    let before = f.handles.started.load(Ordering::SeqCst);

    // 推論を始めて（予算 20ms を超えたので辞書の結果で返る）、バックグラウンドで止まらせる
    let segments = f.pipeline.convert_sentence("こうえんにいく", "");
    assert!(
        !segments
            .iter()
            .any(|s| s.sources.contains(&CandidateSource::Llm)),
        "ゲートが閉じているので LLM の結果はまだ無い"
    );
    assert!(wait_until(
        || f.handles.started.load(Ordering::SeqCst) > before
    ));

    // 実行中の推論があっても、解放は待たずに返る
    let started = Instant::now();
    let r = rpc(&mut f.server, "unload_model", json!({}));
    assert_eq!(r["result"], json!({}));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "unload_model が推論の完了を待った: {:?}",
        started.elapsed()
    );
    assert!(!f.runtime.effective().use_llm);
    assert!(
        !uses_llm(&*f.pipeline),
        "解放後の変換は辞書のみで即座に返る"
    );

    // 止めていた推論が終わっても、その結果は通知にもキャッシュにも使われない
    release.send(()).unwrap();
    assert!(wait_until(|| !f.handles.active.load(Ordering::SeqCst)));
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        f.pipeline.take_completions().is_empty(),
        "解放前に始めた推論の完了通知は捨てられる"
    );
    // 解放後に推論は始まっていない（重みの読み直しもない）
    assert_eq!(f.handles.ran_inactive.load(Ordering::SeqCst), 0);
    // 再読み込みで LLM が戻る（再読み込み後の試し推論のためにゲートを開けておく）
    let _ = release.send(());
    rpc(&mut f.server, "reload_model", json!({}));
    assert!(wait_until(|| f.runtime.effective().use_llm));
    assert!(f.handles.active.load(Ordering::SeqCst));
}

#[test]
fn memory_budget_in_set_environment_caps_profile_and_unloads_llm() {
    let mut f = fixture("budget", None, |_| {});
    f.runtime.set_model_reload_delay(Duration::ZERO);
    assert!(wait_until(|| uses_llm(&*f.pipeline)));

    // LLM に必要な量（150MB）に足りない上限を報告: LLM を解放して辞書のみ
    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_budget_mb":100}),
    );
    assert_eq!(f.runtime.memory_budget_mb(), Some(100));
    assert_eq!(f.runtime.profile(), nagi_profile::Profile::Minimal);
    assert!(!f.runtime.effective().use_llm);
    assert!(wait_until(|| !f.handles.active.load(Ordering::SeqCst)));
    let status = rpc(&mut f.server, "get_status", json!({}));
    assert!(
        status["result"]["profile_reason"]
            .as_str()
            .unwrap()
            .contains("メモリ上限"),
        "{status}"
    );

    // 十分な上限（ただし 1GB 未満）に戻すと、LLM を戻して light に制限される
    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_budget_mb":500}),
    );
    assert!(wait_until(|| {
        rpc(&mut f.server, "ping", json!({}));
        f.runtime.effective().use_llm
    }));
    assert_eq!(f.runtime.profile(), nagi_profile::Profile::Light);

    // 上限が緩めば、上限による制限は外れる
    rpc(
        &mut f.server,
        "set_environment",
        json!({"memory_budget_mb":8192}),
    );
    assert_ne!(f.runtime.profile(), nagi_profile::Profile::Minimal);
    assert!(f.runtime.effective().use_llm);
}

#[test]
fn initialize_capabilities_feed_profile_selection() {
    let init = |capabilities: Value| {
        let (model, _handles) = probe(None);
        let loader: ModelLoader = Box::new(move || Ok(Box::new(model) as Box<dyn ConversionModel>));
        let built = build_with_loader(&options("caps"), Some(loader));
        let runtime = built.runtime.clone();
        let mut server = Server::from_built(built);
        let r = rpc(
            &mut server,
            "initialize",
            json!({"protocol_version":1,"client_name":"t","client_version":"0",
                   "os":"ios","capabilities":capabilities}),
        );
        (r, runtime, server)
    };

    // メモリ上限が小さい: LLM を使わず minimal
    let (r, runtime, _s) = init(json!({
        "input_method":"touch",
        "environment":{"memory_budget_mb":100}
    }));
    assert_eq!(r["result"]["profile"], "minimal", "{r}");
    assert_eq!(runtime.input_method(), nagi_protocol::InputMethod::Touch);
    assert_eq!(runtime.memory_budget_mb(), Some(100));

    // 上限 500MB: light に制限される
    let (r, runtime, _s) = init(json!({"environment":{"memory_budget_mb":500}}));
    assert_eq!(r["result"]["profile"], "light", "{r}");
    assert_eq!(runtime.input_method(), nagi_protocol::InputMethod::Physical);

    // 電源などの環境も同じ経路で反映される
    let (r, runtime, _s) = init(json!({"environment":{"memory_pressure":"critical"}}));
    assert_eq!(r["result"]["profile"], "minimal", "{r}");
    assert!(runtime.model_unloaded());

    // capabilities は任意（省略・空でも従来どおり）
    let (r, _runtime, _s) = init(json!({}));
    assert_ne!(r["result"]["profile"], "minimal", "{r}");
}

#[test]
fn unload_during_reload_keeps_weights_released_and_states_consistent() {
    let mut f = fixture("reload-race", None, |_| {});
    assert!(wait_until(|| uses_llm(&*f.pipeline)));
    rpc(&mut f.server, "unload_model", json!({}));
    assert!(wait_until(|| !f.handles.active.load(Ordering::SeqCst)));

    // 再読み込み（set_active(true)）を途中で止め、その間に解放を要求する
    let (release, gate) = mpsc::channel();
    *f.handles.reload_gate.lock().unwrap() = Some(gate);
    let entered = f.handles.reload_entered.load(Ordering::SeqCst);
    rpc(&mut f.server, "reload_model", json!({}));
    assert!(wait_until(|| f
        .handles
        .reload_entered
        .load(Ordering::SeqCst)
        > entered));
    rpc(&mut f.server, "unload_model", json!({}));
    release.send(()).unwrap();

    // 重みは解放され、Runtime / Pipeline のどちらも解放中で一致する
    assert!(wait_until(|| !f.handles.active.load(Ordering::SeqCst)));
    std::thread::sleep(Duration::from_millis(200));
    assert!(!f.handles.active.load(Ordering::SeqCst));
    assert!(f.runtime.model_unloaded());
    assert!(!f.runtime.effective().use_llm);
    let status = rpc(&mut f.server, "get_status", json!({}));
    assert_eq!(status["result"]["llm"], false);
    // 解放中のモデルで推論されず、勝手に再読み込みもされない
    assert!(!uses_llm(&*f.pipeline));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!f.handles.active.load(Ordering::SeqCst));
    assert_eq!(f.handles.ran_inactive.load(Ordering::SeqCst), 0);
    // 再読み込みの完了通知は出ない
    let lines = f
        .server
        .handle_line(&json!({"jsonrpc":"2.0","id":3,"method":"ping"}).to_string());
    assert!(
        !lines.iter().any(|l| l.contains("llm_reloaded")),
        "{lines:?}"
    );

    // その後の明示の再読み込みでは正常に戻る
    rpc(&mut f.server, "reload_model", json!({}));
    assert!(wait_until(|| f.runtime.effective().use_llm));
    assert!(f.handles.active.load(Ordering::SeqCst));
    assert!(wait_until(|| uses_llm(&*f.pipeline)));
}
