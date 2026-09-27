//! 辞書 + LLM バックエンド（`HybridBackend`）のテスト。モデルは差し替え可能な偽物を使うので、
//! 推論ライブラリもモデルファイルも要らない。

use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};
use nagi_engine::ConversionBackend;
use nagi_infer::{ConversionModel, ConvertRequest, Hypothesis, InferError};
use nagi_protocol::view::CandidateSource;
use nagi_server::{HybridBackend, LlmOptions};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 動詞,自立,*,*,五段・カ行促音便,基本形,行く
4 助動詞,*,*,*,特殊・デス,基本形,です
5 記号,句点,*,*,*,*,。
";

fn dict() -> Arc<Dictionary> {
    let n = 6;
    let mut cost = vec![100u32; n * n];
    for (rid, lid) in [
        (1, 2),
        (1, 4),
        (2, 3),
        (2, 1),
        (0, 1),
        (0, 3),
        (1, 5),
        (3, 5),
        (4, 5),
        (1, 0),
        (4, 0),
        (5, 0),
        (3, 0),
        (2, 0),
    ] {
        cost[rid * n + lid] = 0;
    }
    let mut lines = vec![n.to_string()];
    lines.extend(cost.iter().map(u32::to_string));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text(
        "inline",
        "\
こうえん\t1\t1\t3000\t公園
こうえん\t1\t1\t3500\t講演
こうえん\t1\t1\t4000\t後援
こうえん\t1\t1\t9000\tこうえん
に\t2\t2\t400\tに
いく\t3\t3\t3000\t行く
です\t4\t4\t300\tです
きょう\t1\t1\t3000\t今日
は\t2\t2\t500\tは
",
    )
    .unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    Arc::new(b.build().unwrap())
}

/// 読み → 表記 の表で答え、`score` は表にある表記を最良にする。呼び出し回数と遅延を制御できる
struct FakeModel {
    table: HashMap<String, String>,
    delay: Duration,
    converts: Arc<AtomicUsize>,
    last_contexts: Arc<Mutex<Vec<String>>>,
}

impl FakeModel {
    fn new(pairs: &[(&str, &str)]) -> Self {
        FakeModel {
            table: pairs
                .iter()
                .map(|(r, s)| (r.to_string(), s.to_string()))
                .collect(),
            delay: Duration::ZERO,
            converts: Arc::new(AtomicUsize::new(0)),
            last_contexts: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ConversionModel for FakeModel {
    fn id(&self) -> &str {
        "fake"
    }

    fn convert(&mut self, req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError> {
        std::thread::sleep(self.delay);
        self.converts.fetch_add(1, Ordering::SeqCst);
        self.last_contexts
            .lock()
            .unwrap()
            .push(req.left_context.clone());
        self.table
            .get(&req.reading)
            .map(|s| {
                vec![Hypothesis {
                    surface: s.clone(),
                    score: 0.0,
                }]
            })
            .ok_or_else(|| InferError::Other("未登録".into()))
    }

    fn score(
        &mut self,
        req: &ConvertRequest,
        candidates: &[String],
    ) -> Result<Vec<f32>, InferError> {
        // 左文脈が「大学の先生が」なら 講演 を、それ以外は 後援 を最良にする（辞書順とは逆）
        let favorite = if req.left_context.contains("先生") {
            "講演"
        } else {
            "後援"
        };
        Ok(candidates
            .iter()
            .map(|c| if c == favorite { -0.1 } else { -5.0 })
            .collect())
    }
}

fn backend(model: FakeModel, budget_ms: u64) -> HybridBackend {
    HybridBackend::new(
        dict(),
        Box::new(model),
        LlmOptions {
            sync_budget: Duration::from_millis(budget_ms),
            ..LlmOptions::default()
        },
    )
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
