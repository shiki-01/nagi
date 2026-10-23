//! imserver の stderr の保存先（`%LOCALAPPDATA%\Nagi\logs\imserver-<ホストpid>.log`）と世代の掃除。
//!
//! imserver の stderr は継承しない（GUI ホストの stderr は壊れていることがある）。代わりにホスト
//! プロセスごとのファイルへ向ける。ここはファイル名・掃除対象の選定・ヘッダ行だけの純ロジックで、
//! ファイル I/O は [`crate::process`] が行う。imserver は入力内容を出力しない（`core/server/src/lib.rs`）。

use std::path::{Path, PathBuf};

use crate::diag::{format_line, format_timestamp};

/// 残す最大ファイル数（現在のホストのファイルを含む）
pub const KEEP_FILES: usize = 20;
/// この期間より古いファイルは消す（秒。7 日）
pub const MAX_AGE_SECS: u64 = 7 * 24 * 3600;
/// 1 ファイルの上限。超えていれば起動時に作り直す（再起動の繰り返しで膨らませない）
pub const MAX_FILE_BYTES: u64 = 512 * 1024;

const PREFIX: &str = "imserver-";
const SUFFIX: &str = ".log";

/// ログの置き場（`<LOCALAPPDATA>\Nagi\logs`。`tsf.log` と同じ場所）
pub fn log_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Nagi").join("logs")
}

/// ホストプロセスのログファイル名（`imserver-<pid>.log`）
pub fn file_name(host_pid: u32) -> String {
    format!("{PREFIX}{host_pid}{SUFFIX}")
}

/// `imserver-<数字>.log` の形か（掃除の対象を自分のファイルだけに限る）
pub fn is_log_file_name(name: &str) -> bool {
    name.strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(SUFFIX))
        .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
}

/// 掃除の判断に使うディレクトリ内の 1 ファイル
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// ファイル名
    pub name: String,
    /// 最終更新（UNIX 秒）
    pub modified_unix_secs: u64,
}

/// 消すファイル名を選ぶ。`current`（このホストのファイル）は消さない。
/// `max_age_secs` より古いものを消し、残りが `keep` 件（`current` を含む）を超えるなら古い順に消す
pub fn files_to_delete(
    entries: &[LogEntry],
    current: &str,
    now_unix_secs: u64,
    keep: usize,
    max_age_secs: u64,
) -> Vec<String> {
    let mut delete = Vec::new();
    let mut kept: Vec<&LogEntry> = Vec::new();
    for e in entries {
        if e.name == current || !is_log_file_name(&e.name) {
            continue;
        }
        if now_unix_secs.saturating_sub(e.modified_unix_secs) > max_age_secs {
            delete.push(e.name.clone());
        } else {
            kept.push(e);
        }
    }
    // current の分を差し引いた枠に収める。新しい順に残す（同時刻は名前順で安定させる）
    let slots = keep.saturating_sub(1);
    kept.sort_by(|a, b| {
        b.modified_unix_secs
            .cmp(&a.modified_unix_secs)
            .then_with(|| a.name.cmp(&b.name))
    });
    delete.extend(kept.into_iter().skip(slots).map(|e| e.name.clone()));
    delete.sort();
    delete
}

/// 既存ファイルが上限を超えているか（超えていれば作り直す）
pub fn should_recreate(current_len: u64) -> bool {
    current_len > MAX_FILE_BYTES
}

/// 起動のたびにファイルの先頭（追記）へ書く区切りの行。どの起動の出力か分かるようにする
pub fn launch_header(unix_secs: u64, host_pid: u32, minimal: bool) -> String {
    format_line(
        unix_secs,
        host_pid,
        "launch",
        &format!(
            "imserver launch minimal={minimal} (stderr of this launch follows; {})",
            format_timestamp(unix_secs)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, age_secs: u64) -> LogEntry {
        LogEntry {
            name: name.into(),
            modified_unix_secs: 100_000_000 - age_secs,
        }
    }

    #[test]
    fn names() {
        assert_eq!(file_name(1234), "imserver-1234.log");
        assert!(is_log_file_name("imserver-1.log"));
        assert!(!is_log_file_name("imserver-.log"));
        assert!(!is_log_file_name("imserver-1a.log"));
        assert!(!is_log_file_name("tsf.log"));
        assert!(!is_log_file_name("imserver-1.log.1"));
        assert_eq!(
            log_dir(Path::new("base")),
            Path::new("base").join("Nagi").join("logs")
        );
    }

    #[test]
    fn old_files_are_deleted_but_not_current_or_foreign() {
        let entries = [
            e("imserver-1.log", 8 * 24 * 3600),
            e("imserver-2.log", 3600),
            e("imserver-9.log", 30 * 24 * 3600), // current は古くても消さない
            e("tsf.log", 99 * 24 * 3600),
            e("notes.txt", 99 * 24 * 3600),
        ];
        assert_eq!(
            files_to_delete(&entries, "imserver-9.log", 100_000_000, 20, MAX_AGE_SECS),
            vec!["imserver-1.log"]
        );
    }

    #[test]
    fn count_is_capped_oldest_first_including_current() {
        let entries: Vec<LogEntry> = (1_u64..=5)
            .map(|i| e(&format!("imserver-{i}.log"), i * 100))
            .collect();
        // current(=imserver-9) を含めて 3 件まで → 他は新しい 2 件（1, 2）を残して 3, 4, 5 を消す
        assert_eq!(
            files_to_delete(&entries, "imserver-9.log", 100_000_000, 3, MAX_AGE_SECS),
            vec!["imserver-3.log", "imserver-4.log", "imserver-5.log"]
        );
        // keep が 0 でも現在のファイルしか残らないだけで panic しない
        assert_eq!(
            files_to_delete(&entries, "imserver-9.log", 100_000_000, 0, MAX_AGE_SECS).len(),
            5
        );
        assert!(files_to_delete(&[], "imserver-9.log", 0, 20, MAX_AGE_SECS).is_empty());
    }

    #[test]
    fn recreate_and_header() {
        assert!(!should_recreate(MAX_FILE_BYTES));
        assert!(should_recreate(MAX_FILE_BYTES + 1));
        let h = launch_header(0, 42, true);
        assert!(h.starts_with("1970-01-01T00:00:00Z pid=42 [launch]"), "{h}");
        assert!(h.contains("minimal=true") && h.ends_with('\n'));
        assert_eq!(h.matches('\n').count(), 1);
    }
}
