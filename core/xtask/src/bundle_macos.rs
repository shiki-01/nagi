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
/// 入力モード ID。
const INPUT_MODE_ID: &str = "com.shiki.nagi.Japanese";
/// アイコン画像のファイル名（Contents/Resources に置く）。
const ICON_FILE: &str = "nagi-icon.png";
/// アイコンの一辺（px）。
const ICON_SIZE: u32 = 64;
/// アイコンの色（RGB。落ち着いた青緑）。
const ICON_COLOR: [u8; 3] = [0x3A, 0x7C, 0x89];

/// CRC32（IEEE, PNG 用）。
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Adler-32（zlib 用）。
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// 非圧縮（stored ブロック）の zlib ストリームにする。
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(c) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        let len = c.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// 単色の角丸風正方形（RGBA 8bit, 四隅は透明）を PNG にエンコードする。
pub fn icon_png(size: u32, rgb: [u8; 3]) -> Vec<u8> {
    let radius = size / 5;
    let mut raw = Vec::with_capacity(((size * 4 + 1) * size) as usize);
    for y in 0..size {
        raw.push(0); // フィルタなし
        for x in 0..size {
            // 角の円の中心からの距離で角丸を判定する
            let cx = if x < radius {
                radius - 1 - x
            } else {
                x.saturating_sub(size - radius)
            } as i64;
            let cy = if y < radius {
                radius - 1 - y
            } else {
                y.saturating_sub(size - radius)
            } as i64;
            let r = radius as i64;
            let inside = cx * cx + cy * cy <= r * r;
            raw.extend_from_slice(&[rgb[0], rgb[1], rgb[2], if inside { 255 } else { 0 }]);
        }
    }
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8bit, RGBA, deflate, filter 0, interlace なし
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    png_chunk(&mut out, b"IEND", &[]);
    out
}

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
	<key>ComponentInputModeDict</key>
	<dict>
		<key>tsInputModeListKey</key>
		<dict>
			<key>{mode_id}</key>
			<dict>
				<key>TISInputSourceID</key><string>{mode_id}</string>
				<key>TISIntendedLanguage</key><string>ja</string>
				<key>tsInputModeScriptKey</key><string>smJapanese</string>
				<key>tsInputModeIsVisibleKey</key><true/>
				<key>tsInputModePrimaryInputModeKey</key><true/>
				<key>tsInputModeDefaultsToTransparentKey</key><false/>
				<key>TISIconIsTemplate</key><false/>
				<key>tsInputModeMenuIconFileKey</key><string>{icon}</string>
				<key>tsInputModePaletteIconFileKey</key><string>{icon}</string>
				<key>tsInputModeAlternateMenuIconFileKey</key><string>{icon}</string>
			</dict>
		</dict>
		<key>tsVisibleInputModeOrderedArrayKey</key>
		<array><string>{mode_id}</string></array>
	</dict>
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
	<key>tsInputMethodIconFileKey</key>
	<string>{icon}</string>
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
        mode_id = INPUT_MODE_ID,
        min = MIN_MACOS,
        conn = CONNECTION_NAME,
        icon = ICON_FILE,
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
    let resources_dir = app.join("Contents/Resources");
    fs::create_dir_all(&resources_dir)
        .map_err(|e| format!("ディレクトリの作成に失敗しました: {e}"))?;
    fs::write(
        resources_dir.join(ICON_FILE),
        icon_png(ICON_SIZE, ICON_COLOR),
    )
    .map_err(|e| format!("アイコンの書き込みに失敗しました: {e}"))?;

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
            "<key>ComponentInputModeDict</key>",
            "<key>tsInputModeListKey</key>",
            "<key>com.shiki.nagi.Japanese</key>",
            "<key>tsVisibleInputModeOrderedArrayKey</key>",
            "<string>com.shiki.nagi.Japanese</string>",
            "<key>LSBackgroundOnly</key>\n\t<true/>",
            "<key>LSUIElement</key>\n\t<true/>",
            "<key>InputMethodConnectionName</key>\n\t<string>com.shiki.nagi_Connection</string>",
            "<key>tsInputMethodNameKey</key>",
            "<key>tsInputMethodCharacterRepertoireKey</key>\n\t<array>\n\t\t<string>ja</string>",
            "<key>InputMethodServerControllerClass</key>\n\t<string>NagiInputController</string>",
            "<key>InputMethodServerDelegateClass</key>",
            "<key>LSMinimumSystemVersion</key>\n\t<string>13.0</string>",
            "<key>CFBundleExecutable</key>\n\t<string>NagiIMEApp</string>",
            "<key>tsInputMethodIconFileKey</key>\n\t<string>nagi-icon.png</string>",
            "<key>TISIconIsTemplate</key><false/>",
            "<key>tsInputModeMenuIconFileKey</key><string>nagi-icon.png</string>",
            "<key>tsInputModePaletteIconFileKey</key><string>nagi-icon.png</string>",
            "<key>tsInputModeAlternateMenuIconFileKey</key><string>nagi-icon.png</string>",
        ] {
            assert!(p.contains(needle), "missing: {needle}");
        }
    }

    #[test]
    fn png_structure_is_valid() {
        let png = icon_png(ICON_SIZE, ICON_COLOR);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // IHDR: 長さ 13、種別、64x64、8bit RGBA
        assert_eq!(&png[8..12], &13u32.to_be_bytes());
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &ICON_SIZE.to_be_bytes());
        assert_eq!(&png[20..24], &ICON_SIZE.to_be_bytes());
        assert_eq!(&png[24..29], &[8, 6, 0, 0, 0]);
        // 全チャンクの CRC を検証し、IEND で終わること
        let mut pos = 8;
        let mut kinds = Vec::new();
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let body = &png[pos + 4..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            assert_eq!(crc32(body), crc, "CRC mismatch");
            kinds.push(body[..4].to_vec());
            pos += 12 + len;
        }
        assert_eq!(pos, png.len());
        assert_eq!(kinds.last().unwrap(), b"IEND");
    }

    #[test]
    fn crc32_and_adler32_known_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn plist_escapes_xml() {
        assert!(info_plist("A&B", "1<2").contains("A&amp;B"));
    }
}
