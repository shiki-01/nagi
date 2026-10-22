//! `cargo xtask bundle-macos` — `Nagi.app` を組み立てて ad-hoc 署名し、zip 化する。
//!
//! Apple Developer Program 加入済みだが、現状は ad-hoc 署名（Developer ID 署名 + 公証は検討可。`docs/decisions.md` H-15）。
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
/// アイコンの色（RGB。落ち着いた青緑）。
const ICON_COLOR: [u8; 3] = [0x3A, 0x7C, 0x89];
/// 英数モードのアイコン画像のファイル名。
const ROMAN_ICON_FILE: &str = "nagi-icon-roman.tiff";
/// 英数モードのアイコンの色（RGB。灰色。ひらがなモードと見分けるため）。
const ROMAN_ICON_COLOR: [u8; 3] = [0x80, 0x80, 0x80];

/// 単色の角丸正方形の RGBA 8bit ピクセル列（四隅は透明）。
fn icon_rgba(size: u32, rgb: [u8; 3]) -> Vec<u8> {
    let radius = size / 5;
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            // 角の円の中心からピクセル中心までの距離で角丸を判定する（座標は 2 倍の整数）
            let corner = |v: u32| -> i64 {
                if v < radius {
                    2 * radius as i64 - (2 * v as i64 + 1)
                } else if v >= size - radius {
                    2 * v as i64 + 1 - 2 * (size - radius) as i64
                } else {
                    0
                }
            };
            let (dx, dy) = (corner(x), corner(y));
            let r2 = 2 * radius as i64;
            let inside = dx * dx + dy * dy <= r2 * r2;
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], if inside { 255 } else { 0 }]);
        }
    }
    px
}

/// TIFF 1 ページ分の指定（一辺 px と解像度 dpi）。
#[derive(Clone, Copy)]
pub struct TiffPage {
    pub size: u32,
    pub dpi: u32,
}

/// 16pt アイコン用のページ構成（1x = 16px @72dpi、2x = 32px @144dpi）。
/// macOS は解像度からポイントサイズを決めるため、dpi を持たせないと px がそのまま pt 扱いになる。
const ICON_PAGES: [TiffPage; 2] = [
    TiffPage { size: 16, dpi: 72 },
    TiffPage { size: 32, dpi: 144 },
];

/// 1 ページあたりの IFD エントリ数。
const IFD_ENTRIES: u16 = 13;

/// 非圧縮 RGBA（リトルエンディアン、ページごとに単一ストリップ）のマルチページ TIFF にエンコードする。
///
/// ページごとのレイアウト: IFD（13 エントリ）/ BitsPerSample 配列 / XResolution / YResolution / ピクセルデータ。
/// 各 IFD の NextIFD で次ページに連結する。
pub fn icon_tiff(pages: &[TiffPage], rgb: [u8; 3]) -> Vec<u8> {
    let mut out = vec![b'I', b'I', 42, 0];
    out.extend_from_slice(&8u32.to_le_bytes()); // 最初の IFD
    for (i, page) in pages.iter().enumerate() {
        let ifd_offset = out.len() as u32;
        let ifd_len = 2 + IFD_ENTRIES as u32 * 12 + 4;
        let bps_offset = ifd_offset + ifd_len;
        let xres_offset = bps_offset + 8;
        let yres_offset = xres_offset + 8;
        let data_offset = yres_offset + 8;
        let pixels = icon_rgba(page.size, rgb);
        let next_ifd = if i + 1 < pages.len() {
            data_offset + pixels.len() as u32
        } else {
            0
        };
        let size = page.size;

        out.extend_from_slice(&IFD_ENTRIES.to_le_bytes());
        // (tag, (type(3=SHORT,4=LONG,5=RATIONAL), count, value))。タグ昇順
        let short = |v: u32| (3u16, 1u32, v);
        let long = |v: u32| (4u16, 1u32, v);
        let entries: [(u16, (u16, u32, u32)); IFD_ENTRIES as usize] = [
            (256, long(size)),                // ImageWidth
            (257, long(size)),                // ImageLength
            (258, (3, 4, bps_offset)),        // BitsPerSample(8,8,8,8)
            (259, short(1)),                  // Compression: なし
            (262, short(2)),                  // Photometric: RGB
            (273, long(data_offset)),         // StripOffsets
            (277, short(4)),                  // SamplesPerPixel
            (278, long(size)),                // RowsPerStrip
            (279, long(pixels.len() as u32)), // StripByteCounts
            (282, (5, 1, xres_offset)),       // XResolution
            (283, (5, 1, yres_offset)),       // YResolution
            (296, short(2)),                  // ResolutionUnit: inch
            (338, short(2)),                  // ExtraSamples: unassociated alpha
        ];
        for (tag, (ty, count, value)) in entries {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&ty.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            // SHORT 値は値欄の下位 2 バイトに左詰め（LE ではそのまま u32 で書けばよい）
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&next_ifd.to_le_bytes());
        for _ in 0..4 {
            out.extend_from_slice(&8u16.to_le_bytes());
        }
        for _ in 0..2 {
            // 解像度は RATIONAL（分子 dpi / 分母 1）
            out.extend_from_slice(&page.dpi.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
        }
        out.extend_from_slice(&pixels);
    }
    out
}

/// 入力モード ID（`<bundle-id>.Japanese`）。コアのひらがなモードに対応する。
pub fn input_mode_id(bundle_id: &str) -> String {
    format!("{bundle_id}.Japanese")
}

/// 英数モードの入力モード ID（`<bundle-id>.Roman`）。コアの英数モードに対応する。
pub fn roman_mode_id(bundle_id: &str) -> String {
    format!("{bundle_id}.Roman")
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
    let roman_id = e(&roman_mode_id(bundle_id));
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
			<key>{roman_id}</key>
			<dict>
				<key>TISInputSourceID</key><string>{roman_id}</string>
				<key>TISIntendedLanguage</key><string>en</string>
				<key>tsInputModeScriptKey</key><string>smRoman</string>
				<key>tsInputModeIsVisibleKey</key><true/>
				<key>tsInputModeDefaultStateKey</key><true/>
				<key>tsInputModePrimaryInScriptKey</key><true/>
				<key>tsInputModeJISKeyboardShortcutKey</key><integer>0</integer>
				<key>tsInputModeDefaultsToTransparentKey</key><false/>
				<key>tsInputModeMenuIconFileKey</key><string>{roman_icon}</string>
				<key>tsInputModePaletteIconFileKey</key><string>{roman_icon}</string>
			</dict>
		</dict>
		<key>tsVisibleInputModeOrderedArrayKey</key>
		<array><string>{mode_id}</string><string>{roman_id}</string></array>
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
        roman_icon = ROMAN_ICON_FILE,
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
        icon_tiff(&ICON_PAGES, ICON_COLOR),
    )
    .map_err(|e| format!("アイコンの書き込みに失敗しました: {e}"))?;
    fs::write(
        resources_dir.join(ROMAN_ICON_FILE),
        icon_tiff(&ICON_PAGES, ROMAN_ICON_COLOR),
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
            "<key>com.shiki.inputmethod.Nagi.Roman</key>",
            "<key>tsInputModeScriptKey</key><string>smRoman</string>",
            "<key>tsInputModeJISKeyboardShortcutKey</key><integer>0</integer>",
            "<key>tsInputModeMenuIconFileKey</key><string>nagi-icon-roman.tiff</string>",
            "<array><string>com.shiki.inputmethod.Nagi.Japanese</string><string>com.shiki.inputmethod.Nagi.Roman</string></array>",
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
            "<key>TISInputSourceID</key><string>com.shiki.inputmethod.Nagi.Roman</string>",
            "<string>com.shiki.inputmethod.Nagi_Connection</string>",
        ] {
            assert!(p.contains(needle), "missing: {needle}");
        }
        assert!(!p.contains("com.shiki.nagi"));
    }

    /// テスト用の TIFF リーダ。
    struct Tiff<'a>(&'a [u8]);
    impl Tiff<'_> {
        fn u16_at(&self, o: usize) -> u16 {
            u16::from_le_bytes(self.0[o..o + 2].try_into().unwrap())
        }
        fn u32_at(&self, o: usize) -> u32 {
            u32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
        }
        /// IFD 内のタグのエントリ先頭オフセット。
        fn entry(&self, ifd: usize, tag: u16) -> usize {
            let n = self.u16_at(ifd) as usize;
            (0..n)
                .map(|i| ifd + 2 + i * 12)
                .find(|&o| self.u16_at(o) == tag)
                .unwrap_or_else(|| panic!("tag {tag} missing"))
        }
        fn rational(&self, ifd: usize, tag: u16) -> (u32, u32) {
            let e = self.entry(ifd, tag);
            assert_eq!((self.u16_at(e + 2), self.u32_at(e + 4)), (5, 1));
            let o = self.u32_at(e + 8) as usize;
            (self.u32_at(o), self.u32_at(o + 4))
        }
    }

    #[test]
    fn tiff_has_two_linked_pages_with_resolution() {
        let t = icon_tiff(&ICON_PAGES, ICON_COLOR);
        let r = Tiff(&t);
        assert_eq!(&t[..4], &[b'I', b'I', 42, 0]);
        let mut ifd = r.u32_at(4) as usize;
        assert_eq!(ifd, 8);
        let expected = [(16u32, 72u32), (32, 144)];
        for (page, (size, dpi)) in expected.into_iter().enumerate() {
            let n = r.u16_at(ifd) as usize;
            assert_eq!(n, IFD_ENTRIES as usize);
            let entry = |tag: u16| r.entry(ifd, tag);
            assert_eq!(r.u32_at(entry(256) + 8), size);
            assert_eq!(r.u32_at(entry(257) + 8), size);
            assert_eq!(r.u16_at(entry(259) + 8), 1);
            assert_eq!(r.u16_at(entry(262) + 8), 2);
            assert_eq!(r.u16_at(entry(277) + 8), 4);
            assert_eq!(r.u32_at(entry(278) + 8), size);
            assert_eq!(r.u16_at(entry(338) + 8), 2);
            // 解像度タグ
            assert_eq!(r.rational(ifd, 282), (dpi, 1));
            assert_eq!(r.rational(ifd, 283), (dpi, 1));
            assert_eq!(r.u16_at(entry(296) + 8), 2);
            // タグ昇順
            let tags: Vec<u16> = (0..n).map(|i| r.u16_at(ifd + 2 + i * 12)).collect();
            assert!(tags.windows(2).all(|w| w[0] < w[1]));
            // BitsPerSample は外部配列 (8,8,8,8)
            let bps = entry(258);
            assert_eq!((r.u16_at(bps + 2), r.u32_at(bps + 4)), (3, 4));
            let bps_off = r.u32_at(bps + 8) as usize;
            assert_eq!(
                (0..4)
                    .map(|i| r.u16_at(bps_off + i * 2))
                    .collect::<Vec<_>>(),
                vec![8; 4]
            );
            // ストリップのサイズと、角は透明・中心は不透明で指定色
            let off = r.u32_at(entry(273) + 8) as usize;
            let cnt = r.u32_at(entry(279) + 8) as usize;
            assert_eq!(cnt, (size * size * 4) as usize);
            assert_eq!(t[off + 3], 0);
            let mid = off + ((size / 2 * size + size / 2) * 4) as usize;
            assert_eq!(
                &t[mid..mid + 4],
                &[ICON_COLOR[0], ICON_COLOR[1], ICON_COLOR[2], 255]
            );
            // NextIFD: 最初のページは次ページ（ピクセルデータ直後）、最後は 0
            let next = r.u32_at(ifd + 2 + n * 12) as usize;
            if page == 0 {
                assert_eq!(next, off + cnt);
                ifd = next;
            } else {
                assert_eq!(next, 0);
                assert_eq!(off + cnt, t.len());
            }
        }
    }

    #[test]
    fn derived_ids() {
        assert_eq!(input_mode_id("a.b"), "a.b.Japanese");
        assert_eq!(roman_mode_id("a.b"), "a.b.Roman");
        assert_eq!(connection_name("a.b"), "a.b_Connection");
    }

    #[test]
    fn plist_escapes_xml() {
        assert!(info_plist("A&B", "1<2", "x").contains("A&amp;B"));
    }
}
