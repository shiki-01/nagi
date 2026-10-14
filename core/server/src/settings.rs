//! `get_settings` / `update_settings` の実装（記述子・検証・書き込みは `nagi_config::settings`、
//! ここはプロトコル型との変換とファイル入出力）。

use nagi_config::settings as cfg;
use nagi_protocol::settings::{
    ChoiceOptions, FloatRange, IntRange, SettingChange, SettingChoice, SettingError, SettingItem,
    SettingKind, SettingLevel, SettingPlatforms, SettingValue, SettingsSnapshot,
};
use std::path::Path;

fn value_to_proto(v: cfg::SettingValue) -> SettingValue {
    match v {
        cfg::SettingValue::Bool(b) => SettingValue::Bool(b),
        cfg::SettingValue::Int(i) => SettingValue::Int(i),
        cfg::SettingValue::Float(f) => SettingValue::Float(f),
        cfg::SettingValue::Text(t) => SettingValue::Text(t),
    }
}

fn value_from_proto(v: SettingValue) -> cfg::SettingValue {
    match v {
        SettingValue::Bool(b) => cfg::SettingValue::Bool(b),
        SettingValue::Int(i) => cfg::SettingValue::Int(i),
        SettingValue::Float(f) => cfg::SettingValue::Float(f),
        SettingValue::Text(t) => cfg::SettingValue::Text(t),
    }
}

fn kind_to_proto(k: cfg::SettingKind) -> SettingKind {
    match k {
        cfg::SettingKind::Bool => SettingKind::Bool,
        cfg::SettingKind::Int { min, max, step } => SettingKind::Int(IntRange { min, max, step }),
        cfg::SettingKind::Float { min, max, step } => {
            SettingKind::Float(FloatRange { min, max, step })
        }
        cfg::SettingKind::Choice { options } => SettingKind::Choice(ChoiceOptions {
            options: options
                .into_iter()
                .map(|o| SettingChoice {
                    value: o.value,
                    label: o.label,
                })
                .collect(),
        }),
        cfg::SettingKind::Text => SettingKind::Text,
    }
}

fn item(s: cfg::SettingState) -> SettingItem {
    SettingItem {
        key: s.def.key.to_string(),
        group: s.def.group.to_string(),
        label: s.def.label.to_string(),
        description: s.def.description.to_string(),
        kind: kind_to_proto(s.def.kind),
        value: s.value.map(value_to_proto),
        default: s.default.map(value_to_proto),
        explicit: s.explicit,
        requires_restart: s.def.requires_restart,
        level: match s.def.level {
            cfg::SettingLevel::Simple => SettingLevel::Simple,
            cfg::SettingLevel::Detail => SettingLevel::Detail,
        },
        platforms: match s.def.platforms {
            cfg::SettingPlatforms::Desktop => SettingPlatforms::Desktop,
            cfg::SettingPlatforms::Ios => SettingPlatforms::Ios,
            cfg::SettingPlatforms::Both => SettingPlatforms::Both,
        },
    }
}

/// 設定ファイルの中身（ファイルが無ければ `None`）。読めなければ理由
fn read_source(path: Option<&Path>) -> Result<Option<String>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("設定ファイルを読み込めません（{}）", e.kind())),
    }
}

/// 現在の設定項目と値を返す。ファイルが壊れていれば既定値と `error` を返す
pub fn snapshot(path: Option<&Path>) -> SettingsSnapshot {
    let loaded = read_source(path).and_then(|s| cfg::read(s.as_deref()));
    let (states, error) = match loaded {
        Ok(states) => (states, None),
        Err(e) => (cfg::read(None).unwrap_or_default(), Some(e)),
    };
    SettingsSnapshot {
        items: states.into_iter().map(item).collect(),
        error,
    }
}

/// [`apply`] の結果
pub struct Outcome {
    /// 適用できたキー
    pub applied: Vec<String>,
    /// 適用できなかった項目
    pub errors: Vec<SettingError>,
}

/// 変更を検証して `config.toml` に書き込む（原子的）。適用できた項目が無ければ書き込まない
pub fn apply(path: Option<&Path>, changes: Vec<SettingChange>) -> Outcome {
    let reject_all = |message: String| Outcome {
        applied: Vec::new(),
        errors: changes
            .iter()
            .map(|c| SettingError {
                key: c.key.clone(),
                message: message.clone(),
            })
            .collect(),
    };
    let Some(path) = path else {
        return reject_all("設定ファイルの場所が決まっていないため保存できません".to_string());
    };
    let source = match read_source(Some(path)) {
        Ok(s) => s,
        Err(e) => return reject_all(e),
    };
    let requested: Vec<(String, Option<cfg::SettingValue>)> = changes
        .iter()
        .map(|c| (c.key.clone(), c.value.clone().map(value_from_proto)))
        .collect();
    let result = match cfg::apply_changes(source.as_deref(), &requested) {
        Ok(r) => r,
        Err(e) => return reject_all(format!("{e}。config.toml を直してから再度お試しください")),
    };
    let mut errors: Vec<SettingError> = result
        .errors
        .into_iter()
        .map(|e| SettingError {
            key: e.key,
            message: e.message,
        })
        .collect();
    let mut applied = result.applied;
    if !applied.is_empty()
        && let Err(e) = cfg::write_atomic(path, &result.source)
    {
        let message = format!("設定ファイルに書き込めません（{}）", e.kind());
        errors.extend(applied.drain(..).map(|key| SettingError {
            key,
            message: message.clone(),
        }));
    }
    Outcome { applied, errors }
}
