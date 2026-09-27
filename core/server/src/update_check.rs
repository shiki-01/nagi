//! アップデート確認（`config.toml` の `[update] auto_check`）。
//! マニフェスト（JSON）を取得し、現在のバージョンより新しいものがあれば知らせる。
//! ダウンロード・インストールは行わない（確認のみ）。

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// 既定のマニフェスト URL
pub const DEFAULT_CHECK_URL: &str = "https://nagi.shiki-01.com/updates/latest.json";

/// 確認の最短間隔（24時間）。この間隔より前回確認から経っていなければ確認しない
const MIN_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// マニフェストの内容（必要な項目のみ）
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// 最新バージョン（"x.y.z" 形式）
    pub version: String,
    /// リリースノートの URL（あれば通知メッセージに含める）
    #[serde(default)]
    pub notes_url: Option<String>,
}

/// 前回確認の記録（`<user_dir>/update_check.json`）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckState {
    last_checked_unix: u64,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 前回確認から `MIN_CHECK_INTERVAL_SECS` 以上経っているか（状態ファイルが無い・壊れていれば確認してよい）
fn should_check(state_path: &Path) -> bool {
    let Ok(data) = std::fs::read_to_string(state_path) else {
        return true;
    };
    let Ok(state) = serde_json::from_str::<CheckState>(&data) else {
        return true;
    };
    unix_now().saturating_sub(state.last_checked_unix) >= MIN_CHECK_INTERVAL_SECS
}

/// 確認した時刻を記録する（一時ファイル経由で原子的に）。失敗しても致命的ではないので無視する
fn record_checked(state_path: &Path) {
    let state = CheckState {
        last_checked_unix: unix_now(),
    };
    let Ok(json) = serde_json::to_string(&state) else {
        return;
    };
    let mut temp = state_path.to_path_buf();
    temp.set_extension("tmp");
    if std::fs::write(&temp, json).is_ok() {
        let _ = std::fs::rename(&temp, state_path);
    }
}

/// バージョン文字列（"x.y.z"）を比較用のタプルに変換する。パースできない要素は 0 とする
fn parse_version(v: &str) -> (u64, u64, u64) {
    let mut parts = v.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

/// マニフェストを実際に HTTP で取得する（既定のタイムアウト 5 秒）
fn fetch_manifest(url: &str) -> Result<Manifest, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("アップデート確認のリクエストに失敗: {e}"))?;
    if response.status() != 200 {
        return Err(format!(
            "アップデート確認が HTTP {} を返しました",
            response.status()
        ));
    }
    response
        .into_json::<Manifest>()
        .map_err(|e| format!("アップデート確認のレスポンス解析に失敗: {e}"))
}

/// 取得関数を差し替え可能な形の本体ロジック（テスト用に分離）
fn check_once_with(
    current_version: &str,
    fetch: impl FnOnce() -> Result<Manifest, String>,
) -> Option<Manifest> {
    let manifest = fetch().ok()?;
    (parse_version(&manifest.version) > parse_version(current_version)).then_some(manifest)
}

/// 現在のバージョンより新しいバージョンがあれば `Some(Manifest)` を返す（失敗したら `None`）
pub fn check_once(url: &str, current_version: &str) -> Option<Manifest> {
    check_once_with(current_version, || fetch_manifest(url))
}

/// 状態ファイルによる間引きつきで確認する。`state_path` が `None` なら間引かない
pub fn check_with_throttle(
    url: &str,
    current_version: &str,
    state_path: Option<&Path>,
) -> Option<Manifest> {
    if let Some(path) = state_path
        && !should_check(path)
    {
        return None;
    }
    let result = check_once(url, current_version);
    if let Some(path) = state_path {
        record_checked(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_orders_correctly() {
        assert!(parse_version("0.1.0") > parse_version("0.0.9"));
        assert!(parse_version("1.0.0") > parse_version("0.99.99"));
        assert_eq!(parse_version("1.2.3"), (1, 2, 3));
        assert_eq!(parse_version("1.2"), (1, 2, 0));
        assert_eq!(parse_version("bad"), (0, 0, 0));
    }

    #[test]
    fn check_once_with_returns_manifest_when_newer() {
        let m = check_once_with("0.1.0", || {
            Ok(Manifest {
                version: "0.2.0".to_string(),
                notes_url: None,
            })
        });
        assert_eq!(m.unwrap().version, "0.2.0");
    }

    #[test]
    fn check_once_with_none_when_not_newer() {
        let m = check_once_with("0.2.0", || {
            Ok(Manifest {
                version: "0.1.0".to_string(),
                notes_url: None,
            })
        });
        assert!(m.is_none());
    }

    #[test]
    fn check_once_with_none_when_equal() {
        let m = check_once_with("0.2.0", || {
            Ok(Manifest {
                version: "0.2.0".to_string(),
                notes_url: None,
            })
        });
        assert!(m.is_none());
    }

    #[test]
    fn check_once_with_none_on_fetch_error() {
        let m = check_once_with("0.1.0", || Err("network down".to_string()));
        assert!(m.is_none());
    }

    #[test]
    fn should_check_true_when_state_file_missing() {
        let path = std::env::temp_dir().join(format!(
            "nagi-update-check-test-missing-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        assert!(should_check(&path));
    }

    #[test]
    fn record_and_should_check_respects_interval() {
        let path = std::env::temp_dir().join(format!(
            "nagi-update-check-test-interval-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        record_checked(&path);
        assert!(!should_check(&path), "直後は間引かれるはず");
        let _ = std::fs::remove_file(&path);
    }
}
