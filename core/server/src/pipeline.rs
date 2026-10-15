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

use crate::fuzzy::{FuzzyEngine, Suggestion};
use crate::runtime::{Effective, Event, ModelControl, Runtime};
use nagi_dict::{Dictionary, Overlay};
use nagi_engine::{
    BackendCandidate, BackendSegment, BackendSentence, CommitContext, Completion,
    ConversionBackend, TypingHint,
};
use nagi_infer::{ConversionModel, ConvertRequest};
use nagi_learn::context_hash;
use nagi_protocol::methods::NotifyLevel;
use nagi_protocol::view::CandidateSource;
use nagi_rewriter::{EmojiRewriter, Rewriter, RewriterChain};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
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

/// モデルの解放 / 再読み込みの状態。推論スレッドと共有する
#[derive(Default)]
struct LlmState {
    /// 変換モデルを解放している（または解放を始めた）。真の間は推論を始めない
    unloaded: AtomicBool,
    /// 解放・再読み込みのたびに進める世代。推論の開始時と完了時で変わっていたら、その結果は捨てる
    generation: AtomicU64,
}

impl LlmState {
    fn is_unloaded(&self) -> bool {
        self.unloaded.load(Ordering::SeqCst)
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

struct Llm {
    model: SharedModel,
    state: Arc<LlmState>,
    cache: Cache,
    /// 直前に `reconcile_active` を呼んだときのプロファイル（変化を検出するため）
    last_profile: Mutex<nagi_profile::Profile>,
}

/// LLM の非同期完了通知の置き場
struct Completions {
    queue: Mutex<Vec<Completion>>,
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Completions {
    fn new() -> Self {
        Completions {
            queue: Mutex::new(Vec::new()),
            waker: Mutex::new(None),
        }
    }

    fn push(&self, completion: Completion) {
        if let Ok(mut q) = self.queue.lock() {
            q.push(completion);
        }
        // waker はロックの外で呼ぶ
        let waker = self.waker.lock().ok().and_then(|w| w.clone());
        if let Some(waker) = waker {
            waker();
        }
    }
}

/// 変換パイプライン
pub struct Pipeline {
    dict: Option<Arc<Dictionary>>,
    /// 変換モデル。起動時に無くても、非同期の読み込み完了後に [`Pipeline::attach_model`] で一度だけ入る
    llm: OnceLock<Llm>,
    runtime: Arc<Runtime>,
    completions: Arc<Completions>,
    rewriter: RewriterChain,
    /// 曖昧入力の提案器（隣接表などを最初に使うときに作る。無効な間は作らない）
    fuzzy: OnceLock<FuzzyEngine>,
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
        let llm = OnceLock::new();
        if let Some(m) = model {
            let _ = llm.set(Self::make_llm(m, profile));
        }
        let name = backend_name(dict.is_some(), llm.get().is_some());
        runtime.set_backend_name(name);
        Pipeline {
            dict,
            llm,
            runtime,
            completions: Arc::new(Completions::new()),
            rewriter: RewriterChain::default_chain(),
            fuzzy: OnceLock::new(),
        }
    }

    fn make_llm(model: Box<dyn ConversionModel>, profile: nagi_profile::Profile) -> Llm {
        Llm {
            model: Arc::new(Mutex::new(model)),
            state: Arc::new(LlmState::default()),
            cache: Arc::new(Mutex::new(HashMap::new())),
            last_profile: Mutex::new(profile),
        }
    }

    /// 読み込みが済んだ変換モデルを後から取り付ける（起動を待たせないための非同期読み込み用）。
    /// 取り付けたあと、実行時状態に LLM ありを伝えて環境プロファイルを判定し直し（`profile_changed`
    /// 通知）、続けてウォームアップ / キャリブレーションを別スレッドで始める。
    /// 取り付け前の変換は辞書だけで応答する。すでに取り付け済みなら何もしない
    pub fn attach_model(&self, model: Box<dyn ConversionModel>) {
        // 先に llm を入れてから runtime に知らせる（use_llm が true になった時点で llm が必ずある）
        if self
            .llm
            .set(Self::make_llm(model, self.runtime.profile()))
            .is_err()
        {
            return;
        }
        self.runtime
            .set_backend_name(backend_name(self.dict.is_some(), true));
        // 解放・再読み込みの判定（Runtime::evaluate_model）と直列化して、判定と取り付けの間に
        // 入った解放要求を取りこぼさない
        let unloaded = self.runtime.serialize_model_changes(|| {
            if self.runtime.model_unloaded()
                && let Some(llm) = self.llm.get()
            {
                // 読み込み中にメモリ逼迫などで解放が要求された: 取り付けた直後に重みを手放す
                // （再読み込みが要求されるまで辞書のみ）
                llm.state.unloaded.store(true, Ordering::SeqCst);
                let generation = llm.state.generation.fetch_add(1, Ordering::SeqCst) + 1;
                Self::spawn_set_active(llm.model.clone(), llm.state.clone(), generation, false);
                true
            } else {
                self.runtime.set_llm_loaded(true);
                false
            }
        });
        if !unloaded {
            self.warm_up();
        }
    }

    /// 別スレッドでモデルの重みを解放 / 再読み込みする（呼び出し側を待たせない。実行中の推論があれば
    /// それが終わってから）。実行時に世代が `generation` から進んでいる（その後に解放・再読み込みが
    /// 要求された）要求は捨てる
    fn spawn_set_active(model: SharedModel, state: Arc<LlmState>, generation: u64, active: bool) {
        let _ = std::thread::Builder::new()
            .name("nagi-llm-active".into())
            .spawn(move || {
                if let Ok(mut m) = model.lock() {
                    if state.generation() != generation {
                        return;
                    }
                    let _ = m.set_active(active);
                }
            });
    }

    /// プロファイルの変化を見て、LLM をアクティブ / 非アクティブに切り替え、辞書の逆引き索引
    /// （`reconvert` 用。実データで約300MB規模。G-29 追記）を解放する（`Minimal` になったら
    /// 解放、そこから戻ったら次回使うときに再構築・再読み込み）。LLM の切り替えは別スレッドで
    /// 行うので待たせない
    fn reconcile_active(&self) {
        let current = self.runtime.profile();

        if let Some(llm) = self.llm.get()
            && let Ok(mut last) = llm.last_profile.lock()
            && *last != current
        {
            *last = current;
            drop(last);
            // 明示的に解放している間は、プロファイルが上がっても再読み込みしない（reload_model の仕事）
            let should_be_active =
                current != nagi_profile::Profile::Minimal && !llm.state.is_unloaded();
            Self::spawn_set_active(
                llm.model.clone(),
                llm.state.clone(),
                llm.state.generation(),
                should_be_active,
            );
        }

        if matches!(
            current,
            nagi_profile::Profile::Minimal | nagi_profile::Profile::Light
        ) && let Some(dict) = &self.dict
        {
            dict.release_reverse_index();
        }
    }

    /// 起動直後の最初の推論が遅くならないよう、別スレッドでモデルを温め、続けてキャリブレーション
    /// 推論（短い変換 3 回の中央値）を行って環境プロファイルの初回判定に使う
    pub fn warm_up(&self) {
        let Some(llm) = self.llm.get() else {
            return;
        };
        let model = llm.model.clone();
        let state = llm.state.clone();
        let runtime = self.runtime.clone();
        let _ = std::thread::Builder::new()
            .name("nagi-llm-warmup".into())
            .spawn(move || {
                let started_at = Instant::now();
                let req = Self::request(CALIBRATION_READING, "", None);
                let mut samples = Vec::new();
                if let Ok(mut m) = model.lock() {
                    if state.is_unloaded() {
                        return;
                    }
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
                eprintln!(
                    "imserver: ウォームアップ/キャリブレーション完了 ({} ms)",
                    started_at.elapsed().as_millis()
                );
            });
    }

    fn request(reading: &str, left: &str, persona: Option<&str>) -> ConvertRequest {
        ConvertRequest {
            reading: reading.to_string(),
            left_context: tail(left, 40),
            profile: persona.map(str::to_string),
            max_new_tokens: reading.chars().count() * 3 + 8,
            // 文全体の N-best（候補ウィンドウの sentence モード）は次段で追加する。
            // ここではまだ既存の貪欲デコードと同じ動作にする
            beam_width: 1,
        }
    }

    /// LLM の最良変換（貪欲デコード）。予算内に終わらない・失敗・不正な出力なら `None`
    /// outer が Some ならバックグラウンド完了時に通知を積む
    fn llm_convert(
        &self,
        reading: &str,
        left: &str,
        eff: &Effective,
        outer: Option<&Completion>,
    ) -> Option<String> {
        let llm = self.llm.get()?;
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

        // スロット: タイムアウト時に状態を判定して、Abandoned ならキャッシュに入れて通知を積む
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Slot {
            Waiting,
            Abandoned,
            Done,
        }
        let slot = Arc::new(Mutex::new(Slot::Waiting));

        let (tx, rx) = mpsc::sync_channel::<Option<String>>(1);
        let (model, cache, runtime) = (llm.model.clone(), llm.cache.clone(), self.runtime.clone());
        let state = llm.state.clone();
        let generation = state.generation();
        let completions = self.completions.clone();
        let req = Self::request(reading, left, eff.persona.as_deref());
        let slot_bg = slot.clone();
        let key_bg = key.clone();
        let outer_bg = outer.cloned();
        let n_threads = eff.n_threads;
        std::thread::Builder::new()
            .name("nagi-llm-convert".into())
            .spawn(move || {
                let result = model.lock().ok().and_then(|mut m| {
                    // 待っている間に解放されたら、推論せず（重みを読み直さず）捨てる
                    if state.is_unloaded() || state.generation() != generation {
                        return None;
                    }
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
                // 推論中に解放・再読み込みされていたら、結果はキャッシュにも通知にも使わない
                let result = result.filter(|_| state.generation() == generation);
                if let (Some(surface), Ok(mut cache)) = (&result, cache.lock()) {
                    if cache.len() >= CACHE_CAPACITY {
                        cache.clear();
                    }
                    cache.insert(key_bg.clone(), surface.clone());
                }

                // 呼び出し側が待ちをやめていたら、完了を通知として積む。そうでなければ結果を渡す
                let abandoned = slot_bg.lock().is_ok_and(|mut s| {
                    let abandoned = *s == Slot::Abandoned;
                    if !abandoned {
                        *s = Slot::Done;
                    }
                    abandoned
                });
                if abandoned {
                    if let Some(c) = outer_bg
                        && result.is_some()
                    {
                        completions.push(c);
                    }
                } else {
                    let _ = tx.send(result);
                }
            })
            .ok()?;

        match rx.recv_timeout(eff.sync_budget) {
            Ok(result) => result,
            Err(_) => {
                // タイムアウト。ただし直前に結果が届いていれば（Done）それを使う
                let done = slot.lock().is_ok_and(|mut s| {
                    let done = *s == Slot::Done;
                    if !done {
                        *s = Slot::Abandoned;
                    }
                    done
                });
                if done {
                    rx.try_recv().ok().flatten()
                } else {
                    None
                }
            }
        }
    }

    /// 文全体の N-best（ビームサーチ）。予算内に終わらない・失敗・モデルなしなら空を返す
    /// （呼び出し側が最良の 1 案に縮退する）。文節候補の再採点（[`Self::llm_scores`]）と同様、
    /// 候補ウィンドウを開いたときだけの呼び出しなので、非同期の完了通知は行わない
    fn llm_beam_convert(
        &self,
        reading: &str,
        left: &str,
        eff: &Effective,
        width: usize,
    ) -> Vec<String> {
        let Some(llm) = self.llm.get() else {
            return Vec::new();
        };
        let (tx, rx) = mpsc::sync_channel::<Vec<String>>(1);
        let model = llm.model.clone();
        let state = llm.state.clone();
        let mut req = Self::request(reading, left, eff.persona.as_deref());
        req.beam_width = width;
        let reading_owned = reading.to_string();
        let spawned = std::thread::Builder::new()
            .name("nagi-llm-beam".into())
            .spawn(move || {
                let hyps = model
                    .lock()
                    .ok()
                    .filter(|_| !state.is_unloaded())
                    .and_then(|mut m| m.convert(&req).ok())
                    .unwrap_or_default();
                let surfaces: Vec<String> = hyps
                    .into_iter()
                    .map(|h| h.surface)
                    .filter(|s| plausible(&reading_owned, s))
                    .collect();
                let _ = tx.send(surfaces);
            });
        if spawned.is_err() {
            return Vec::new();
        }
        rx.recv_timeout(eff.sync_budget).unwrap_or_default()
    }

    /// 候補の対数確率スコア（予算内に終わらなければ `None`）
    fn llm_scores(
        &self,
        reading: &str,
        left: &str,
        candidates: Vec<String>,
        eff: &Effective,
    ) -> Option<Vec<f32>> {
        let llm = self.llm.get()?;
        let (tx, rx) = mpsc::sync_channel::<Option<Vec<f32>>>(1);
        let model = llm.model.clone();
        let state = llm.state.clone();
        let req = Self::request(reading, left, eff.persona.as_deref());
        let n_threads = eff.n_threads;
        std::thread::Builder::new()
            .name("nagi-llm-score".into())
            .spawn(move || {
                let scores = model.lock().ok().and_then(|mut m| {
                    if state.is_unloaded() {
                        return None;
                    }
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
        outer: Option<&Completion>,
    ) -> Vec<BackendSegment> {
        let Some(surface) = self.llm_convert(reading, left, eff, outer) else {
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
        outer: Option<&Completion>,
    ) -> Vec<BackendSegment> {
        if reading.chars().count() <= MAX_LLM_CHARS {
            return self.convert_chunk(dict, reading, left, eff, overlay, outer);
        }
        // 長い入力: 辞書の文節境界で 30 文字前後のチャンクに分け、前のチャンクの結果を左文脈にして順に変換する
        let mut out: Vec<BackendSegment> = Vec::new();
        let mut chunk = String::new();
        let mut context = left.to_string();
        let flush = |chunk: &mut String, out: &mut Vec<BackendSegment>, context: &mut String| {
            if chunk.is_empty() {
                return;
            }
            for seg in self.convert_chunk(dict, chunk, context, eff, overlay, outer) {
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

    /// 曖昧入力の補正候補（「もしかして」）。無効・辞書なし・読みが既知（学習・ユーザー辞書・辞書語）なら空。
    /// 展開は `EXPANSION_BUDGET_MS` と同期予算のうち短い方で打ち切る
    fn fuzzy_suggestions(
        &self,
        reading: &str,
        eff: &Effective,
        overlay: &Overlay,
        known: bool,
        typing: &TypingHint<'_>,
    ) -> Vec<Suggestion> {
        let Some(dict) = &self.dict else {
            return Vec::new();
        };
        if !eff.fuzzy.enabled {
            return Vec::new();
        }
        self.fuzzy.get_or_init(FuzzyEngine::new).suggest(
            dict,
            overlay,
            reading,
            typing,
            &eff.fuzzy,
            known,
            eff.sync_budget,
        )
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
        CandidateSource::Fuzzy,
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
                        reading: None,
                    }),
                }
            }
        }
    }
    out
}

/// 候補を `limit` 件に切り詰める。曖昧入力の候補（出所が `Fuzzy` だけ）は、通常の候補が多くても
/// 押し出されないよう、通常の候補の方を切る（並びは変えない）
fn truncate_keeping_fuzzy(out: &mut Vec<BackendCandidate>, limit: usize) {
    let is_fuzzy = |c: &BackendCandidate| c.sources == [CandidateSource::Fuzzy];
    let n_fuzzy = out.iter().filter(|c| is_fuzzy(c)).count().min(limit);
    let mut keep_normal = limit - n_fuzzy;
    let mut keep_fuzzy = n_fuzzy;
    out.retain(|c| {
        let slot = if is_fuzzy(c) {
            &mut keep_fuzzy
        } else {
            &mut keep_normal
        };
        let keep = *slot > 0;
        *slot = slot.saturating_sub(1);
        keep
    });
}

impl ModelControl for Pipeline {
    fn unload_model(&self) {
        let Some(llm) = self.llm.get() else {
            // まだ取り付け前: 取り付け（attach_model）が Runtime の状態を見て解放する
            return;
        };
        // 先に印を付けて、待機中・これから始まる推論が重みに触らないようにする。実行中の推論は
        // 待たず、世代を進めて結果を捨てる
        llm.state.unloaded.store(true, Ordering::SeqCst);
        let generation = llm.state.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut cache) = llm.cache.lock() {
            cache.clear();
        }
        self.runtime.set_llm_loaded(false);
        Self::spawn_set_active(llm.model.clone(), llm.state.clone(), generation, false);
        self.runtime.push_event(Event {
            level: NotifyLevel::Info,
            code: "llm_unloaded",
            message: "変換モデルを解放しました（辞書のみで動作します）".to_string(),
        });
    }

    fn reload_model(&self) {
        let Some(llm) = self.llm.get() else {
            return;
        };
        if !llm.state.is_unloaded() {
            return;
        }
        // 以前の再読み込み・解放の要求を無効にする
        let generation = llm.state.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (model, state, runtime) = (llm.model.clone(), llm.state.clone(), self.runtime.clone());
        let spawned = std::thread::Builder::new()
            .name("nagi-llm-reload".into())
            .spawn(move || {
                let Ok(mut m) = model.lock() else {
                    return;
                };
                if state.generation() != generation {
                    // 待っている間に解放などが要求された。重みは手放したままにする
                    return;
                }
                match m.set_active(true) {
                    Ok(()) => {
                        // 読み込み中に解放が要求された（モデルのロックを握ったまま再確認する）:
                        // 重みを戻し、unloaded は true のまま通知も出さない
                        if state.generation() != generation {
                            let _ = m.set_active(false);
                            return;
                        }
                        state.unloaded.store(false, Ordering::SeqCst);
                        runtime.set_llm_loaded(true);
                        // 上の確認と状態反映の間に解放が割り込んだ場合（解放側は世代を先に進める）は戻す
                        if state.generation() != generation {
                            let _ = m.set_active(false);
                            state.unloaded.store(true, Ordering::SeqCst);
                            runtime.set_llm_loaded(false);
                            return;
                        }
                        // 読み込み済みなので、ページインを兼ねて 1 回だけ推論しておく（結果は使わない）
                        let _ = m.convert(&Self::request(CALIBRATION_READING, "", None));
                        drop(m);
                        runtime.push_event(Event {
                            level: NotifyLevel::Info,
                            code: "llm_reloaded",
                            message: "変換モデルを再び読み込みました".to_string(),
                        });
                    }
                    Err(_) => {
                        let _ = m.set_active(false);
                        drop(m);
                        if state.generation() != generation {
                            // 失敗を待つ間に解放などが要求された: その要求の状態を優先する
                            return;
                        }
                        runtime.model_reload_failed();
                        runtime.push_event(Event {
                            level: NotifyLevel::Warning,
                            code: "llm_reload_failed",
                            message:
                                "変換モデルを再読み込みできませんでした（辞書のみで動作を続けます）"
                                    .to_string(),
                        });
                    }
                }
            });
        if spawned.is_err() {
            self.runtime.model_reload_failed();
        }
    }
}

impl ConversionBackend for Pipeline {
    fn name(&self) -> &str {
        backend_name(self.dict.is_some(), self.llm.get().is_some())
    }

    fn take_completions(&self) -> Vec<Completion> {
        if let Ok(mut q) = self.completions.queue.lock() {
            let mut out = std::mem::take(&mut *q);
            // 重複を除く
            out.sort_by(|a, b| {
                (a.reading.as_str(), a.left_context.as_str())
                    .cmp(&(b.reading.as_str(), b.left_context.as_str()))
            });
            out.dedup();
            out
        } else {
            Vec::new()
        }
    }

    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut w) = self.completions.waker.lock() {
            *w = Some(waker);
        }
    }

    fn convert_sentence(&self, reading: &str, left_context: &str) -> Vec<BackendSegment> {
        self.runtime.tick();
        self.reconcile_active();
        let eff = self.runtime.effective();
        let overlay = self.runtime.overlay();
        let left_llm = tail(left_context, eff.max_context_chars);

        // 予算超過で LLM の結果を待てなかったとき、完了後に engine へ知らせるためのキー
        let outer = Completion {
            reading: reading.to_string(),
            left_context: left_context.to_string(),
        };

        let mut segments = match &self.dict {
            None => vec![BackendSegment {
                reading: reading.to_string(),
                surface: reading.to_string(),
                sources: vec![CandidateSource::Kana],
            }],
            Some(dict) if eff.use_llm && self.llm.get().is_some() => {
                self.convert_with_llm(dict, reading, &left_llm, &eff, &overlay, Some(&outer))
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
        // 学習・ユーザー辞書に読みがあるときは、意図した入力とみなして補正候補を出さない
        let known = groups.iter().any(|(_, items)| !items.is_empty());

        // LLM とシステム辞書
        if let Some(dict) = &self.dict {
            let dict_candidates: Vec<String> = dict
                .segment_candidates_with(reading, limit.max(30), Some(&overlay))
                .into_iter()
                .map(|c| c.surface)
                .collect();
            let llm_top = if eff.use_llm && reading.chars().count() <= MAX_LLM_CHARS {
                self.llm_convert(reading, &left_llm, &eff, None)
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
        let mut rewriter_candidates = self.rewriter.candidates(reading);
        // 絵文字・顔文字は Rewriter の末尾（読みが表に完全一致するときだけ）
        rewriter_candidates.extend(
            EmojiRewriter {
                emoji: eff.emoji,
                kaomoji: eff.kaomoji,
            }
            .rewrite(reading),
        );
        groups.push((CandidateSource::Rewriter, rewriter_candidates));

        // 曖昧入力: 通常の候補にすでにある表記は重ねない
        // 文節の候補には実際の打鍵列を対応づけられないので、読みからの復元で推定する
        let fuzzy: Vec<Suggestion> = self
            .fuzzy_suggestions(reading, &eff, &overlay, known, &TypingHint::default())
            .into_iter()
            .filter(|f| !groups.iter().any(|(_, items)| items.contains(&f.surface)))
            .collect();
        groups.push((
            CandidateSource::Fuzzy,
            fuzzy.iter().map(|f| f.surface.clone()).collect(),
        ));

        let mut out = merge_by_order(&eff.ranking, groups);
        for c in &mut out {
            if c.sources == [CandidateSource::Fuzzy] {
                c.reading = fuzzy
                    .iter()
                    .find(|f| f.surface == c.surface)
                    .map(|f| f.reading.clone());
            }
        }
        truncate_keeping_fuzzy(&mut out, limit);
        out
    }

    fn fuzzy_candidates(
        &self,
        reading: &str,
        typing: &TypingHint<'_>,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendCandidate> {
        let eff = self.runtime.effective();
        if !eff.fuzzy.enabled || self.dict.is_none() {
            return Vec::new();
        }
        let overlay = self.runtime.overlay();
        let hash = context_hash(&tail(left_context, HASH_CONTEXT_CHARS));
        let known = self
            .runtime
            .learn_lookup(reading, hash)
            .iter()
            .any(|(_, score)| *score > 0.0)
            || !overlay.lookup(reading).is_empty()
            || !overlay.snippets(reading).is_empty();
        self.fuzzy_suggestions(reading, &eff, &overlay, known, typing)
            .into_iter()
            .take(limit)
            .map(|s| BackendCandidate {
                surface: s.surface,
                sources: vec![CandidateSource::Fuzzy],
                reading: Some(s.reading),
            })
            .collect()
    }

    fn predict_next(
        &self,
        ctx: &CommitContext<'_>,
        prev: &str,
        limit: usize,
    ) -> Vec<BackendCandidate> {
        // 学習を止める状況（設定・secure input・停止アプリ）では出さない
        if prev.is_empty() || !self.runtime.learning_allowed(ctx.secure_input, ctx.app_id) {
            return Vec::new();
        }
        self.runtime
            .next_word_lookup(prev, limit)
            .into_iter()
            .map(|(surface, _)| BackendCandidate {
                surface,
                sources: vec![CandidateSource::Learned],
                reading: None,
            })
            .collect()
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
                    reading: None,
                });
            }
        }
        for (_, entry) in overlay.lookup_prefix(prefix, limit) {
            if seen.insert(entry.surface.clone()) {
                out.push(BackendCandidate {
                    surface: entry.surface.clone(),
                    sources: vec![CandidateSource::UserDict],
                    reading: None,
                });
            }
        }
        out.truncate(limit);
        out
    }

    fn reconvert(&self, surface: &str) -> Vec<BackendSegment> {
        self.reconcile_active();
        let Some(dict) = &self.dict else {
            return Vec::new();
        };
        let result = dict
            .reverse_convert(surface)
            .into_iter()
            .map(|s| BackendSegment {
                reading: s.reading,
                surface: s.surface,
                sources: vec![CandidateSource::SystemDict],
            })
            .collect();
        // `minimal`/`light` では速度よりメモリを優先する方針（G-13）に合わせ、使い終えたら
        // 都度手放す（実データで約270MB規模になる。次回 reconvert 時に作り直すコストを払う。
        // G-29 追記。圧縮しても大きく減らなかった経緯も参照）
        if matches!(
            self.runtime.profile(),
            nagi_profile::Profile::Minimal | nagi_profile::Profile::Light
        ) {
            dict.release_reverse_index();
        }
        result
    }

    /// 文全体の N-best（`conversion.initial_view = "sentence"` 用）。
    ///
    /// 1件目は必ず `convert_sentence`（LLMが使えるなら再採点済みの1-best）と同じ結果にする。
    /// コスト順・ビームサーチのスコア順だけで並べると、LLM再採点で選ばれるはずの表記が後ろに
    /// 回って変換品質が落ちるため（G-30の訂正と同じ理由）。
    ///
    /// 2件目以降は、実LLMのビームサーチ（G-33・G-34。真のモデル確率順）が使えればそちらを
    /// 優先し、使えない・件数が足りない場合は辞書ベースの k-best Viterbi（G-30）で補う
    fn sentence_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendSentence> {
        self.runtime.tick();
        self.reconcile_active();
        let Some(dict) = &self.dict else {
            return Vec::new();
        };
        if limit == 0 {
            return Vec::new();
        }
        let eff = self.runtime.effective();
        let overlay = self.runtime.overlay();
        let left_llm = tail(left_context, eff.max_context_chars);

        let best = self.convert_sentence(reading, left_context);
        let best_surface: String = best.iter().map(|s| s.surface.as_str()).collect();
        let mut out: Vec<BackendSentence> = vec![BackendSentence { segments: best }];
        let mut seen: Vec<String> = vec![best_surface];

        if eff.use_llm
            && self.llm.get().is_some()
            && eff.beam_width > 1
            && reading.chars().count() <= MAX_LLM_CHARS
        {
            let width = eff.beam_width.min(limit.max(1));
            for surface in self.llm_beam_convert(reading, &left_llm, &eff, width) {
                if out.len() >= limit || seen.contains(&surface) {
                    continue;
                }
                seen.push(surface.clone());
                let mut segments = match dict.align_with(reading, &surface, Some(&overlay)) {
                    Some(segs) => segs
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
                };
                self.apply_preferences(&mut segments, left_context, &eff, &overlay);
                out.push(BackendSentence { segments });
            }
        }

        if out.len() < limit {
            for c in dict.convert_n_best(reading, limit, Some(&overlay)) {
                if out.len() >= limit {
                    break;
                }
                let surface: String = c.segments.iter().map(|s| s.surface.as_str()).collect();
                if seen.contains(&surface) {
                    continue;
                }
                seen.push(surface.clone());
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
        }

        out
    }
}

/// 使い方の確認用に、設定なしの既定の待ち時間を返す
pub fn default_sync_budget() -> Duration {
    Duration::from_millis(150)
}
