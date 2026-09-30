//! `cargo xtask bundle-macos` — `Nagi.app` を組み立てて ad-hoc 署名し、zip 化する。
//!
//! 配布は Apple Developer Program 非加入のため ad-hoc 署名のみ（設計書 §5.2）。
//! 署名と zip 化は macOS 上でのみ行い、他の OS では警告してスキップする。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// バンドル識別子。
const BUNDLE_ID: &str = "com.shiki.nagi";
/// IMKServer の接続名。
const CONNECTION_NAME: &str = "com.shiki.nagi_Connection";
/// 最小サポート macOS。
const MIN_MACOS: &str = "13.0";

/// `bundle-macos` のオプション。
pub struct Options {
    /// Swift 実行ファイルのターゲット名。
    pub swift_target: String,
    /// 指定時は `swift build` を行わず、このファイルを実行ファイルとして使う。
    pub executable: Option<PathBuf>,
    /// imserver を `--features llama` でビルドする。
    pub llama: bool,
}

fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// XML の特殊文字をエスケープする。
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Info.plist の内容を生成する。
pub fn info_plist(executable: &str, version: &str) -> String {
    let e = xml_escape;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>ja</string>
	<key>CFBundleExecutable</key>
	<string>{exe}</string>
	<key>CFBundleIdentifier</key>
	<string>{id}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Nagi</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{ver}</string>
	<key>CFBundleVersion</key>
	<string>{ver}</string>
	<key>LSMinimumSystemVersion</key>
	<string>{min}</string>
	<key>LSBackgroundOnly</key>
	<true/>
	<key>LSUIElement</key>
	<true/>
	<key>InputMethodConnectionName</key>
	<string>{conn}</string>
	<key>InputMethodServerControllerClass</key>
	<string>NagiInputController</string>
	<key>InputMethodServerDelegateClass</key>
	<string>NagiInputController</string>
	<key>tsInputMethodNameKey</key>
	<string>Nagi</string>
	<key>tsInputMethodCharacterRepertoireKey</key>
	<array>
		<string>ja</string>
	</array>
</dict>
</plist>
"#,
        exe = e(executable),
        id = BUNDLE_ID,
        ver = e(version),
        min = MIN_MACOS,
        conn = CONNECTION_NAME,
    )
}

fn run_cmd(cmd: &mut Command, what: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|e| format!("{what} の実行に失敗しました: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{what} が失敗しました（終了コード: {}）",
            status.code().unwrap_or(-1)
        ))
    }
}

/// 生成物の実行ファイル名（Windows では `.exe` が付く）。
fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

pub fn run(opts: Options) -> Result<(), String> {
    let repo = repo_root();
    let core = repo.join("core");
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| core.join("target"));

    // 1. Swift 実行ファイル
    let executable = match opts.executable {
        Some(p) => p,
        None => {
            let frontend = repo.join("frontend-macos");
            run_cmd(
                Command::new("swift")
                    .args(["build", "-c", "release", "--product", &opts.swift_target])
                    .current_dir(&frontend),
                "swift build",
            )?;
            let out = Command::new("swift")
                .args(["build", "-c", "release", "--show-bin-path"])
                .current_dir(&frontend)
                .output()
                .map_err(|e| format!("swift build --show-bin-path の実行に失敗しました: {e}"))?;
            let bin_dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            PathBuf::from(bin_dir).join(&opts.swift_target)
        }
    };
    if !executable.is_file() {
        return Err(format!(
            "実行ファイルが見つかりません: {}",
            executable.display()
        ));
    }

    // 2. imserver
    let mut build = Command::new("cargo");
    build
        .args([
            "build",
            "--release",
            "-p",
            "nagi-server",
            "--bin",
            "imserver",
        ])
        .current_dir(&core);
    if opts.llama {
        build.args(["--features", "llama"]);
    }
    run_cmd(&mut build, "cargo build imserver")?;
    let imserver = target_dir.join("release").join(exe_name("imserver"));
    if !imserver.is_file() {
        return Err(format!("imserver が見つかりません: {}", imserver.display()));
    }

    // 3. .app の組み立て
    let out_dir = target_dir.join("macos-bundle");
    let app = out_dir.join("Nagi.app");
    if app.exists() {
        fs::remove_dir_all(&app).map_err(|e| format!("既存の .app の削除に失敗しました: {e}"))?;
    }
    let macos_dir = app.join("Contents/MacOS");
    fs::create_dir_all(&macos_dir).map_err(|e| format!("ディレクトリの作成に失敗しました: {e}"))?;
    let exe_file_name = executable
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("実行ファイル名が不正です")?
        .to_string();
    fs::copy(&executable, macos_dir.join(&exe_file_name))
        .map_err(|e| format!("実行ファイルのコピーに失敗しました: {e}"))?;
    // imserver はメインの実行ファイルと同じ Contents/MacOS に置く
    // （--deep 署名の対象になり、フロントは実行ファイルの隣として解決できる）
    fs::copy(&imserver, macos_dir.join("imserver"))
        .map_err(|e| format!("imserver のコピーに失敗しました: {e}"))?;
    fs::write(
        app.join("Contents/Info.plist"),
        info_plist(&exe_file_name, env!("CARGO_PKG_VERSION")),
    )
    .map_err(|e| format!("Info.plist の書き込みに失敗しました: {e}"))?;

    // 4. 署名・zip（macOS のみ）
    let zip = out_dir.join("Nagi.app.zip");
    if cfg!(target_os = "macos") {
        run_cmd(
            Command::new("codesign")
                .args(["--force", "--deep", "-s", "-"])
                .arg(&app),
            "codesign",
        )?;
        if zip.exists() {
            let _ = fs::remove_file(&zip);
        }
        run_cmd(
            Command::new("ditto")
                .args(["-c", "-k", "--keepParent"])
                .arg(&app)
                .arg(&zip),
            "ditto",
        )?;
        println!("✓ zip を作成しました: {}", zip.display());
    } else {
        eprintln!("警告: macOS 以外のため ad-hoc 署名と zip 化をスキップしました");
    }

    println!("✓ .app を作成しました: {}", app.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_has_required_keys() {
        let p = info_plist("NagiIMEApp", "0.0.1");
        for needle in [
            "<string>com.shiki.nagi</string>",
            "<key>LSBackgroundOnly</key>\n\t<true/>",
            "<key>LSUIElement</key>\n\t<true/>",
            "<key>InputMethodConnectionName</key>\n\t<string>com.shiki.nagi_Connection</string>",
            "<key>tsInputMethodNameKey</key>",
            "<key>tsInputMethodCharacterRepertoireKey</key>\n\t<array>\n\t\t<string>ja</string>",
            "<key>InputMethodServerControllerClass</key>\n\t<string>NagiInputController</string>",
            "<key>InputMethodServerDelegateClass</key>",
            "<key>LSMinimumSystemVersion</key>\n\t<string>13.0</string>",
            "<key>CFBundleExecutable</key>\n\t<string>NagiIMEApp</string>",
        ] {
            assert!(p.contains(needle), "missing: {needle}");
        }
    }

    #[test]
    fn plist_escapes_xml() {
        assert!(info_plist("A&B", "1<2").contains("A&amp;B"));
    }
}
