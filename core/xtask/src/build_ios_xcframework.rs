//! `cargo xtask build-ios-xcframework` — iOS 用 XCFramework を生成する。
//!
//! aarch64-apple-ios（実機）と aarch64-apple-ios-sim（シミュレータ）向けに nagi-ffi をビルドし、
//! xcodebuild で統合した XCFramework を生成する。macOS + Xcode が必須。

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Options {
    pub llama: bool,
    pub out_dir: Option<PathBuf>,
}

/// 現在のプラットフォームが macOS か確認する
fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

/// cargo の --release フラグ付きビルドコマンドを実行する
fn run_cargo_build(target: &str, features: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--release", "-p", "nagi-ffi", "--target", target]);

    if !features.is_empty() {
        cmd.arg("--features");
        cmd.arg(features.join(","));
    }

    let status = cmd
        .status()
        .map_err(|e| format!("cargo build を実行できません: {e}"))?;

    if !status.success() {
        return Err(format!(
            "cargo build が失敗しました（ターゲット: {target}）"
        ));
    }

    Ok(())
}

/// 両 iOS ターゲット向けにビルドする
fn build_ios_targets(llama: bool) -> Result<(), String> {
    let targets = ["aarch64-apple-ios", "aarch64-apple-ios-sim"];
    let features_strs: Vec<&str> = if llama { vec!["llama"] } else { vec![] };

    for target in &targets {
        eprintln!("xtask: {} をビルド中...", target);
        run_cargo_build(target, &features_strs)?;
    }

    Ok(())
}

/// ビルド出力から .a ファイルのパスを取得する
fn get_library_path(target: &str) -> Result<PathBuf, String> {
    let root =
        std::env::current_dir().map_err(|e| format!("現在のディレクトリが取得できません: {e}"))?;
    let lib_path = root
        .join("target")
        .join(target)
        .join("release")
        .join("libnagi_ffi.a");

    if !lib_path.exists() {
        return Err(format!(
            "ライブラリが見つかりません: {}",
            lib_path.display()
        ));
    }

    Ok(lib_path)
}

/// xcodebuild で xcframework を生成する
fn build_xcframework(
    lib_ios: &Path,
    lib_sim: &Path,
    headers_dir: &Path,
    out_dir: &Path,
) -> Result<(), String> {
    let xcframework_path = out_dir.join("NagiCore.xcframework");

    // 既存を削除（再生成の場合）
    if xcframework_path.exists() {
        std::fs::remove_dir_all(&xcframework_path)
            .map_err(|e| format!("既存の xcframework を削除できません: {e}"))?;
    }

    // xcodebuild コマンド実行
    let status = Command::new("xcodebuild")
        .arg("-create-xcframework")
        .arg("-library")
        .arg(lib_ios)
        .arg("-headers")
        .arg(headers_dir)
        .arg("-library")
        .arg(lib_sim)
        .arg("-headers")
        .arg(headers_dir)
        .arg("-output")
        .arg(&xcframework_path)
        .status()
        .map_err(|e| format!("xcodebuild を実行できません: {e}"))?;

    if !status.success() {
        return Err("xcodebuild xcframework 生成が失敗しました".to_string());
    }

    eprintln!(
        "xtask: XCFramework を生成しました: {}",
        xcframework_path.display()
    );
    Ok(())
}

pub fn run(options: Options) -> Result<(), String> {
    // macOS + Xcode チェック
    if !is_macos() {
        return Err(
            "build-ios-xcframework は macOS + Xcode が必須です（Windows / Linux では実行不可）"
                .to_string(),
        );
    }

    // xcodebuild が利用可能か確認
    let xcodebuild_check = Command::new("xcodebuild")
        .arg("-version")
        .status()
        .map_err(|_| {
            "Xcode がインストールされていません（xcodebuild が見つかりません）".to_string()
        })?;

    if !xcodebuild_check.success() {
        return Err("Xcode のセットアップが不完全です（xcodebuild が実行できません）".to_string());
    }

    // iOS ターゲットがインストール済みか確認（rustup による仮のチェック）
    eprintln!("xtask: iOS ターゲットをチェック中...");
    let check_status = Command::new("rustc")
        .args(["--target", "aarch64-apple-ios", "--print", "sysroot"])
        .status();

    if check_status.is_err() {
        return Err(
            "iOS ターゲットがインストールされていません。\n  rustup target add aarch64-apple-ios aarch64-apple-ios-sim を実行してください".to_string(),
        );
    }

    // ビルド
    eprintln!("xtask: iOS 用 nagi-ffi をビルド中...");
    build_ios_targets(options.llama)?;

    // ライブラリパス取得
    let lib_ios = get_library_path("aarch64-apple-ios")?;
    let lib_sim = get_library_path("aarch64-apple-ios-sim")?;

    // ヘッダディレクトリ
    let headers_dir = {
        let root = std::env::current_dir()
            .map_err(|e| format!("現在のディレクトリが取得できません: {e}"))?;
        root.join("ffi").join("include")
    };

    if !headers_dir.exists() {
        return Err(format!(
            "ヘッダディレクトリが見つかりません: {}",
            headers_dir.display()
        ));
    }

    // 出力ディレクトリ
    let out_dir = options.out_dir.unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|d| d.join("..").join("frontend-ios-spike"))
            .unwrap_or_else(|| PathBuf::from("../frontend-ios-spike"))
    });

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("出力ディレクトリを作成できません: {e}"))?;

    // XCFramework 生成
    eprintln!("xtask: XCFramework を生成中...");
    build_xcframework(&lib_ios, &lib_sim, &headers_dir, &out_dir)?;

    eprintln!(
        "xtask: 完了。XCFramework: {}/NagiCore.xcframework",
        out_dir.display()
    );
    Ok(())
}
