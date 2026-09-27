//! 変換パイプライン（設計書 §4.5）: 学習 → ユーザー辞書 → LLM → システム辞書 → かな → Rewriter。
//!
//! engine の [`ConversionBackend`] の実装。各供給元の結果を、設定の優先順
//! （`ranking.source_order`）で 1 つの候補列に統合する。
//!
//! - **文全体の変換**: 読み全体を LLM（zenz）で変換して表記を得て、辞書の格子で強制アライメント
//!   （[`Dictionary::align_with`]）して文節に分ける。アライメントできなければ 1 文節にする。
//!   その後、優先順で LLM より前にある出所（学習・ユーザー辞書）が文節の読みと完全一致する
//!   結果を持っていれば、その表記に置き換える
//! - **文節の候補**: 各出所の候補を優先順に並べる。LLM の最良変換と、辞書候補の上位を LLM で
//!   文脈込みに再採点した順が `llm` / `system_dict` の位置に入る
//! - **待たせない**: 推論は別スレッドで実行し、予算（`conversion.sync_budget_ms`）を超えたら
//!   辞書の結果で応答する。推論は続行して結果をキャッシュに入れる
//! - **縮退**: モデルが無い・エラー・`minimal` プロファイルでは辞書だけ、辞書も無ければ読みのまま

use crate::runtime::{Effective, Runtime};
use nagi_dict::{Dictionary, Overlay};
use nagi_engine::{BackendCandidate, BackendSegment, BackendSentence, ConversionBackend};
use nagi_infer::{ConversionModel, ConvertRequest};
use nagi_learn::context_hash;
use nagi_protocol::view::CandidateSource;
use nagi_rewriter::RewriterChain;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 1 回の LLM 変換に渡す読みの最大文字数。超える場合は文節境界で分けて順に変換する
const MAX_LLM_CHARS: usize = 60;
/// 分割するときの 1 チャンクの目標文字数
const CHUNK_TARGET_CHARS: usize = 30;
/// キャッシュの最大件数
const CACHE_CAPACITY: usize = 512;
/// 学習結果で LLM の変換を置き換える最小スコア（`ln(1+count)` × 減衰。14 日以内の 1 回の確定なら超える）
const LEARN_OVERRIDE_MIN_SCORE: f64 = 0.3;
/// 文脈のハッシュ（学習の context_hash）に使う左文脈の文字数（engine が渡す上限）
const HASH_CONTEXT_CHARS: usize = 40;
/// キャリブレーション用の変換対象
const CALIBRATION_READING: &str = "きょうはいいてんきです";

type SharedModel = Arc<Mutex<Box<dyn ConversionModel>>>;
type Cache = Arc<Mutex<HashMap<(String, String), String>>>;

struct Llm {
    model: SharedModel,
    cache: Cache,
    /// 直前に `reconcile_active` を呼んだときのプロファイル（変化を検出するため）
    last_profile: Mutex<nagi_profile::Profile>,
}

/// 変換パイプライン
pub struct Pipeline {
    dict: Option<Arc<Dictionary>>,
    llm: Option<Llm>,
    runtime: Arc<Runtime>,
    rewriter: RewriterChain,
}

/// dict・LLM の有無からバックエンド名を決める（`Pipeline::new` と `name()` の重複を1箇所にまとめる）
pub fn backend_name(has_dict: bool, has_llm: bool) -> &'static str {
    match (has_dict, has_llm) {
        (true, true) => "dict+llm",
        (true, false) => "dict",
        (false, _) => "kana",
    }
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

fn source_of(surface: &str, reading: &str) -> CandidateSource {
    if surface == reading {
        CandidateSource::Kana
    } else {
        CandidateSource::SystemDict
    }
}

impl Pipeline {
    /// パイプラインを作る。`dict` が無ければ読みをそのまま返す。`model` が無ければ辞書のみ
    pub fn new(
        dict: Option<Arc<Dictionary>>,
        model: Option<Box<dyn ConversionModel>>,
        runtime: Arc<Runtime>,
    ) -> Self {
        let profile = runtime.profile();
        let llm = model.map(|m| Llm {
            model: Arc::new(Mutex::new(m)),
            cache: Arc::new(Mutex::new(HashMap::new())),
            last_profile: Mutex::new(profile),
        });
        let name = backend_name(dict.is_some(), llm.is_some());
        runtime.set_backend_name(name);
        Pipeline {
            dict,
            llm,
            runtime,
            rewriter: RewriterChain::default_chain(),
        }
    }

    /// プロファイルの変化を見て、LLM をアクティブ / 非アクティブに切り替える
    /// （`Minimal` になったら解放、そこから戻ったら再読み込み）。別スレッドで行うので待たせない
    fn reconcile_active(&self) {
        let Some(llm) = &self.llm else { return };
        let current = self.runtime.profile();
        let Ok(mut last) = llm.last_profile.lock() else {
            return;
        };
        if *last == current {
            return;
        }
        *last = current;
        drop(last);
        let should_be_active = current != nagi_profile::Profile::Minimal;
        let model = llm.model.clone();
        let _ = std::thread::Builder::new()
            .name("nagi-llm-active".into())
            .spawn(move || {
                if let Ok(mut m) = model.lock() {
                    let _ = m.set_active(should_be_active);
                }
            });
    }

    /// 起動直後の最初の推論が遅くならないよう、別スレッドでモデルを温め、続けてキャリブレーション
    /// 推論（短い変換 3 回の中央値）を行って環境プロファイルの初回判定に使う
    pub fn warm_up(&self) {
        let Some(llm) = &self.llm else {
            return;
        };
        let model = llm.model.clone();
        let runtime = self.runtime.clone();
        let _ = std::thread::Builder::new()
            .name("nagi-llm-warmup".into())
            .spawn(move || {
                let req = Self::request(CALIBRATION_READING, "", None);
                let mut samples = Vec::new();
                if let Ok(mut m) = model.lock() {
                    m.set_n_threads(runtime.effective().n_threads);
                    let _ = m.convert(&req); // ページイン
                    for _ in 0..3 {
                        let started = Instant::now();
                        if m.convert(&req).is_err() {
                            return;
                        }
                        samples.push(started.elapsed());
                    }
                }
                if let Some(ms) = nagi_profile::median_ms(&samples) {
                    runtime.finish_calibration(ms);
                }
            });
    }

    fn request(reading: &str, left: &str, persona: Option<&str>) -> ConvertRequest {
        ConvertRequest {
            reading: reading.to_string(),
            left_context: tail(left, 40),
            profile: persona.map(str::to_string),
            max_new_tokens: reading.chars().count() * 3 + 8,
        }
    }

    /// LLM の最良変換（貪欲デコード）。予算内に終わらない・失敗・不正な出力なら `None`
    fn llm_convert(&self, reading: &str, left: &str, eff: &Effective) -> Option<String> {
        let llm = self.llm.as_ref()?;
        let key = (
            format!(
                "{}\u{1}{}",
                tail(left, 40),
                eff.persona.as_deref().unwrap_or("")
            ),
            reading.to_string(),
        );
        if let Some(hit) = llm.cache.lock().ok()?.get(&key) {
            return Some(hit.clone());
        }
        let (tx, rx) = mpsc::sync_channel::<Option<String>>(1);
        let (model, cache, runtime) = (llm.model.clone(), llm.cache.clone(), self.runtime.clone());
        let req = Self::request(reading, left, eff.persona.as_deref());
        let n_threads = eff.n_threads;
        std::thread::Builder::new()
            .name("nagi-llm-convert".into())
            .spawn(move || {
                let result = model.lock().ok().and_then(|mut m| {
                    m.set_n_threads(n_threads);
                    let started = Instant::now();
                    let out = m.convert(&req).ok();
                    runtime.note_latency(started.elapsed().as_secs_f64() * 1000.0);
                    out
                });
                let result = result
                    .and_then(|h| h.into_iter().next())
                    .map(|h| h.surface)
                    .filter(|s| plausible(&req.reading, s));
                if let (Some(surface), Ok(mut cache)) = (&result, cache.lock()) {
                    if cache.len() >= CACHE_CAPACITY {
                        cache.clear();
                    }
                    cache.insert(key, surface.clone());
                }
                let _ = tx.send(result);
            })
            .ok()?;
        rx.recv_timeout(eff.sync_budget).ok().flatten()
    }

    /// 候補の対数確率スコア（予算内に終わらなければ `None`）
    fn llm_scores(
        &self,
        reading: &str,
        left: &str,
        candidates: Vec<String>,
        eff: &Effective,
    ) -> Option<Vec<f32>> {
        let llm = self.llm.as_ref()?;
        let (tx, rx) = mpsc::sync_channel::<Option<Vec<f32>>>(1);
        let model = llm.model.clone();
        let req = Self::request(reading, left, eff.persona.as_deref());
        let n_threads = eff.n_threads;
        std::thread::Builder::new()
            .name("nagi-llm-score".into())
            .spawn(move || {
                let scores = model.lock().ok().and_then(|mut m| {
                    m.set_n_threads(n_threads);
                    m.score(&req, &candidates).ok()
                });
                let _ = tx.send(scores);
            })
            .ok()?;
        rx.recv_timeout(eff.sync_budget).ok().flatten()
    }

    fn dict_segments(
        &self,
        dict: &Dictionary,
        reading: &str,
        overlay: &Overlay,
    ) -> Vec<BackendSegment> {
        dict.convert_with(reading, Some(overlay))
            .into_iter()
            .map(|s| BackendSegment {
                sources: vec![source_of(&s.surface, &s.reading)],
                reading: s.reading,
                surface: s.surface,
            })
            .collect()
    }

    /// 1 チャンク（読みが `MAX_LLM_CHARS` 以下）を LLM で変換して文節に分ける
    fn convert_chunk(
        &self,
        dict: &Dictionary,
        reading: &str,
        left: &str,
        eff: &Effective,
        overlay: &Overlay,
    ) -> Vec<BackendSegment> {
        let Some(surface) = self.llm_convert(reading, left, eff) else {
            return self.dict_segments(dict, reading, overlay);
        };
        match dict.align_with(reading, &surface, Some(overlay)) {
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

    fn convert_with_llm(
        &self,
        dict: &Dictionary,
        reading: &str,
        left: &str,
        eff: &Effective,
        overlay: &Overlay,
    ) -> Vec<BackendSegment> {
        if reading.chars().count() <= MAX_LLM_CHARS {
            return self.convert_chunk(dict, reading, left, eff, overlay);
        }
        // 長い入力: 辞書の文節境界で 30 文字前後のチャンクに分け、前のチャンクの結果を左文脈にして順に変換する
        let mut out: Vec<BackendSegment> = Vec::new();
        let mut chunk = String::new();
        let mut context = left.to_string();
        let flush = |chunk: &mut String, out: &mut Vec<BackendSegment>, context: &mut String| {
            if chunk.is_empty() {
                return;
            }
            for seg in self.convert_chunk(dict, chunk, context, eff, overlay) {
                context.push_str(&seg.surface);
                out.push(seg);
            }
            chunk.clear();
        };
        for seg in dict.convert_with(reading, Some(overlay)) {
            chunk.push_str(&seg.reading);
            if chunk.chars().count() >= CHUNK_TARGET_CHARS {
                flush(&mut chunk, &mut out, &mut context);
            }
        }
        flush(&mut chunk, &mut out, &mut context);
        out
    }

    /// 優先順で LLM より前にある出所（学習・ユーザー辞書）の結果で、文節の表記を置き換える
    fn apply_preferences(
        &self,
        segments: &mut [BackendSegment],
        left_full: &str,
        eff: &Effective,
        overlay: &Overlay,
    ) {
        let mut left = left_full.to_string();
        for seg in segments.iter_mut() {
            let hash = context_hash(&tail(&left, HASH_CONTEXT_CHARS));
            for source in &eff.ranking {
                let chosen = match source {
                    CandidateSource::Learned => self
                        .runtime
                        .learn_lookup(&seg.reading, hash)
                        .into_iter()
                        .find(|(_, score)| *score >= LEARN_OVERRIDE_MIN_SCORE)
                        .map(|(surface, _)| surface),
                    CandidateSource::UserDict => overlay
                        .lookup(&seg.reading)
                        .first()
                        .map(|e| e.surface.clone()),
                    // LLM・システム辞書より後ろの出所は、変換結果を置き換えない
                    _ => break,
                };
                if let Some(surface) = chosen {
                    seg.surface = surface;
                    seg.sources = vec![*source];
                    break;
                }
            }
            left.push_str(&seg.surface);
        }
    }
}

/// 出所ごとの候補を、優先順に 1 つの列へ統合する（同じ表記は出所を併記）。
/// 優先順に無い出所は、既定の順で最後に付ける
fn merge_by_order(
    order: &[CandidateSource],
    mut groups: Vec<(CandidateSource, Vec<String>)>,
) -> Vec<BackendCandidate> {
    let default_order = [
        CandidateSource::Learned,
        CandidateSource::UserDict,
        CandidateSource::Llm,
        CandidateSource::SystemDict,
        CandidateSource::Kana,
        CandidateSource::Rewriter,
    ];
    let mut out: Vec<BackendCandidate> = Vec::new();
    for source in order.iter().chain(default_order.iter()) {
        // 同じ出所のグループはまとめて 1 回だけ処理する
        for (_, items) in groups.iter_mut().filter(|(s, _)| s == source) {
            for surface in std::mem::take(items) {
                match out.iter_mut().find(|c| c.surface == surface) {
                    Some(existing) => {
                        if !existing.sources.contains(source) {
                            existing.sources.push(*source);
                        }
                    }
                    None => out.push(BackendCandidate {
                        surface,
                        sources: vec![*source],
                    }),
                }
            }
        }
    }
    out
}

impl ConversionBackend for Pipeline {
    fn name(&self) -> &str {
        backend_name(self.dict.is_some(), self.llm.is_some())
    }

    fn convert_sentence(&self, reading: &str, left_context: &str) -> Vec<BackendSegment> {
        self.runtime.tick();
        self.reconcile_active();
        let eff = self.runtime.effective();
        let overlay = self.runtime.overlay();
        let left_llm = tail(left_context, eff.max_context_chars);
        let mut segments = match &self.dict {
            None => vec![BackendSegment {
                reading: reading.to_string(),
                surface: reading.to_string(),
                sources: vec![CandidateSource::Kana],
            }],
            Some(dict) if eff.use_llm && self.llm.is_some() => {
                self.convert_with_llm(dict, reading, &left_llm, &eff, &overlay)
            }
            Some(dict) => self.dict_segments(dict, reading, &overlay),
        };
        self.apply_preferences(&mut segments, left_context, &eff, &overlay);
        segments
    }

    fn segment_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendCandidate> {
        self.runtime.tick();
        self.reconcile_active();
        let eff = self.runtime.effective();
        let overlay = self.runtime.overlay();
        let left_llm = tail(left_context, eff.max_context_chars);
        let mut groups: Vec<(CandidateSource, Vec<String>)> = Vec::new();

        // 学習
        let hash = context_hash(&tail(left_context, HASH_CONTEXT_CHARS));
        let learned: Vec<String> = self
            .runtime
            .learn_lookup(reading, hash)
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .map(|(s, _)| s)
            .collect();
        groups.push((CandidateSource::Learned, learned));
        // ユーザー辞書（語 → スニペットの順）
        let mut user: Vec<String> = overlay
            .lookup(reading)
            .iter()
            .map(|e| e.surface.clone())
            .collect();
        user.extend(overlay.snippets(reading).iter().cloned());
        groups.push((CandidateSource::UserDict, user));

        // LLM とシステム辞書
        if let Some(dict) = &self.dict {
            let dict_candidates: Vec<String> = dict
                .segment_candidates_with(reading, limit.max(30), Some(&overlay))
                .into_iter()
                .map(|c| c.surface)
                .collect();
            let llm_top = if eff.use_llm && reading.chars().count() <= MAX_LLM_CHARS {
                self.llm_convert(reading, &left_llm, &eff)
            } else {
                None
            };
            let mut system: Vec<String> = dict_candidates.clone();
            if let Some(top) = &llm_top {
                groups.push((CandidateSource::Llm, vec![top.clone()]));
                // 辞書候補の上位を LLM で再採点して並べ替える
                let n_head = eff.rescore_top.min(dict_candidates.len());
                let head: Vec<String> = dict_candidates[..n_head]
                    .iter()
                    .filter(|s| *s != top)
                    .cloned()
                    .collect();
                if !head.is_empty()
                    && let Some(scores) = self.llm_scores(reading, &left_llm, head.clone(), &eff)
                    && scores.len() == head.len()
                {
                    let mut ranked: Vec<(String, f32)> = head.into_iter().zip(scores).collect();
                    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
                    let mut reordered: Vec<String> = ranked.into_iter().map(|(s, _)| s).collect();
                    reordered.extend(dict_candidates.iter().skip(n_head).cloned());
                    // LLM の最良変換自体は llm グループにあるので、system 側にも残して出所を併記させる
                    if dict_candidates.contains(top) {
                        reordered.push(top.clone());
                    }
                    system = reordered;
                }
            }
            // システム辞書の候補は、読みと同じ表記だけ出所を kana にする
            let (kana, system): (Vec<String>, Vec<String>) = system
                .into_iter()
                .partition(|s| source_of(s, reading) == CandidateSource::Kana);
            groups.push((CandidateSource::SystemDict, system));
            groups.push((CandidateSource::Kana, kana));
        }
        let rewriter_candidates = self.rewriter.candidates(reading);
        groups.push((CandidateSource::Rewriter, rewriter_candidates));

        let mut out = merge_by_order(&eff.ranking, groups);
        out.truncate(limit);
        out
    }

    fn predict(&self, prefix: &str, left_context: &str, limit: usize) -> Vec<BackendCandidate> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let overlay = self.runtime.overlay();
        let hash = context_hash(&tail(left_context, HASH_CONTEXT_CHARS));
        let mut out: Vec<BackendCandidate> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (surface, _score) in self.runtime.learn_predict(prefix, hash, limit) {
            if seen.insert(surface.clone()) {
                out.push(BackendCandidate {
                    surface,
                    sources: vec![CandidateSource::Learned],
                });
            }
        }
        for (_, entry) in overlay.lookup_prefix(prefix, limit) {
            if seen.insert(entry.surface.clone()) {
                out.push(BackendCandidate {
                    surface: entry.surface.clone(),
                    sources: vec![CandidateSource::UserDict],
                });
            }
        }
        out.truncate(limit);
        out
    }

    fn reconvert(&self, surface: &str) -> Vec<BackendSegment> {
        match &self.dict {
            Some(dict) => dict
                .reverse_convert(surface)
                .into_iter()
                .map(|s| BackendSegment {
                    reading: s.reading,
                    surface: s.surface,
                    sources: vec![CandidateSource::SystemDict],
                })
                .collect(),
            None => Vec::new(),
        }
    }

    fn sentence_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendSentence> {
        match &self.dict {
            Some(dict) if limit > 0 => {
                // 1件目は convert_sentence（LLMが使えるなら再採点済みの1-best）と同じ結果にする。
                // 辞書のコスト順だけで並べると、LLM再採点で選ばれるはずの表記が後ろに回って
                // 変換品質が落ちるため（辞書ベースのN-bestはLLM再採点を行わない。v1のスコープ外）
                let best = self.convert_sentence(reading, left_context);
                let best_surface: String = best.iter().map(|s| s.surface.as_str()).collect();
                let mut out = vec![BackendSentence { segments: best }];
                let overlay = self.runtime.overlay();
                for c in dict.convert_n_best(reading, limit, Some(&overlay)) {
                    if out.len() >= limit {
                        break;
                    }
                    let surface: String = c.segments.iter().map(|s| s.surface.as_str()).collect();
                    if surface == best_surface {
                        continue;
                    }
                    out.push(BackendSentence {
                        segments: c
                            .segments
                            .into_iter()
                            .map(|s| BackendSegment {
                                reading: s.reading,
                                surface: s.surface,
                                sources: vec![CandidateSource::SystemDict],
                            })
                            .collect(),
                    });
                }
                out
            }
            _ => Vec::new(),
        }
    }
}

/// 使い方の確認用に、設定なしの既定の待ち時間を返す
pub fn default_sync_budget() -> Duration {
    Duration::from_millis(150)
}
