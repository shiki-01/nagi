//! アップデート確認（`config.toml` の `[update] auto_check`）。
//! マニフェスト（JSON）を取得し、現在のバージョンより新しいものがあれば知らせる。
//! ダウンロード・インストールは行わない（確認のみ）。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 既定のマニフェスト URL
pub const DEFAULT_CHECK_URL: &str = "https://nagi.shiki-01.com/updates/latest.json";

/// アップデート配布物の署名検証に使う ed25519 公開鍵（16進数、32バイト）。
///
/// **これはこの実装のテスト・動作確認用に生成したプレースホルダーの鍵です。実際に配布する前に、
/// 本物の鍵ペアを新たに生成し、この値を実際の公開鍵に置き換える必要があります**。対応する
/// 秘密鍵は絶対にこのリポジトリに含めず、リリースの署名を行う環境（ビルドサーバー等）でのみ
/// オフラインで保管してください（例: `openssl genpkey -algorithm ed25519` で生成できます）。
pub const UPDATE_PUBLIC_KEY_HEX: &str =
    "48880411a8c51a1e707022d84b711e9ed883a20143eb1734da57d11877fa3abc";

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
    /// 配布物（インストーラ等）の取得先 URL
    #[serde(default)]
    pub download_url: Option<String>,
    /// 配布物の SHA-256（16進数文字列）
    #[serde(default)]
    pub sha256: Option<String>,
    /// 配布物への ed25519 署名（16進数文字列、64バイト = 128文字）
    #[serde(default)]
    pub signature: Option<String>,
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

/// 16進数文字列をバイト列にデコードする（大文字小文字は区別しない）。長さが奇数、
/// 16進数として不正な文字がある場合はエラー
fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("16進数文字列の長さが奇数です".to_string());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("16進数として不正です: {e}"))
        })
        .collect()
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

/// マニフェストを実際に HTTP で取得する（既定のタイムアウト 5 秒）。
/// feature `update-check` が無効なビルド（iOS の XCFramework 等）では常にエラー（= 確認しない）
#[cfg(not(feature = "update-check"))]
fn fetch_manifest(_url: &str) -> Result<Manifest, String> {
    Err("update-check feature が無効です".to_string())
}

/// マニフェストを実際に HTTP で取得する（既定のタイムアウト 5 秒）
#[cfg(feature = "update-check")]
fn fetch_manifest(url: &str) -> Result<Manifest, String> {
    use std::time::Duration;
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

/// バイト列が、指定した ed25519 公開鍵（16進数、32バイト）による署名（16進数、64バイト）と
/// 一致することを検証する
fn verify_signature(data: &[u8], signature_hex: &str, public_key_hex: &str) -> Result<(), String> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let key_bytes = hex_decode(public_key_hex)?;
    let key_bytes: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| "公開鍵の長さが32バイトではありません".to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_bytes).map_err(|e| format!("公開鍵が不正です: {e}"))?;

    let sig_bytes = hex_decode(signature_hex)?;
    let sig_bytes: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "署名の長さが64バイトではありません".to_string())?;
    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(data, &signature)
        .map_err(|e| format!("署名検証に失敗しました: {e}"))
}

/// マニフェストで指定された配布物を取得し、SHA-256・ed25519署名の両方を検証したバイト列を返す。
/// 検証に失敗した場合（改竄・鍵不一致・チェックサム不一致等）は取得したバイト列を捨ててエラーを返す。
/// `public_key_hex` は呼び出し側が渡す（このプロジェクトの実際の公開鍵は
/// [`UPDATE_PUBLIC_KEY_HEX`] を参照。埋め込み鍵を差し替える運用は README 等で管理する）。
/// **ダウンロードしたバイト列をディスクへ書き込む・インストールする処理は含まない**
/// （呼び出し側の責任。プラットフォームごとに異なるため）。
/// feature `update-check` が無効なビルドでは、マニフェストの検査後にエラーを返す（取得しない）
pub fn download_verified(manifest: &Manifest, public_key_hex: &str) -> Result<Vec<u8>, String> {
    let url = manifest
        .download_url
        .as_deref()
        .ok_or_else(|| "マニフェストに download_url がありません".to_string())?;
    let signature = manifest
        .signature
        .as_deref()
        .ok_or_else(|| "マニフェストに signature がありません".to_string())?;
    let expected_sha256 = manifest
        .sha256
        .as_deref()
        .ok_or_else(|| "マニフェストに sha256 がありません".to_string())?;

    let bytes = http_get_bytes(url)?;
    verify_download(&bytes, expected_sha256, signature, public_key_hex)?;
    Ok(bytes)
}

/// 配布物のバイト列を HTTP で取得する（タイムアウト 30 秒）
#[cfg(feature = "update-check")]
fn http_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    use std::time::Duration;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("配布物の取得に失敗: {e}"))?;
    if response.status() != 200 {
        return Err(format!(
            "配布物の取得が HTTP {} を返しました",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("配布物の読み取りに失敗: {e}"))?;
    Ok(bytes)
}

/// feature `update-check` が無効なビルドでは取得しない
#[cfg(not(feature = "update-check"))]
fn http_get_bytes(_url: &str) -> Result<Vec<u8>, String> {
    Err("update-check feature が無効です".to_string())
}

/// 取得したバイト列の SHA-256 と ed25519 署名を検証する（HTTP に依存しない）
fn verify_download(
    bytes: &[u8],
    expected_sha256: &str,
    signature_hex: &str,
    public_key_hex: &str,
) -> Result<(), String> {
    let actual_sha256 = sha256_hex(bytes);
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(format!(
            "SHA-256 が一致しません（期待 {expected_sha256}、実際 {actual_sha256}）"
        ));
    }
    verify_signature(bytes, signature_hex, public_key_hex)
}

/// バイト列の SHA-256 を16進数文字列で返す
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
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
                download_url: None,
                sha256: None,
                signature: None,
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
                download_url: None,
                sha256: None,
                signature: None,
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
                download_url: None,
                sha256: None,
                signature: None,
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

    #[test]
    fn hex_decode_roundtrips_and_rejects_bad_input() {
        assert_eq!(hex_decode("00ff").unwrap(), vec![0x00, 0xff]);
        assert!(hex_decode("0").is_err()); // 奇数長
        assert!(hex_decode("zz").is_err()); // 16進数でない
    }

    /// 実際に openssl（ed25519）で生成した鍵ペアで、既知のメッセージに署名した既知の値。
    /// 対応する秘密鍵は破棄済みで、テストの検証にのみ使う
    const TEST_PUBLIC_KEY_HEX: &str =
        "48880411a8c51a1e707022d84b711e9ed883a20143eb1734da57d11877fa3abc";
    const TEST_MESSAGE: &[u8] = b"nagi-update-test";
    const TEST_SIGNATURE_HEX: &str = "d28314b502a250ea0471de96a5ec949bf254473f0ce3cc4c1078099e1434ef478a56a49bb49a7b60a25409be2342914b7d36782288e088885d1b83f5e824bc02";

    #[test]
    fn verify_signature_accepts_known_good_signature() {
        verify_signature(TEST_MESSAGE, TEST_SIGNATURE_HEX, TEST_PUBLIC_KEY_HEX).unwrap();
    }

    #[test]
    fn verify_signature_rejects_tampered_message() {
        let tampered = b"nagi-update-tex1"; // 1バイト変えた
        assert!(verify_signature(tampered, TEST_SIGNATURE_HEX, TEST_PUBLIC_KEY_HEX).is_err());
    }

    #[test]
    fn verify_signature_rejects_wrong_public_key() {
        // 別の（全く無関係な）32バイトの16進数文字列
        let wrong_key = "0".repeat(64);
        assert!(verify_signature(TEST_MESSAGE, TEST_SIGNATURE_HEX, &wrong_key).is_err());
    }

    #[test]
    fn verify_signature_rejects_malformed_signature_length() {
        assert!(verify_signature(TEST_MESSAGE, "abcd", TEST_PUBLIC_KEY_HEX).is_err());
    }

    #[test]
    fn verify_download_checks_sha256_before_signature() {
        let err = verify_download(
            TEST_MESSAGE,
            &"0".repeat(64),
            TEST_SIGNATURE_HEX,
            TEST_PUBLIC_KEY_HEX,
        )
        .unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        let good_sha = sha256_hex(TEST_MESSAGE);
        verify_download(
            TEST_MESSAGE,
            &good_sha,
            TEST_SIGNATURE_HEX,
            TEST_PUBLIC_KEY_HEX,
        )
        .unwrap();
    }

    /// feature 無効時は、確認が安全に no-op（`None`。パニックもエラー通知もしない）になる
    #[cfg(not(feature = "update-check"))]
    #[test]
    fn check_is_noop_without_update_check_feature() {
        assert!(check_once("https://example.invalid/latest.json", "0.0.1").is_none());
        assert!(
            check_with_throttle("https://example.invalid/latest.json", "0.0.1", None).is_none()
        );
        let manifest = Manifest {
            version: "9.9.9".to_string(),
            notes_url: None,
            download_url: Some("https://example.invalid/x".to_string()),
            sha256: Some("00".to_string()),
            signature: Some("00".to_string()),
        };
        assert!(download_verified(&manifest, TEST_PUBLIC_KEY_HEX).is_err());
    }

    #[test]
    fn download_verified_requires_all_manifest_fields() {
        let manifest = Manifest {
            version: "1.0.0".to_string(),
            notes_url: None,
            download_url: None,
            sha256: None,
            signature: None,
        };
        let err = download_verified(&manifest, TEST_PUBLIC_KEY_HEX).unwrap_err();
        assert!(err.contains("download_url"), "{err}");
    }
}
