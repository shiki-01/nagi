//! `cargo xtask bundle-macos` — `Nagi.app` を組み立てて ad-hoc 署名し、zip 化する。
//!
//! 配布は Apple Developer Program 非加入のため ad-hoc 署名のみ（設計書 §5.2）。
//! 署名と zip 化は macOS 上でのみ行い、他の OS では警告してスキップする。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 既定のバンドル識別子。
#[cfg(test)]
// macOS が入力メソッドとして認識する（TIS に登録される）には `.inputmethod.` を含む必要がある。
// 設定/データの識別子（`com.shiki.nagi`）とは別物。
const DEFAULT_BUNDLE_ID: &str = "com.shiki.inputmethod.Nagi";
/// 最小サポート macOS。
const MIN_MACOS: &str = "13.0";
/// アイコン画像のファイル名（Contents/Resources に置く。Info.plist が参照する）。
const ICON_FILE: &str = "nagi-icon.tiff";
/// PNG 版アイコンのファイル名（参考用に同梱）。
const ICON_PNG_FILE: &str = "nagi-icon.png";
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
    let pixels = icon_rgba(size, rgb);
    let mut raw = Vec::with_capacity(((size * 4 + 1) * size) as usize);
    for row in pixels.chunks((size * 4) as usize) {
        raw.push(0); // フィルタなし
        raw.extend_from_slice(row);
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

/// 単色の角丸正方形の RGBA 8bit ピクセル列（PNG / TIFF 共通）。
fn icon_rgba(size: u32, rgb: [u8; 3]) -> Vec<u8> {
    let radius = size / 5;
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
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
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], if inside { 255 } else { 0 }]);
        }
    }
    px
}

/// 非圧縮 RGBA（リトルエンディアン、単一ストリップ）の最小 TIFF にエンコードする。
///
/// レイアウト: ヘッダ 8B / IFD（10 エントリ）/ BitsPerSample 配列 / ピクセルデータ。
pub fn icon_tiff(size: u32, rgb: [u8; 3]) -> Vec<u8> {
    const ENTRIES: u16 = 10;
    const IFD_OFFSET: u32 = 8;
    let ifd_len = 2 + ENTRIES as u32 * 12 + 4;
    let bps_offset = IFD_OFFSET + ifd_len;
    let data_offset = bps_offset + 8;
    let pixels = icon_rgba(size, rgb);

    let mut out = vec![b'I', b'I', 42, 0];
    out.extend_from_slice(&IFD_OFFSET.to_le_bytes());
    out.extend_from_slice(&ENTRIES.to_le_bytes());
    // (tag, (type(3=SHORT,4=LONG), count, value))。タグ昇順
    let short = |v: u32| (3u16, 1u32, v);
    let long = |v: u32| (4u16, 1u32, v);
    let entries: [(u16, (u16, u32, u32)); 10] = [
        (256, long(size)),                // ImageWidth
        (257, long(size)),                // ImageLength
        (258, (3, 4, bps_offset)),        // BitsPerSample(8,8,8,8)
        (259, short(1)),                  // Compression: なし
        (262, short(2)),                  // Photometric: RGB
        (273, long(data_offset)),         // StripOffsets
        (277, short(4)),                  // SamplesPerPixel
        (278, long(size)),                // RowsPerStrip
        (279, long(pixels.len() as u32)), // StripByteCounts
        (338, short(2)),                  // ExtraSamples: unassociated alpha
    ];
    for (tag, (ty, count, value)) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        // SHORT 値は値欄の下位 2 バイトに左詰め（LE ではそのまま u32 で書けばよい）
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // 次の IFD なし
    for _ in 0..4 {
        out.extend_from_slice(&8u16.to_le_bytes());
    }
    out.extend_from_slice(&pixels);
    out
}

/// 入力モード ID（`<bundle-id>.Japanese`）。
pub fn input_mode_id(bundle_id: &str) -> String {
    format!("{bundle_id}.Japanese")
}

/// IMKServer の接続名（`<bundle-id>_Connection`）。
pub fn connection_name(bundle_id: &str) -> String {
    format!("{bundle_id}_Connection")
}

/// `bundle-macos` のオプション。
pub struct Options {
    /// Swift 実行ファイルのターゲット名。
    pub swift_target: String,
    /// 指定時は `swift build` を行わず、このファイルを実行ファイルとして使う。
    pub executable: Option<PathBuf>,
    /// imserver を `--features llama` でビルドする。
    pub llama: bool,
    /// バンドル識別子（モード ID・接続名もここから派生する）。
    pub bundle_id: String,
    /// 出力先ディレクトリ。未指定は `<target>/macos-bundle`。
    pub out_dir: Option<PathBuf>,
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
pub fn info_plist(executable: &str, version: &str, bundle_id: &str) -> String {
    let e = xml_escape;
    let mode_id = e(&input_mode_id(bundle_id));
    let conn = e(&connection_name(bundle_id));
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
	<key>CFBundleDisplayName</key>
	<string>Nagi</string>
	<key>CFBundleName</key>
	<string>Nagi</string>
	<key>NSPrincipalClass</key>
	<string>NSApplication</string>
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
				<key>tsInputModeDefaultStateKey</key><true/>
				<key>tsInputModePrimaryInScriptKey</key><true/>
				<key>tsInputModeJISKeyboardShortcutKey</key><integer>1</integer>
				<key>tsInputModeDefaultsToTransparentKey</key><false/>
				<key>tsInputModeMenuIconFileKey</key><string>{icon}</string>
				<key>tsInputModePaletteIconFileKey</key><string>{icon}</string>
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
	<key>TISIntendedLanguage</key>
	<string>ja</string>
	<key>TISIconIsTemplate</key>
	<false/>
	<key>tsInputMethodNameKey</key>
	<string>Nagi</string>
	<key>tsInputMethodIconFileKey</key>
	<string>{icon}</string>
	<key>tsInputMethodCharacterRepertoireKey</key>
	<array>
		<string>Latn</string>
		<string>Hira</string>
		<string>Kana</string>
	</array>
</dict>
</plist>
"#,
        exe = e(executable),
        id = e(bundle_id),
        ver = e(version),
        mode_id = mode_id,
        min = MIN_MACOS,
        conn = conn,
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
    let out_dir = opts
        .out_dir
        .unwrap_or_else(|| target_dir.join("macos-bundle"));
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
        info_plist(&exe_file_name, env!("CARGO_PKG_VERSION"), &opts.bundle_id),
    )
    .map_err(|e| format!("Info.plist の書き込みに失敗しました: {e}"))?;
    let resources_dir = app.join("Contents/Resources");
    fs::create_dir_all(&resources_dir)
        .map_err(|e| format!("ディレクトリの作成に失敗しました: {e}"))?;
    fs::write(
        resources_dir.join(ICON_FILE),
        icon_tiff(ICON_SIZE, ICON_COLOR),
    )
    .map_err(|e| format!("アイコンの書き込みに失敗しました: {e}"))?;
    fs::write(
        resources_dir.join(ICON_PNG_FILE),
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

    fn plist() -> String {
        info_plist("NagiIMEApp", "0.0.1", DEFAULT_BUNDLE_ID)
    }

    #[test]
    fn plist_has_required_keys() {
        let p = plist();
        for needle in [
            "<string>com.shiki.inputmethod.Nagi</string>",
            "<key>ComponentInputModeDict</key>",
            "<key>tsInputModeListKey</key>",
            "<key>com.shiki.inputmethod.Nagi.Japanese</key>",
            "<key>tsVisibleInputModeOrderedArrayKey</key>",
            "<string>com.shiki.inputmethod.Nagi.Japanese</string>",
            "<key>LSBackgroundOnly</key>\n\t<true/>",
            "<key>LSUIElement</key>\n\t<true/>",
            "<key>InputMethodConnectionName</key>\n\t<string>com.shiki.inputmethod.Nagi_Connection</string>",
            "<key>tsInputMethodNameKey</key>",
            "<key>tsInputMethodCharacterRepertoireKey</key>\n\t<array>\n\t\t<string>Latn</string>\n\t\t<string>Hira</string>\n\t\t<string>Kana</string>",
            "<key>InputMethodServerControllerClass</key>\n\t<string>NagiInputController</string>",
            "<key>LSMinimumSystemVersion</key>\n\t<string>13.0</string>",
            "<key>CFBundleExecutable</key>\n\t<string>NagiIMEApp</string>",
            "<key>CFBundleDisplayName</key>\n\t<string>Nagi</string>",
            "<key>NSPrincipalClass</key>\n\t<string>NSApplication</string>",
            "<key>TISIntendedLanguage</key>\n\t<string>ja</string>",
            "<key>TISIconIsTemplate</key>\n\t<false/>",
            "<key>tsInputMethodIconFileKey</key>\n\t<string>nagi-icon.tiff</string>",
            "<key>tsInputModeDefaultStateKey</key><true/>",
            "<key>tsInputModePrimaryInScriptKey</key><true/>",
            "<key>tsInputModeJISKeyboardShortcutKey</key><integer>1</integer>",
            "<key>tsInputModeMenuIconFileKey</key><string>nagi-icon.tiff</string>",
            "<key>tsInputModePaletteIconFileKey</key><string>nagi-icon.tiff</string>",
        ] {
            assert!(p.contains(needle), "missing: {needle}");
        }
    }

    #[test]
    fn plist_omits_removed_keys() {
        let p = plist();
        for needle in [
            "tsInputModePrimaryInputModeKey",
            "tsInputModeAlternateMenuIconFileKey",
            "InputMethodServerDelegateClass",
            "<key>TISIconIsTemplate</key><false/>",
        ] {
            assert!(!p.contains(needle), "unexpected: {needle}");
        }
    }

    #[test]
    fn plist_derives_ids_from_bundle_id() {
        let p = info_plist("X", "1", "com.shiki.inputmethod.Nagi");
        for needle in [
            "<key>CFBundleIdentifier</key>\n\t<string>com.shiki.inputmethod.Nagi</string>",
            "<key>com.shiki.inputmethod.Nagi.Japanese</key>",
            "<key>TISInputSourceID</key><string>com.shiki.inputmethod.Nagi.Japanese</string>",
            "<string>com.shiki.inputmethod.Nagi_Connection</string>",
        ] {
            assert!(p.contains(needle), "missing: {needle}");
        }
        assert!(!p.contains("com.shiki.nagi"));
    }

    #[test]
    fn tiff_header_and_ifd_are_valid() {
        let t = icon_tiff(ICON_SIZE, ICON_COLOR);
        let u16_at = |o: usize| u16::from_le_bytes(t[o..o + 2].try_into().unwrap());
        let u32_at = |o: usize| u32::from_le_bytes(t[o..o + 4].try_into().unwrap());
        assert_eq!(&t[..4], &[b'I', b'I', 42, 0]);
        let ifd = u32_at(4) as usize;
        assert_eq!(ifd, 8);
        let n = u16_at(ifd) as usize;
        assert_eq!(n, 10);
        let entry = |tag: u16| {
            (0..n)
                .map(|i| ifd + 2 + i * 12)
                .find(|&o| u16_at(o) == tag)
                .unwrap_or_else(|| panic!("tag {tag} missing"))
        };
        let size = ICON_SIZE;
        assert_eq!(u32_at(entry(256) + 8), size);
        assert_eq!(u32_at(entry(257) + 8), size);
        assert_eq!(u16_at(entry(259) + 8), 1);
        assert_eq!(u16_at(entry(262) + 8), 2);
        assert_eq!(u16_at(entry(277) + 8), 4);
        assert_eq!(u32_at(entry(278) + 8), size);
        assert_eq!(u16_at(entry(338) + 8), 2);
        // タグ昇順
        let tags: Vec<u16> = (0..n).map(|i| u16_at(ifd + 2 + i * 12)).collect();
        assert!(tags.windows(2).all(|w| w[0] < w[1]));
        // 次の IFD なし
        assert_eq!(u32_at(ifd + 2 + n * 12), 0);
        // BitsPerSample は外部配列 (8,8,8,8)
        let bps = entry(258);
        assert_eq!((u16_at(bps + 2), u32_at(bps + 4)), (3, 4));
        let bps_off = u32_at(bps + 8) as usize;
        assert_eq!(
            (0..4).map(|i| u16_at(bps_off + i * 2)).collect::<Vec<_>>(),
            vec![8; 4]
        );
        // ストリップがファイル末尾までを過不足なく覆う
        let off = u32_at(entry(273) + 8) as usize;
        let cnt = u32_at(entry(279) + 8) as usize;
        assert_eq!(cnt, (size * size * 4) as usize);
        assert_eq!(off + cnt, t.len());
        // 角は透明、中心は不透明で指定色
        assert_eq!(t[off + 3], 0);
        let mid = off + ((size / 2 * size + size / 2) * 4) as usize;
        assert_eq!(
            &t[mid..mid + 4],
            &[ICON_COLOR[0], ICON_COLOR[1], ICON_COLOR[2], 255]
        );
    }

    #[test]
    fn derived_ids() {
        assert_eq!(input_mode_id("a.b"), "a.b.Japanese");
        assert_eq!(connection_name("a.b"), "a.b_Connection");
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
        assert!(info_plist("A&B", "1<2", "x").contains("A&amp;B"));
    }
}
