//! 辞書 + LLM の変換バックエンド（`balanced` 相当の経路）。
//!
//! 変換パイプライン（設計書 §4.5）のうち、この crate が受け持つ部分:
//!
//! - **文全体の変換**: 読み全体を LLM（zenz）で変換して表記を得る。辞書の格子で強制アライメント
//!   （[`Dictionary::align`]）して文節ごとの (読み, 表記) に分ける。アライメントできなければ
//!   辞書の Viterbi 結果を使う
//! - **文節の候補**: LLM の最良変換を先頭に、辞書の候補の上位を LLM で文脈込みに再採点して並べる。
//!   残りは辞書のコスト順
//! - **待たせない**: 推論は別スレッドで実行し、`sync_budget` を超えたら辞書だけの結果を返す
//!   （推論は続行し、結果はキャッシュに入るので、次に同じ入力が来れば即座に使える）。
//!   モデルの読み込み失敗・推論エラーでも辞書だけで変換を続ける（`minimal` への縮退）

use nagi_dict::Dictionary;
use nagi_engine::{BackendCandidate, BackendSegment, ConversionBackend};
use nagi_infer::{ConversionModel, ConvertRequest};
use nagi_protocol::view::CandidateSource;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 1 回の LLM 変換に渡す読みの最大文字数。超える場合は文節境界で分けて順に変換する
const MAX_LLM_CHARS: usize = 60;
/// 分割するときの 1 チャンクの目標文字数
const CHUNK_TARGET_CHARS: usize = 30;
/// キャッシュの最大件数
const CACHE_CAPACITY: usize = 512;

/// LLM 経路の設定
#[derive(Debug, Clone)]
pub struct LlmOptions {
    /// 推論を待つ最長時間（`conversion.sync_budget_ms`）。超えたら辞書の結果で応答する
    pub sync_budget: Duration,
    /// 文節候補のうち LLM で再採点する辞書候補の数
    pub rescore_top: usize,
}

impl Default for LlmOptions {
    fn default() -> Self {
        LlmOptions {
            sync_budget: Duration::from_millis(150),
            rescore_top: 8,
        }
    }
}

type SharedModel = Arc<Mutex<Box<dyn ConversionModel>>>;
type Cache = Arc<Mutex<HashMap<(String, String), String>>>;

/// 辞書 + LLM のバックエンド
pub struct HybridBackend {
    dict: Arc<Dictionary>,
    model: SharedModel,
    cache: Cache,
    options: LlmOptions,
}

/// LLM の出力として受け入れられるか（空・極端な長さ・私用領域文字・置換文字を弾く）
fn plausible(reading: &str, surface: &str) -> bool {
    let n_reading = reading.chars().count();
    let n_surface = surface.chars().count();
    n_surface > 0
        && n_surface <= n_reading * 3 + 8
        && !surface
            .chars()
            .any(|c| c == '\u{fffd}' || ('\u{e000}'..='\u{f8ff}').contains(&c) || c.is_control())
}

fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

impl HybridBackend {
    /// 辞書と変換モデルから作る
    pub fn new(
        dict: Arc<Dictionary>,
        model: Box<dyn ConversionModel>,
        options: LlmOptions,
    ) -> Self {
        HybridBackend {
            dict,
            model: Arc::new(Mutex::new(model)),
            cache: Arc::new(Mutex::new(HashMap::new())),
            options,
        }
    }

    /// 起動直後の最初の推論が遅くならないよう、ダミーの入力でモデルのページインとキャッシュ確保を済ませる
    ///（別スレッド。結果は捨てる）
    pub fn warm_up(&self) {
        let model = self.model.clone();
        let _ = std::thread::Builder::new()
            .name("nagi-llm-warmup".into())
            .spawn(move || {
                if let Ok(mut m) = model.lock() {
                    let _ = m.convert(&Self::request("あいう", ""));
                }
            });
    }

    fn request(reading: &str, left: &str) -> ConvertRequest {
        ConvertRequest {
            reading: reading.to_string(),
            left_context: tail(left, 40),
            profile: None,
            max_new_tokens: reading.chars().count() * 3 + 8,
        }
    }

    /// LLM の最良変換（貪欲デコード）。予算内に終わらない・失敗・不正な出力なら `None`
    fn llm_convert(&self, reading: &str, left: &str) -> Option<String> {
        let key = (tail(left, 40), reading.to_string());
        if let Some(hit) = self.cache.lock().ok()?.get(&key) {
            return Some(hit.clone());
        }
        let (tx, rx) = mpsc::sync_channel::<Option<String>>(1);
        let model = self.model.clone();
        let cache = self.cache.clone();
        let req = Self::request(reading, left);
        let worker_key = key.clone();
        std::thread::Builder::new()
            .name("nagi-llm-convert".into())
            .spawn(move || {
                let result = model
                    .lock()
                    .ok()
                    .and_then(|mut m| m.convert(&req).ok())
                    .and_then(|h| h.into_iter().next())
                    .map(|h| h.surface)
                    .filter(|s| plausible(&req.reading, s));
                if let (Some(surface), Ok(mut cache)) = (&result, cache.lock()) {
                    if cache.len() >= CACHE_CAPACITY {
                        cache.clear();
                    }
                    cache.insert(worker_key, surface.clone());
                }
                let _ = tx.send(result);
            })
            .ok()?;
        rx.recv_timeout(self.options.sync_budget).ok().flatten()
    }

    /// 候補の対数確率スコア（予算内に終わらなければ `None`）
    fn llm_scores(&self, reading: &str, left: &str, candidates: Vec<String>) -> Option<Vec<f32>> {
        let (tx, rx) = mpsc::sync_channel::<Option<Vec<f32>>>(1);
        let model = self.model.clone();
        let req = Self::request(reading, left);
        std::thread::Builder::new()
            .name("nagi-llm-score".into())
            .spawn(move || {
                let scores = model
                    .lock()
                    .ok()
                    .and_then(|mut m| m.score(&req, &candidates).ok());
                let _ = tx.send(scores);
            })
            .ok()?;
        rx.recv_timeout(self.options.sync_budget).ok().flatten()
    }

    fn dict_segments(&self, reading: &str, extra: Option<CandidateSource>) -> Vec<BackendSegment> {
        self.dict
            .convert(reading)
            .into_iter()
            .map(|s| {
                let mut sources = vec![if s.surface == s.reading {
                    CandidateSource::Kana
                } else {
                    CandidateSource::SystemDict
                }];
                sources.extend(extra);
                BackendSegment {
                    sources,
                    reading: s.reading,
                    surface: s.surface,
                }
            })
            .collect()
    }

    /// 1 チャンク（読みが `MAX_LLM_CHARS` 以下）を LLM で変換して文節に分ける
    fn convert_chunk(&self, reading: &str, left: &str) -> Vec<BackendSegment> {
        let Some(surface) = self.llm_convert(reading, left) else {
            return self.dict_segments(reading, None);
        };
        match self.dict.align(reading, &surface) {
            Some(segments) => segments
                .into_iter()
                .map(|s| BackendSegment {
                    sources: vec![CandidateSource::Llm],
                    reading: s.reading,
                    surface: s.surface,
                })
                .collect(),
            None => vec![BackendSegment {
                sources: vec![CandidateSource::Llm],
                reading: reading.to_string(),
                surface,
            }],
        }
    }
}

impl ConversionBackend for HybridBackend {
    fn name(&self) -> &str {
        "dict+llm"
    }

    fn convert_sentence(&self, reading: &str, left_context: &str) -> Vec<BackendSegment> {
        if reading.chars().count() <= MAX_LLM_CHARS {
            return self.convert_chunk(reading, left_context);
        }
        // 長い入力: 辞書の文節境界で 30 文字前後のチャンクに分け、前のチャンクの結果を文脈にして順に変換する
        let mut out: Vec<BackendSegment> = Vec::new();
        let mut chunk = String::new();
        let mut context = left_context.to_string();
        let flush = |chunk: &mut String, out: &mut Vec<BackendSegment>, context: &mut String| {
            if chunk.is_empty() {
                return;
            }
            for seg in self.convert_chunk(chunk, context) {
                context.push_str(&seg.surface);
                out.push(seg);
            }
            chunk.clear();
        };
        for seg in self.dict.convert(reading) {
            chunk.push_str(&seg.reading);
            if chunk.chars().count() >= CHUNK_TARGET_CHARS {
                flush(&mut chunk, &mut out, &mut context);
            }
        }
        flush(&mut chunk, &mut out, &mut context);
        out
    }

    fn segment_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendCandidate> {
        let dict_candidates = self.dict.segment_candidates(reading, limit.max(30));
        let source_of = |surface: &str| {
            if surface == reading {
                CandidateSource::Kana
            } else {
                CandidateSource::SystemDict
            }
        };
        let mut out: Vec<BackendCandidate> = Vec::new();
        let push = |out: &mut Vec<BackendCandidate>, surface: &str, source: CandidateSource| {
            if let Some(existing) = out.iter_mut().find(|c| c.surface == surface) {
                if !existing.sources.contains(&source) {
                    existing.sources.push(source);
                }
            } else {
                out.push(BackendCandidate {
                    surface: surface.to_string(),
                    sources: vec![source],
                });
            }
        };

        let llm_top = if reading.chars().count() <= MAX_LLM_CHARS {
            self.llm_convert(reading, left_context)
        } else {
            None
        };
        if let Some(top) = &llm_top {
            push(&mut out, top, CandidateSource::Llm);
        }
        // 辞書候補の上位を LLM で再採点して並べ替える
        let n_head = self.options.rescore_top.min(dict_candidates.len());
        let head: Vec<String> = dict_candidates[..n_head]
            .iter()
            .map(|c| c.surface.clone())
            .filter(|s| llm_top.as_deref() != Some(s.as_str()))
            .collect();
        let mut ranked: Vec<(String, f32)> = Vec::new();
        if llm_top.is_some()
            && !head.is_empty()
            && let Some(scores) = self.llm_scores(reading, left_context, head.clone())
            && scores.len() == head.len()
        {
            ranked = head.iter().cloned().zip(scores).collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        }
        for (surface, _) in &ranked {
            push(&mut out, surface, source_of(surface));
        }
        for c in &dict_candidates {
            push(&mut out, &c.surface, source_of(&c.surface));
        }
        out.truncate(limit);
        out
    }
}
