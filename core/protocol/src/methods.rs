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
    /// `import_user_dict` メソッド（他の IME のユーザー辞書の取り込み）
    pub const IMPORT_USER_DICT: &str = "import_user_dict";
    /// `get_key_table` メソッド（タッチ入力: 12 キーの文字表と濁点・小書きの循環）
    pub const GET_KEY_TABLE: &str = "get_key_table";
    /// `unload_model` メソッド（変換モデルだけをメモリから解放し、辞書のみで動く状態にする）
    pub const UNLOAD_MODEL: &str = "unload_model";
    /// `reload_model` メソッド（解放した変換モデルを再び読み込む）
    pub const RELOAD_MODEL: &str = "reload_model";
    /// `insert_reading` メソッド（タッチ入力: ローマ字を経由せず読みを挿入）
    pub const INSERT_READING: &str = "insert_reading";
    /// `set_layout_geometry` メソッド（タッチ入力: キーボードのキー矩形。曖昧入力のキー近接に使う）
    pub const SET_LAYOUT_GEOMETRY: &str = "set_layout_geometry";
    /// `perform_action` メソッド（タッチ UI のボタンから操作を直接実行）
    pub const PERFORM_ACTION: &str = "perform_action";
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
    /// ホストが把握している実行環境（任意。省略すると従来どおり。後から `set_environment` で更新できる）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub capabilities: Option<ClientCapabilities>,
}

/// 入力方式（フロントエンドの入力手段）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMethod {
    /// 物理キーボード（macOS / Windows など）
    Physical,
    /// タッチ（ソフトウェアキーボード。iOS / Android）
    Touch,
}

/// `initialize` でフロントエンドが申告する実行環境（すべて任意）。OS への問い合わせはコアでは行わず、
/// ホストが報告した値でプロファイルを判定する（mobile-design §3-2）
#[typeshare]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// 入力方式。省略は `physical` 相当
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub input_method: Option<InputMethod>,
    /// 電源・サーマル・メモリ逼迫・メモリ上限（`set_environment` と同じ項目）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub environment: Option<SetEnvironmentParams>,
    /// `Some(true)` なら、入力中の候補（サジェスト）に「読みの接頭辞だけを置き換える候補」を多数含めてよい
    /// （`CandidateView.covers` が読み全体より短い候補を選ぶと、その分だけ確定して残りは入力中のまま続く
    /// ことにフロントが対応している。iOS の候補バー）。省略・`false` は従来どおりの少数の候補
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub partial_suggestions: Option<bool>,
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

    /// 確定済みテキストの再変換の要求（未入力でのかなキー・変換キー等。キーマップの `Reconvert`）。
    /// `Some(true)` なら、フロントは選択中のテキストがあれば `reconvert` で再変換する
    /// （選択が無い・取得できない場合は何もしない）。選択の取得はフロントの仕事なのでコアは要求だけを返す
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reconvert_selection: Option<bool>,

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
    /// このプロセスが OS から課されているメモリ上限（MB。iOS 拡張の `phys_footprint + 残り` など）。
    /// 上限が小さいと軽いプロファイルに制限し、LLM に必要な量（150MB）未満なら LLM を解放する
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub memory_budget_mb: Option<u32>,
    /// 現在のローカル時刻の UTC からのずれ（分。夏時間を含む今の値。例: 日本 540）。
    /// 日付・時刻の候補（「きょう」「いま」）に使う。`convert.time_zone` が固定されていればそちらが優先
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub utc_offset_minutes: Option<i32>,
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

// ---- タッチ入力（モバイル。`insert_reading` / `perform_action`） ----

/// 12 キーのキー（行）。レイアウトの描画はフロント、各キーの文字（タップ・フリック）の対応はコアが持つ
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KanaKey {
    /// あ行（あ い う え お）
    A,
    /// か行
    Ka,
    /// さ行
    Sa,
    /// た行
    Ta,
    /// な行
    Na,
    /// は行
    Ha,
    /// ま行
    Ma,
    /// や行（や （ ゆ ） よ）
    Ya,
    /// ら行
    Ra,
    /// わ行（わ を ん ー 〜）
    Wa,
    /// 句読点（、 。 ？ ！ …）
    Punct,
}

/// 12 キーの操作
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyGesture {
    /// タップ（行の先頭の文字。連続してもトグルしない。フリック入力用）
    Tap,
    /// トグル入力のタップ（連続して同じキーを押すと、直前の文字を同じ行の次の文字に置き換える）
    Toggle,
    /// 左フリック
    FlickLeft,
    /// 上フリック
    FlickUp,
    /// 右フリック
    FlickRight,
    /// 下フリック
    FlickDown,
}

/// `insert_reading` の入力
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum ReadingInput {
    /// 読み（かな・記号）をそのまま挿入する
    Text(String),
    /// 12 キー: キーと操作（タップ・トグル・フリック）から、挿入する文字をコアが決める
    Key12 {
        /// キー（行）
        key: KanaKey,
        /// 操作
        gesture: KeyGesture,
    },
    /// 直前の文字の濁点・半濁点・小書きを循環させる（「゛゜小」キー。か→が、は→ば→ぱ→は、つ→っ→づ→つ）
    Variant,
}

/// `insert_reading` メソッドの要求パラメータ。応答は `process_key` と同じ [`ProcessKeyResult`]
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsertReadingParams {
    /// セッション ID
    pub session_id: String,
    /// 入力
    pub input: ReadingInput,
}

/// キー内のタッチ位置。押されたキーの矩形内の相対位置（左上が (0, 0)、右下が (1, 1)。
/// 座標系に依存しないので、フロントはキーのローカル座標を幅・高さで割って送る）。
/// 曖昧入力（`input.fuzzy.use_touch_coords`）が、隣のキーとの境界に近い打鍵ほど打ち間違いとして
/// 補正しやすくするのに使う。preedit の間だけメモリに持ち、確定・破棄・フォーカス喪失で消す（ログに出さない）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TouchPoint {
    /// 横の相対位置（0..1）
    pub x: f64,
    /// 縦の相対位置（0..1）
    pub y: f64,
}

/// フリックの詳細（12 キーのかな入力）。方向が閾値に近いときに、隣の方向を低コストで候補にする
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FlickDetail {
    /// フリックの角度（度。右 = 0、上 = 90、左 = 180、下 = 270 または -90。反時計回り）
    pub angle_deg: f64,
    /// 指を動かした距離（キー幅を 1 とする）
    pub distance: f64,
}

/// `insert_reading` メソッドの要求パラメータ（座標つき）。[`InsertReadingParams`] に任意項目を足した上位互換で、
/// 項目を省略すれば `InsertReadingParams` と同じ。サーバーは `insert_reading` をこの型で受ける
/// （既存のフロントが `InsertReadingParams` のまま送っても動く）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsertReadingTouchParams {
    /// セッション ID
    pub session_id: String,
    /// 入力
    pub input: ReadingInput,
    /// `input` が QWERTY の 1 文字（`ReadingInput::Text`）のときの、キー内のタッチ位置
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub touch: Option<TouchPoint>,
    /// `input` が 12 キーのフリック（`ReadingInput::Key12`）のときの、角度と距離
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub flick: Option<FlickDetail>,
}

/// キー矩形 1 つ（`set_layout_geometry`）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutKey {
    /// フロントのキー ID（識別用。補正には使わない）
    pub id: String,
    /// キーで入力される文字（QWERTY の英字キーは `"q"` など 1 文字。大文字表示でも構わない）。
    /// 英字以外のキー（Shift・空白・数字など）は無視される
    pub label: String,
    /// 左端
    pub x: f64,
    /// 上端
    pub y: f64,
    /// 幅
    pub w: f64,
    /// 高さ
    pub h: f64,
}

/// `set_layout_geometry` メソッドの要求パラメータ。応答は [`EmptyResult`]。
///
/// キーボードのキー矩形を渡すと、曖昧入力のキー近接（どのキーが隣か）を、内蔵の QWERTY ではなく
/// このレイアウトから導出する（QWERTY 以外・カスタム配列でも使える）。座標系はキーボード view の
/// ローカル座標（左上が原点、`width` × `height`）で、`keys` も同じ座標系。単位は問わない
/// （正規化座標 0..1 でもよい）。受け取るまでは内蔵の QWERTY を使う。英字キーが 10 個未満の
/// レイアウト（12 キーのかななど）は無視される
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetLayoutGeometryParams {
    /// 対象セッション。省略するとすべてのセッションの既定にする
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub session_id: Option<String>,
    /// レイアウトの識別子（同じ識別子・内容なら再計算しない）
    pub layout_id: String,
    /// モード（例：`qwerty`、`kana12`。診断用）
    pub mode: String,
    /// キー矩形
    pub keys: Vec<LayoutKey>,
    /// キーボード view の幅
    pub width: f64,
    /// キーボード view の高さ
    pub height: f64,
}

impl SetLayoutGeometryParams {
    /// 曖昧入力に使えるキー（ラベルが 1 文字の英字）を、小文字の文字とともに返す
    pub fn letter_keys(&self) -> impl Iterator<Item = (char, &LayoutKey)> {
        self.keys.iter().filter_map(|k| {
            let mut it = k.label.chars();
            let c = it.next()?.to_ascii_lowercase();
            (it.next().is_none() && c.is_ascii_lowercase()).then_some((c, k))
        })
    }

    /// 曖昧入力のキー配置として使えるか（英字キーが 10 個以上）
    pub fn is_usable_for_fuzzy(&self) -> bool {
        self.letter_keys().count() >= 10
    }
}

/// 左右の方向
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchDirection {
    /// 左
    Left,
    /// 右
    Right,
}

/// `perform_action` で実行する操作（タッチ UI のボタンやジェスチャーに対応する）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum TouchAction {
    /// 空白キー: 入力中は変換、変換中は次の候補。未入力なら消費しない（フロントが空白を入れる）
    Space,
    /// 改行（確定）キー: 入力中・変換中は確定。未入力なら消費しない（フロントが改行を入れる）
    Enter,
    /// 変換を開始する
    StartConvert,
    /// 予測変換
    Predict,
    /// 次の候補
    NextCandidate,
    /// 前の候補
    PrevCandidate,
    /// 確定
    Commit,
    /// 変換中: 先頭から注目文節（複数文節の選択中は選択範囲の末尾）までを確定し、残りの文節は
    /// 変換中のまま続ける（注目は残りの先頭へ）。注目文節が最後の文節なら `Commit` と同じ。
    /// 入力中（変換前）は `Commit` と同じ
    CommitFocused,
    /// 取消
    Cancel,
    /// 1 文字削除（未入力なら消費しない）
    Backspace,
    /// カーソル直後を削除
    DeleteForward,
    /// 入力中のカーソル移動
    MoveCursor(TouchDirection),
    /// カーソルを先頭へ
    MoveCursorHome,
    /// カーソルを末尾へ
    MoveCursorEnd,
    /// 注目文節の移動
    MoveFocus(TouchDirection),
    /// 文節の区切りの伸縮
    ResizeSegment(TouchDirection),
    /// 複数文節の選択
    ExtendSelection(TouchDirection),
    /// 注目文節の学習を削除
    DeleteLearning,
    /// カタカナに変換
    ToKatakana,
    /// ひらがなに変換
    ToHiragana,
    /// 半角化
    ToHankaku,
    /// 入力モードのトグル
    ToggleMode,
    /// 入力モードを設定
    SetMode(InputMode),
    /// 直前の確定の取消
    UndoCommit,
}

/// `perform_action` メソッドの要求パラメータ。応答は `process_key` と同じ [`ProcessKeyResult`]
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformActionParams {
    /// セッション ID
    pub session_id: String,
    /// 操作
    pub action: TouchAction,
}

// ---- 12 キーの文字表（`get_key_table`） ----

/// 12 キーのうち 1 つのキー（行）の文字表。入力される文字の決定はコア（`insert_reading`）が行うので、
/// フロントはこれを押下ポップアップの表示などにだけ使う
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyTableEntry {
    /// キー（行）
    pub key: KanaKey,
    /// タップ（中央）で入る文字
    pub center: String,
    /// 左フリックで入る文字
    pub left: String,
    /// 上フリックで入る文字
    pub up: String,
    /// 右フリックで入る文字
    pub right: String,
    /// 下フリックで入る文字
    pub down: String,
    /// トグル入力で連続して押したときの文字の循環（先頭から順に進み、末尾の次は先頭に戻る）
    pub toggle: Vec<String>,
}

/// `get_key_table` メソッドの応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyTableResult {
    /// 各キーの文字表（レイアウト順）
    pub keys: Vec<KeyTableEntry>,
    /// 「゛゜小」キーの循環（`variant`）。各要素が 1 つの循環で、末尾の次は先頭に戻る
    /// （例：`["は", "ば", "ぱ"]`）。循環に含まれない文字は変化しない
    pub variants: Vec<Vec<String>>,
}

// ---- 他の IME のユーザー辞書の取り込み（`import_user_dict`） ----

/// 取り込む辞書の形式（`import_user_dict` のヒント・結果）
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDictFormat {
    /// Mozc / Google 日本語入力の TSV
    Mozc,
    /// Microsoft IME のテキスト
    MsIme,
    /// ATOK のテキスト
    Atok,
    /// macOS 標準のユーザ辞書（XML plist）
    MacPlist,
    /// `読み<TAB>単語` の 2 列テキスト
    TwoColumn,
}

/// `import_user_dict` メソッドの要求パラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportUserDictParams {
    /// ファイルの中身（バイト列そのものを base64 で。文字コードの判定はコアが行う）
    pub data_base64: String,
    /// 形式のヒント。省略するとファイルの先頭の行と構造から自動判定する
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<UserDictFormat>,
}

/// 取り込みをスキップした理由ごとの件数
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSkipReason {
    /// 理由の識別子（`missing_columns` / `invalid_reading` / `empty_surface` /
    /// `duplicate_in_file` / `duplicate_existing`）
    pub reason: String,
    /// 表示用の説明（日本語）
    pub message: String,
    /// 件数
    pub count: u32,
    /// 最初に見つかった行番号（plist では辞書の通し番号）
    pub first_line: u32,
}

/// `import_user_dict` メソッドの応答
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportUserDictResult {
    /// 判定（または指定）された形式
    pub format: UserDictFormat,
    /// 読み取った項目の総数
    pub total: u32,
    /// 取り込んだ件数
    pub imported: u32,
    /// スキップした件数
    pub skipped: u32,
    /// スキップの理由ごとの内訳
    pub skipped_reasons: Vec<ImportSkipReason>,
}
