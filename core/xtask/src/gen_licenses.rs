//! `cargo xtask gen-licenses` — iOS 向け依存（nagi-ffi の依存閉包）のライセンス一覧を
//! リポジトリルートの `THIRD_PARTY_LICENSES.md` に出力する。
//!
//! 依存閉包は `cargo tree -p nagi-ffi --features llama -e normal --target aarch64-apple-ios`
//! で求め（nagi-ffi 単体で feature 解決するため、デスクトップ専用の ureq/rustls は入らない）、
//! crate 名・バージョンに対応する SPDX ライセンスとリポジトリを `cargo metadata` から引く。
//! 出力は名前・バージョン順で決定的。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 一覧の対象ターゲット（iOS 実機）
const TARGET: &str = "aarch64-apple-ios";

fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cargo_output(args: &[&str], cwd: &Path) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cargo を実行できません: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo {} が失敗しました: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|e| format!("cargo の出力が UTF-8 ではありません: {e}"))
}

/// 依存閉包（レジストリ/git 由来のみ）の (名前, バージョン) を集める
fn closure(core_dir: &Path) -> Result<BTreeSet<(String, String)>, String> {
    let out = cargo_output(
        &[
            "tree",
            "-p",
            "nagi-ffi",
            "--features",
            "llama",
            "-e",
            "normal",
            "--target",
            TARGET,
            "--prefix",
            "none",
            "--format",
            "{p}",
        ],
        core_dir,
    )?;
    let mut set = BTreeSet::new();
    for line in out.lines() {
        // 形式: `name vX.Y.Z`、後ろに `(proc-macro)` / `(*)` / `(/path)` / `(registry ...)` など
        let mut parts = line.split_whitespace();
        let (Some(name), Some(version)) = (parts.next(), parts.next()) else {
            continue;
        };
        let rest: Vec<&str> = parts.collect();
        // proc-macro は実行バイナリに入らない。ローカル（path）クレートは自作コードなので除く
        if rest.contains(&"(proc-macro)") || rest.iter().any(|r| r.starts_with("(/")) {
            continue;
        }
        let Some(version) = version.strip_prefix('v') else {
            continue;
        };
        set.insert((name.to_string(), version.to_string()));
    }
    Ok(set)
}

struct Info {
    license: String,
    repository: String,
}

/// `cargo metadata` から (名前, バージョン) → ライセンス・リポジトリ
fn metadata(core_dir: &Path) -> Result<BTreeMap<(String, String), Info>, String> {
    let out = cargo_output(
        &[
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--all-features",
        ],
        core_dir,
    )?;
    let json: serde_json::Value =
        serde_json::from_str(&out).map_err(|e| format!("cargo metadata の解析に失敗: {e}"))?;
    let mut map = BTreeMap::new();
    for p in json["packages"].as_array().into_iter().flatten() {
        let (Some(name), Some(version)) = (p["name"].as_str(), p["version"].as_str()) else {
            continue;
        };
        let license = p["license"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                p["license_file"]
                    .as_str()
                    .map(|f| format!("(license-file: {f})"))
            })
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let repository = p["repository"]
            .as_str()
            .or_else(|| p["homepage"].as_str())
            .unwrap_or("")
            .to_string();
        map.insert(
            (name.to_string(), version.to_string()),
            Info {
                license,
                repository,
            },
        );
    }
    Ok(map)
}

fn render(
    crates: &BTreeSet<(String, String)>,
    meta: &BTreeMap<(String, String), Info>,
) -> Result<String, String> {
    let mut md = String::new();
    md.push_str("# Third-Party Licenses\n\n");
    md.push_str(
        "このファイルは `cargo xtask gen-licenses` で生成される（手で編集しない）。\n\
         iOS 向けビルド（`nagi-ffi`、feature `llama` 有効、ターゲット `aarch64-apple-ios`）の\n\
         Rust 依存クレートの一覧である。\n\n",
    );
    md.push_str(
        "- 辞書（Mozc OSS 辞書 / IPAdic）・ローマ字表（Mozc）・変換モデル（zenz）の出典とライセンスは、\
         リポジトリルートの [`NOTICE`](NOTICE) を参照すること\n\
         - llama.cpp（MIT License、Copyright (c) The ggml authors）は `llama-cpp-sys-2` 経由で\
         静的に取り込まれる（下表の `llama-cpp-sys-2` はそのバインディング）\n\
         - Nagi 自身のコード（`nagi-*`）は `MIT OR Apache-2.0`\n\n",
    );
    md.push_str(&format!("依存クレート数: {}\n\n", crates.len()));
    md.push_str("| crate | version | license (SPDX) | repository |\n| --- | --- | --- | --- |\n");
    for key in crates {
        let info = meta
            .get(key)
            .ok_or_else(|| format!("cargo metadata に {} v{} がありません", key.0, key.1))?;
        md.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            key.0,
            key.1,
            info.license.replace('|', "\\|"),
            info.repository
        ));
    }
    Ok(md)
}

pub fn run() -> Result<(), String> {
    let root = repo_root();
    let core_dir = root.join("core");
    let crates = closure(&core_dir)?;
    if crates.is_empty() {
        return Err("依存閉包が空です（cargo tree の出力を確認してください）".to_string());
    }
    let meta = metadata(&core_dir)?;
    let md = render(&crates, &meta)?;
    let out = root.join("THIRD_PARTY_LICENSES.md");
    std::fs::write(&out, md).map_err(|e| format!("{} の書き込みに失敗: {e}", out.display()))?;
    println!(
        "{} を生成しました（{} crates）",
        out.display(),
        crates.len()
    );
    Ok(())
}
