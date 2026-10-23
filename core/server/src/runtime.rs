//! サーバー全体で共有する実行時の状態: 環境プロファイル・学習・ユーザー辞書・ランキング設定・通知。
//!
//! 変換パイプライン（[`crate::pipeline::Pipeline`]）、確定の通知先（[`crate::learning`]）、
//! JSON-RPC サーバー（[`crate::server::Server`]）が同じ [`Runtime`] を `Arc` で共有する。
//! 設定の再読み込み・`set_environment`・レイテンシ計測は、すべてここを通して反映される。

use crate::fuzzy::FuzzySettings;
use nagi_config::{Config, ProfileMode};
use nagi_dict::Overlay;
use nagi_learn::{LearnConfig, Learner, NextWordLearner};
use nagi_profile::{
    AutoConfig, Change, Controller, Decision, Environment, MIN_MEMORY_BUDGET_FOR_LLM_MB,
    MemoryPressure, Profile, SystemInfo, Thermal, initial_profile, initial_profile_with_budget,
};
use nagi_protocol::methods::{
    InputMethod, MemoryPressureState, NotifyLevel, SetEnvironmentParams, StatusResult,
    ThermalState, UpdateCandidatesParams, UpdatePreeditParams,
};
use nagi_protocol::view::CandidateSource;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

/// 学習ファイルを書き出す最短の間隔（設計書 §4.11: 5 分ごと）
const FLUSH_INTERVAL: Duration = Duration::from_secs(300);
/// 定期の flush が他プロセスの学習ファイルのロックを待つ最長時間（超えたら次回に回す）
const FLUSH_LOCK_TIMEOUT: Duration = Duration::from_millis(100);
/// 明示の flush（忘却・全削除）が待つ最長時間
const FLUSH_LOCK_TIMEOUT_FORCED: Duration = Duration::from_secs(2);
/// 次語予測のバイグラムを保存するファイル名（学習ファイルと同じディレクトリ）
const NEXT_WORD_FILE: &str = "learning-next.tsv";

/// メモリ逼迫が解消（`normal`）してから、解放していた変換モデルを再読み込みするまでの待ち時間。
/// 逼迫が短い間隔で繰り返されても、読み込みと解放を繰り返さないための猶予
pub const MODEL_RELOAD_DELAY: Duration = Duration::from_secs(30);

/// 変換モデルの解放・再読み込みを実際に行うもの（[`crate::pipeline::Pipeline`] が実装する）。
/// どちらも呼び出し側を待たせない（読み込みは別スレッド）。実行中の推論があっても安全
pub trait ModelControl: Send + Sync {
    /// 変換モデルをメモリから解放する。以降、再読み込みまで辞書のみで動く
    fn unload_model(&self);
    /// 解放したモデルを別スレッドで再読み込みする。完了すると [`Runtime::set_llm_loaded`] で LLM ありに戻る。
    /// 失敗したら [`Runtime::model_reload_failed`] を呼ぶ（辞書のみで継続）
    fn reload_model(&self);
}

/// 変換モデルの解放状態（Runtime から見たもの）
#[derive(Debug, Default)]
struct ModelState {
    /// 解放している（または解放を要求した）
    unloaded: bool,
    /// `unload_model`（明示の解放）が要求されている。`reload_model` まで自動では戻さない
    manual: bool,
    /// 自動再読み込みの予定時刻（`now_ms`）。条件が解消してから [`MODEL_RELOAD_DELAY`] 後
    resume_at: Option<u64>,
    /// 自動の再読み込みに失敗した。次に解放が必要になるまで（または明示の `reload_model` まで）再試行しない
    gave_up: bool,
}

/// ユーザーに知らせる出来事（`notify` 通知になる）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// 重大度
    pub level: NotifyLevel,
    /// 機械可読なコード（`profile_changed` / `config_error` など）
    pub code: &'static str,
    /// ユーザー向けの文言（入力内容は含めない）
    pub message: String,
}

/// 設定から決まる、実行中に参照される値
#[derive(Debug, Clone)]
pub struct Tunables {
    /// `profile.mode` が `auto` 以外なら、そのプロファイルに固定
    pub fixed_profile: Option<Profile>,
    /// 自動切替の設定
    pub auto: AutoConfig,
    /// LLM の推論を待つ最長時間
    pub sync_budget: Duration,
    /// `conversion.max_context_length`（未指定ならプロファイルに従う）
    pub max_context_override: Option<usize>,
    /// `conversion.beam_width`（未指定ならプロファイルに従う）
    pub beam_width_override: Option<usize>,
    /// `conversion.use_llm`
    pub llm_enabled: bool,
    /// `conversion.use_context`
    pub use_context: bool,
    /// `profile.persona`（空なら注入しない）
    pub persona: String,
    /// `learning.enabled`
    pub learning_enabled: bool,
    /// `predict.next_word`（確定直後の次語予測。バイグラムの記録と表示の両方を決める）
    pub next_word_enabled: bool,
    /// 学習しないアプリの識別子
    pub disabled_apps: Vec<String>,
    /// 候補の出所の優先順（`ranking.source_order`）
    pub ranking: Vec<CandidateSource>,
    /// `conversion.n_threads`（0 = 自動）
    pub n_threads: usize,
    /// `conversion.live_conversion`
    pub live_conversion_enabled: bool,
    /// `conversion.live_conversion_delay_ms`
    pub live_conversion_delay_ms: u64,
    /// `conversion.live_conversion_min_key_length`
    pub live_conversion_min_key_length: usize,
    /// `userdict.enabled`
    pub userdict_enabled: bool,
    /// `snippet.enabled`
    pub snippet_enabled: bool,
    /// `rewriter.emoji`
    pub emoji_enabled: bool,
    /// `rewriter.kaomoji`
    pub kaomoji_enabled: bool,
    /// `convert.calculator`
    pub calculator_enabled: bool,
    /// `convert.loanword.enabled`
    pub loanword_enabled: bool,
    /// `input.fuzzy.*`（曖昧入力）
    pub fuzzy: FuzzySettings,
}

/// 出所名（`ranking.source_order` の要素）を出所に直す
pub fn parse_source(name: &str) -> Option<CandidateSource> {
    Some(match name {
        "learned" => CandidateSource::Learned,
        "user_dict" => CandidateSource::UserDict,
        "llm" => CandidateSource::Llm,
        "system_dict" => CandidateSource::SystemDict,
        "kana" => CandidateSource::Kana,
        "rewriter" => CandidateSource::Rewriter,
        "fuzzy" => CandidateSource::Fuzzy,
        _ => return None,
    })
}

impl Default for Tunables {
    fn default() -> Self {
        Tunables::from_config(&Config::default())
    }
}

impl Tunables {
    /// 設定から作る
    pub fn from_config(config: &Config) -> Tunables {
        Tunables {
            fixed_profile: match config.profile.mode {
                ProfileMode::Auto => None,
                ProfileMode::Quality => Some(Profile::Quality),
                ProfileMode::Balanced => Some(Profile::Balanced),
                ProfileMode::Light => Some(Profile::Light),
                ProfileMode::Minimal => Some(Profile::Minimal),
            },
            auto: AutoConfig {
                enabled: config.profile.auto_switch,
                max_latency_ms: config.conversion.max_latency_ms as f64,
                ..AutoConfig::default()
            },
            sync_budget: Duration::from_millis(config.conversion.sync_budget_ms),
            max_context_override: config.conversion.max_context_length,
            beam_width_override: config.conversion.beam_width,
            llm_enabled: config.conversion.use_llm,
            use_context: config.conversion.use_context,
            persona: config.profile.persona.clone(),
            learning_enabled: config.learning.enabled,
            next_word_enabled: config.predict.next_word,
            disabled_apps: config.learning.disabled_apps.clone(),
            ranking: config
                .ranking
                .source_order
                .iter()
                .filter_map(|n| parse_source(n))
                .collect(),
            n_threads: config.conversion.n_threads,
            live_conversion_enabled: config.conversion.live_conversion,
            live_conversion_delay_ms: config.conversion.live_conversion_delay_ms,
            live_conversion_min_key_length: config.conversion.live_conversion_min_key_length,
            userdict_enabled: config.userdict.enabled,
            snippet_enabled: config.snippet.enabled,
            emoji_enabled: config.rewriter.emoji,
            kaomoji_enabled: config.rewriter.kaomoji,
            calculator_enabled: config.convert.calculator,
            loanword_enabled: config.convert.loanword.enabled,
            fuzzy: FuzzySettings::from_config(&config.input.fuzzy),
        }
    }
}

/// 変換の 1 回ごとに参照する有効なパラメータ（プロファイルと設定を合成したもの）
#[derive(Debug, Clone)]
pub struct Effective {
    /// LLM を使うか
    pub use_llm: bool,
    /// LLM で再採点する辞書候補の数
    pub rescore_top: usize,
    /// 左文脈の最大文字数（`use_context = false` なら 0）
    pub max_context_chars: usize,
    /// LLM に渡す persona（空なら `None`）
    pub persona: Option<String>,
    /// 推論を待つ最長時間
    pub sync_budget: Duration,
    /// 候補の出所の優先順
    pub ranking: Vec<CandidateSource>,
    /// 推論のスレッド数（`light` プロファイルでは半分に絞る）
    pub n_threads: usize,
    /// 文全体の N-best（辞書のk-best・実LLMのビームサーチとも）の幅・候補数の起点。
    /// 1 なら貪欲デコードと同じ（`conversion.beam_width`。未指定ならプロファイルの既定値）
    pub beam_width: usize,
    /// 読みからの絵文字候補（`rewriter.emoji`）
    pub emoji: bool,
    /// 読みからの顔文字候補（`rewriter.kaomoji`）
    pub kaomoji: bool,
    /// 電卓候補（`convert.calculator`）
    pub calculator: bool,
    /// カタカナ語の原語の候補（`convert.loanword.enabled`）
    pub loanword: bool,
    /// 曖昧入力（`input.fuzzy.*`）
    pub fuzzy: FuzzySettings,
}

/// 実行時の共有状態
pub struct Runtime {
    started: Instant,
    system: SystemInfo,
    controller: Mutex<Controller>,
    llm_loaded: AtomicBool,
    calibration_ms: Mutex<Option<f64>>,
    tunables: RwLock<Tunables>,
    learner: Mutex<Learner>,
    learn_path: Option<PathBuf>,
    /// 次語予測のバイグラム（`learning-next.tsv`。`learn_path` と同じディレクトリ）
    next_learner: Mutex<NextWordLearner>,
    next_path: Option<PathBuf>,
    last_flush: Mutex<Instant>,
    /// 読み込んだユーザー辞書そのもの（件数の表示と、設定変更時の作り直しに使う）
    overlay_raw: RwLock<Arc<Overlay>>,
    /// 変換に使うオーバーレイ（`userdict.enabled` / `snippet.enabled` で絞ったもの）
    overlay: RwLock<Arc<Overlay>>,
    events: Mutex<Vec<Event>>,
    /// ライブ変換等の非同期な preedit 更新（`update_preedit` 通知になる）
    preedit_updates: Mutex<Vec<UpdatePreeditParams>>,
    candidate_updates: Mutex<Vec<UpdateCandidatesParams>>,
    /// イベントが積まれたときに呼ぶフック（`Server::run` が非同期通知の配信に使う）。
    /// `Box<dyn Fn() + Send + Sync>` なので中身は任意（チャンネルへの送信等）
    event_hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    config_path: Mutex<Option<PathBuf>>,
    config_error: AtomicBool,
    backend_name: Mutex<String>,
    calibration_path: Mutex<Option<PathBuf>>,
    /// プロセスのメモリ上限（MB）。0 は上限なし
    memory_budget_mb: AtomicU64,
    /// フロントエンドの入力方式（`initialize` の capabilities）
    input_method: Mutex<InputMethod>,
    model: Mutex<ModelState>,
    model_control: Mutex<Option<Weak<dyn ModelControl>>>,
    /// モデルの解放・再読み込みの判定（状態の更新と実行先への指示）を直列化する
    model_apply: Mutex<()>,
    reload_delay: Mutex<Duration>,
}

impl Runtime {
    /// 実行時状態を作る。`has_llm` は変換モデルを読み込めたか
    pub fn new(
        system: SystemInfo,
        has_llm: bool,
        tunables: Tunables,
        learner: Learner,
        learn_path: Option<PathBuf>,
    ) -> Arc<Runtime> {
        Self::new_with_budget(system, has_llm, tunables, learner, learn_path, None)
    }

    /// [`Self::new`] に、プロセスのメモリ上限（MB。`None` なら上限なし）を加えた版。
    /// 初回のプロファイル判定から上限を反映する
    pub fn new_with_budget(
        system: SystemInfo,
        has_llm: bool,
        tunables: Tunables,
        learner: Learner,
        learn_path: Option<PathBuf>,
        memory_budget_mb: Option<u64>,
    ) -> Arc<Runtime> {
        let decision = Self::decide(&tunables, &system, has_llm, None, memory_budget_mb);
        let controller =
            Controller::new(decision, tunables.fixed_profile.is_some(), tunables.auto, 0);
        // 次語予測の学習ファイルは通常の学習ファイルの隣。読めなければ空で始め、上書きしない
        let mut next_path = learn_path
            .as_ref()
            .map(|p| p.with_file_name(NEXT_WORD_FILE));
        let mut next_load_error = None;
        let next_learner = match &next_path {
            Some(path) => match NextWordLearner::load(path, learner.config().clone()) {
                Ok((l, _)) => l,
                Err(e) => {
                    next_load_error = Some(e.to_string());
                    next_path = None;
                    NextWordLearner::new(learner.config().clone())
                }
            },
            None => NextWordLearner::new(learner.config().clone()),
        };
        let runtime = Arc::new(Runtime {
            started: Instant::now(),
            system,
            controller: Mutex::new(controller),
            llm_loaded: AtomicBool::new(has_llm),
            calibration_ms: Mutex::new(None),
            tunables: RwLock::new(tunables),
            learner: Mutex::new(learner),
            learn_path,
            next_learner: Mutex::new(next_learner),
            next_path,
            last_flush: Mutex::new(Instant::now()),
            overlay_raw: RwLock::new(Arc::new(Overlay::default())),
            overlay: RwLock::new(Arc::new(Overlay::default())),
            events: Mutex::new(Vec::new()),
            preedit_updates: Mutex::new(Vec::new()),
            candidate_updates: Mutex::new(Vec::new()),
            event_hook: Mutex::new(None),
            config_path: Mutex::new(None),
            config_error: AtomicBool::new(false),
            backend_name: Mutex::new(String::new()),
            calibration_path: Mutex::new(None),
            memory_budget_mb: AtomicU64::new(memory_budget_mb.unwrap_or(0)),
            input_method: Mutex::new(InputMethod::Physical),
            model: Mutex::new(ModelState::default()),
            model_control: Mutex::new(None),
            model_apply: Mutex::new(()),
            reload_delay: Mutex::new(MODEL_RELOAD_DELAY),
        });
        if let Some(e) = next_load_error {
            runtime.push_event(Event {
                level: NotifyLevel::Warning,
                code: "next_word_load_failed",
                message: format!("次語予測の学習データを読み込めませんでした: {e}"),
            });
        }
        runtime
    }

    /// テスト・LLM なしの構成向けの既定（学習は空、ファイルへは書かない）
    pub fn detached(has_llm: bool) -> Arc<Runtime> {
        Runtime::new(
            nagi_profile::detect(),
            has_llm,
            Tunables::default(),
            Learner::new(LearnConfig::default()),
            None,
        )
    }

    fn decide(
        t: &Tunables,
        system: &SystemInfo,
        has_llm: bool,
        calibration: Option<f64>,
        memory_budget_mb: Option<u64>,
    ) -> Decision {
        match t.fixed_profile {
            Some(p) if has_llm || p == Profile::Minimal => Decision {
                profile: p,
                reason: format!("設定（profile.mode = {}）で固定されています", p.as_str()),
            },
            Some(_) => initial_profile(system, None, false),
            None => initial_profile_with_budget(system, calibration, has_llm, memory_budget_mb),
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// 経過時間を進める（ヒステリシス後の復帰判定）。`Server` が要求のたびに呼ぶ
    pub fn tick(&self) {
        let change = self
            .controller
            .lock()
            .ok()
            .and_then(|mut c| c.tick(self.now_ms()));
        self.announce(change);
        // メモリ逼迫の解消から時間が経っていれば、解放していたモデルを戻す
        self.evaluate_model(false);
    }

    fn announce(&self, change: Option<Change>) {
        if let Some(c) = change {
            self.push_event(Event {
                level: NotifyLevel::Info,
                code: "profile_changed",
                message: format!(
                    "環境プロファイル: {} → {}（{}）",
                    c.from.as_str(),
                    c.to.as_str(),
                    c.reason
                ),
            });
        }
    }

    /// イベントが積まれたときに呼ぶフックを設定する（`Server::run` 用。既存のフックは置き換える）
    pub fn set_event_hook(&self, hook: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut h) = self.event_hook.lock() {
            *h = Some(hook);
        }
    }

    /// `event_hook` が設定されていれば呼ぶ（`push_event` / `push_preedit_update` 共通）
    fn fire_event_hook(&self) {
        if let Ok(hook) = self.event_hook.lock()
            && let Some(f) = hook.as_ref()
        {
            f();
        }
    }

    /// 通知を積む。フックが設定されていれば呼ぶ（`Server::run` を起こして即座に配信させる）
    pub fn push_event(&self, event: Event) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
        self.fire_event_hook();
    }

    /// 積まれた通知を取り出す
    pub fn drain_events(&self) -> Vec<Event> {
        self.events
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }

    /// ライブ変換等の preedit 更新を積む。フックが設定されていれば呼ぶ
    pub fn push_preedit_update(&self, params: UpdatePreeditParams) {
        if let Ok(mut q) = self.preedit_updates.lock() {
            q.push(params);
        }
        self.fire_event_hook();
    }

    /// 入力中の候補の更新を積む（バックグラウンドで作った候補をフロントへ届ける）
    pub fn push_candidates_update(&self, params: UpdateCandidatesParams) {
        if let Ok(mut q) = self.candidate_updates.lock() {
            q.push(params);
        }
        self.fire_event_hook();
    }

    /// 積まれた候補の更新を取り出す
    pub fn drain_candidates_updates(&self) -> Vec<UpdateCandidatesParams> {
        self.candidate_updates
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }

    /// 積まれた preedit 更新を取り出す
    pub fn drain_preedit_updates(&self) -> Vec<UpdatePreeditParams> {
        self.preedit_updates
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }

    // ---- 環境プロファイル ----

    /// 現在のプロファイル
    pub fn profile(&self) -> Profile {
        self.controller
            .lock()
            .map_or(Profile::Minimal, |c| c.current())
    }

    /// 現在のプロファイルになった理由
    pub fn profile_reason(&self) -> String {
        self.controller
            .lock()
            .map(|c| c.reason().to_string())
            .unwrap_or_default()
    }

    /// LLM 変換 1 回のレイテンシを報告する（目標を超え続ければ 1 段軽くなる）
    pub fn note_latency(&self, ms: f64) {
        let change = self
            .controller
            .lock()
            .ok()
            .and_then(|mut c| c.observe_latency(ms, self.now_ms()));
        self.announce(change);
    }

    /// キャリブレーション結果の保存先を設定する（`None` なら保存しない）
    pub fn set_calibration_path(&self, path: Option<PathBuf>) {
        if let Ok(mut p) = self.calibration_path.lock() {
            *p = path;
        }
    }

    /// 前回の実測値を初回判定の入力として置く（判定のやり直し・保存はしない。モデルの非同期読み込みで、
    /// [`Self::set_llm_loaded`] より前に呼んで、プロファイルが一度で確定するようにする）
    pub fn set_calibration_hint(&self, median_ms: f64) {
        if let Ok(mut c) = self.calibration_ms.lock() {
            *c = Some(median_ms);
        }
    }

    /// 変換モデルの読み込み完了（または解放）を反映して、環境プロファイルを判定し直す。
    /// プロファイルが変われば `profile_changed` 通知が積まれる
    pub fn set_llm_loaded(&self, loaded: bool) {
        self.llm_loaded.store(loaded, Ordering::Relaxed);
        self.recompute_profile(true);
    }

    /// キャリブレーション推論の実測（中央値、ミリ秒）で、初回判定をやり直す
    pub fn finish_calibration(&self, median_ms: f64) {
        if let Ok(mut c) = self.calibration_ms.lock() {
            *c = Some(median_ms);
        }
        self.recompute_profile(true);
        self.save_calibration(median_ms);
    }

    fn save_calibration(&self, median_ms: f64) {
        let Ok(path_guard) = self.calibration_path.lock() else {
            return;
        };
        let Some(path) = path_guard.as_ref() else {
            return;
        };
        let path = path.clone();
        drop(path_guard);
        let backend = self
            .backend_name
            .lock()
            .map(|n| n.clone())
            .unwrap_or_default();
        let record = crate::calibration::CalibrationRecord {
            median_ms,
            cpu_cores: self.system.cpu_cores,
            total_memory_mb: self.system.total_memory_mb,
            backend,
        };
        if let Err(e) = crate::calibration::save(&path, &record) {
            self.push_event(Event {
                level: NotifyLevel::Warning,
                code: "calibration_save_failed",
                message: format!("キャリブレーション結果を保存できませんでした: {e}"),
            });
        }
    }

    fn recompute_profile(&self, force: bool) {
        let (Ok(t), Ok(cal)) = (self.tunables.read(), self.calibration_ms.lock()) else {
            return;
        };
        let has_llm = self.llm_loaded.load(Ordering::Relaxed);
        let decision = Self::decide(&t, &self.system, has_llm, *cal, self.memory_budget_mb());
        let now = self.now_ms();
        let change = self.controller.lock().ok().and_then(|mut c| {
            let _ = force;
            c.set_base(decision, t.fixed_profile.is_some(), t.auto, now)
        });
        drop((t, cal));
        self.announce(change);
    }

    /// フロントエンドが報告した実行環境（電源・サーマル・メモリ）を反映する。省略された項目は前回の値を維持
    pub fn set_environment(&self, p: &SetEnvironmentParams) {
        if let Some(mb) = p.memory_budget_mb {
            self.set_memory_budget_mb(Some(u64::from(mb)));
        }
        let change = self.controller.lock().ok().and_then(|mut c| {
            let mut env = c.environment();
            if let Some(v) = p.on_battery {
                env.on_battery = Some(v);
            }
            if let Some(v) = p.low_power_mode {
                env.low_power_mode = Some(v);
            }
            if let Some(v) = p.thermal {
                env.thermal = match v {
                    ThermalState::Nominal => Thermal::Nominal,
                    ThermalState::Fair => Thermal::Fair,
                    ThermalState::Serious => Thermal::Serious,
                    ThermalState::Critical => Thermal::Critical,
                };
            }
            if let Some(v) = p.memory_pressure {
                env.memory_pressure = match v {
                    MemoryPressureState::Normal => MemoryPressure::Normal,
                    MemoryPressureState::Warning => MemoryPressure::Warning,
                    MemoryPressureState::Critical => MemoryPressure::Critical,
                };
            }
            c.set_environment(env, self.now_ms())
        });
        self.announce(change);
        self.evaluate_model(false);
    }

    /// プロセスのメモリ上限（MB）。未報告なら `None`
    pub fn memory_budget_mb(&self) -> Option<u64> {
        match self.memory_budget_mb.load(Ordering::Relaxed) {
            0 => None,
            mb => Some(mb),
        }
    }

    /// メモリ上限（MB）を報告する（`None` または 0 で上限なし）。変わったらプロファイルを判定し直し、
    /// LLM に必要な量（[`MIN_MEMORY_BUDGET_FOR_LLM_MB`]）を下回れば変換モデルを解放する
    pub fn set_memory_budget_mb(&self, mb: Option<u64>) {
        let new = mb.unwrap_or(0);
        if self.memory_budget_mb.swap(new, Ordering::Relaxed) != new {
            self.recompute_profile(true);
            self.evaluate_model(false);
        }
    }

    /// フロントエンドの入力方式を記録する（`initialize` の capabilities）
    pub fn set_input_method(&self, method: InputMethod) {
        if let Ok(mut m) = self.input_method.lock() {
            *m = method;
        }
    }

    /// 報告されたフロントエンドの入力方式（未報告は物理キー）
    pub fn input_method(&self) -> InputMethod {
        self.input_method
            .lock()
            .map_or(InputMethod::Physical, |m| *m)
    }

    // ---- 変換モデルの解放・再読み込み ----

    /// 変換モデルの解放・再読み込みの実行先を登録する（弱い参照。`build` が Pipeline を登録する）
    pub fn set_model_control(&self, control: Weak<dyn ModelControl>) {
        if let Ok(mut c) = self.model_control.lock() {
            *c = Some(control);
        }
    }

    /// 自動の再読み込みまでの待ち時間を変える（テスト用）
    pub fn set_model_reload_delay(&self, delay: Duration) {
        if let Ok(mut d) = self.reload_delay.lock() {
            *d = delay;
        }
    }

    /// モデルの解放・再読み込みの判定と直列化して `f` を実行する（`f` の中から
    /// `unload_model` / `reload_model` / `tick` などの判定を呼ばないこと）
    pub fn serialize_model_changes<R>(&self, f: impl FnOnce() -> R) -> R {
        let _serial = self.model_apply.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }

    /// 変換モデルを解放している（解放を要求した）か
    pub fn model_unloaded(&self) -> bool {
        self.model.lock().is_ok_and(|m| m.unloaded)
    }

    /// 変換モデルだけをメモリから解放して、辞書のみで動く状態にする（`unload_model`）。
    /// 実行中の推論は待たずに捨てる。明示の再読み込み（[`Self::reload_model`]）まで戻らない
    pub fn unload_model(&self) {
        if let Ok(mut m) = self.model.lock() {
            m.manual = true;
        }
        self.evaluate_model(false);
    }

    /// 解放した変換モデルを再び読み込む（`reload_model`）。読み込みは別スレッドで、完了までは辞書のみ。
    /// メモリ逼迫（危険）・メモリ上限不足・LLM を使えないプロファイルでは読み込まない（理由は
    /// `notify`）。モデルを解放していなければ何もしない
    pub fn reload_model(&self) {
        if let Ok(mut m) = self.model.lock() {
            m.manual = false;
            m.gave_up = false;
        }
        self.evaluate_model(true);
    }

    /// 再読み込みに失敗した（辞書のみで継続する。自動では再試行しない）
    pub fn model_reload_failed(&self) {
        if let Ok(mut m) = self.model.lock() {
            m.unloaded = true;
            m.gave_up = true;
            m.resume_at = None;
        }
    }

    /// このホスト・設定で、モデルを読み込めば LLM を使えるか（`light` 以上になるか）
    fn can_run_llm(&self) -> bool {
        let (Ok(t), Ok(cal)) = (self.tunables.read(), self.calibration_ms.lock()) else {
            return false;
        };
        t.llm_enabled
            && Self::decide(&t, &self.system, true, *cal, self.memory_budget_mb()).profile
                >= Profile::Light
    }

    fn control(&self) -> Option<Arc<dyn ModelControl>> {
        self.model_control
            .lock()
            .ok()
            .and_then(|c| c.as_ref().and_then(Weak::upgrade))
    }

    /// 解放すべきか・戻すべきかを判定して実行する。`immediate` は待ち時間を置かずに戻す（明示の要求）。
    ///
    /// 解放する条件: 明示の `unload_model`、メモリ逼迫が `critical`、メモリ上限が LLM の最小値未満。
    /// 戻す条件: どれも解消し、逼迫が `normal` に戻ってから待ち時間（[`MODEL_RELOAD_DELAY`]）が経ち、
    /// `light` 以上で動けること。戻せなければ（失敗も含め）辞書のみで継続する
    fn evaluate_model(&self, immediate: bool) {
        // 状態の更新から実行先への指示までを直列化する（別スレッドの判定が割り込んで、
        // Runtime の状態と実際の重みの状態の順序が入れ替わらないように）
        let _serial = self.model_apply.lock().unwrap_or_else(|e| e.into_inner());
        enum Action {
            Unload,
            Reload,
        }
        let env = self.environment();
        let budget_low = self
            .memory_budget_mb()
            .is_some_and(|mb| mb < MIN_MEMORY_BUDGET_FOR_LLM_MB);
        let delay = self.reload_delay.lock().map_or(MODEL_RELOAD_DELAY, |d| *d);
        let now = self.now_ms();
        let action = {
            let Ok(mut st) = self.model.lock() else {
                return;
            };
            let blocked =
                st.manual || budget_low || env.memory_pressure == MemoryPressure::Critical;
            if blocked {
                st.resume_at = None;
                st.gave_up = false;
                if st.unloaded {
                    None
                } else {
                    st.unloaded = true;
                    Some(Action::Unload)
                }
            } else if !st.unloaded || (st.gave_up && !immediate) {
                None
            } else if env.memory_pressure != MemoryPressure::Normal && !immediate {
                // 逼迫（警告）が続いている間は戻さない
                st.resume_at = None;
                None
            } else {
                let at = *st.resume_at.get_or_insert(now + delay.as_millis() as u64);
                if immediate || now >= at {
                    st.resume_at = None;
                    if self.can_run_llm() {
                        st.unloaded = false;
                        Some(Action::Reload)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };
        match (action, self.control()) {
            (Some(Action::Unload), Some(c)) => c.unload_model(),
            (Some(Action::Reload), Some(c)) => c.reload_model(),
            // 実行先が無い（モデルを持たない構成・テスト）: 状態だけ反映する
            (Some(Action::Unload), None) => self.set_llm_loaded(false),
            (Some(Action::Reload), None) => self.set_llm_loaded(true),
            (None, _) => {}
        }
    }

    /// 現在の実行環境（テスト用）
    pub fn environment(&self) -> Environment {
        self.controller
            .lock()
            .map(|c| c.environment())
            .unwrap_or_default()
    }

    // ---- 設定 ----

    /// 設定を反映する（初回読み込みと再読み込み）
    pub fn apply_config(&self, config: &Config) {
        let tunables = Tunables::from_config(config);
        if let Ok(mut learner) = self.learner.lock() {
            let mut lc = learner.config().clone();
            lc.max_entries = config.learning.max_entries;
            lc.strength = config.learning.strength;
            lc.forget_days = config.learning.forget_days;
            if let Ok(mut n) = self.next_learner.lock() {
                n.set_config(lc.clone());
            }
            learner.set_config(lc);
        }
        if let Ok(mut t) = self.tunables.write() {
            *t = tunables;
        }
        self.refresh_overlay();
        self.recompute_profile(true);
    }

    /// 設定ファイルの場所と、直近の読み込みに失敗したか
    pub fn set_config_state(&self, path: Option<PathBuf>, error: bool) {
        if let Ok(mut p) = self.config_path.lock() {
            *p = path;
        }
        self.config_error.store(error, Ordering::Relaxed);
    }

    /// 診断用のバックエンド名を記録する
    pub fn set_backend_name(&self, name: &str) {
        if let Ok(mut n) = self.backend_name.lock() {
            *n = name.to_string();
        }
    }

    /// 現在の Tunables のコピー（`Server` がデバウンス判定に使う）
    pub fn tunables(&self) -> Tunables {
        self.tunables.read().map(|t| t.clone()).unwrap_or_default()
    }

    /// 現在の有効なパラメータ
    pub fn effective(&self) -> Effective {
        let (params, profile) = self
            .controller
            .lock()
            .map_or((Profile::Minimal.params(), Profile::Minimal), |c| {
                (c.params(), c.current())
            });
        let t = self.tunables.read().map(|t| t.clone()).unwrap_or_default();
        let max_ctx = if t.use_context {
            t.max_context_override.unwrap_or(params.max_context_chars)
        } else {
            0
        };
        let cpu_cores = self.system.cpu_cores.max(1);
        let base_threads = if t.n_threads > 0 {
            t.n_threads
        } else {
            cpu_cores
        };
        let n_threads = if profile == Profile::Light {
            (base_threads / 2).max(1)
        } else {
            base_threads
        };
        let beam_width = t.beam_width_override.unwrap_or(params.beam_width).max(1);
        Effective {
            use_llm: t.llm_enabled && params.use_llm && self.llm_loaded.load(Ordering::Relaxed),
            rescore_top: params.rescore_top,
            max_context_chars: max_ctx,
            persona: (!t.persona.is_empty()).then_some(t.persona),
            sync_budget: t.sync_budget,
            ranking: t.ranking,
            n_threads,
            beam_width,
            emoji: t.emoji_enabled,
            kaomoji: t.kaomoji_enabled,
            calculator: t.calculator_enabled,
            loanword: t.loanword_enabled,
            fuzzy: t.fuzzy,
        }
    }

    // ---- ユーザー辞書 ----

    /// 現在のユーザー辞書のオーバーレイ
    pub fn overlay(&self) -> Arc<Overlay> {
        self.overlay
            .read()
            .map(|o| o.clone())
            .unwrap_or_else(|_| Arc::new(Overlay::default()))
    }

    /// ユーザー辞書を差し替える
    pub fn set_overlay(&self, overlay: Overlay) {
        if let Ok(mut o) = self.overlay_raw.write() {
            *o = Arc::new(overlay);
        }
        self.refresh_overlay();
    }

    /// 読み込んだユーザー辞書から、設定（語・スニペットの使用の有無）に合わせた変換用の
    /// オーバーレイを作り直す
    fn refresh_overlay(&self) {
        let (words, snippets) = self
            .tunables
            .read()
            .map_or((true, true), |t| (t.userdict_enabled, t.snippet_enabled));
        let raw = self
            .overlay_raw
            .read()
            .map(|o| o.clone())
            .unwrap_or_default();
        let effective = if words && snippets {
            raw
        } else {
            Arc::new(raw.filtered(words, snippets))
        };
        if let Ok(mut o) = self.overlay.write() {
            *o = effective;
        }
    }

    // ---- 学習 ----

    fn unix_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// このセッションの状況で学習してよいか（設定・secure input・アプリの停止リスト）
    pub fn learning_allowed(&self, secure_input: bool, app_id: Option<&str>) -> bool {
        if secure_input {
            return false;
        }
        let Ok(t) = self.tunables.read() else {
            return false;
        };
        t.learning_enabled && !app_id.is_some_and(|a| t.disabled_apps.iter().any(|d| d == a))
    }

    /// 確定を学習に記録する（呼び出し側が `learning_allowed` を確認済みであること）
    pub fn learn_record(&self, reading: &str, surface: &str, context_hash: u64) {
        if let Ok(mut l) = self.learner.lock() {
            l.record(reading, surface, context_hash, "conv", Self::unix_now());
        }
    }

    /// 学習済みの候補（スコア降順の (表記, スコア)）。学習が無効なら空
    pub fn learn_lookup(&self, reading: &str, context_hash: u64) -> Vec<(String, f64)> {
        let enabled = self.tunables.read().is_ok_and(|t| t.learning_enabled);
        if !enabled {
            return Vec::new();
        }
        self.learner
            .lock()
            .map(|l| {
                l.lookup_exact(reading, context_hash, Self::unix_now())
                    .into_iter()
                    .map(|s| (s.surface, s.score))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 次語予測のバイグラム（直前の語 → 次の語）を記録する。`predict.next_word` が無効なら何もしない
    /// （呼び出し側が `learning_allowed` を確認済みであること）
    pub fn learn_record_next(&self, prev: &str, next: &str) {
        if !self.tunables.read().is_ok_and(|t| t.next_word_enabled) {
            return;
        }
        if let Ok(mut l) = self.next_learner.lock() {
            l.record(prev, next, Self::unix_now());
        }
    }

    /// 確定直後の次語候補（スコア降順の (表記, スコア)）。次語予測・学習のどちらかが無効なら空
    pub fn next_word_lookup(&self, prev: &str, limit: usize) -> Vec<(String, f64)> {
        let enabled = self
            .tunables
            .read()
            .is_ok_and(|t| t.next_word_enabled && t.learning_enabled);
        if !enabled {
            return Vec::new();
        }
        self.next_learner
            .lock()
            .map(|l| {
                l.lookup(prev, Self::unix_now(), limit)
                    .into_iter()
                    .map(|s| (s.surface, s.score))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 予測変換用: 学習済みの候補（スコア降順の (表記, スコア)）を前方一致で返す。学習が無効なら空
    pub fn learn_predict(
        &self,
        prefix: &str,
        context_hash: u64,
        limit: usize,
    ) -> Vec<(String, f64)> {
        let enabled = self.tunables.read().is_ok_and(|t| t.learning_enabled);
        if !enabled {
            return Vec::new();
        }
        self.learner
            .lock()
            .map(|l| {
                l.lookup_prefix(prefix, context_hash, Self::unix_now(), limit)
                    .into_iter()
                    .map(|s| (s.surface, s.score))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// (読み, 表記) の学習を忘れさせる
    pub fn learn_forget(&self, reading: &str, surface: &str) -> bool {
        let forgot = self
            .learner
            .lock()
            .is_ok_and(|mut l| l.forget(reading, surface));
        // その語を次語とするバイグラムも忘れる
        let forgot_next = self
            .next_learner
            .lock()
            .is_ok_and(|mut n| n.forget(surface));
        if forgot || forgot_next {
            self.flush_learning(true);
        }
        forgot
    }

    /// 学習データを全削除する
    pub fn learn_clear(&self) {
        if let Ok(mut l) = self.learner.lock() {
            l.clear();
        }
        if let Ok(mut n) = self.next_learner.lock() {
            n.clear();
        }
        self.flush_learning(true);
    }

    /// 学習データの件数
    pub fn learned_entries(&self) -> usize {
        self.learner.lock().map_or(0, |l| l.len())
    }

    /// 学習ファイルへ書き出す。`force` でなければ前回から 5 分経っている場合だけ
    pub fn flush_learning(&self, force: bool) {
        let Some(path) = &self.learn_path else {
            return;
        };
        let due = self
            .last_flush
            .lock()
            .is_ok_and(|last| force || last.elapsed() >= FLUSH_INTERVAL);
        if !due {
            return;
        }
        // 他プロセスの flock を待つのは learner の Mutex の外で、待ち時間に上限を付ける
        // （変換経路を止めない）。取れなければ次回（定期の poll か次の明示の flush）に回す
        let timeout = Some(if force {
            FLUSH_LOCK_TIMEOUT_FORCED
        } else {
            FLUSH_LOCK_TIMEOUT
        });
        let result = flush_shared(
            &self.learner,
            path,
            timeout,
            |l| l.is_dirty(),
            |l, lock| l.flush_with_lock(path, lock),
        );
        let next_result = match &self.next_path {
            Some(np) => flush_shared(
                &self.next_learner,
                np,
                timeout,
                |n| n.is_dirty(),
                |n, lock| n.flush_with_lock(np, lock),
            ),
            None => Ok(true),
        };
        if matches!(result, Ok(true) | Err(_))
            && matches!(next_result, Ok(true) | Err(_))
            && let Ok(mut last) = self.last_flush.lock()
        {
            *last = Instant::now();
        }
        let error = match (result, next_result) {
            (Err(e), _) | (_, Err(e)) => Some(e),
            _ => None,
        };
        if let Some(e) = error {
            self.push_event(Event {
                level: NotifyLevel::Warning,
                code: "learning_flush_failed",
                message: format!("学習データを保存できませんでした: {e}"),
            });
        }
    }

    // ---- 状態 ----

    /// `get_status` の結果
    pub fn status(&self) -> StatusResult {
        let overlay = self
            .overlay_raw
            .read()
            .map(|o| o.clone())
            .unwrap_or_default();
        let e = self.effective();
        StatusResult {
            profile: self.profile().as_str().to_string(),
            profile_reason: self.profile_reason(),
            backend: self
                .backend_name
                .lock()
                .map(|n| n.clone())
                .unwrap_or_default(),
            llm: e.use_llm,
            learning: self.tunables.read().is_ok_and(|t| t.learning_enabled),
            learned_entries: self.learned_entries() as u32,
            user_dict_entries: (overlay.word_count() + overlay.snippet_count()) as u32,
            config_path: self
                .config_path
                .lock()
                .ok()
                .and_then(|p| p.as_ref().map(|p| p.display().to_string())),
            config_error: self.config_error.load(Ordering::Relaxed),
        }
    }
}

/// 学習ファイルの flock を `m` の外で取ってから、短い間だけ `m` を握って書き出す。
/// 戻り値は「完了した（書くものが無かった場合を含む）」か。ロックが取れず先送りしたら `Ok(false)`
fn flush_shared<T>(
    m: &Mutex<T>,
    path: &std::path::Path,
    timeout: Option<Duration>,
    is_dirty: impl Fn(&T) -> bool,
    flush: impl Fn(&mut T, &nagi_learn::FlushLock) -> Result<bool, nagi_learn::LearnError>,
) -> Result<bool, nagi_learn::LearnError> {
    if !m.lock().is_ok_and(|g| is_dirty(&g)) {
        return Ok(true);
    }
    let Some(lock) = nagi_learn::FlushLock::acquire(path, timeout)? else {
        return Ok(false);
    };
    match m.lock() {
        Ok(mut g) => flush(&mut g, &lock).map(|_| true),
        Err(_) => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_calibration_saves_to_configured_path() {
        let runtime = Runtime::detached(true);
        let path = std::env::temp_dir().join(format!(
            "nagi-runtime-calibration-test-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        runtime.set_calibration_path(Some(path.clone()));
        runtime.finish_calibration(42.0);
        assert!(path.exists(), "キャリブレーション結果が保存されているはず");
        let record = crate::calibration::load(&path).expect("読み込める");
        assert_eq!(record.median_ms, 42.0);
        let _ = std::fs::remove_file(&path);
    }
}
