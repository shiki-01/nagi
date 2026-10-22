//! 実行環境（電源・メモリ・負荷）の報告: OS の値 → `set_environment` の params（OS 非依存）。
//!
//! macOS 版 `EnvironmentReporter.swift` に相当する。OS への問い合わせは [`EnvironmentProbe`] で注入し
//! （Windows の実装は `GetSystemPowerStatus` / `GlobalMemoryStatusEx` / `CallNtPowerInformation` を
//! 呼ぶ次の層が担う）、ここは値の解釈と「変化したときだけ送る」判定だけを持つ。
//! プロファイルの切り替えを決めるのはコア（設計書 §4.7）。省略した項目は「前回の値を維持」になる。

use nagi_protocol::{MemoryPressureState, SetEnvironmentParams, ThermalState};

/// `SYSTEM_POWER_STATUS.ACLineStatus`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcLineStatus {
    /// 電池駆動（0）
    Offline,
    /// AC 接続（1）
    Online,
    /// 不明（255）
    Unknown,
}

impl AcLineStatus {
    /// `ACLineStatus` の生の値から変換する
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Offline,
            1 => Self::Online,
            _ => Self::Unknown,
        }
    }
}

/// 電源状態（`GetSystemPowerStatus`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerSample {
    /// AC 接続の状態
    pub ac_line: AcLineStatus,
    /// 電池が無いと分かっているか（`BatteryFlag` の 128）。デスクトップ PC は AC 扱いにする
    pub no_battery: bool,
    /// バッテリー節約機能（`SystemStatusFlag` が 1）が有効か。`None` は不明
    pub battery_saver: Option<bool>,
}

/// 物理メモリ（`GlobalMemoryStatusEx`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySample {
    /// 物理メモリの総量（MB）
    pub total_mb: u64,
    /// 利用可能な物理メモリ（MB。スタンバイを含む `ullAvailPhys`）
    pub available_mb: u64,
}

/// プロセッサの周波数（`CallNtPowerInformation(ProcessorInformation)`）。サーマル制限の推定に使う
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorSample {
    /// 現在の周波数（MHz）
    pub current_mhz: u32,
    /// 上限の周波数（MHz）
    pub max_mhz: u32,
}

/// OS への問い合わせ。取得できない項目は `None`（その項目は報告せず、コアは前回の値を維持する）
pub trait EnvironmentProbe {
    /// 電源状態
    fn power(&self) -> Option<PowerSample>;
    /// 物理メモリ
    fn memory(&self) -> Option<MemorySample>;
    /// プロセッサの周波数
    fn processor(&self) -> Option<ProcessorSample>;
}

/// 電池駆動か。不明なら `None`
pub fn on_battery(power: &PowerSample) -> Option<bool> {
    if power.no_battery {
        return Some(false);
    }
    match power.ac_line {
        AcLineStatus::Offline => Some(true),
        AcLineStatus::Online => Some(false),
        AcLineStatus::Unknown => None,
    }
}

/// メモリ逼迫の判定。Windows には OS のイベントが無いので、利用可能量から推定する。
/// 利用可能が 5% 未満または 300MB 未満で `Critical`、10% 未満または 800MB 未満で `Warning`
/// （閾値は実測なし。要調整）
pub fn memory_pressure(mem: &MemorySample) -> MemoryPressureState {
    if mem.total_mb == 0 {
        return MemoryPressureState::Normal;
    }
    // available * 100 / total の整数比較（小数を避ける）
    let pct_x100 = mem.available_mb.saturating_mul(100);
    if pct_x100 < mem.total_mb * 5 || mem.available_mb < 300 {
        MemoryPressureState::Critical
    } else if pct_x100 < mem.total_mb * 10 || mem.available_mb < 800 {
        MemoryPressureState::Warning
    } else {
        MemoryPressureState::Normal
    }
}

/// サーマル状態の推定。Windows にはサーマル状態の公開 API が無いため、現在の周波数 / 上限の比で見る
/// （85% 以上 `Nominal`、60% 以上 `Fair`、40% 以上 `Serious`、未満 `Critical`。閾値は実測なし）。
/// 周波数はバッテリー節約機能でも下がるので、節約が有効または不明なときは推定せず `None`
/// （誤って軽いプロファイルへ落とさないため）
pub fn thermal(cpu: &ProcessorSample, battery_saver: Option<bool>) -> Option<ThermalState> {
    if battery_saver != Some(false) || cpu.max_mhz == 0 {
        return None;
    }
    let ratio_x100 = u64::from(cpu.current_mhz) * 100 / u64::from(cpu.max_mhz);
    Some(match ratio_x100 {
        85.. => ThermalState::Nominal,
        60..=84 => ThermalState::Fair,
        40..=59 => ThermalState::Serious,
        _ => ThermalState::Critical,
    })
}

/// 現在の環境を `set_environment` の params にする。`memory_budget_mb` は Windows では報告しない
/// （macOS と同じ。上限を課されるのは iOS 拡張のようなプロセス）
pub fn snapshot(probe: &dyn EnvironmentProbe) -> SetEnvironmentParams {
    let power = probe.power();
    let battery_saver = power.and_then(|p| p.battery_saver);
    SetEnvironmentParams {
        on_battery: power.as_ref().and_then(on_battery),
        low_power_mode: battery_saver,
        thermal: probe
            .processor()
            .and_then(|cpu| thermal(&cpu, battery_saver)),
        memory_pressure: probe.memory().as_ref().map(memory_pressure),
        memory_budget_mb: None,
    }
}

/// 環境の変化を検出して、送るべき params を返す。
/// 呼び出し側が電源変更通知（`WM_POWERBROADCAST`）や一定間隔のタイマーで [`poll`](Self::poll) を呼び、
/// `Some` が返ったら `EngineSupervisor::set_environment` で送る。
/// エンジンの起動・再起動のたびに [`reset`](Self::reset) を呼ぶ（新しいエンジンは前回の値を知らない）
pub struct EnvironmentReporter<P: EnvironmentProbe> {
    probe: P,
    last_sent: Option<SetEnvironmentParams>,
}

impl<P: EnvironmentProbe> EnvironmentReporter<P> {
    /// 新しい報告器
    pub fn new(probe: P) -> Self {
        Self {
            probe,
            last_sent: None,
        }
    }

    /// 前回から変化していれば新しい値を返す（返した値を「送信済み」として記録する）。
    /// 何も取得できなかった（全項目 `None`）ときは送らない
    pub fn poll(&mut self) -> Option<SetEnvironmentParams> {
        let current = snapshot(&self.probe);
        if current == SetEnvironmentParams::default() || self.last_sent.as_ref() == Some(&current) {
            return None;
        }
        self.last_sent = Some(current.clone());
        Some(current)
    }

    /// 送信済みの記録を捨てる。次の [`poll`](Self::poll) は現在値を必ず返す
    pub fn reset(&mut self) {
        self.last_sent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const AC: PowerSample = PowerSample {
        ac_line: AcLineStatus::Online,
        no_battery: false,
        battery_saver: Some(false),
    };

    #[test]
    fn ac_line_raw_values() {
        assert_eq!(AcLineStatus::from_raw(0), AcLineStatus::Offline);
        assert_eq!(AcLineStatus::from_raw(1), AcLineStatus::Online);
        assert_eq!(AcLineStatus::from_raw(255), AcLineStatus::Unknown);
    }

    #[test]
    fn battery_detection() {
        assert_eq!(on_battery(&AC), Some(false));
        let bat = PowerSample {
            ac_line: AcLineStatus::Offline,
            ..AC
        };
        assert_eq!(on_battery(&bat), Some(true));
        let unknown = PowerSample {
            ac_line: AcLineStatus::Unknown,
            ..AC
        };
        assert_eq!(on_battery(&unknown), None);
        // 電池が無い PC は AC 扱い（ACLineStatus が不明でも）
        let desktop = PowerSample {
            no_battery: true,
            ..unknown
        };
        assert_eq!(on_battery(&desktop), Some(false));
    }

    #[test]
    fn memory_pressure_thresholds() {
        let m = |total, available| MemorySample {
            total_mb: total,
            available_mb: available,
        };
        assert_eq!(
            memory_pressure(&m(16000, 8000)),
            MemoryPressureState::Normal
        );
        // 割合で Warning（16GB の 9%）
        assert_eq!(
            memory_pressure(&m(16000, 1440)),
            MemoryPressureState::Warning
        );
        // 割合で Critical（16GB の 4%）
        assert_eq!(
            memory_pressure(&m(16000, 640)),
            MemoryPressureState::Critical
        );
        // 絶対量: 小さな総量でも 300MB 未満は Critical、800MB 未満は Warning
        assert_eq!(
            memory_pressure(&m(2000, 299)),
            MemoryPressureState::Critical
        );
        assert_eq!(memory_pressure(&m(4000, 700)), MemoryPressureState::Warning);
        assert_eq!(memory_pressure(&m(0, 0)), MemoryPressureState::Normal);
    }

    #[test]
    fn thermal_from_frequency_ratio() {
        let cpu = |cur| ProcessorSample {
            current_mhz: cur,
            max_mhz: 3000,
        };
        let t = |cur| thermal(&cpu(cur), Some(false));
        assert_eq!(t(3000), Some(ThermalState::Nominal));
        assert_eq!(t(2550), Some(ThermalState::Nominal));
        assert_eq!(t(2549), Some(ThermalState::Fair));
        assert_eq!(t(1800), Some(ThermalState::Fair));
        assert_eq!(t(1700), Some(ThermalState::Serious));
        assert_eq!(t(1000), Some(ThermalState::Critical));
    }

    #[test]
    fn thermal_not_estimated_under_battery_saver_or_unknown() {
        let cpu = ProcessorSample {
            current_mhz: 800,
            max_mhz: 3000,
        };
        assert_eq!(thermal(&cpu, Some(true)), None);
        assert_eq!(thermal(&cpu, None), None);
        let zero = ProcessorSample {
            current_mhz: 1,
            max_mhz: 0,
        };
        assert_eq!(thermal(&zero, Some(false)), None);
    }

    #[derive(Default)]
    struct FakeProbe {
        power: Cell<Option<PowerSample>>,
        memory: Cell<Option<MemorySample>>,
        cpu: Cell<Option<ProcessorSample>>,
    }

    impl EnvironmentProbe for FakeProbe {
        fn power(&self) -> Option<PowerSample> {
            self.power.get()
        }
        fn memory(&self) -> Option<MemorySample> {
            self.memory.get()
        }
        fn processor(&self) -> Option<ProcessorSample> {
            self.cpu.get()
        }
    }

    #[test]
    fn snapshot_maps_all_fields() {
        let probe = FakeProbe::default();
        probe.power.set(Some(PowerSample {
            ac_line: AcLineStatus::Offline,
            no_battery: false,
            battery_saver: Some(false),
        }));
        probe.memory.set(Some(MemorySample {
            total_mb: 8000,
            available_mb: 100,
        }));
        probe.cpu.set(Some(ProcessorSample {
            current_mhz: 1000,
            max_mhz: 3000,
        }));
        assert_eq!(
            snapshot(&probe),
            SetEnvironmentParams {
                on_battery: Some(true),
                low_power_mode: Some(false),
                thermal: Some(ThermalState::Critical),
                memory_pressure: Some(MemoryPressureState::Critical),
                memory_budget_mb: None,
            }
        );
    }

    #[test]
    fn snapshot_omits_unavailable_items() {
        let probe = FakeProbe::default();
        probe.memory.set(Some(MemorySample {
            total_mb: 8000,
            available_mb: 4000,
        }));
        let s = snapshot(&probe);
        assert_eq!(s.on_battery, None);
        assert_eq!(s.low_power_mode, None);
        assert_eq!(s.thermal, None);
        assert_eq!(s.memory_pressure, Some(MemoryPressureState::Normal));
    }

    #[test]
    fn reporter_sends_only_on_change_and_after_reset() {
        let probe = FakeProbe::default();
        probe.power.set(Some(AC));
        let mut r = EnvironmentReporter::new(probe);
        assert!(r.poll().is_some());
        assert!(r.poll().is_none());
        r.probe.power.set(Some(PowerSample {
            ac_line: AcLineStatus::Offline,
            ..AC
        }));
        assert_eq!(r.poll().unwrap().on_battery, Some(true));
        assert!(r.poll().is_none());
        // エンジン再起動後は同じ値でも送り直す
        r.reset();
        assert!(r.poll().is_some());
    }

    #[test]
    fn reporter_skips_when_nothing_available() {
        let mut r = EnvironmentReporter::new(FakeProbe::default());
        assert!(r.poll().is_none());
    }
}
