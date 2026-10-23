//! 診断ログ `%LOCALAPPDATA%\Nagi\logs\tsf.log` の書き込み（薄い I/O 層）。
//!
//! 整形・ローテーションの判断・文言は `nagi_windows::diag`。ここは追記だけ:
//! 複数のホストプロセスから同時に追記しても壊れないよう、追記モードで 1 行を 1 回の書き込みにする。
//! 書けなくても何もしない（ログのために入力を止めない）。入力内容は渡さないこと。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use nagi_windows::diag;

/// ログファイルのパス。`LOCALAPPDATA` が無ければ `None`（ログなし）
fn default_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|dir| diag::log_path(Path::new(&dir)))
    })
    .as_ref()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 1 行追記する。上限を超えるなら先に 1 世代前（`tsf.log.1`）へ回す。失敗は無視する
pub fn append_line(path: &Path, line: &str, max_bytes: u64) {
    let open = || OpenOptions::new().create(true).append(true).open(path);
    let mut file = match open() {
        Ok(file) => file,
        Err(_) => {
            // 初回はディレクトリが無い
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            match open() {
                Ok(file) => file,
                Err(_) => return,
            }
        }
    };
    let current = file.metadata().map_or(0, |m| m.len());
    if diag::needs_rotation(current, line.len() as u64, max_bytes) {
        // 他のプロセスが先に回していても構わない（rename の失敗は無視。std は共有削除付きで開く）
        drop(file);
        let _ = fs::rename(path, diag::rotated_path(path));
        file = match open() {
            Ok(file) => file,
            Err(_) => return,
        };
    }
    let _ = file.write_all(line.as_bytes());
}

/// イベントを 1 行記録する（`tag` は分類、`message` に入力内容を含めないこと）
pub fn log(tag: &str, message: &str) {
    let Some(path) = default_path() else {
        return;
    };
    let line = diag::format_line(unix_now(), std::process::id(), tag, message);
    append_line(path, &line, diag::MAX_LOG_BYTES);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nagi-tsf-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn appends_lines_and_creates_directories() {
        let dir = temp_dir("append");
        let path = dir.join("logs").join("tsf.log");
        append_line(&path, "one\n", 1024);
        append_line(&path, "two\n", 1024);
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotates_one_generation_when_over_the_limit() {
        let dir = temp_dir("rotate");
        let path = dir.join("tsf.log");
        append_line(&path, "aaaa\n", 12);
        append_line(&path, "bbbb\n", 12);
        // 5 + 5 + 5 > 12 → 3 行目の前に回る
        append_line(&path, "cccc\n", 12);
        assert_eq!(fs::read_to_string(&path).unwrap(), "cccc\n");
        assert_eq!(
            fs::read_to_string(diag::rotated_path(&path)).unwrap(),
            "aaaa\nbbbb\n"
        );
        // さらに回すと古い世代は上書きされる（世代は 1 つだけ）
        append_line(&path, "dddd\n", 12);
        append_line(&path, "eeee\n", 12);
        assert_eq!(
            fs::read_to_string(diag::rotated_path(&path)).unwrap(),
            "cccc\ndddd\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unwritable_path_is_ignored() {
        // ディレクトリをファイルとして開こうとして失敗する。パニックしなければよい
        let dir = temp_dir("unwritable");
        fs::create_dir_all(&dir).unwrap();
        append_line(&dir, "x\n", 1024);
        let _ = fs::remove_dir_all(&dir);
    }
}
