//! サーバー全体で共有する実行時の状態: 環境プロファイル・学習・ユーザー辞書・ランキング設定・通知。
//!
//! 変換パイプライン（[`crate::pipeline::Pipeline`]）、確定の通知先（[`crate::learning`]）、
//! JSON-RPC サーバー（[`crate::server::Server`]）が同じ [`Runtime`] を `Arc` で共有する。
//! 設定の再読み込み・`set_environment`・レイテンシ計測は、すべてここを通して反映される。

use nagi_config::{Config, ProfileMode};
use nagi_dict::Overlay;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::{
    AutoConfig, Change, Controller, Decision, Environment, MemoryPressure, Profile, SystemInfo,
    Thermal, initial_profile,
};
use nagi_protocol::methods::{
    MemoryPressureState, NotifyLevel, SetEnvironmentParams, StatusResult, ThermalState,
};
use nagi_protocol::view::CandidateSource;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// 学習ファイルを書き出す最短の間隔（設計書 §4.11: 5 分ごと）
const FLUSH_INTERVAL: Duration = Duration::from_secs(300);

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
    /// `conversion.use_context`
    pub use_context: bool,
    /// `profile.persona`（空なら注入しない）
    pub persona: String,
    /// `learning.enabled`
    pub learning_enabled: bool,
    /// 学習しないアプリの識別子
    pub disabled_apps: Vec<String>,
    /// 候補の出所の優先順（`ranking.source_order`）
    pub ranking: Vec<CandidateSource>,
    /// `conversion.n_threads`（0 = 自動）
    pub n_threads: usize,
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
            use_context: config.conversion.use_context,
            persona: config.profile.persona.clone(),
            learning_enabled: config.learning.enabled,
            disabled_apps: config.learning.disabled_apps.clone(),
            ranking: config
                .ranking
                .source_order
                .iter()
                .filter_map(|n| parse_source(n))
                .collect(),
            n_threads: config.conversion.n_threads,
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
    last_flush: Mutex<Instant>,
    overlay: RwLock<Arc<Overlay>>,
    events: Mutex<Vec<Event>>,
    /// イベントが積まれたときに呼ぶフック（`Server::run` が非同期通知の配信に使う）。
    /// `Box<dyn Fn() + Send + Sync>` なので中身は任意（チャンネルへの送信等）
    event_hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    config_path: Mutex<Option<PathBuf>>,
    config_error: AtomicBool,
    backend_name: Mutex<String>,
    calibration_path: Mutex<Option<PathBuf>>,
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
        let decision = Self::decide(&tunables, &system, has_llm, None);
        let controller =
            Controller::new(decision, tunables.fixed_profile.is_some(), tunables.auto, 0);
        Arc::new(Runtime {
            started: Instant::now(),
            system,
            controller: Mutex::new(controller),
            llm_loaded: AtomicBool::new(has_llm),
            calibration_ms: Mutex::new(None),
            tunables: RwLock::new(tunables),
            learner: Mutex::new(learner),
            learn_path,
            last_flush: Mutex::new(Instant::now()),
            overlay: RwLock::new(Arc::new(Overlay::default())),
            events: Mutex::new(Vec::new()),
            event_hook: Mutex::new(None),
            config_path: Mutex::new(None),
            config_error: AtomicBool::new(false),
            backend_name: Mutex::new(String::new()),
            calibration_path: Mutex::new(None),
        })
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
    ) -> Decision {
        match t.fixed_profile {
            Some(p) if has_llm || p == Profile::Minimal => Decision {
                profile: p,
                reason: format!("設定（profile.mode = {}）で固定されています", p.as_str()),
            },
            Some(_) => initial_profile(system, None, false),
            None => initial_profile(system, calibration, has_llm),
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

    /// 通知を積む。フックが設定されていれば呼ぶ（`Server::run` を起こして即座に配信させる）
    pub fn push_event(&self, event: Event) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
        if let Ok(hook) = self.event_hook.lock()
            && let Some(f) = hook.as_ref()
        {
            f();
        }
    }

    /// 積まれた通知を取り出す
    pub fn drain_events(&self) -> Vec<Event> {
        self.events
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
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
        let decision = Self::decide(&t, &self.system, has_llm, *cal);
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
            learner.set_config(lc);
        }
        if let Ok(mut t) = self.tunables.write() {
            *t = tunables;
        }
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
        Effective {
            use_llm: params.use_llm && self.llm_loaded.load(Ordering::Relaxed),
            rescore_top: params.rescore_top,
            max_context_chars: max_ctx,
            persona: (!t.persona.is_empty()).then_some(t.persona),
            sync_budget: t.sync_budget,
            ranking: t.ranking,
            n_threads,
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
        if let Ok(mut o) = self.overlay.write() {
            *o = Arc::new(overlay);
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
        if forgot {
            self.flush_learning(true);
        }
        forgot
    }

    /// 学習データを全削除する
    pub fn learn_clear(&self) {
        if let Ok(mut l) = self.learner.lock() {
            l.clear();
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
        let Ok(mut last) = self.last_flush.lock() else {
            return;
        };
        if !force && last.elapsed() < FLUSH_INTERVAL {
            return;
        }
        let result = self.learner.lock().map(|mut l| l.flush(path));
        *last = Instant::now();
        if let Ok(Err(e)) = result {
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
        let overlay = self.overlay();
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
