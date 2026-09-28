//! キャリブレーション推論の実測結果をファイルに永続化する（G-12）。
//! ハードウェア・バックエンドが変われば使わない。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 保存するキャリブレーション結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationRecord {
    /// キャリブレーション推論（短い変換3回）の中央値（ミリ秒）
    pub median_ms: f64,
    /// 記録時の論理 CPU コア数
    pub cpu_cores: usize,
    /// 記録時の搭載メモリ（MB）
    pub total_memory_mb: u64,
    /// 記録時のバックエンド名（"dict+llm" など）
    pub backend: String,
}

impl CalibrationRecord {
    /// 現在のハードウェア・バックエンドと一致するか（違えば使わない）
    pub fn matches(&self, system: &nagi_profile::SystemInfo, backend: &str) -> bool {
        self.cpu_cores == system.cpu_cores
            && self.total_memory_mb == system.total_memory_mb
            && self.backend == backend
    }
}

/// ファイルから読む。無い・壊れていれば `None`
pub fn load(path: &Path) -> Option<CalibrationRecord> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// ファイルに書く（一時ファイル経由で原子的に）
pub fn save(path: &Path, record: &CalibrationRecord) -> std::io::Result<()> {
    let json =
        serde_json::to_string_pretty(record).map_err(|e| std::io::Error::other(e.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp = path.to_path_buf();
    temp.set_extension("tmp");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_profile::SystemInfo;

    fn record() -> CalibrationRecord {
        CalibrationRecord {
            median_ms: 65.0,
            cpu_cores: 8,
            total_memory_mb: 16384,
            backend: "dict+llm".to_string(),
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "nagi-calibration-test-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let path = temp_path("roundtrip");
        let record = record();
        save(&path, &record).expect("保存できる");
        let loaded = load(&path).expect("読み込める");
        assert_eq!(loaded.median_ms, record.median_ms);
        assert_eq!(loaded.cpu_cores, record.cpu_cores);
        assert_eq!(loaded.total_memory_mb, record.total_memory_mb);
        assert_eq!(loaded.backend, record.backend);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_creates_missing_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "nagi-calibration-test-missing-parent-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("calibration.json");
        save(&path, &record()).expect("親ディレクトリが無くても保存できる");
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matches_is_false_when_hardware_differs() {
        let record = record();
        let same = SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        };
        let different_cores = SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 4,
            has_gpu: false,
        };
        assert!(record.matches(&same, "dict+llm"));
        assert!(!record.matches(&different_cores, "dict+llm"));
        assert!(!record.matches(&same, "dict"));
    }

    #[test]
    fn load_returns_none_when_file_is_missing() {
        let path = temp_path("missing");
        assert!(load(&path).is_none());
    }
}
