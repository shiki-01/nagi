//! `cargo xtask bundle-windows` — Windows 版の配布物（zip）を組み立てる。
//!
//! ビルド自体（MSVC ターゲットの `nagi_windows_tsf.dll` と `imserver.exe`）は CI の windows ランナーで
//! 行い、ここでは成果物の収集・（任意で）Authenticode 署名・チェックサム・zip 化だけを行う
//! （`docs/decisions.md` H-42）。収集と zip 化は OS 非依存で、Linux でも単体テストできる。
//!
//! zip の構成（`Nagi-windows-<version>.zip`、最上位に `Nagi-windows-<version>/` を 1 つ置く）:
//!
//! ```text
//! nagi_windows_tsf.dll   TSF の入力方式（COM の InprocServer32）
//! imserver.exe           変換エンジン
//! install.ps1 / uninstall.ps1
//! README.md              docs/windows-install.md（SmartScreen の手順を含む）
//! LICENSE-MIT / LICENSE-APACHE / NOTICE / THIRD_PARTY_LICENSES.md
//! VERSION                バージョン（x.y.z）
//! SHA256SUMS             上記すべての SHA-256（`<hex>  <相対パス>`。install.ps1 が検証する）
//! ```

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// TSF の DLL の名前（`frontend-windows-tsf` の cdylib）。
pub const DLL_FILE: &str = "nagi_windows_tsf.dll";
/// エンジンの実行ファイル名。
pub const IMSERVER_FILE: &str = "imserver.exe";
/// チェックサム一覧のファイル名。
pub const CHECKSUM_FILE: &str = "SHA256SUMS";
/// 署名用 PFX のパスワードを渡す環境変数名（コマンドラインには載せない）。
const PFX_PASSWORD_ENV: &str = "NAGI_WINDOWS_PFX_PASSWORD";
/// 既定のタイムスタンプサーバー（RFC 3161）。
const DEFAULT_TIMESTAMP_URL: &str = "http://timestamp.digicert.com";

/// `bundle-windows` のオプション。
pub struct Options {
    /// DLL のパス。未指定なら `<target>[/<triple>]/release/nagi_windows_tsf.dll`。
    pub dll: Option<PathBuf>,
    /// imserver.exe のパス。未指定なら `<target>[/<triple>]/release/imserver.exe`。
    pub imserver: Option<PathBuf>,
    /// 既定パスを探すときのターゲットトリプル（例: `x86_64-pc-windows-msvc`）。
    pub target: Option<String>,
    /// 出力先ディレクトリ。未指定は `<target>/windows-bundle`。
    pub out_dir: Option<PathBuf>,
    /// Authenticode 署名に使う PFX。指定時は DLL と exe を signtool で署名する。
    pub sign_pfx: Option<PathBuf>,
    /// signtool.exe のパス（PATH 上に無い場合）。
    pub signtool: String,
    /// タイムスタンプサーバーの URL。
    pub timestamp_url: Option<String>,
}

/// 組み立てる 1 ファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 最上位ディレクトリの下の相対パス（`/` 区切り）。
    pub name: String,
    /// 中身の取得元。
    pub source: Source,
}

/// ファイルの中身の取得元。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// 既存のファイルをコピーする。
    File(PathBuf),
    /// 生成した内容を書く。
    Bytes(Vec<u8>),
}

/// 収集に使う入力パス。
pub struct Inputs {
    pub dll: PathBuf,
    pub imserver: PathBuf,
    pub repo: PathBuf,
    pub version: String,
}

fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// zip のベース名（拡張子なし）。`Nagi-windows-<version>`。
/// アップデートの `download_url` と `windows-release.yml` はこの名前に依存する。
pub fn bundle_name(version: &str) -> String {
    format!("Nagi-windows-{version}")
}

/// 配布物に入れるファイルの一覧（`SHA256SUMS` を除く）。入力ファイルの存在もここで確かめる。
pub fn plan(inputs: &Inputs) -> Result<Vec<Entry>, String> {
    let repo = &inputs.repo;
    let file = |name: &str, path: PathBuf| Entry {
        name: name.to_string(),
        source: Source::File(path),
    };
    let entries = vec![
        file(DLL_FILE, inputs.dll.clone()),
        file(IMSERVER_FILE, inputs.imserver.clone()),
        file(
            "install.ps1",
            repo.join("frontend-windows/dist/install.ps1"),
        ),
        file(
            "uninstall.ps1",
            repo.join("frontend-windows/dist/uninstall.ps1"),
        ),
        file("README.md", repo.join("docs/windows-install.md")),
        file("LICENSE-MIT", repo.join("LICENSE-MIT")),
        file("LICENSE-APACHE", repo.join("LICENSE-APACHE")),
        file("NOTICE", repo.join("NOTICE")),
        file(
            "THIRD_PARTY_LICENSES.md",
            repo.join("THIRD_PARTY_LICENSES.md"),
        ),
        Entry {
            name: "VERSION".to_string(),
            source: Source::Bytes(format!("{}\n", inputs.version).into_bytes()),
        },
    ];
    for e in &entries {
        if let Source::File(p) = &e.source
            && !p.is_file()
        {
            return Err(format!("{} が見つかりません: {}", e.name, p.display()));
        }
    }
    Ok(entries)
}

/// 先頭が `MZ`（PE / DOS ヘッダ）か。Linux の `.so` などを取り違えて入れないための検査。
pub fn looks_like_pe(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MZ")
}

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// PowerShell スクリプトを UTF-8 BOM 付きにする（Windows PowerShell 5.1 は BOM が無いと
/// 日本語を ANSI として読み、文字化けする）。リポジトリのソースは BOM 無しで持つ。
pub fn with_utf8_bom(bytes: &[u8]) -> Vec<u8> {
    if bytes.starts_with(UTF8_BOM) {
        return bytes.to_vec();
    }
    let mut out = Vec::with_capacity(bytes.len() + UTF8_BOM.len());
    out.extend_from_slice(UTF8_BOM);
    out.extend_from_slice(bytes);
    out
}

/// ステージング先に書くときの内容の整形（スクリプトは BOM 付き、それ以外はそのまま）。
fn finalize_content(name: &str, bytes: Vec<u8>) -> Vec<u8> {
    if name.ends_with(".ps1") {
        with_utf8_bom(&bytes)
    } else {
        bytes
    }
}

/// `SHA256SUMS` の内容（`<小文字16進>  <相対パス>`、パス昇順、LF 改行）。
pub fn checksums(files: &[(String, Vec<u8>)]) -> String {
    let mut lines: Vec<(String, String)> = files
        .iter()
        .map(|(name, data)| {
            (
                name.clone(),
                crate::update_sign::hex_encode(&Sha256::digest(data)),
            )
        })
        .collect();
    lines.sort();
    lines
        .into_iter()
        .map(|(name, hash)| format!("{hash}  {name}\n"))
        .collect()
}

/// ファイル群を 1 つの zip にする。最上位に `root/` を置き、更新時刻は固定（再現性のため）。
pub fn write_zip(root: &str, files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (name, data) in files {
        writer
            .start_file(format!("{root}/{name}"), options)
            .map_err(|e| format!("zip への追加に失敗しました（{name}）: {e}"))?;
        writer
            .write_all(data)
            .map_err(|e| format!("zip への書き込みに失敗しました（{name}）: {e}"))?;
    }
    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|e| format!("zip の確定に失敗しました: {e}"))
}

/// `signtool sign` の引数（PFX のパスワードは含めない。呼び出し側が追加する）。
pub fn signtool_sign_args(pfx: &Path, timestamp_url: &str, target: &Path) -> Vec<String> {
    vec![
        "sign".into(),
        "/fd".into(),
        "SHA256".into(),
        "/f".into(),
        pfx.display().to_string(),
        "/tr".into(),
        timestamp_url.into(),
        "/td".into(),
        "SHA256".into(),
        target.display().to_string(),
    ]
}

/// `signtool verify` の引数（既定の認証ポリシー）。
pub fn signtool_verify_args(target: &Path) -> Vec<String> {
    vec![
        "verify".into(),
        "/pa".into(),
        "/q".into(),
        target.display().to_string(),
    ]
}

fn run_signtool(signtool: &str, args: &[String], password: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new(signtool);
    cmd.args(args);
    if let Some(pw) = password {
        cmd.args(["/p", pw]);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("signtool の実行に失敗しました（{signtool}）: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "signtool が失敗しました（終了コード: {}）",
            status.code().unwrap_or(-1)
        ))
    }
}

/// ステージング先のファイルを書き出す。
fn stage(stage_dir: &Path, entries: &[Entry]) -> Result<(), String> {
    for e in entries {
        let bytes = match &e.source {
            Source::File(p) => fs::read(p)
                .map_err(|err| format!("{} の読み込みに失敗しました: {err}", p.display()))?,
            Source::Bytes(b) => b.clone(),
        };
        if (e.name == DLL_FILE || e.name == IMSERVER_FILE) && !looks_like_pe(&bytes) {
            return Err(format!(
                "{} が Windows の実行形式（PE）ではありません。MSVC ターゲットでビルドした成果物を指定してください",
                e.name
            ));
        }
        let dest = stage_dir.join(&e.name);
        fs::write(&dest, finalize_content(&e.name, bytes))
            .map_err(|err| format!("{} の書き込みに失敗しました: {err}", dest.display()))?;
    }
    Ok(())
}

/// ステージング先のファイル（`SHA256SUMS` 以外）を読み直す。署名後のバイト列で計算するため。
fn read_staged(stage_dir: &Path, entries: &[Entry]) -> Result<Vec<(String, Vec<u8>)>, String> {
    entries
        .iter()
        .map(|e| {
            let p = stage_dir.join(&e.name);
            fs::read(&p)
                .map(|data| (e.name.clone(), data))
                .map_err(|err| format!("{} の読み込みに失敗しました: {err}", p.display()))
        })
        .collect()
}

/// 収集 → ステージング → 署名（任意）→ チェックサム → zip。zip のパスを返す。
pub fn assemble(
    inputs: &Inputs,
    out_dir: &Path,
    sign: Option<(&str, &Path, &str)>,
) -> Result<PathBuf, String> {
    let entries = plan(inputs)?;
    let name = bundle_name(&inputs.version);
    let stage_dir = out_dir.join(&name);
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .map_err(|e| format!("既存のステージングの削除に失敗しました: {e}"))?;
    }
    fs::create_dir_all(&stage_dir).map_err(|e| format!("ディレクトリの作成に失敗しました: {e}"))?;
    stage(&stage_dir, &entries)?;

    if let Some((signtool, pfx, timestamp_url)) = sign {
        let password = std::env::var(PFX_PASSWORD_ENV).ok();
        for file in [DLL_FILE, IMSERVER_FILE] {
            let target = stage_dir.join(file);
            run_signtool(
                signtool,
                &signtool_sign_args(pfx, timestamp_url, &target),
                password.as_deref(),
            )?;
            run_signtool(signtool, &signtool_verify_args(&target), None)?;
            println!("✓ 署名しました: {file}");
        }
    }

    let mut files = read_staged(&stage_dir, &entries)?;
    let sums = checksums(&files);
    fs::write(stage_dir.join(CHECKSUM_FILE), &sums)
        .map_err(|e| format!("{CHECKSUM_FILE} の書き込みに失敗しました: {e}"))?;
    files.push((CHECKSUM_FILE.to_string(), sums.into_bytes()));

    let zip_path = out_dir.join(format!("{name}.zip"));
    let zip = write_zip(&name, &files)?;
    fs::write(&zip_path, zip)
        .map_err(|e| format!("{} の書き込みに失敗しました: {e}", zip_path.display()))?;
    Ok(zip_path)
}

/// 既定の成果物パス（`<target_dir>[/<triple>]/release/<file>`）。
fn default_artifact(target_dir: &Path, triple: Option<&str>, file: &str) -> PathBuf {
    let mut p = target_dir.to_path_buf();
    if let Some(t) = triple {
        p.push(t);
    }
    p.join("release").join(file)
}

pub fn run(opts: Options) -> Result<(), String> {
    let repo = repo_root();
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("core").join("target"));
    let triple = opts.target.as_deref();
    let inputs = Inputs {
        dll: opts
            .dll
            .unwrap_or_else(|| default_artifact(&target_dir, triple, DLL_FILE)),
        imserver: opts
            .imserver
            .unwrap_or_else(|| default_artifact(&target_dir, triple, IMSERVER_FILE)),
        repo,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let out_dir = opts
        .out_dir
        .unwrap_or_else(|| target_dir.join("windows-bundle"));
    fs::create_dir_all(&out_dir).map_err(|e| format!("ディレクトリの作成に失敗しました: {e}"))?;
    let timestamp = opts
        .timestamp_url
        .as_deref()
        .unwrap_or(DEFAULT_TIMESTAMP_URL);
    let sign = opts
        .sign_pfx
        .as_deref()
        .map(|pfx| (opts.signtool.as_str(), pfx, timestamp));
    if sign.is_none() {
        eprintln!(
            "注意: Authenticode 署名なし（SmartScreen の警告が出ます。docs/windows-install.md）"
        );
    }
    let zip = assemble(&inputs, &out_dir, sign)?;
    println!("✓ zip を作成しました: {}", zip.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// 一時ディレクトリ（テストごとに別。終了時に削除）。
    struct Temp(PathBuf);
    impl Temp {
        fn new(tag: &str) -> Temp {
            let p = std::env::temp_dir()
                .join(format!("nagi-bundle-windows-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Temp(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 偽のリポジトリ（配布物の元ファイルを一通り置く）と偽の成果物を作る。
    fn fake_inputs(root: &Path) -> Inputs {
        let repo = root.join("repo");
        for (path, content) in [
            (
                "frontend-windows/dist/install.ps1",
                "Write-Host 'インストール'\n",
            ),
            ("frontend-windows/dist/uninstall.ps1", "Write-Host 'x'\n"),
            ("docs/windows-install.md", "# README\n"),
            ("LICENSE-MIT", "mit\n"),
            ("LICENSE-APACHE", "apache\n"),
            ("NOTICE", "notice\n"),
            ("THIRD_PARTY_LICENSES.md", "third\n"),
        ] {
            let p = repo.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        let dll = root.join("a.dll");
        let exe = root.join("b.exe");
        fs::write(&dll, b"MZ-dll").unwrap();
        fs::write(&exe, b"MZ-exe").unwrap();
        Inputs {
            dll,
            imserver: exe,
            repo,
            version: "1.2.3".to_string(),
        }
    }

    fn read_zip(bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut f = archive.by_index(i).unwrap();
                let mut data = Vec::new();
                f.read_to_end(&mut data).unwrap();
                (f.name().to_string(), data)
            })
            .collect()
    }

    #[test]
    fn bundle_name_matches_release_asset() {
        // windows-release.yml と update の download_url が参照する名前
        assert_eq!(bundle_name("0.1.1"), "Nagi-windows-0.1.1");
    }

    #[test]
    fn plan_lists_all_distribution_files() {
        let t = Temp::new("plan");
        let entries = plan(&fake_inputs(&t.0)).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "nagi_windows_tsf.dll",
                "imserver.exe",
                "install.ps1",
                "uninstall.ps1",
                "README.md",
                "LICENSE-MIT",
                "LICENSE-APACHE",
                "NOTICE",
                "THIRD_PARTY_LICENSES.md",
                "VERSION",
            ]
        );
    }

    #[test]
    fn plan_reports_missing_input() {
        let t = Temp::new("missing");
        let mut inputs = fake_inputs(&t.0);
        inputs.dll = t.0.join("nope.dll");
        let err = plan(&inputs).unwrap_err();
        assert!(err.contains("nagi_windows_tsf.dll"), "{err}");
    }

    #[test]
    fn assemble_rejects_non_pe_binaries() {
        let t = Temp::new("notpe");
        let inputs = fake_inputs(&t.0);
        fs::write(&inputs.imserver, b"\x7fELF").unwrap();
        let err = assemble(&inputs, &t.0.join("out"), None).unwrap_err();
        assert!(err.contains("PE"), "{err}");
    }

    #[test]
    fn assemble_builds_zip_with_checksums() {
        let t = Temp::new("zip");
        let inputs = fake_inputs(&t.0);
        let out = t.0.join("out");
        fs::create_dir_all(&out).unwrap();
        let zip_path = assemble(&inputs, &out, None).unwrap();
        assert_eq!(zip_path, out.join("Nagi-windows-1.2.3.zip"));

        let files = read_zip(fs::read(&zip_path).unwrap());
        let get = |name: &str| -> &Vec<u8> {
            &files
                .iter()
                .find(|(n, _)| n == &format!("Nagi-windows-1.2.3/{name}"))
                .unwrap_or_else(|| panic!("{name} が zip に無い"))
                .1
        };
        // すべて最上位ディレクトリの下にあり、SHA256SUMS を含めて 11 個
        assert_eq!(files.len(), 11);
        assert!(
            files
                .iter()
                .all(|(n, _)| n.starts_with("Nagi-windows-1.2.3/"))
        );
        assert_eq!(get("VERSION"), b"1.2.3\n");
        assert_eq!(get("nagi_windows_tsf.dll"), b"MZ-dll");

        // スクリプトだけ UTF-8 BOM 付き（Windows PowerShell 5.1 の文字化け対策）
        assert!(get("install.ps1").starts_with(UTF8_BOM));
        assert!(get("uninstall.ps1").starts_with(UTF8_BOM));
        assert!(!get("README.md").starts_with(UTF8_BOM));

        // SHA256SUMS は SHA256SUMS 以外のすべてを、zip の中身どおりに列挙する
        let sums = String::from_utf8(get(CHECKSUM_FILE).clone()).unwrap();
        assert_eq!(sums.lines().count(), 10);
        for (name, data) in files.iter().filter(|(n, _)| !n.ends_with(CHECKSUM_FILE)) {
            let short = name.strip_prefix("Nagi-windows-1.2.3/").unwrap();
            let expected = format!("{}  {short}", crate::update_sign::sha256_hex(data));
            assert!(sums.lines().any(|l| l == expected), "missing: {expected}");
        }
        // ステージング先にも同じものがある（署名の確認用に残す）
        assert_eq!(
            fs::read(out.join("Nagi-windows-1.2.3").join(CHECKSUM_FILE)).unwrap(),
            sums.as_bytes()
        );
    }

    #[test]
    fn zip_is_reproducible() {
        let t = Temp::new("repro");
        let inputs = fake_inputs(&t.0);
        let out = t.0.join("out");
        fs::create_dir_all(&out).unwrap();
        let a = fs::read(assemble(&inputs, &out, None).unwrap()).unwrap();
        let b = fs::read(assemble(&inputs, &out, None).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn bom_is_added_once() {
        assert_eq!(with_utf8_bom(b"a"), [0xEF, 0xBB, 0xBF, b'a']);
        let once = with_utf8_bom(b"a");
        assert_eq!(with_utf8_bom(&once), once);
    }

    #[test]
    fn checksums_are_sorted_and_two_space_separated() {
        let s = checksums(&[("b".to_string(), vec![]), ("a".to_string(), vec![])]);
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(s, format!("{empty}  a\n{empty}  b\n"));
    }

    #[test]
    fn signtool_args_keep_password_out() {
        let args = signtool_sign_args(Path::new("c.pfx"), "http://ts", Path::new("x.dll"));
        assert_eq!(
            args,
            [
                "sign",
                "/fd",
                "SHA256",
                "/f",
                "c.pfx",
                "/tr",
                "http://ts",
                "/td",
                "SHA256",
                "x.dll"
            ]
        );
        assert!(!args.iter().any(|a| a == "/p"));
        assert_eq!(
            signtool_verify_args(Path::new("x.dll")),
            ["verify", "/pa", "/q", "x.dll"]
        );
    }

    #[test]
    fn default_artifact_includes_triple() {
        let base = Path::new("t");
        assert_eq!(
            default_artifact(base, Some("x86_64-pc-windows-msvc"), "a.dll"),
            Path::new("t/x86_64-pc-windows-msvc/release/a.dll")
        );
        assert_eq!(
            default_artifact(base, None, "a.dll"),
            Path::new("t/release/a.dll")
        );
    }
}
