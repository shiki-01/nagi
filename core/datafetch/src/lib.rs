//! Nagi データ取得: Mozc OSS 辞書・zenz モデルの取得（SHA-256 検証）と、辞書バイナリの組み立て。
//!
//! `cargo xtask fetch-dict` / `build-dict` / `fetch-model` と `imserver --install-data` が共有する。
//! データは同梱・再配布せず、利用者の手元で配布元（Mozc の GitHub raw、Hugging Face）から取得する。
//! 進捗は [`Progress`] のコールバックで通知し、表示（stderr / JSON）は呼び出し側が決める。
//!
//! iOS の XCFramework（`nagi-ffi`）にはこの crate を含めない（TLS 実装を静的ライブラリに入れないため）。

mod fsutil;
pub mod http;
pub mod model;
pub mod mozc;

pub use http::{Body, Http, UreqHttp};
pub use model::{ModelInfo, ModelKind, fetch_model, fetch_model_with};
pub use mozc::{
    MOZC_COMMIT, build_mozc_dict, fetch_mozc, fetch_mozc_with, mozc_file_count, mozc_file_names,
};

use std::path::PathBuf;

/// 取得・組み立ての進捗イベント。
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// 取得済み（SHA-256 一致）のためスキップ
    Skipped {
        /// ファイル名
        file: String,
        /// 1 始まりの通し番号
        index: usize,
        /// 全ファイル数
        count: usize,
    },
    /// 既存ファイルの SHA-256 が不一致のため再取得する（モデルのみ）
    Mismatched {
        /// ファイル名
        file: String,
    },
    /// ダウンロード開始
    Download {
        /// ファイル名
        file: String,
        /// 1 始まりの通し番号
        index: usize,
        /// 全ファイル数
        count: usize,
        /// 取得元 URL
        url: String,
        /// 期待サイズ（表にあれば）
        size: Option<u64>,
    },
    /// ダウンロードのバイト進捗（チャンクごと。間引きは呼び出し側）
    Bytes {
        /// ファイル名
        file: String,
        /// 受信済みバイト数
        done: u64,
        /// 全体のバイト数（不明なら `None`）
        total: Option<u64>,
    },
    /// 受信完了・SHA-256 検証に成功
    Verified {
        /// ファイル名
        file: String,
        /// 受信バイト数
        bytes: u64,
        /// SHA-256（16 進）
        sha256: String,
    },
    /// 最終パスへ保存した
    Saved {
        /// 保存先
        path: PathBuf,
    },
    /// 辞書の組み立て開始
    BuildStart,
    /// 辞書の組み立て完了
    BuildDone {
        /// 出力サイズ（バイト）
        bytes: u64,
        /// 所要時間（秒）
        seconds: f64,
        /// 出力先
        path: PathBuf,
    },
}
