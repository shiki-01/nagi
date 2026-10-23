//! Nagi 開発用タスクランナー。`cargo xtask <task>` で実行する（`core/` で実行）。
//!
//! タスクは OS 非依存にするため、シェルスクリプトではなくこの crate に Rust で書く。

mod app_icon;
mod build_dict;
mod build_ios_xcframework;
mod bundle_macos;
mod bundle_windows;
mod fetch_dict;
mod fetch_english;
mod fetch_loanword;
mod fetch_model;
mod gen_licenses;
mod mode_icons;
mod types;
mod update_sign;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use build_dict::DictSource;
use fetch_model::ModelVariant;

#[derive(Parser)]
#[command(name = "xtask", about = "Nagi 開発用タスク（OS 非依存）")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Mozc OSS 辞書を取得し SHA-256 検証する（data/dict/mozc/ へ）
    FetchDict,
    /// 取得済みの辞書（Mozc / SudachiDict）から Nagi 辞書バイナリを生成する
    /// （data/dict/nagi-dict.bin または data/dict/nagi-dict-sudachi.bin）
    BuildDict {
        #[arg(long, value_enum, default_value_t = DictSource::Mozc)]
        source: DictSource,
    },
    /// 英単語の語彙（SCOWL）を取得し SHA-256 検証して、fst 索引を組み立てる
    /// （data/english/words.fst。英語の自動判定の語彙。無くても動く）
    FetchEnglish,
    /// カタカナ語の原語データ（JMdict。jmdict-simplified の固定した版）を取得し SHA-256 検証して、
    /// 索引（data/loanword/loanword.fst）を組み立てる
    FetchLoanword {
        /// 取得せず、手元の jmdict-eng-*.json.zip から索引だけ組み立てる
        #[arg(long)]
        from: Option<std::path::PathBuf>,
    },
    /// 変換モデル（zenz GGUF）を取得し SHA-256 検証する（data/models/ へ）
    FetchModel {
        #[arg(long, value_enum, default_value_t = ModelVariant::Small)]
        variant: ModelVariant,
    },
    /// protocol の型から Swift の型を生成する（typeshare-cli が必要）
    GenTypes,
    /// gen-types を実行し、生成物に差分がないことを検査する
    CheckTypes,
    /// Nagi.app を組み立てて署名（既定は ad-hoc、`--sign-identity` で Developer ID）し zip 化する
    /// （target/macos-bundle/、署名・zip は macOS のみ）
    BundleMacos {
        /// swift build するプロダクト（実行ファイル）名
        #[arg(long, default_value = "NagiIMEApp")]
        swift_target: String,
        /// 指定すると swift build せず、このファイルを実行ファイルとして使う
        #[arg(long)]
        executable: Option<std::path::PathBuf>,
        /// imserver を feature llama 付きでビルドする
        #[arg(long)]
        llama: bool,
        /// バンドル識別子（モード ID `<id>.Japanese`・接続名 `<id>_Connection` も派生する）
        #[arg(long, default_value = "com.shiki.inputmethod.Nagi")]
        bundle_id: String,
        /// 出力先ディレクトリ（既定は target/macos-bundle）
        #[arg(long)]
        out_dir: Option<std::path::PathBuf>,
        /// Developer ID の署名 ID（例: "Developer ID Application: ..."）。未指定なら ad-hoc 署名
        #[arg(long)]
        sign_identity: Option<String>,
    },
    /// Windows 版の配布物（TSF の DLL・imserver.exe・インストーラスクリプト・ライセンス）を収集し、
    /// チェックサムをつけて zip 化する（target/windows-bundle/Nagi-windows-<version>.zip）。
    /// ビルドは行わない（MSVC ターゲットでビルド済みの成果物を集める。`windows-release.yml` を参照）
    BundleWindows {
        /// nagi_windows_tsf.dll のパス（既定は target[/<triple>]/release/）
        #[arg(long)]
        dll: Option<std::path::PathBuf>,
        /// imserver.exe のパス（既定は target[/<triple>]/release/）
        #[arg(long)]
        imserver: Option<std::path::PathBuf>,
        /// 既定パスを探すターゲットトリプル（例: x86_64-pc-windows-msvc）
        #[arg(long)]
        target: Option<String>,
        /// 出力先ディレクトリ（既定は target/windows-bundle）
        #[arg(long)]
        out_dir: Option<std::path::PathBuf>,
        /// Authenticode 署名に使う PFX。指定すると DLL と exe を signtool で署名する
        /// （パスワードは環境変数 NAGI_WINDOWS_PFX_PASSWORD）。未指定なら署名しない
        #[arg(long)]
        sign_pfx: Option<std::path::PathBuf>,
        /// signtool.exe のパス（PATH 上に無い場合）
        #[arg(long, default_value = "signtool")]
        signtool: String,
        /// タイムスタンプサーバーの URL（既定は DigiCert）
        #[arg(long)]
        timestamp_url: Option<String>,
    },
    /// アップデート署名用の ed25519 鍵ペアを生成して標準出力に出す（秘密鍵は保存しない）
    GenUpdateKey,
    /// 配布物（zip）に ed25519 署名して latest.json を書き出す（秘密鍵は環境変数 NAGI_UPDATE_SIGNING_KEY）
    SignUpdate {
        /// 署名する配布物（Nagi.app / Nagi-windows の zip）
        #[arg(long)]
        file: std::path::PathBuf,
        /// 配布物のバージョン（x.y.z）
        #[arg(long)]
        version: String,
        /// 配布物のダウンロード URL
        #[arg(long)]
        download_url: String,
        /// リリースノートの URL
        #[arg(long)]
        notes_url: Option<String>,
        /// 書き出す latest.json のパス
        #[arg(long)]
        out: std::path::PathBuf,
    },
    /// 配布物ごとの依存ライセンス一覧を生成する（ios: THIRD_PARTY_LICENSES.md、windows: THIRD_PARTY_LICENSES_WINDOWS.md）
    GenLicenses {
        /// 対象（ios / windows / all）
        #[arg(long, default_value = "all")]
        platform: String,
    },
    /// iOS 用 XCFramework を生成する（nagi-ffi を aarch64-apple-ios/sim ターゲット向けにビルド）
    BuildIosXcframework {
        /// feature llama を有効化する
        #[arg(long)]
        llama: bool,
        /// 出力先ディレクトリ（既定は target/ios-xcframework）
        #[arg(long)]
        out_dir: Option<std::path::PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.task {
        Task::FetchDict => fetch_dict::run(),
        Task::BuildDict { source } => build_dict::run(source),
        Task::FetchEnglish => fetch_english::run(),
        Task::FetchLoanword { from } => fetch_loanword::run(from),
        Task::FetchModel { variant } => fetch_model::run(variant),
        Task::GenTypes => types::generate(),
        Task::CheckTypes => types::check(),
        Task::BundleMacos {
            swift_target,
            executable,
            llama,
            bundle_id,
            out_dir,
            sign_identity,
        } => bundle_macos::run(bundle_macos::Options {
            swift_target,
            executable,
            llama,
            bundle_id,
            out_dir,
            sign_identity,
        }),
        Task::BundleWindows {
            dll,
            imserver,
            target,
            out_dir,
            sign_pfx,
            signtool,
            timestamp_url,
        } => bundle_windows::run(bundle_windows::Options {
            dll,
            imserver,
            target,
            out_dir,
            sign_pfx,
            signtool,
            timestamp_url,
        }),
        Task::GenUpdateKey => update_sign::gen_key(),
        Task::SignUpdate {
            file,
            version,
            download_url,
            notes_url,
            out,
        } => update_sign::sign_update(update_sign::SignOptions {
            file,
            version,
            download_url,
            notes_url,
            out,
        }),
        Task::GenLicenses { platform } => gen_licenses::run(&platform),
        Task::BuildIosXcframework { llama, out_dir } => {
            build_ios_xcframework::run(build_ios_xcframework::Options { llama, out_dir })
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}
