//! 変換パイプライン（`Pipeline`）の LLM 経路のテスト。モデルは差し替え可能な偽物を使うので、
//! 推論ライブラリもモデルファイルも要らない。

use nagi_engine::ConversionBackend;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::view::CandidateSource;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::time::{Duration, Instant};

mod common;
use common::{FakeModel, dict};
use std::sync::atomic::Ordering;
fn backend(model: FakeModel, budget_ms: u64) -> Pipeline {
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
        },
        true,
        Tunables {
            sync_budget: Duration::from_millis(budget_ms),
            ..Tunables::default()
        },
        Learner::new(LearnConfig::default()),
        None,
    );
    Pipeline::new(Some(dict()), Some(Box::new(model)), runtime)
}

fn joined(segs: &[nagi_engine::BackendSegment]) -> Vec<(String, String)> {
    segs.iter()
        .map(|s| (s.reading.clone(), s.surface.clone()))
        .collect()
}

#[test]
fn llm_surface_is_aligned_into_bunsetsu_with_llm_source() {
    let b = backend(
        FakeModel::new(&[("きょうはこうえんにいく", "今日は講演に行く")]),
        1000,
    );
    let segs = b.convert_sentence("きょうはこうえんにいく", "");
    assert_eq!(
        joined(&segs),
        [
            ("きょうは".to_string(), "今日は".to_string()),
            ("こうえんに".to_string(), "講演に".to_string()),
            ("いく".to_string(), "行く".to_string()),
        ]
    );
    assert!(segs.iter().all(|s| s.sources == [CandidateSource::Llm]));
}

#[test]
fn model_errors_fall_back_to_dictionary() {
    let b = backend(FakeModel::new(&[]), 1000);
    let segs = b.convert_sentence("きょうはこうえんにいく", "");
    let surface: String = segs.iter().map(|s| s.surface.as_str()).collect();
    assert_eq!(surface, "今日は公園に行く");
    assert!(
        segs.iter()
            .all(|s| !s.sources.contains(&CandidateSource::Llm))
    );
}

#[test]
fn implausible_llm_output_is_rejected() {
    // 私用領域文字・極端に長い出力は採用しない
    for bad in ["\u{EE01}公園", "公園".repeat(30).as_str(), ""] {
        let b = backend(FakeModel::new(&[("こうえん", bad)]), 1000);
        let segs = b.convert_sentence("こうえん", "");
        assert_eq!(joined(&segs)[0].1, "公園", "{bad:?}");
    }
}

#[test]
fn unalignable_output_becomes_single_segment() {
    // 読みに存在しないひらがなを含む出力: アライメントできないので 1 文節（表記は LLM のまま）
    let b = backend(FakeModel::new(&[("こうえん", "ほげ")]), 1000);
    let segs = b.convert_sentence("こうえん", "");
    assert_eq!(
        joined(&segs),
        [("こうえん".to_string(), "ほげ".to_string())]
    );
    assert_eq!(segs[0].sources, [CandidateSource::Llm]);
}

#[test]
fn slow_model_times_out_then_result_is_cached() {
    let mut model = FakeModel::new(&[("こうえん", "講演")]);
    model.delay = Duration::from_millis(300);
    let converts = model.converts.clone();
    let b = backend(model, 30);

    let started = Instant::now();
    let segs = b.convert_sentence("こうえん", "");
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "予算を超えて待たない"
    );
    assert_eq!(joined(&segs)[0].1, "公園", "予算超過時は辞書の結果");

    // 推論はバックグラウンドで完了し、キャッシュに入る
    let deadline = Instant::now() + Duration::from_secs(3);
    while converts.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    let segs = b.convert_sentence("こうえん", "");
    assert_eq!(
        joined(&segs)[0].1,
        "講演",
        "次回はキャッシュから LLM の結果が即座に返る"
    );
    assert!(started.elapsed() < Duration::from_millis(30));
    assert_eq!(converts.load(Ordering::SeqCst), 1, "再推論しない");
}

#[test]
fn left_context_reaches_the_model_and_keys_the_cache() {
    let model = FakeModel::new(&[("こうえん", "公園")]);
    let contexts = model.last_contexts.clone();
    let b = backend(model, 1000);
    b.convert_sentence("こうえん", "近所の");
    b.convert_sentence("こうえん", "近所の"); // キャッシュ
    b.convert_sentence("こうえん", "大学の先生が"); // 文脈が違えば再推論
    assert_eq!(*contexts.lock().unwrap(), ["近所の", "大学の先生が"]);
}

#[test]
fn candidates_put_llm_first_then_rescored_dictionary_candidates() {
    let b = backend(FakeModel::new(&[("こうえん", "公園")]), 1000);
    // 文脈なし: LLM の 公園 → 再採点で最良の 後援 → 残りは辞書順
    let c = b.segment_candidates("こうえん", "", 10);
    let surf: Vec<_> = c.iter().map(|x| x.surface.as_str()).collect();
    assert_eq!(surf, ["公園", "後援", "講演", "こうえん"]);
    assert!(c[0].sources.contains(&CandidateSource::Llm));
    assert!(
        c[0].sources.contains(&CandidateSource::SystemDict),
        "辞書にもある表記は出所を併記する"
    );
    assert_eq!(c[3].sources, [CandidateSource::Kana]);
    // 文脈が変われば再採点の結果も変わる
    let c = b.segment_candidates("こうえん", "大学の先生が", 10);
    let surf: Vec<_> = c.iter().map(|x| x.surface.as_str()).collect();
    assert_eq!(&surf[..2], ["公園", "講演"]);
    // limit
    assert_eq!(b.segment_candidates("こうえん", "", 2).len(), 2);
    // 表記の重複はない
    let unique: std::collections::HashSet<_> = surf.iter().collect();
    assert_eq!(unique.len(), surf.len());
}

#[test]
fn candidates_fall_back_to_dictionary_order_without_llm_result() {
    let b = backend(FakeModel::new(&[]), 1000);
    let c = b.segment_candidates("こうえん", "", 10);
    let surf: Vec<_> = c.iter().map(|x| x.surface.as_str()).collect();
    assert_eq!(surf[0], "公園");
    assert!(surf.contains(&"講演"));
}

#[test]
fn long_input_is_converted_in_chunks() {
    // 60 文字を超える入力は辞書の文節境界で分け、前のチャンクの結果を左文脈にして順に変換する
    let reading = "きょうはこうえんにいく".repeat(8); // 88 文字
    let model = FakeModel::new(&[]);
    let contexts = model.last_contexts.clone();
    let b = backend(model, 1000);
    let segs = b.convert_sentence(&reading, "");
    assert_eq!(
        segs.iter().map(|s| s.reading.as_str()).collect::<String>(),
        reading
    );
    let calls = contexts.lock().unwrap().len();
    assert!(calls >= 2, "チャンクごとに推論する: {calls}");
    assert!(
        contexts
            .lock()
            .unwrap()
            .iter()
            .skip(1)
            .all(|c| !c.is_empty()),
        "2 つ目以降のチャンクには文脈が渡る"
    );
}

#[test]
fn slow_model_queues_completion_after_timeout() {
    use std::sync::atomic::Ordering;

    let mut model = FakeModel::new(&[("こうえん", "講演")]);
    model.delay = Duration::from_millis(300);
    let converts = model.converts.clone();
    let b = backend(model, 30);

    // 最初の convert_sentence: 予算超過で辞書の結果
    let started = Instant::now();
    let segs = b.convert_sentence("こうえん", "");
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "予算内に返る"
    );
    assert_eq!(joined(&segs)[0].1, "公園", "予算超過時は辞書の結果");

    // バックグラウンド推論が完了するまで待つ
    let deadline = Instant::now() + Duration::from_secs(5);
    while converts.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }

    // 完了通知が積まれている
    let completions = b.take_completions();
    assert!(!completions.is_empty(), "完了通知が積まれる");
    assert_eq!(completions[0].reading, "こうえん");
    assert_eq!(completions[0].left_context, "");

    // 次回は キャッシュから即座に LLM の結果が返る
    let started = Instant::now();
    let segs = b.convert_sentence("こうえん", "");
    assert_eq!(
        joined(&segs)[0].1,
        "講演",
        "キャッシュから LLM の結果が即座に返る"
    );
    assert!(started.elapsed() < Duration::from_millis(30));
    assert_eq!(converts.load(Ordering::SeqCst), 1, "再推論しない");
}

/// 予算超過で辞書の結果を返した後、LLM の完了で `update_preedit` 通知が出て表示が差し替わる
#[test]
fn late_llm_result_is_pushed_as_update_preedit() {
    use nagi_engine::EngineConfig;
    use nagi_server::Server;
    use serde_json::{Value, json};
    use std::sync::Arc;

    let mut model = FakeModel::new(&[("こうえん", "講演")]);
    model.delay = Duration::from_millis(300);
    let mut server = Server::new(
        Arc::new(backend(model, 30)),
        "balanced",
        EngineConfig::default(),
    );
    let mut send = |request: Value| -> Vec<Value> {
        server
            .handle_line(&request.to_string())
            .iter()
            .map(|l| serde_json::from_str(l).expect("JSON"))
            .collect()
    };
    send(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocol_version":1,"client_name":"t","client_version":"0","os":"test"}}),
    );
    let mut key = |id: i64, c: char| {
        send(
            json!({"jsonrpc":"2.0","id":id,"method":"process_key","params":{
            "session_id":"s","keysym":c as u32,"keycode":0,"text":c.to_string(),
            "modifiers":{},"kind":"down"}}),
        )
    };
    for (i, c) in "kouenn".chars().enumerate() {
        key(10 + i as i64, c);
    }
    let space = key(99, ' ');
    let response = &space[0]["result"];
    assert_eq!(
        response["preedit"]["text"], "公園",
        "予算超過時は辞書の結果"
    );
    let space_revision = response["revision"].as_u64().unwrap();

    // LLM の完了を待つ（ping の応答のあとに通知が続く）。固定 sleep ではなく上限付きで待つ
    let deadline = Instant::now() + Duration::from_secs(5);
    let update = loop {
        let lines = send(json!({"jsonrpc":"2.0","id":2,"method":"ping"}));
        if let Some(n) = lines.into_iter().find(|l| l["method"] == "update_preedit") {
            break n;
        }
        assert!(Instant::now() < deadline, "update_preedit が届かない");
        std::thread::sleep(Duration::from_millis(10));
    };
    let params = &update["params"];
    assert_eq!(params["session_id"], "s");
    assert_eq!(params["preedit"]["text"], "講演");
    assert!(params["revision"].as_u64().unwrap() > space_revision);
}
