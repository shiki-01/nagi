//! Nagi 環境プロファイル: ハードウェア検出・キャリブレーション・実行時の自動切替の判定。
//!
//! 設計書 §4.7。同じ Nagi が、機械とその時の状態に合わせて「精度」と「軽さ」を切り替える。
//!
//! - [`Profile`] — `quality` / `balanced` / `light` / `minimal`
//! - [`initial_profile`] — 初回起動時の判定（メモリ・コア数・キャリブレーション推論の実測）
//! - [`Controller`] — 実行時の自動切替（バッテリー・低電力モード・サーマル・メモリ逼迫・
//!   直近のレイテンシ）。**下げるのは即座に、戻すのはヒステリシス付き**
//!
//! 時刻は呼び出し側が `now_ms`（単調増加のミリ秒）で渡す。テストが決定的になり、OS 依存の
//! 時計を持ち込まない。OS からの電源・サーマル情報はフロントエンドが [`Environment`] として送る
//! （フロントの責務。コアは OS の API を直接呼ばない）。

use std::collections::VecDeque;
use std::time::Duration;

/// 環境プロファイル。大きいほど高精度・高負荷
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Profile {
    /// LLM を使わない（辞書 + Viterbi のみ）。モデル未取得・障害時の最後の砦
    Minimal,
    /// 軽量: 再採点の候補数・文脈を絞る
    Light,
    /// 標準
    Balanced,
    /// 高性能機・電源接続向け: 再採点を広げ、文脈を長く使う
    Quality,
}

impl Profile {
    /// 設定・プロトコルでの名前
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Minimal => "minimal",
            Profile::Light => "light",
            Profile::Balanced => "balanced",
            Profile::Quality => "quality",
        }
    }

    /// 名前から作る
    pub fn from_name(name: &str) -> Option<Profile> {
        match name {
            "minimal" => Some(Profile::Minimal),
            "light" => Some(Profile::Light),
            "balanced" => Some(Profile::Balanced),
            "quality" => Some(Profile::Quality),
            _ => None,
        }
    }

    /// `steps` 段軽くしたプロファイル（`Minimal` で止まる）
    pub fn lighter(self, steps: u32) -> Profile {
        let mut p = self;
        for _ in 0..steps {
            p = match p {
                Profile::Quality => Profile::Balanced,
                Profile::Balanced => Profile::Light,
                Profile::Light | Profile::Minimal => Profile::Minimal,
            };
        }
        p
    }

    /// 1 段重いプロファイル（`limit` を超えない）
    pub fn heavier(self, limit: Profile) -> Profile {
        let next = match self {
            Profile::Minimal => Profile::Light,
            Profile::Light => Profile::Balanced,
            Profile::Balanced | Profile::Quality => Profile::Quality,
        };
        next.min(limit)
    }

    /// このプロファイルが既定で決めるパラメータ（設定で明示された項目が優先される）
    pub fn params(self) -> ProfileParams {
        match self {
            Profile::Quality => ProfileParams {
                use_llm: true,
                rescore_top: 12,
                max_context_chars: 40,
                beam_width: 3,
                composing_chunk_len: 40,
            },
            Profile::Balanced => ProfileParams {
                use_llm: true,
                rescore_top: 8,
                max_context_chars: 20,
                beam_width: 3,
                composing_chunk_len: 30,
            },
            Profile::Light => ProfileParams {
                use_llm: true,
                rescore_top: 3,
                max_context_chars: 10,
                beam_width: 1,
                composing_chunk_len: 20,
            },
            Profile::Minimal => ProfileParams {
                use_llm: false,
                rescore_top: 0,
                max_context_chars: 0,
                beam_width: 1,
                composing_chunk_len: 20,
            },
        }
    }
}

/// プロファイルが決めるパラメータ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileParams {
    /// LLM を使うか（`Minimal` は使わない）
    pub use_llm: bool,
    /// 文節候補のうち LLM で再採点する辞書候補の数
    pub rescore_top: usize,
    /// 左文脈として LLM に渡す最大文字数
    pub max_context_chars: usize,
    /// ビームサーチの幅（ビームサーチ実装後に使う）
    pub beam_width: usize,
    /// ライブ変換の 1 チャンクの文字数（ライブ変換の実装後に使う）
    pub composing_chunk_len: usize,
}

/// ハードウェアの情報
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemInfo {
    /// 搭載メモリ（MB）
    pub total_memory_mb: u64,
    /// 論理 CPU コア数
    pub cpu_cores: usize,
}

/// 実機のハードウェア情報を検出する
pub fn detect() -> SystemInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    SystemInfo {
        total_memory_mb: sys.total_memory() / (1024 * 1024),
        cpu_cores: std::thread::available_parallelism().map_or(1, usize::from),
    }
}

/// 複数回の計測（1 回の推論にかかった時間）の中央値をミリ秒で返す。空なら `None`
pub fn median_ms(samples: &[Duration]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    ms.sort_by(f64::total_cmp);
    let mid = ms.len() / 2;
    Some(if ms.len() % 2 == 1 {
        ms[mid]
    } else {
        (ms[mid - 1] + ms[mid]) / 2.0
    })
}

/// プロファイルの選択結果と、その理由（メニューで「なぜ今この状態か」を表示するため）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// 選ばれたプロファイル
    pub profile: Profile,
    /// 理由（日本語）
    pub reason: String,
}

/// 初回起動時の判定。`calibration_ms` はキャリブレーション推論（短い変換の中央値）の実測、
/// `has_llm` は変換モデルを読み込めたか
pub fn initial_profile(sys: &SystemInfo, calibration_ms: Option<f64>, has_llm: bool) -> Decision {
    let decide = |profile: Profile, reason: String| Decision { profile, reason };
    if !has_llm {
        return decide(
            Profile::Minimal,
            "変換モデルが使えないため辞書のみで動作します".to_string(),
        );
    }
    if sys.total_memory_mb < 4096 || sys.cpu_cores < 2 {
        return decide(
            Profile::Light,
            format!(
                "メモリ {}MB・{} コアのため軽量プロファイルにしました",
                sys.total_memory_mb, sys.cpu_cores
            ),
        );
    }
    // メモリが 8GB 未満なら標準まで上げない
    let cap = if sys.total_memory_mb < 8192 {
        Profile::Light
    } else if sys.total_memory_mb >= 16384 && sys.cpu_cores >= 8 {
        Profile::Quality
    } else {
        Profile::Balanced
    };
    let Some(ms) = calibration_ms else {
        let p = cap.min(Profile::Balanced);
        return decide(
            p,
            format!("実測なしのためハードウェアから {} を選びました", p.as_str()),
        );
    };
    let by_speed = if ms <= 40.0 {
        Profile::Quality
    } else if ms <= 120.0 {
        Profile::Balanced
    } else if ms <= 300.0 {
        Profile::Light
    } else {
        Profile::Minimal
    };
    let p = by_speed.min(cap);
    let why = if p == by_speed {
        format!(
            "キャリブレーション推論 {ms:.0}ms から {} を選びました",
            p.as_str()
        )
    } else {
        format!(
            "キャリブレーション推論は {ms:.0}ms ですが、メモリ {}MB・{} コアのため {} に制限しました",
            sys.total_memory_mb,
            sys.cpu_cores,
            p.as_str()
        )
    };
    decide(p, why)
}

/// サーマル状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Thermal {
    /// 通常
    #[default]
    Nominal,
    /// やや高い
    Fair,
    /// 高い
    Serious,
    /// 危険
    Critical,
}

/// メモリ逼迫の状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryPressure {
    /// 通常
    #[default]
    Normal,
    /// 警告
    Warning,
    /// 危険
    Critical,
}

/// フロントエンドが報告する実行環境の状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Environment {
    /// バッテリー駆動か（不明なら `None`）
    pub on_battery: Option<bool>,
    /// OS の低電力モードか
    pub low_power_mode: Option<bool>,
    /// サーマル状態
    pub thermal: Thermal,
    /// メモリ逼迫
    pub memory_pressure: MemoryPressure,
}

impl Environment {
    /// この環境が要求する「軽くする段数」と理由
    fn penalty(&self) -> (u32, Option<&'static str>) {
        let mut steps = 0;
        let mut why = None;
        let mut bump = |s: u32, reason: &'static str| {
            if s > steps {
                steps = s;
                why = Some(reason);
            }
        };
        if self.on_battery == Some(true) {
            bump(1, "バッテリー駆動");
        }
        if self.low_power_mode == Some(true) {
            bump(1, "低電力モード");
        }
        match self.thermal {
            Thermal::Nominal => {}
            Thermal::Fair => {}
            Thermal::Serious => bump(1, "サーマル状態の悪化"),
            Thermal::Critical => bump(2, "サーマル状態が危険"),
        }
        match self.memory_pressure {
            MemoryPressure::Normal => {}
            MemoryPressure::Warning => bump(1, "メモリ逼迫"),
            MemoryPressure::Critical => bump(3, "メモリ逼迫（危険）"),
        }
        (steps, why)
    }
}

/// プロファイルの変化
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// 変化前
    pub from: Profile,
    /// 変化後
    pub to: Profile,
    /// 理由（日本語）
    pub reason: String,
}

/// 自動切替の設定
#[derive(Debug, Clone, Copy)]
pub struct AutoConfig {
    /// 自動切替を行うか（`profile.auto_switch`）
    pub enabled: bool,
    /// レイテンシの目標（`conversion.max_latency_ms`）。直近の p95 がこれを超えたら 1 段軽くする
    pub max_latency_ms: f64,
    /// 軽くしたあと、上げ直すまでの最短時間（ミリ秒）
    pub upgrade_dwell_ms: u64,
    /// レイテンシの評価に使う直近のサンプル数
    pub window: usize,
}

impl Default for AutoConfig {
    fn default() -> Self {
        AutoConfig {
            enabled: true,
            max_latency_ms: 100.0,
            upgrade_dwell_ms: 60_000,
            window: 20,
        }
    }
}

/// 実行時のプロファイル制御
///
/// `base` は初回判定（または設定で固定された値）。自動切替が有効なら、環境とレイテンシに応じて
/// `base` より軽い値へ下げる。条件が解消されたら、ヒステリシス（`upgrade_dwell_ms`）を置いて戻す
#[derive(Debug)]
pub struct Controller {
    base: Profile,
    base_reason: String,
    fixed: bool,
    config: AutoConfig,
    env: Environment,
    /// レイテンシ由来の追加の軽量化段数
    latency_steps: u32,
    samples: VecDeque<f64>,
    current: Profile,
    reason: String,
    last_change_ms: u64,
    last_latency_change_ms: u64,
}

impl Controller {
    /// `base` から始める。`fixed` = 設定で `profile.mode` を明示（自動切替もしない）
    pub fn new(decision: Decision, fixed: bool, config: AutoConfig, now_ms: u64) -> Self {
        Controller {
            base: decision.profile,
            base_reason: decision.reason.clone(),
            fixed,
            config,
            env: Environment::default(),
            latency_steps: 0,
            samples: VecDeque::new(),
            current: decision.profile,
            reason: decision.reason,
            last_change_ms: now_ms,
            last_latency_change_ms: now_ms,
        }
    }

    /// 現在のプロファイル
    pub fn current(&self) -> Profile {
        self.current
    }

    /// 現在のプロファイルになった理由
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// 直近に報告された実行環境
    pub fn environment(&self) -> Environment {
        self.env
    }

    /// 現在のパラメータ
    pub fn params(&self) -> ProfileParams {
        self.current.params()
    }

    /// 基準（初回判定・固定値）を差し替える（設定の再読み込み時）
    pub fn set_base(
        &mut self,
        decision: Decision,
        fixed: bool,
        config: AutoConfig,
        now_ms: u64,
    ) -> Option<Change> {
        self.base = decision.profile;
        self.base_reason = decision.reason;
        self.fixed = fixed;
        self.config = config;
        self.latency_steps = 0;
        self.samples.clear();
        self.recompute(now_ms, true)
    }

    /// 環境の変化を報告する。プロファイルが変わればその内容を返す
    pub fn set_environment(&mut self, env: Environment, now_ms: u64) -> Option<Change> {
        self.env = env;
        self.recompute(now_ms, false)
    }

    /// LLM 変換 1 回のレイテンシ（ミリ秒）を報告する。目標を超え続けていれば 1 段軽くする
    pub fn observe_latency(&mut self, ms: f64, now_ms: u64) -> Option<Change> {
        if self.fixed || !self.config.enabled || self.current == Profile::Minimal {
            return None;
        }
        self.samples.push_back(ms);
        while self.samples.len() > self.config.window {
            self.samples.pop_front();
        }
        if self.samples.len() < self.config.window {
            return None;
        }
        let p95 = percentile(&self.samples, 0.95);
        if p95 > self.config.max_latency_ms {
            self.latency_steps += 1;
            self.last_latency_change_ms = now_ms;
            self.samples.clear();
            return self.recompute(now_ms, false);
        }
        // 十分に速い状態が続いていれば、軽くした分を 1 段戻す
        if self.latency_steps > 0
            && p95 < self.config.max_latency_ms * 0.5
            && now_ms.saturating_sub(self.last_latency_change_ms) >= self.config.upgrade_dwell_ms
        {
            self.latency_steps -= 1;
            self.last_latency_change_ms = now_ms;
            self.samples.clear();
            return self.recompute(now_ms, false);
        }
        None
    }

    fn recompute(&mut self, now_ms: u64, force: bool) -> Option<Change> {
        let (target, reason) = if self.fixed || !self.config.enabled {
            (self.base, self.base_reason.clone())
        } else {
            let (env_steps, env_reason) = self.env.penalty();
            let steps = env_steps + self.latency_steps;
            let target = self.base.lighter(steps);
            let reason = if steps == 0 {
                self.base_reason.clone()
            } else {
                let mut parts = Vec::new();
                if let Some(r) = env_reason {
                    parts.push(r.to_string());
                }
                if self.latency_steps > 0 {
                    parts.push(format!(
                        "推論レイテンシが目標（{:.0}ms）を超過",
                        self.config.max_latency_ms
                    ));
                }
                format!(
                    "{}のため {} に切り替えました",
                    parts.join("・"),
                    target.as_str()
                )
            };
            (target, reason)
        };
        if target == self.current {
            self.reason = reason;
            return None;
        }
        // 下げるのは即座に、上げるのはヒステリシス（最短保持時間）を置く。設定の再読み込みは即時
        if target > self.current
            && !force
            && now_ms.saturating_sub(self.last_change_ms) < self.config.upgrade_dwell_ms
        {
            return None;
        }
        let change = Change {
            from: self.current,
            to: target,
            reason: reason.clone(),
        };
        self.current = target;
        self.reason = reason;
        self.last_change_ms = now_ms;
        Some(change)
    }

    /// 時間経過だけで戻せるか確認する（環境の変化が無くても、ヒステリシス経過後に呼ぶ）
    pub fn tick(&mut self, now_ms: u64) -> Option<Change> {
        self.recompute(now_ms, false)
    }
}

fn percentile(samples: &VecDeque<f64>, p: f64) -> f64 {
    let mut v: Vec<f64> = samples.iter().copied().collect();
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(mem_mb: u64, cores: usize) -> SystemInfo {
        SystemInfo {
            total_memory_mb: mem_mb,
            cpu_cores: cores,
        }
    }

    #[test]
    fn profile_ordering_and_steps() {
        assert!(Profile::Quality > Profile::Balanced && Profile::Light > Profile::Minimal);
        assert_eq!(Profile::Quality.lighter(1), Profile::Balanced);
        assert_eq!(Profile::Quality.lighter(2), Profile::Light);
        assert_eq!(Profile::Quality.lighter(9), Profile::Minimal);
        assert_eq!(Profile::Minimal.lighter(1), Profile::Minimal);
        assert_eq!(Profile::Light.heavier(Profile::Balanced), Profile::Balanced);
        assert_eq!(
            Profile::Balanced.heavier(Profile::Balanced),
            Profile::Balanced
        );
        assert_eq!(Profile::from_name("light"), Some(Profile::Light));
        assert_eq!(Profile::from_name("x"), None);
        for p in [
            Profile::Minimal,
            Profile::Light,
            Profile::Balanced,
            Profile::Quality,
        ] {
            assert_eq!(Profile::from_name(p.as_str()), Some(p));
        }
    }

    #[test]
    fn params_get_lighter_with_profile() {
        assert!(!Profile::Minimal.params().use_llm);
        assert!(Profile::Light.params().use_llm);
        let (q, b, l) = (
            Profile::Quality.params(),
            Profile::Balanced.params(),
            Profile::Light.params(),
        );
        assert!(q.rescore_top > b.rescore_top && b.rescore_top > l.rescore_top);
        assert!(
            q.max_context_chars > b.max_context_chars && b.max_context_chars > l.max_context_chars
        );
    }

    #[test]
    fn median_of_samples() {
        let ms = |v: &[u64]| {
            v.iter()
                .map(|&x| Duration::from_millis(x))
                .collect::<Vec<_>>()
        };
        assert_eq!(median_ms(&[]), None);
        assert_eq!(median_ms(&ms(&[30])), Some(30.0));
        assert_eq!(median_ms(&ms(&[50, 10, 30])), Some(30.0));
        assert_eq!(median_ms(&ms(&[10, 20, 30, 40])), Some(25.0));
    }

    #[test]
    fn detect_returns_plausible_values() {
        let s = detect();
        assert!(s.cpu_cores >= 1);
        assert!(s.total_memory_mb >= 256, "{s:?}");
    }

    #[test]
    fn initial_profile_rules() {
        // モデルが無ければ minimal
        assert_eq!(
            initial_profile(&sys(32768, 16), Some(20.0), false).profile,
            Profile::Minimal
        );
        // 非力な機械は light
        assert_eq!(
            initial_profile(&sys(2048, 4), Some(20.0), true).profile,
            Profile::Light
        );
        assert_eq!(
            initial_profile(&sys(16384, 1), Some(20.0), true).profile,
            Profile::Light
        );
        // 速い + 大きい機械は quality、8GB 機は速くても balanced 止まり
        assert_eq!(
            initial_profile(&sys(32768, 16), Some(30.0), true).profile,
            Profile::Quality
        );
        assert_eq!(
            initial_profile(&sys(8192, 8), Some(30.0), true).profile,
            Profile::Balanced
        );
        // 遅さで下がる
        assert_eq!(
            initial_profile(&sys(16384, 8), Some(100.0), true).profile,
            Profile::Balanced
        );
        assert_eq!(
            initial_profile(&sys(16384, 8), Some(200.0), true).profile,
            Profile::Light
        );
        assert_eq!(
            initial_profile(&sys(16384, 8), Some(500.0), true).profile,
            Profile::Minimal
        );
        // メモリが 8GB 未満なら速くても light まで
        assert_eq!(
            initial_profile(&sys(6144, 8), Some(20.0), true).profile,
            Profile::Light
        );
        // 実測なしはハードウェアから（quality にはしない）
        assert_eq!(
            initial_profile(&sys(32768, 16), None, true).profile,
            Profile::Balanced
        );
        assert_eq!(
            initial_profile(&sys(6144, 8), None, true).profile,
            Profile::Light
        );
        // 理由が付く
        let d = initial_profile(&sys(8192, 8), Some(30.0), true);
        assert!(
            d.reason.contains("30ms") && d.reason.contains("制限"),
            "{}",
            d.reason
        );
    }

    fn controller(base: Profile) -> Controller {
        Controller::new(
            Decision {
                profile: base,
                reason: "初回判定".into(),
            },
            false,
            AutoConfig::default(),
            0,
        )
    }

    #[test]
    fn battery_lowers_one_step_and_restores_after_dwell() {
        let mut c = controller(Profile::Balanced);
        let env = Environment {
            on_battery: Some(true),
            ..Default::default()
        };
        let change = c.set_environment(env, 1_000).expect("下がる");
        assert_eq!(
            (change.from, change.to),
            (Profile::Balanced, Profile::Light)
        );
        assert!(change.reason.contains("バッテリー"), "{}", change.reason);
        assert!(c.reason().contains("light"));
        // 電源接続に戻っても、最短保持時間が経つまでは戻さない
        assert_eq!(c.set_environment(Environment::default(), 10_000), None);
        assert_eq!(c.current(), Profile::Light);
        // 経過後は tick で戻る
        let back = c.tick(1_000 + 60_000).expect("戻る");
        assert_eq!(back.to, Profile::Balanced);
        assert_eq!(c.reason(), "初回判定");
    }

    #[test]
    fn environment_penalties_combine_by_worst() {
        let mut c = controller(Profile::Quality);
        let ch = c
            .set_environment(
                Environment {
                    on_battery: Some(true),
                    thermal: Thermal::Critical,
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        assert_eq!(ch.to, Profile::Light, "危険なサーマルは 2 段");
        let ch = c
            .set_environment(
                Environment {
                    memory_pressure: MemoryPressure::Critical,
                    ..Default::default()
                },
                1,
            )
            .unwrap();
        assert_eq!(ch.to, Profile::Minimal);
        assert!(ch.reason.contains("メモリ"));
    }

    #[test]
    fn slow_inference_lowers_profile_and_fast_restores_it() {
        let mut c = controller(Profile::Balanced);
        let mut now = 0u64;
        let mut changed = None;
        for _ in 0..20 {
            now += 100;
            if let Some(ch) = c.observe_latency(180.0, now) {
                changed = Some(ch);
            }
        }
        let ch = changed.expect("目標 100ms を超え続けたので下がる");
        assert_eq!(ch.to, Profile::Light);
        assert!(ch.reason.contains("レイテンシ"));
        // 速くなっても、最短保持時間（60 秒）までは戻らない
        for _ in 0..20 {
            now += 100;
            assert_eq!(c.observe_latency(20.0, now), None);
        }
        assert_eq!(c.current(), Profile::Light);
        // 保持時間が過ぎて速い状態が続けば戻る
        now += 60_000;
        let mut back = None;
        for _ in 0..20 {
            now += 100;
            if let Some(ch) = c.observe_latency(20.0, now) {
                back = Some(ch);
            }
        }
        assert_eq!(back.expect("戻る").to, Profile::Balanced);
    }

    #[test]
    fn isolated_slow_samples_do_not_switch() {
        let mut c = controller(Profile::Balanced);
        for i in 0..40u64 {
            let ms = if i % 20 == 0 { 300.0 } else { 40.0 };
            assert_eq!(
                c.observe_latency(ms, i * 100),
                None,
                "p95 が目標内なら変えない"
            );
        }
    }

    #[test]
    fn fixed_or_disabled_never_changes() {
        let mut fixed = Controller::new(
            Decision {
                profile: Profile::Quality,
                reason: "設定で固定".into(),
            },
            true,
            AutoConfig::default(),
            0,
        );
        let env = Environment {
            on_battery: Some(true),
            memory_pressure: MemoryPressure::Critical,
            ..Default::default()
        };
        assert_eq!(fixed.set_environment(env, 0), None);
        for i in 0..50 {
            assert_eq!(fixed.observe_latency(999.0, i), None);
        }
        assert_eq!(fixed.current(), Profile::Quality);

        let mut off = Controller::new(
            Decision {
                profile: Profile::Balanced,
                reason: "初回判定".into(),
            },
            false,
            AutoConfig {
                enabled: false,
                ..AutoConfig::default()
            },
            0,
        );
        assert_eq!(off.set_environment(env, 0), None);
        assert_eq!(off.current(), Profile::Balanced);
    }

    #[test]
    fn set_base_applies_immediately() {
        let mut c = controller(Profile::Balanced);
        let ch = c
            .set_base(
                Decision {
                    profile: Profile::Quality,
                    reason: "設定".into(),
                },
                true,
                AutoConfig::default(),
                5,
            )
            .expect("再読み込みでは保持時間を待たず上げる");
        assert_eq!(ch.to, Profile::Quality);
    }

    #[test]
    fn minimal_ignores_latency() {
        let mut c = controller(Profile::Minimal);
        for i in 0..40 {
            assert_eq!(c.observe_latency(500.0, i), None);
        }
    }
}
