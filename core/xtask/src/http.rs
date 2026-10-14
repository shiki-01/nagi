//! ダウンロード用 HTTP クライアント。環境変数のプロキシと追加 CA を尊重する（TLS 検証は無効化しない）。
//!
//! - プロキシ: `HTTPS_PROXY` / `https_proxy` / `ALL_PROXY` / `all_proxy`（`NO_PROXY` / `no_proxy` で除外）
//! - 追加 CA: `SSL_CERT_FILE` / `CURL_CA_BUNDLE` / `REQUESTS_CA_BUNDLE` が指す PEM を
//!   webpki-roots のルートに加える

use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::{CertificateDer, pem::PemObject};

const CA_ENV_VARS: &[&str] = &["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE"];
const PROXY_ENV_VARS: &[&str] = &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// webpki-roots + 環境変数が指す追加 CA バンドルから TLS 設定を作る。
fn tls_config() -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    for var in CA_ENV_VARS {
        let Some(path) = env_nonempty(var) else {
            continue;
        };
        let certs = CertificateDer::pem_file_iter(&path)
            .and_then(|iter| iter.collect::<Result<Vec<_>, _>>())
            .map_err(|e| format!("CA バンドル読み込み失敗 {var}={path}: {e}"))?;
        let (added, ignored) = roots.add_parsable_certificates(certs);
        eprintln!("追加 CA: {var}={path} ({added} 件追加, {ignored} 件無視)");
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS 設定失敗: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// `NO_PROXY` に `host` が該当するか（`*`、完全一致、`.example.com` / `example.com` のサフィックス一致）。
fn no_proxy_matches(host: &str) -> bool {
    let Some(list) = env_nonempty("NO_PROXY").or_else(|| env_nonempty("no_proxy")) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    list.split(',')
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            let entry = entry.trim_start_matches('.');
            // ポート指定（host:port）は無視してホスト部で比較する
            let entry = entry.split(':').next().unwrap_or(entry);
            host == entry || host.ends_with(&format!(".{entry}"))
        })
}

fn proxy_from_env(url: &str) -> Result<Option<ureq::Proxy>, String> {
    let host = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?']).next())
        .map(|authority| authority.rsplit('@').next().unwrap_or(authority))
        .and_then(|hostport| hostport.split(':').next())
        .unwrap_or("");
    if no_proxy_matches(host) {
        return Ok(None);
    }
    let Some(proxy_url) = PROXY_ENV_VARS.iter().find_map(|v| env_nonempty(v)) else {
        return Ok(None);
    };
    ureq::Proxy::new(&proxy_url)
        .map(Some)
        .map_err(|e| format!("プロキシ設定が不正 ({}): {e}", redact(&proxy_url)))
}

/// 資格情報を含みうる URL をログ用に伏せる。
fn redact(url: &str) -> String {
    match (url.split_once("://"), url.rfind('@')) {
        (Some((scheme, _)), Some(at)) => format!("{scheme}://***@{}", &url[at + 1..]),
        _ => url.to_string(),
    }
}

/// プロキシ・追加 CA を反映して GET する。
pub fn get(url: &str) -> Result<ureq::Response, String> {
    let mut builder = ureq::AgentBuilder::new().tls_config(tls_config()?);
    if let Some(proxy) = proxy_from_env(url)? {
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .get(url)
        .call()
        .map_err(|e| format!("HTTP リクエスト失敗 {url}: {e}"))
}
