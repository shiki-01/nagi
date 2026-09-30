//! JSON-RPC メソッドの params/result 型と method 名定数

use crate::key::{KeyEvent, KeyKind, Modifiers};
use crate::view::{CandidatesView, InputMode, PreeditView, SegmentKind};
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

pub mod method {
    //! JSON-RPC メソッド名

    /// `initialize` メソッド
    pub const INITIALIZE: &str = "initialize";
    /// `process_key` メソッド
    pub const PROCESS_KEY: &str = "process_key";
    /// `focus_in` メソッド
    pub const FOCUS_IN: &str = "focus_in";
    /// `focus_out` メソッド
    pub const FOCUS_OUT: &str = "focus_out";
    /// `set_context` 通知
    pub const SET_CONTEXT: &str = "set_context";
    /// `set_environment` 通知（電源・サーマル・メモリの状態）
    pub const SET_ENVIRONMENT: &str = "set_environment";
    /// `get_status` メソッド
    pub const GET_STATUS: &str = "get_status";
    /// `clear_learning` メソッド（学習データの全削除）
    pub const CLEAR_LEARNING: &str = "clear_learning";
    /// `get_settings` メソッド（GUI 設定画面向けの設定項目と現在値）
    pub const GET_SETTINGS: &str = "get_settings";
    /// `update_settings` メソッド（設定の書き込み）
    pub const UPDATE_SETTINGS: &str = "update_settings";
    /// `select_candidate` メソッド
    pub const SELECT_CANDIDATE: &str = "select_candidate";
    /// `reconvert` メソッド
    pub const RECONVERT: &str = "reconvert";
    /// `restore_session` メソッド
    pub const RESTORE_SESSION: &str = "restore_session";
    /// `reset` メソッド
    pub const RESET: &str = "reset";
    /// `reload_config` メソッド
    pub const RELOAD_CONFIG: &str = "reload_config";
    /// `ping` メソッド
    pub const PING: &str = "ping";
    /// `shutdown` メソッド
    pub const SHUTDOWN: &str = "shutdown";
    /// `update_preedit` 通知（C→F）
    pub const UPDATE_PREEDIT: &str = "update_preedit";
    /// `update_candidates` 通知（C→F）
    pub const UPDATE_CANDIDATES: &str = "update_candidates";
    /// `notify` 通知（C→F）
    pub const NOTIFY: &str = "notify";
}

/// `initialize` メソッドの要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeParams {
    /// プロトコルバージョン
    pub protocol_version: u32,
    /// クライアント名（例：`nagi-macos`）
    pub client_name: String,
    /// クライアントバージョン
    pub client_version: String,
    /// OS 識別子（例：`macos`、`windows`）
    pub os: String,
}

/// サーバーの機能
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// 再変換に対応しているか
    pub reconvert: bool,
    /// ライブ変換に対応しているか
    pub live_conversion: bool,
    /// スタイル変換に対応しているか
    pub style_conversion: bool,
}

/// `initialize` メソッドの応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeResult {
    /// サーバーが対応するプロトコルバージョン
    pub protocol_version: u32,
    /// サーバー名（例：`nagi-engine`）
    pub server_name: String,
    /// サーバーバージョン
    pub server_version: String,
    /// 自動選択されたプロファイル（`minimal`、`light`、`balanced`、`quality` など）
    pub profile: String,
    /// 初期入力モード
    pub mode: InputMode,
    /// サーバーの機能
    pub capabilities: ServerCapabilities,
}

/// 通知レベル
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    /// 情報
    Info,
    /// 警告
    Warning,
    /// エラー
    Error,
}

/// 通知パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyParams {
    /// 通知レベル
    pub level: NotifyLevel,
    /// メッセージ
    pub message: String,
    /// エラーコード（オプション）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
}

/// `process_key` メソッドの要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessKeyParams {
    /// セッション ID
    pub session_id: String,
    /// XKB keysym（数値）
    pub keysym: u32,
    /// OS キーコード
    pub keycode: u32,
    /// 生成されたテキスト
    pub text: String,
    /// 修飾キーの状態
    pub modifiers: Modifiers,
    /// キーの種別
    pub kind: KeyKind,
}

impl ProcessKeyParams {
    /// KeyEvent を構成する
    pub fn key_event(&self) -> KeyEvent {
        KeyEvent {
            keysym: self.keysym,
            keycode: self.keycode,
            text: self.text.clone(),
            modifiers: self.modifiers,
            kind: self.kind,
        }
    }
}

/// `process_key` / `focus_out` / `select_candidate` / `reconvert` / `restore_session` / `reset` の共通応答（描画差分）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProcessKeyResult {
    /// キーが消費されたか（IME が処理を行ったか）
    pub consumed: bool,

    /// 確定すべき文字列。None なら確定なし
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commit: Option<String>,

    /// 更新されたプリエディット。None なら変化なし
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub preedit: Option<PreeditView>,

    /// 更新された候補。None なら変化なし
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub candidates: Option<CandidatesView>,

    /// 入力モードが変わった場合の新しいモード。None なら変化なし
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<InputMode>,

    /// 確定取消（Ctrl+Backspace）が成功した場合、フロントがホストアプリのテキストから
    /// 削除すべき直前の確定文字列の長さ（UTF-16 コード単位）。挿入ではなく削除なので
    /// `commit` とは意味が異なる（両方が同時に Some になることは無い）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub delete_before: Option<u32>,

    /// リビジョン番号（古い非同期通知を破棄するため）
    pub revision: u32,
}

impl ProcessKeyResult {
    /// キーが消費されなかった（素通し）場合の簡易コンストラクタ
    pub fn passthrough(revision: u32) -> Self {
        Self {
            consumed: false,
            revision,
            ..Default::default()
        }
    }
}

/// セッションを指定するパラメータ（`focus_in` / `focus_out` / `reset`）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionParams {
    /// セッション ID
    pub session_id: String,
}

/// Surrounding Text（周囲のテキスト）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurroundingText {
    /// テキスト内容
    pub text: String,
    /// カーソル位置（UTF-16 コード単位）
    pub cursor: u32,
}

/// `set_context` 通知のパラメータ（応答なし）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetContextParams {
    /// セッション ID
    pub session_id: String,
    /// Surrounding Text（オプション）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub surrounding_text: Option<SurroundingText>,
    /// アプリケーション ID（オプション）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub app_id: Option<String>,
    /// Secure Input 有効か（オプション、学習・ログを停止するため）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub secure_input: Option<bool>,
}

/// `select_candidate` メソッドのパラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectCandidateParams {
    /// セッション ID
    pub session_id: String,
    /// 選択する候補のインデックス
    pub index: u32,
}

/// `reconvert` メソッドのパラメータ（確定済みテキストの再変換）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconvertParams {
    /// セッション ID
    pub session_id: String,
    /// 再変換対象のテキスト
    pub text: String,
}

/// エンジン再起動時の復元セグメント
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreSegment {
    /// ひらがな読み
    pub reading: String,
    /// 表示文字列
    pub surface: String,
    /// セグメント種別
    pub kind: SegmentKind,
}

/// `restore_session` メソッドのパラメータ（エンジン再起動後の状態復元）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreSessionParams {
    /// セッション ID
    pub session_id: String,
    /// 復元するセグメント列
    pub segments: Vec<RestoreSegment>,
    /// カーソル位置（segments 連結 reading 上の Unicode スカラー単位での位置）
    pub cursor: u32,
    /// Converting の注目文節インデックス（オプション）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focus: Option<u32>,
    /// 復元時に Converting 状態か
    pub converting: bool,
}

/// 設定読み込みエラー
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigError {
    /// エラー行番号
    pub line: u32,
    /// エラーメッセージ
    pub message: String,
}

/// `reload_config` メソッドの応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReloadConfigResult {
    /// 読み込み成功したか
    pub ok: bool,
    /// 発生したエラーのリスト
    pub errors: Vec<ConfigError>,
}

/// 空の結果（`ping` / `shutdown` / `focus_in`）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyResult {}

/// `update_preedit` 通知のパラメータ（C→F、ライブ変換結果など）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePreeditParams {
    /// セッション ID
    pub session_id: String,
    /// リビジョン番号（古い通知は捨てる）
    pub revision: u32,
    /// 新しいプリエディット
    pub preedit: PreeditView,
}

/// `update_candidates` 通知のパラメータ（C→F、ライブ変換結果など）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCandidatesParams {
    /// セッション ID
    pub session_id: String,
    /// リビジョン番号（古い通知は捨てる）
    pub revision: u32,
    /// 新しい候補
    pub candidates: CandidatesView,
}

/// サーマル状態（OS が報告するもの）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThermalState {
    /// 通常
    Nominal,
    /// やや高い
    Fair,
    /// 高い
    Serious,
    /// 危険
    Critical,
}

/// メモリ逼迫の状態（OS が報告するもの）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPressureState {
    /// 通常
    Normal,
    /// 警告
    Warning,
    /// 危険
    Critical,
}

/// `set_environment` 通知のパラメータ（応答なし）。実行環境の状態を報告し、コアはこれに応じて
/// 環境プロファイルを自動で切り替える（設計書 §4.7）。省略した項目は「前回の値を維持」
#[typeshare]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetEnvironmentParams {
    /// バッテリー駆動か
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub on_battery: Option<bool>,
    /// OS の低電力モードか
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub low_power_mode: Option<bool>,
    /// サーマル状態
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thermal: Option<ThermalState>,
    /// メモリ逼迫
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub memory_pressure: Option<MemoryPressureState>,
}

/// `get_status` の結果: メニューなどで「今どういう状態か・なぜか」を表示するための情報
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResult {
    /// 現在の環境プロファイル（`quality` / `balanced` / `light` / `minimal`）
    pub profile: String,
    /// そのプロファイルになった理由（日本語）
    pub profile_reason: String,
    /// 変換バックエンド名（診断用）
    pub backend: String,
    /// LLM を使っているか
    pub llm: bool,
    /// 学習が有効か（secure input 中などで一時停止していても、設定として有効なら true）
    pub learning: bool,
    /// 学習データの件数
    pub learned_entries: u32,
    /// ユーザー辞書の語数
    pub user_dict_entries: u32,
    /// 設定ファイルのパス（未解決なら None）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub config_path: Option<String>,
    /// 直近の設定読み込みに失敗して旧設定で動いているか
    pub config_error: bool,
}
