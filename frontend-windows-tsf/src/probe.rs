//! `EnvironmentProbe` の Win32 実装（電源・メモリ・プロセッサ周波数の取得だけ。解釈は `nagi_windows::environment`）。

#![allow(unsafe_code)]

use std::ffi::c_void;

use nagi_windows::environment::{
    AcLineStatus, EnvironmentProbe, MemorySample, PowerSample, ProcessorSample,
};
use windows::Win32::System::Power::{
    CallNtPowerInformation, GetSystemPowerStatus, PROCESSOR_POWER_INFORMATION,
    ProcessorInformation, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::SystemInformation::{
    GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};
use windows::Win32::System::SystemServices::{TIME_ZONE_ID_DAYLIGHT, TIME_ZONE_ID_STANDARD};
use windows::Win32::System::Time::{
    DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation, TIME_ZONE_ID_INVALID,
};

/// `SYSTEM_POWER_STATUS.BatteryFlag` の「電池なし」
const BATTERY_FLAG_NO_BATTERY: u8 = 128;
/// `SYSTEM_POWER_STATUS.BatteryFlag` の「不明」
const BATTERY_FLAG_UNKNOWN: u8 = 255;

/// Win32 の問い合わせ
#[derive(Debug, Default, Clone, Copy)]
pub struct Win32Probe;

impl EnvironmentProbe for Win32Probe {
    fn power(&self) -> Option<PowerSample> {
        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
        Some(PowerSample {
            ac_line: AcLineStatus::from_raw(status.ACLineStatus),
            no_battery: status.BatteryFlag != BATTERY_FLAG_UNKNOWN
                && status.BatteryFlag & BATTERY_FLAG_NO_BATTERY != 0,
            // SystemStatusFlag: 1 = バッテリー節約が有効（Windows 10 以降。それ以前は常に 0）
            battery_saver: Some(status.SystemStatusFlag & 1 != 0),
        })
    }

    fn memory(&self) -> Option<MemorySample> {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
        Some(MemorySample {
            total_mb: status.ullTotalPhys / (1024 * 1024),
            available_mb: status.ullAvailPhys / (1024 * 1024),
        })
    }

    fn processor(&self) -> Option<ProcessorSample> {
        let mut info = SYSTEM_INFO::default();
        unsafe { GetSystemInfo(&mut info) };
        let count = info.dwNumberOfProcessors as usize;
        if count == 0 {
            return None;
        }
        let mut buf = vec![PROCESSOR_POWER_INFORMATION::default(); count];
        let bytes = u32::try_from(std::mem::size_of_val(buf.as_slice())).ok()?;
        let status = unsafe {
            CallNtPowerInformation(
                ProcessorInformation,
                None,
                0,
                Some(buf.as_mut_ptr().cast::<c_void>()),
                bytes,
            )
        };
        if status.0 != 0 {
            return None;
        }
        // 「現在の周波数」ではなく、システムが課している上限（MhzLimit）を使う。
        // 現在の周波数はアイドルで下がるのが正常で、サーマル制限とは区別できない
        // （電力・熱の制約はこの上限に反映される）。コア間の平均を取る
        let n = u64::try_from(count).ok()?;
        let limit: u64 = buf.iter().map(|p| u64::from(p.MhzLimit)).sum::<u64>() / n;
        let max: u64 = buf.iter().map(|p| u64::from(p.MaxMhz)).sum::<u64>() / n;
        if max == 0 {
            return None;
        }
        Some(ProcessorSample {
            current_mhz: u32::try_from(limit).ok()?,
            max_mhz: u32::try_from(max).ok()?,
        })
    }

    fn utc_offset_minutes(&self) -> Option<i32> {
        let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
        let id = unsafe { GetDynamicTimeZoneInformation(&mut info) };
        // Bias は「UTC = ローカル + Bias」の分。今が夏時間かどうかで追加の差（DaylightBias）を足す
        let extra = match id {
            TIME_ZONE_ID_INVALID => return None,
            TIME_ZONE_ID_DAYLIGHT => info.DaylightBias,
            TIME_ZONE_ID_STANDARD => info.StandardBias,
            _ => 0,
        };
        Some(-(info.Bias + extra))
    }
}
