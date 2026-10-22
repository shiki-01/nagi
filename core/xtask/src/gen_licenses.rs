//! `cargo xtask gen-licenses [--platform ios|windows|all]` — 配布物ごとの依存ライセンス一覧を
//! リポジトリルートに出力する（既定は全部）。
//!
//! - `ios`: `THIRD_PARTY_LICENSES.md`（iOS アプリが同梱）。`nagi-ffi` の依存閉包
//!   （`--features llama`、`aarch64-apple-ios`。nagi-ffi 単体で feature 解決するため、
//!   デスクトップ専用の ureq/rustls は入らない）
//! - `windows`: `THIRD_PARTY_LICENSES_WINDOWS.md`（`bundle-windows` が zip に
//!   `THIRD_PARTY_LICENSES.md` の名前で入れる）。TSF の DLL（`nagi-windows-tsf`）と
//!   エンジン（`nagi-server` の imserver、`--features llama`）の依存閉包の和
//!   （`x86_64-pc-windows-msvc`。`windows` クレート、ureq/rustls/ring など）
//!
//! 別ファイルにしたのは、配布物ごとに実際に取り込まれる依存が違い（iOS の一覧に Windows 専用の
//! crate を混ぜない、その逆もしない）、iOS のリソース同梱（`project.yml`）を変えずに済むため
//! （`docs/decisions.md` H-43）。
//!
//! 依存閉包は `cargo tree -e normal --target <triple>` で求め、crate 名・バージョンに対応する
//! SPDX ライセンスとリポジトリを `cargo metadata` から引く。出力は名前・バージョン順で決定的。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 依存閉包の起点にする crate（`cargo tree -p <package> --features <features>`）
struct Root {
    package: &'static str,
    features: &'static str,
}

/// 一覧を作る配布物
struct Platform {
    /// `--platform` での名前
    key: &'static str,
    /// 出力ファイル名（リポジトリルート）
    output: &'static str,
    /// 対象ターゲット
    target: &'static str,
    /// 起点の crate（閉包の和を取る）
    roots: &'static [Root],
    /// 本文の冒頭に入れる説明（対象のビルド）
    scope: &'static str,
    /// 補足の箇条書き（先頭に `- ` を付けて出す）
    notes: &'static [&'static str],
}

const COMMON_NOTES: [&str; 2] = [
    "辞書（Mozc OSS 辞書 / IPAdic）・ローマ字表（Mozc）・変換モデル（zenz）の出典とライセンスは、\
     リポジトリルートの [`NOTICE`](NOTICE) を参照すること",
    "Nagi 自身のコード（`nagi-*`）は `MIT OR Apache-2.0`",
];

const LLAMA_NOTE: &str = "llama.cpp（MIT License、Copyright (c) The ggml authors）は \
     `llama-cpp-sys-2` 経由で静的に取り込まれる（下表の `llama-cpp-sys-2` はそのバインディング）";

const PLATFORMS: [Platform; 2] = [
    Platform {
        key: "ios",
        output: "THIRD_PARTY_LICENSES.md",
        target: "aarch64-apple-ios",
        roots: &[Root {
            package: "nagi-ffi",
            features: "llama",
        }],
        scope: "iOS 向けビルド（`nagi-ffi`、feature `llama` 有効、ターゲット `aarch64-apple-ios`）の\n\
                Rust 依存クレートの一覧である。",
        notes: &[COMMON_NOTES[0], LLAMA_NOTE, COMMON_NOTES[1]],
    },
    Platform {
        key: "windows",
        output: "THIRD_PARTY_LICENSES_WINDOWS.md",
        target: "x86_64-pc-windows-msvc",
        roots: &[
            Root {
                package: "nagi-windows-tsf",
                features: "",
            },
            Root {
                package: "nagi-server",
                features: "llama",
            },
        ],
        scope: "Windows 向けビルド（TSF の DLL `nagi-windows-tsf` と、エンジン `imserver` = `nagi-server`\n\
                feature `llama` 有効、ターゲット `x86_64-pc-windows-msvc`）の Rust 依存クレートの一覧である。\n\
                配布 zip（`bundle-windows`）には `THIRD_PARTY_LICENSES.md` の名前で同梱される。",
        notes: &[
            COMMON_NOTES[0],
            LLAMA_NOTE,
            "`windows` / `windows-*` クレートは Microsoft の公式バインディング（`MIT OR Apache-2.0`）",
            COMMON_NOTES[1],
        ],
    },
];

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

/// `cargo tree --prefix none --format {p}` の出力から (名前, バージョン) を集める。
/// proc-macro とローカル（path）クレートは除く
fn parse_tree(out: &str) -> BTreeSet<(String, String)> {
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
    set
}

/// `cargo tree` の引数（起点 1 つ分）
fn tree_args<'a>(root: &'a Root, target: &'a str) -> Vec<&'a str> {
    let mut args = vec!["tree", "-p", root.package];
    if !root.features.is_empty() {
        args.extend(["--features", root.features]);
    }
    args.extend([
        "-e", "normal", "--target", target, "--prefix", "none", "--format", "{p}",
    ]);
    args
}

/// 依存閉包（レジストリ/git 由来のみ）の (名前, バージョン)。起点ごとの閉包の和
fn closure(core_dir: &Path, platform: &Platform) -> Result<BTreeSet<(String, String)>, String> {
    let mut set = BTreeSet::new();
    for root in platform.roots {
        let out = cargo_output(&tree_args(root, platform.target), core_dir)?;
        set.extend(parse_tree(&out));
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
    platform: &Platform,
    crates: &BTreeSet<(String, String)>,
    meta: &BTreeMap<(String, String), Info>,
) -> Result<String, String> {
    let mut md = String::new();
    md.push_str("# Third-Party Licenses\n\n");
    md.push_str("このファイルは `cargo xtask gen-licenses` で生成される（手で編集しない）。\n");
    md.push_str(platform.scope);
    md.push_str("\n\n");
    for note in platform.notes {
        md.push_str(&format!("- {note}\n"));
    }
    md.push('\n');
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

/// `--platform` の値から対象を選ぶ（`all` は全部）
fn select(platform: &str) -> Result<Vec<&'static Platform>, String> {
    if platform == "all" {
        return Ok(PLATFORMS.iter().collect());
    }
    PLATFORMS
        .iter()
        .find(|p| p.key == platform)
        .map(|p| vec![p])
        .ok_or_else(|| {
            format!("--platform は ios / windows / all のいずれかです（指定: {platform}）")
        })
}

pub fn run(platform: &str) -> Result<(), String> {
    let targets = select(platform)?;
    let root = repo_root();
    let core_dir = root.join("core");
    let meta = metadata(&core_dir)?;
    for platform in targets {
        let crates = closure(&core_dir, platform)?;
        if crates.is_empty() {
            return Err("依存閉包が空です（cargo tree の出力を確認してください）".to_string());
        }
        let md = render(platform, &crates, &meta)?;
        let out = root.join(platform.output);
        std::fs::write(&out, md).map_err(|e| format!("{} の書き込みに失敗: {e}", out.display()))?;
        println!(
            "{} を生成しました（{}、{} crates）",
            out.display(),
            platform.key,
            crates.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tree_skips_proc_macro_local_and_dedups() {
        let out = "\
serde v1.0.229
serde v1.0.229 (*)
serde_derive v1.0.229 (proc-macro)
nagi-windows v0.1.1 (/home/user/nagi/frontend-windows)
windows v0.62.2
not-a-version
";
        let set = parse_tree(out);
        let names: Vec<_> = set.iter().map(|(n, v)| format!("{n} {v}")).collect();
        assert_eq!(names, ["serde 1.0.229", "windows 0.62.2"]);
    }

    #[test]
    fn tree_args_include_features_only_when_given() {
        let with = tree_args(
            &Root {
                package: "a",
                features: "llama",
            },
            "t",
        );
        assert!(with.windows(2).any(|w| w == ["--features", "llama"]));
        let without = tree_args(
            &Root {
                package: "a",
                features: "",
            },
            "t",
        );
        assert!(!without.contains(&"--features"));
        assert!(without.windows(2).any(|w| w == ["--target", "t"]));
    }

    #[test]
    fn platforms_are_distinct_and_selectable() {
        assert_eq!(select("all").unwrap().len(), PLATFORMS.len());
        let win = select("windows").unwrap();
        assert_eq!(win[0].output, "THIRD_PARTY_LICENSES_WINDOWS.md");
        // iOS の同梱ファイル名は project.yml が参照しているので変えない
        assert_eq!(select("ios").unwrap()[0].output, "THIRD_PARTY_LICENSES.md");
        assert!(select("android").is_err());
        // Windows の起点は DLL とエンジンの両方
        let packages: Vec<_> = win[0].roots.iter().map(|r| r.package).collect();
        assert_eq!(packages, ["nagi-windows-tsf", "nagi-server"]);
    }

    #[test]
    fn render_lists_crates_and_fails_on_missing_metadata() {
        let mut crates = BTreeSet::new();
        crates.insert(("a".to_string(), "1.0.0".to_string()));
        let mut meta = BTreeMap::new();
        assert!(render(&PLATFORMS[1], &crates, &meta).is_err());
        meta.insert(
            ("a".to_string(), "1.0.0".to_string()),
            Info {
                license: "MIT | Apache-2.0".to_string(),
                repository: "https://example.com/a".to_string(),
            },
        );
        let md = render(&PLATFORMS[1], &crates, &meta).unwrap();
        assert!(md.contains("依存クレート数: 1"));
        assert!(md.contains("| a | 1.0.0 | MIT \\| Apache-2.0 | https://example.com/a |"));
        assert!(md.contains("x86_64-pc-windows-msvc"));
        assert!(md.contains("- `windows` / `windows-*`"));
        // iOS 版には Windows 専用の注記を入れない
        let ios = render(&PLATFORMS[0], &crates, &meta).unwrap();
        assert!(!ios.contains("`windows` /"));
    }
}
