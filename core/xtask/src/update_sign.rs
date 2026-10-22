//! `cargo xtask gen-update-key` / `cargo xtask sign-update` — アップデート配布物の ed25519 署名。
//!
//! 署名対象は配布物（Nagi.app の zip）のバイト列全体。生成する `latest.json` は
//! `nagi_server::update_check::Manifest` と同じ形（sha256・signature は小文字16進）。
//! 秘密鍵（32 バイトの seed）はファイルに書かず、環境変数 `NAGI_UPDATE_SIGNING_KEY` で渡す。

use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// 署名鍵（seed、16進 64 文字）を渡す環境変数名。
const SIGNING_KEY_ENV: &str = "NAGI_UPDATE_SIGNING_KEY";

/// `sign-update` のオプション。
pub struct SignOptions {
    pub file: PathBuf,
    pub version: String,
    pub download_url: String,
    pub notes_url: Option<String>,
    pub out: PathBuf,
}

fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 公開鍵の単一ソース（`config/update-public-key.txt`）のパス。
pub fn public_key_path() -> PathBuf {
    repo_root().join("config/update-public-key.txt")
}

/// バイト列を小文字16進数にする。
pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 16進数文字列をバイト列にデコードする（大文字小文字は区別しない）。
pub fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.is_ascii() {
        return Err("16進数として不正です: ASCII 以外の文字があります".to_string());
    }
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

/// 16進 64 文字の seed から署名鍵を作る。
pub fn signing_key_from_hex(seed_hex: &str) -> Result<SigningKey, String> {
    let seed: [u8; 32] = hex_decode(seed_hex.trim())?
        .try_into()
        .map_err(|_| "秘密鍵の長さが32バイト（16進 64 文字）ではありません".to_string())?;
    Ok(SigningKey::from_bytes(&seed))
}

/// 署名鍵に対応する公開鍵（小文字16進）。
pub fn public_key_hex(key: &SigningKey) -> String {
    hex_encode(key.verifying_key().as_bytes())
}

/// バイト列の SHA-256（小文字16進）。
pub fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&Sha256::digest(data))
}

/// バイト列全体への ed25519 署名（小文字16進、128 文字）。
pub fn sign_hex(key: &SigningKey, data: &[u8]) -> String {
    hex_encode(&key.sign(data).to_bytes())
}

/// 署名を公開鍵（16進）で検証する（`nagi_server::update_check` と同じ方式）。
pub fn verify_hex(data: &[u8], signature_hex: &str, public_key_hex: &str) -> Result<(), String> {
    let key: [u8; 32] = hex_decode(public_key_hex)?
        .try_into()
        .map_err(|_| "公開鍵の長さが32バイトではありません".to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&key).map_err(|e| format!("公開鍵が不正です: {e}"))?;
    let sig: [u8; 64] = hex_decode(signature_hex)?
        .try_into()
        .map_err(|_| "署名の長さが64バイトではありません".to_string())?;
    verifying_key
        .verify(data, &Signature::from_bytes(&sig))
        .map_err(|e| format!("署名検証に失敗しました: {e}"))
}

/// `latest.json` の内容（`Manifest` と同じ形）を組み立てる。
pub fn build_manifest(
    key: &SigningKey,
    data: &[u8],
    version: &str,
    download_url: &str,
    notes_url: Option<&str>,
) -> serde_json::Value {
    let mut manifest = serde_json::json!({
        "version": version,
        "download_url": download_url,
        "sha256": sha256_hex(data),
        "signature": sign_hex(key, data),
    });
    if let Some(url) = notes_url {
        manifest["notes_url"] = serde_json::Value::String(url.to_string());
    }
    manifest
}

/// ed25519 鍵ペアを生成して標準出力に出す（秘密鍵はファイルに書かない）。
pub fn gen_key() -> Result<(), String> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("乱数の取得に失敗しました: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    println!("秘密鍵（seed）: {}", hex_encode(&seed));
    println!("公開鍵        : {}", public_key_hex(&key));
    println!();
    println!(
        "秘密鍵は GitHub の Actions Secrets `{SIGNING_KEY_ENV}` に登録し、公開鍵は config/update-public-key.txt に書いてください。"
    );
    println!("秘密鍵はリポジトリ・ログ・チャットなどに残さないこと（この出力は保存されません）。");
    Ok(())
}

/// 配布物に署名して `latest.json` を書き出す。
pub fn sign_update(opts: SignOptions) -> Result<(), String> {
    let seed_hex = std::env::var(SIGNING_KEY_ENV).map_err(|_| {
        format!("環境変数 {SIGNING_KEY_ENV}（秘密鍵の seed、16進 64 文字）が未設定です")
    })?;
    let key = signing_key_from_hex(&seed_hex)?;
    let data = fs::read(&opts.file)
        .map_err(|e| format!("{} の読み込みに失敗しました: {e}", opts.file.display()))?;

    let manifest = build_manifest(
        &key,
        &data,
        &opts.version,
        &opts.download_url,
        opts.notes_url.as_deref(),
    );

    // 埋め込み公開鍵との不一致は警告のみ（鍵の差し替え前の段階でも動かすため）
    let derived = public_key_hex(&key);
    match fs::read_to_string(public_key_path()) {
        Ok(embedded) if embedded.trim().eq_ignore_ascii_case(&derived) => {}
        Ok(embedded) => eprintln!(
            "警告: 署名鍵の公開鍵（{derived}）が config/update-public-key.txt（{}）と一致しません。このままではアプリが署名を検証できません",
            embedded.trim()
        ),
        Err(e) => eprintln!("警告: config/update-public-key.txt を読めませんでした: {e}"),
    }

    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("latest.json の生成に失敗しました: {e}"))?;
    fs::write(&opts.out, format!("{json}\n"))
        .map_err(|e| format!("{} の書き込みに失敗しました: {e}", opts.out.display()))?;

    // 書き出した結果を読み直して自己検証する
    let written: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&opts.out)
            .map_err(|e| format!("{} の再読み込みに失敗しました: {e}", opts.out.display()))?,
    )
    .map_err(|e| format!("latest.json の再解析に失敗しました: {e}"))?;
    let field = |name: &str| written[name].as_str().unwrap_or_default().to_string();
    if !field("sha256").eq_ignore_ascii_case(&sha256_hex(&data)) {
        return Err("自己検証に失敗しました: sha256 が一致しません".to_string());
    }
    verify_hex(&data, &field("signature"), &derived)
        .map_err(|e| format!("自己検証に失敗しました: {e}"))?;

    println!("✓ latest.json を作成しました: {}", opts.out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED_HEX: &str = "0101010101010101010101010101010101010101010101010101010101010101";

    #[test]
    fn hex_roundtrips_and_rejects_bad_input() {
        assert_eq!(hex_encode(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(hex_decode("00ABff").unwrap(), vec![0x00, 0xab, 0xff]);
        assert!(hex_decode("0").is_err());
        assert!(hex_decode("zz").is_err());
    }

    #[test]
    fn signing_key_rejects_bad_seed() {
        assert!(signing_key_from_hex("zz").is_err());
        assert!(signing_key_from_hex("abcd").is_err()); // 短すぎる
        assert!(signing_key_from_hex(&"g".repeat(64)).is_err());
        assert!(signing_key_from_hex(SEED_HEX).is_ok());
    }

    #[test]
    fn sign_then_verify_and_reject_tamper() {
        let key = signing_key_from_hex(SEED_HEX).unwrap();
        let data = b"nagi-update-test";
        let sig = sign_hex(&key, data);
        assert_eq!(sig.len(), 128);
        let public = public_key_hex(&key);
        verify_hex(data, &sig, &public).unwrap();
        assert!(verify_hex(b"nagi-update-tex1", &sig, &public).is_err());
        assert!(verify_hex(data, &sig, &"0".repeat(64)).is_err());
    }

    #[test]
    fn manifest_has_expected_fields() {
        let key = signing_key_from_hex(SEED_HEX).unwrap();
        let data = b"zip bytes";
        let m = build_manifest(&key, data, "1.2.3", "https://example.com/Nagi.zip", None);
        assert_eq!(m["version"], "1.2.3");
        assert_eq!(m["download_url"], "https://example.com/Nagi.zip");
        assert_eq!(m["sha256"], sha256_hex(data));
        assert!(m.get("notes_url").is_none());
        verify_hex(
            data,
            m["signature"].as_str().unwrap(),
            &public_key_hex(&key),
        )
        .unwrap();
        let m = build_manifest(&key, data, "1.2.3", "u", Some("https://example.com/notes"));
        assert_eq!(m["notes_url"], "https://example.com/notes");
    }

    #[test]
    fn sha256_known_value() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
