//! 設定 RPC（`get_settings` / `update_settings`）の型。
//!
//! 設定画面のフロントは汎用フォームを描画するだけにするため、コアが設定項目を「記述子」として返す
//! （スキーマ駆動）。値の検証と `config.toml` への書き込みはコアが行う。

use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// 設定値（`{"type":"bool","value":true}` のように型名つきで表す）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SettingValue {
    /// 真偽値
    Bool(bool),
    /// 整数
    Int(i32),
    /// 小数
    Float(f64),
    /// 文字列（`Choice` の選択値も含む）
    Text(String),
}

/// 選択肢
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingChoice {
    /// 設定値（`SettingValue::Text` に入れる文字列）
    pub value: String,
    /// 表示名（日本語）
    pub label: String,
}

/// 整数項目の範囲
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntRange {
    /// 最小値
    pub min: i32,
    /// 最大値
    pub max: i32,
    /// UI の刻み
    pub step: i32,
}

/// 小数項目の範囲
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FloatRange {
    /// 最小値
    pub min: f64,
    /// 最大値
    pub max: f64,
    /// UI の刻み
    pub step: f64,
}

/// 選択肢の一覧
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOptions {
    /// 選択肢（表示順）
    pub options: Vec<SettingChoice>,
}

/// 設定項目の型（`{"type":"int","params":{"min":1,"max":200,"step":1}}`。`bool` / `text` は `params` なし）
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "params", rename_all = "snake_case")]
pub enum SettingKind {
    /// 真偽値（トグル）
    Bool,
    /// 整数
    Int(IntRange),
    /// 小数
    Float(FloatRange),
    /// 選択肢のいずれか（値は `SettingValue::Text`）
    Choice(ChoiceOptions),
    /// 自由入力の文字列
    Text,
}

/// 設定項目 1 件
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingItem {
    /// ドット区切りの TOML パス（例: `conversion.sync_budget_ms`）
    pub key: String,
    /// 表示グループ名（日本語）
    pub group: String,
    /// 表示名（日本語）
    pub label: String,
    /// 説明（日本語・1〜2 文）
    pub description: String,
    /// 型
    pub kind: SettingKind,
    /// 現在の有効値。`null` は「未指定でプロファイルが決める」項目（`conversion.beam_width` など）
    pub value: Option<SettingValue>,
    /// 既定値（`null` の意味は `value` と同じ）
    pub default: Option<SettingValue>,
    /// `config.toml` に明示指定があるか
    pub explicit: bool,
    /// 反映にエンジンの再起動が要るか
    pub requires_restart: bool,
}

/// `get_settings` の結果
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    /// 設定項目（表示順。同じ `group` が連続する）
    pub items: Vec<SettingItem>,
    /// 設定ファイルの構文が壊れている等で、値を読めなかった場合の理由（日本語）。
    /// このとき `items` の値はすべて既定値で、`update_settings` は書き込まない
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

/// 設定の変更 1 件
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingChange {
    /// 設定キー
    pub key: String,
    /// 新しい値。`null`（省略も可）なら明示指定を削除して既定（プロファイル任せ）に戻す
    #[serde(default)]
    pub value: Option<SettingValue>,
}

/// `update_settings` のパラメータ
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettingsParams {
    /// 変更のリスト（先頭から順に適用）
    pub changes: Vec<SettingChange>,
}

/// 項目ごとのエラー
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingError {
    /// 設定キー
    pub key: String,
    /// メッセージ（日本語）
    pub message: String,
}

/// `update_settings` の結果
#[typeshare]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettingsResult {
    /// 適用できたキー
    pub applied: Vec<String>,
    /// 適用できなかった項目（他の項目は適用済み）
    pub errors: Vec<SettingError>,
    /// 適用後の最新の状態
    pub snapshot: SettingsSnapshot,
}
