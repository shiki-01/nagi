//! GUI 設定画面向けの設定項目の記述子と、`config.toml` の（コメントを保つ）書き換え。
//!
//! フロントは汎用フォームを描画するだけにするため、GUI から変更できる項目（キー・表示名・型・範囲）を
//! ここで定義する。書き込みは `toml_edit` で行い、既存のコメント・書式・未知のキーを保つ。

use crate::{Config, ProfileMode};
use std::path::Path;

/// 設定値
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    /// 真偽値
    Bool(bool),
    /// 整数
    Int(i32),
    /// 小数
    Float(f64),
    /// 文字列（選択肢の値も含む）
    Text(String),
}

/// 選択肢
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    /// TOML に書く値
    pub value: String,
    /// 表示名（日本語）
    pub label: String,
}

/// 設定項目の型
#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    /// 真偽値
    Bool,
    /// 整数（範囲 `min..=max`、UI の刻み `step`）
    Int {
        /// 最小値
        min: i32,
        /// 最大値
        max: i32,
        /// 刻み
        step: i32,
    },
    /// 小数
    Float {
        /// 最小値
        min: f64,
        /// 最大値
        max: f64,
        /// 刻み
        step: f64,
    },
    /// 選択肢のいずれか
    Choice {
        /// 選択肢
        options: Vec<ChoiceOption>,
    },
    /// 自由入力の文字列
    Text,
}

/// 設定項目の記述子（静的な情報）
#[derive(Debug, Clone, PartialEq)]
pub struct SettingDef {
    /// ドット区切りの TOML パス（`conversion.sync_budget_ms`）
    pub key: &'static str,
    /// 表示グループ名
    pub group: &'static str,
    /// 表示名
    pub label: &'static str,
    /// 説明（1〜2 文）
    pub description: &'static str,
    /// 型
    pub kind: SettingKind,
    /// 反映に再起動が要るか
    pub requires_restart: bool,
}

/// 現在の状態つきの設定項目
#[derive(Debug, Clone, PartialEq)]
pub struct SettingState {
    /// 記述子
    pub def: SettingDef,
    /// 現在の有効値。`None` は「未指定でプロファイル等が決める」項目（`beam_width` など）
    pub value: Option<SettingValue>,
    /// 既定値（`None` は同上）
    pub default: Option<SettingValue>,
    /// `config.toml` に明示指定があるか
    pub explicit: bool,
}

/// 項目ごとのエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemError {
    /// 設定キー
    pub key: String,
    /// メッセージ（日本語。入力値は含めない）
    pub message: String,
}

/// [`apply_changes`] の結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// 書き換え後の TOML 全文
    pub source: String,
    /// 適用できたキー
    pub applied: Vec<String>,
    /// 適用できなかった項目
    pub errors: Vec<ItemError>,
}

fn choice(options: &[(&str, &str)]) -> SettingKind {
    SettingKind::Choice {
        options: options
            .iter()
            .map(|(v, l)| ChoiceOption {
                value: (*v).to_string(),
                label: (*l).to_string(),
            })
            .collect(),
    }
}

fn int(min: i32, max: i32, step: i32) -> SettingKind {
    SettingKind::Int { min, max, step }
}

fn float(min: f64, max: f64, step: f64) -> SettingKind {
    SettingKind::Float { min, max, step }
}

fn def(
    key: &'static str,
    group: &'static str,
    label: &'static str,
    description: &'static str,
    kind: SettingKind,
) -> SettingDef {
    SettingDef {
        key,
        group,
        label,
        description,
        kind,
        requires_restart: false,
    }
}

/// GUI から変更できる設定項目の一覧（表示順）
pub fn definitions() -> Vec<SettingDef> {
    let mut update_check = def(
        "update.auto_check",
        "アップデート",
        "アップデートを自動で確認",
        "起動時に新しいバージョンがないか確認し、あれば通知します。ダウンロードやインストールは行いません。",
        SettingKind::Bool,
    );
    update_check.requires_restart = true;
    vec![
        def(
            "profile.mode",
            "動作モード",
            "プロファイル",
            "変換の品質と軽さのバランスです。「自動」では実行環境（電源・負荷など）に応じて切り替えます。",
            choice(&[
                ("auto", "自動"),
                ("quality", "品質優先"),
                ("balanced", "バランス"),
                ("light", "軽量"),
                ("minimal", "辞書のみ"),
            ]),
        ),
        def(
            "profile.auto_switch",
            "動作モード",
            "環境に応じて自動で切り替え",
            "電源・発熱・メモリ逼迫・変換の遅れに応じて、プロファイルを実行中に切り替えます。",
            SettingKind::Bool,
        ),
        def(
            "profile.persona",
            "動作モード",
            "ペルソナ",
            "文体の好みなどを自由に書きます。スタイル変換などで使われます（将来の機能）。",
            SettingKind::Text,
        ),
        def(
            "conversion.live_conversion",
            "変換",
            "ライブ変換",
            "Space を押さなくても、入力に合わせて自動で漢字に変換します。",
            SettingKind::Bool,
        ),
        def(
            "conversion.live_conversion_delay_ms",
            "変換",
            "ライブ変換の待ち時間（ミリ秒）",
            "入力が止まってから変換するまでの時間です。短いほど反応が速い代わりに負荷が上がります。",
            int(0, 5000, 10),
        ),
        def(
            "conversion.composing_chunk_len",
            "変換",
            "ライブ変換のチャンク長（文字数）",
            "長い入力を分けて変換する単位です。小さいほど反応が速い代わりに、文脈が切れやすくなります。",
            int(1, 200, 1),
        ),
        def(
            "conversion.use_llm",
            "変換",
            "LLM による変換",
            "オフにすると辞書のみで変換します（軽くなりますが、精度は下がります）。",
            SettingKind::Bool,
        ),
        def(
            "conversion.sync_budget_ms",
            "変換",
            "Space 変換の待ち時間（ミリ秒）",
            "Space を押してから LLM の結果を待つ最大時間です。超えるとまず読みのまま表示し、結果が出たら差し替えます。",
            int(10, 5000, 10),
        ),
        def(
            "conversion.beam_width",
            "変換",
            "ビーム幅",
            "LLM が同時に検討する候補の数です。大きいほど精度が上がりますが遅くなります。未指定ならプロファイルに従います。",
            int(1, 16, 1),
        ),
        def(
            "conversion.num_candidates",
            "変換",
            "候補ウィンドウの 1 ページの候補数",
            "候補ウィンドウに一度に表示する候補の数です。",
            int(1, 50, 1),
        ),
        def(
            "conversion.use_context",
            "変換",
            "前後の文脈を使う",
            "カーソル前後の文章を読み取り、変換の精度向上に使います。パスワード入力欄などでは自動で無効になります。",
            SettingKind::Bool,
        ),
        def(
            "conversion.initial_view",
            "変換",
            "変換開始時の表示",
            "Space で変換を始めたときに、文全体の候補と先頭文節の候補のどちらを表示するかを選びます。",
            choice(&[("sentence", "文全体"), ("segment", "先頭の文節")]),
        ),
        def(
            "input.shift_letter",
            "入力",
            "Shift + 英字の動作",
            "「続けて入力」では大文字から始めて後続も英字のまま入力できます（例: Nagi）。",
            choice(&[
                ("literal_continue", "続けて英字入力"),
                ("single_upper", "大文字 1 文字だけ"),
            ]),
        ),
        def(
            "input.punctuation",
            "入力",
            "句読点の種類",
            "「、」「。」キーで入力する句読点の組み合わせです。",
            choice(&[("、。", "、。"), ("，．", "，．"), ("，。", "，。")]),
        ),
        def(
            "learning.enabled",
            "学習",
            "学習する",
            "確定した変換結果を記録し、次回以降の候補の順位に反映します。",
            SettingKind::Bool,
        ),
        def(
            "learning.max_entries",
            "学習",
            "学習データの最大件数",
            "これを超えると古いものから忘れます。",
            int(100, 1_000_000, 100),
        ),
        def(
            "predict.enabled",
            "予測変換",
            "予測変換",
            "入力中の読みから、学習履歴やユーザー辞書の候補を先回りして表示します。",
            SettingKind::Bool,
        ),
        def(
            "predict.min_chars",
            "予測変換",
            "予測を始める文字数",
            "読みがこの文字数以上になったら予測を表示します。",
            int(1, 20, 1),
        ),
        def(
            "fuzzy.enabled",
            "曖昧入力",
            "曖昧入力（補正候補）",
            "打ち間違いやタッチのずれを想定した読みを、Space 変換のときに補正候補として追加します。打った読みは選ぶまで変わりません。",
            SettingKind::Bool,
        ),
        def(
            "fuzzy.sigma",
            "曖昧入力",
            "タッチ誤差の大きさ",
            "小さいほど、隣のキーとの取り違えを疑いにくくなります（キー幅に対する比）。",
            float(0.05, 2.0, 0.05),
        ),
        def(
            "fuzzy.max_distance",
            "曖昧入力",
            "代替キーとして扱う最大距離",
            "タッチ位置からこの距離（キー幅比）を超えるキーは、取り違えの候補にしません。",
            float(0.0, 3.0, 0.1),
        ),
        def(
            "fuzzy.max_edits",
            "曖昧入力",
            "タイポ補正の最大回数",
            "打鍵の欠落・余剰・入れ替えを直せる回数です。0 でタイポ補正を使いません。",
            int(0, 3, 1),
        ),
        def(
            "fuzzy.window",
            "曖昧入力",
            "タイポ補正の対象打鍵数",
            "入力の末尾からこの打鍵数だけタイポ補正を試みます。大きいほど遅くなります。",
            int(1, 32, 1),
        ),
        def(
            "fuzzy.beam",
            "曖昧入力",
            "探索の幅",
            "読みの候補を同時に検討する数です。大きいほど見落としが減りますが遅くなります。",
            int(1, 64, 1),
        ),
        def(
            "fuzzy.max_cost",
            "曖昧入力",
            "補正の許容コスト",
            "入力のずれの大きさがこの値を超える読みは、補正候補にしません。",
            float(0.0, 30.0, 0.5),
        ),
        def(
            "fuzzy.hypotheses",
            "曖昧入力",
            "検討する読みの数",
            "打った読みを含めて、変換にかける読みの最大数です。補正候補はこの数より 1 つ少なくなります。",
            int(1, 8, 1),
        ),
        def(
            "fuzzy.input_weight",
            "曖昧入力",
            "入力のずれの重み",
            "大きいほど、打った読みに近い補正候補を優先します。",
            float(0.0, 10.0, 0.1),
        ),
        def(
            "fuzzy.ambiguity_margin",
            "曖昧入力",
            "補正を検討する曖昧さ",
            "打った読みとの差がこの範囲内の補正だけを候補に残します。大きいほど多くの補正候補が出ます。",
            float(0.0, 30.0, 0.5),
        ),
        def(
            "fuzzy.poor_coverage",
            "曖昧入力",
            "補正を検討する未知語の割合",
            "打った読みのうち辞書に載らない部分がこの割合以上のときは、常に補正を検討します。",
            float(0.0, 1.0, 0.05),
        ),
        def(
            "fuzzy.promote_corrected",
            "曖昧入力",
            "補正候補を先頭に出す",
            "打った読みがほとんど辞書に載らないときだけ、補正候補を先頭に出します。",
            SettingKind::Bool,
        ),
        update_check,
    ]
}

/// 設定値の現在値（`Config` から取り出す）。未指定の任意項目は `None`
fn from_config(config: &Config, key: &str) -> Option<SettingValue> {
    use SettingValue::{Bool, Float, Int, Text};
    let n = |v: usize| Int(i32::try_from(v).unwrap_or(i32::MAX));
    match key {
        "profile.mode" => Some(Text(config.profile.mode.as_str().to_string())),
        "profile.auto_switch" => Some(Bool(config.profile.auto_switch)),
        "profile.persona" => Some(Text(config.profile.persona.clone())),
        "conversion.live_conversion" => Some(Bool(config.conversion.live_conversion)),
        "conversion.live_conversion_delay_ms" => Some(Int(i32::try_from(
            config.conversion.live_conversion_delay_ms,
        )
        .unwrap_or(i32::MAX))),
        "conversion.composing_chunk_len" => Some(n(config.conversion.composing_chunk_len)),
        "conversion.use_llm" => Some(Bool(config.conversion.use_llm)),
        "conversion.sync_budget_ms" => Some(Int(
            i32::try_from(config.conversion.sync_budget_ms).unwrap_or(i32::MAX)
        )),
        "conversion.beam_width" => config.conversion.beam_width.map(n),
        "conversion.num_candidates" => Some(n(config.conversion.num_candidates)),
        "conversion.use_context" => Some(Bool(config.conversion.use_context)),
        "conversion.initial_view" => {
            Some(Text(config.conversion.initial_view.as_str().to_string()))
        }
        "input.shift_letter" => Some(Text(config.input.shift_letter.as_str().to_string())),
        "input.punctuation" => {
            let p = &config.input.punctuation;
            Some(Text(format!("{}{}", p.comma, p.period)))
        }
        "learning.enabled" => Some(Bool(config.learning.enabled)),
        "learning.max_entries" => Some(n(config.learning.max_entries)),
        "predict.enabled" => Some(Bool(config.predict.enabled)),
        "predict.min_chars" => Some(n(config.predict.min_chars)),
        "fuzzy.enabled" => Some(Bool(config.fuzzy.enabled)),
        "fuzzy.sigma" => Some(Float(config.fuzzy.sigma)),
        "fuzzy.max_distance" => Some(Float(config.fuzzy.max_distance)),
        "fuzzy.max_edits" => Some(n(config.fuzzy.max_edits)),
        "fuzzy.window" => Some(n(config.fuzzy.window)),
        "fuzzy.beam" => Some(n(config.fuzzy.beam)),
        "fuzzy.max_cost" => Some(Float(config.fuzzy.max_cost)),
        "fuzzy.hypotheses" => Some(n(config.fuzzy.hypotheses)),
        "fuzzy.input_weight" => Some(Float(config.fuzzy.input_weight)),
        "fuzzy.ambiguity_margin" => Some(Float(config.fuzzy.ambiguity_margin)),
        "fuzzy.poor_coverage" => Some(Float(config.fuzzy.poor_coverage)),
        "fuzzy.promote_corrected" => Some(Bool(config.fuzzy.promote_corrected)),
        "update.auto_check" => Some(Bool(config.update.auto_check)),
        _ => None,
    }
}

/// 値が項目の型・範囲・選択肢に合っているか。合わなければ日本語のメッセージ
pub fn validate_value(def: &SettingDef, value: &SettingValue) -> Result<(), String> {
    match (&def.kind, value) {
        (SettingKind::Bool, SettingValue::Bool(_)) => Ok(()),
        (SettingKind::Int { min, max, .. }, SettingValue::Int(v)) => {
            if (*min..=*max).contains(v) {
                Ok(())
            } else {
                Err(format!("{min} から {max} の間で指定してください"))
            }
        }
        (SettingKind::Float { min, max, .. }, SettingValue::Float(v)) => {
            if v.is_finite() && (*min..=*max).contains(v) {
                Ok(())
            } else {
                Err(format!("{min} から {max} の間で指定してください"))
            }
        }
        (SettingKind::Float { min, max, .. }, SettingValue::Int(v)) => {
            if (*min..=*max).contains(&f64::from(*v)) {
                Ok(())
            } else {
                Err(format!("{min} から {max} の間で指定してください"))
            }
        }
        (SettingKind::Choice { options }, SettingValue::Text(v)) => {
            if options.iter().any(|o| &o.value == v) {
                Ok(())
            } else {
                Err("選択肢にない値です".to_string())
            }
        }
        (SettingKind::Text, SettingValue::Text(v)) => {
            if v.chars().count() > 200 {
                Err("200 文字以内で指定してください".to_string())
            } else if v.chars().any(char::is_control) {
                Err("改行などの制御文字は使えません".to_string())
            } else {
                Ok(())
            }
        }
        _ => Err("値の型が項目と合いません".to_string()),
    }
}

fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once('.')
}

/// TOML の値を項目の型に読み替える。型が合わなければ `None`
fn read_toml_value(def: &SettingDef, item: &toml_edit::Item) -> Option<SettingValue> {
    let v = item.as_value()?;
    let value = match &def.kind {
        SettingKind::Bool => SettingValue::Bool(v.as_bool()?),
        SettingKind::Int { .. } => SettingValue::Int(i32::try_from(v.as_integer()?).ok()?),
        SettingKind::Float { .. } => {
            SettingValue::Float(v.as_float().or_else(|| v.as_integer().map(|i| i as f64))?)
        }
        SettingKind::Choice { .. } | SettingKind::Text => {
            SettingValue::Text(v.as_str()?.to_string())
        }
    };
    validate_value(def, &value).ok()?;
    Some(value)
}

/// `source`（`config.toml` の全文。無ければ `None`）から、各項目の現在値と明示指定の有無を読む。
/// 他の部分にエラーがあっても、読める項目は読む。TOML の構文が壊れていれば `Err`
pub fn read(source: Option<&str>) -> Result<Vec<SettingState>, String> {
    let doc: toml_edit::DocumentMut = source
        .unwrap_or("")
        .trim_start_matches('\u{feff}')
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("config.toml の構文エラー: {}", e.message()))?;
    let defaults = Config::default();
    Ok(definitions()
        .into_iter()
        .map(|def| {
            let default = from_config(&defaults, def.key);
            let explicit_value = split_key(def.key)
                .and_then(|(s, k)| doc.get(s)?.get(k))
                .and_then(|item| read_toml_value(&def, item));
            let explicit = explicit_value.is_some();
            SettingState {
                value: explicit_value.or_else(|| default.clone()),
                default,
                explicit,
                def,
            }
        })
        .collect())
}

fn to_toml_value(value: &SettingValue) -> toml_edit::Value {
    match value {
        SettingValue::Bool(b) => (*b).into(),
        SettingValue::Int(i) => i64::from(*i).into(),
        SettingValue::Float(f) => (*f).into(),
        SettingValue::Text(s) => s.as_str().into(),
    }
}

/// 変更（`None` は明示指定の削除）を `source` に適用する。コメント・書式・未知のキーは保つ。
/// 検証に通らない項目だけをエラーにし、残りは適用する。TOML の構文が壊れていれば `Err`
pub fn apply_changes(
    source: Option<&str>,
    changes: &[(String, Option<SettingValue>)],
) -> Result<Applied, String> {
    let text = source.unwrap_or("");
    let bom = text.starts_with('\u{feff}');
    let mut doc: toml_edit::DocumentMut = text
        .trim_start_matches('\u{feff}')
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("config.toml の構文エラー: {}", e.message()))?;
    let defs = definitions();
    let mut applied = Vec::new();
    let mut errors = Vec::new();
    for (key, value) in changes {
        let fail = |message: &str| ItemError {
            key: key.clone(),
            message: message.to_string(),
        };
        let Some(def) = defs.iter().find(|d| d.key == key) else {
            errors.push(fail("未知の設定キーです"));
            continue;
        };
        let (section, name) = split_key(def.key).unwrap_or((def.key, ""));
        if let Some(value) = value
            && let Err(message) = validate_value(def, value)
        {
            errors.push(fail(&message));
            continue;
        }
        match value {
            Some(value) => {
                let table = match doc.entry(section).or_insert_with(toml_edit::table) {
                    toml_edit::Item::Table(t) => t,
                    _ => {
                        errors.push(fail("config.toml のセクションの形式が不正です"));
                        continue;
                    }
                };
                let mut new = to_toml_value(value);
                // 既存の値は、キー（直前のコメント）と値の前後の空白・行末コメントを保ったまま置き換える
                match table.get_mut(name).and_then(toml_edit::Item::as_value_mut) {
                    Some(old) => {
                        *new.decor_mut() = old.decor().clone();
                        *old = new;
                    }
                    None => {
                        table.insert(name, toml_edit::Item::Value(new));
                    }
                }
            }
            None => {
                if let Some(table) = doc.get_mut(section).and_then(toml_edit::Item::as_table_mut) {
                    table.remove(name);
                }
            }
        }
        applied.push(key.clone());
    }
    let mut out = doc.to_string();
    if bom {
        out.insert(0, '\u{feff}');
    }
    Ok(Applied {
        source: out,
        applied,
        errors,
    })
}

/// `path` の内容を原子的に置き換える（同じディレクトリの一時ファイルへ書いてから rename）
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let result = std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// `ProfileMode` の選択肢と一致しているかの検査用（テストから使う）
#[doc(hidden)]
pub fn profile_mode_values() -> Vec<&'static str> {
    [
        ProfileMode::Auto,
        ProfileMode::Quality,
        ProfileMode::Balanced,
        ProfileMode::Light,
        ProfileMode::Minimal,
    ]
    .iter()
    .map(ProfileMode::as_str)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(key: &str, v: SettingValue) -> (String, Option<SettingValue>) {
        (key.to_string(), Some(v))
    }

    #[test]
    fn defaults_match_config_default_and_default_toml() {
        let (from_toml, _) = crate::parse(crate::DEFAULT_TOML).unwrap();
        let states = read(None).unwrap();
        assert_eq!(states.len(), definitions().len());
        for s in &states {
            assert!(!s.explicit, "{}", s.def.key);
            assert_eq!(s.value, s.default, "{}", s.def.key);
            assert_eq!(
                s.default,
                from_config(&from_toml, s.def.key),
                "default.toml と Config::default が食い違い: {}",
                s.def.key
            );
            // 既定値は自身の検証に通る
            if let Some(v) = &s.default {
                assert_eq!(validate_value(&s.def, v), Ok(()), "{}", s.def.key);
            }
        }
    }

    #[test]
    fn every_definition_key_is_a_known_config_key() {
        // 記述子のキーが Config に実在する（typo 防止）: 全項目を書いても警告が出ない
        let mut src = String::new();
        for s in read(None).unwrap() {
            if let (Some((sec, k)), Some(v)) = (split_key(s.def.key), &s.default) {
                let lit = match v {
                    SettingValue::Bool(b) => b.to_string(),
                    SettingValue::Int(i) => i.to_string(),
                    SettingValue::Float(f) => f.to_string(),
                    SettingValue::Text(t) => format!("{t:?}"),
                };
                src.push_str(&format!("{sec}.{k} = {lit}\n"));
            }
        }
        let (_, issues) = crate::parse(&src).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn profile_choices_match_enum() {
        let defs = definitions();
        let d = defs.iter().find(|d| d.key == "profile.mode").unwrap();
        let SettingKind::Choice { options } = &d.kind else {
            panic!()
        };
        let values: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, profile_mode_values());
    }

    #[test]
    fn apply_preserves_comments_and_unknown_keys() {
        let src = "# 先頭のコメント\n[conversion]\n# 待ち時間\nsync_budget_ms = 150 # 行末\nfuture_key = 1\n\n[keymap.composing]\n\"Ctrl+j\" = \"ToKatakana\"\n";
        let r = apply_changes(
            Some(src),
            &[
                set("conversion.sync_budget_ms", SettingValue::Int(300)),
                set("learning.enabled", SettingValue::Bool(false)),
            ],
        )
        .unwrap();
        assert_eq!(r.applied.len(), 2);
        assert!(r.errors.is_empty());
        assert!(r.source.contains("# 先頭のコメント"));
        assert!(r.source.contains("# 待ち時間\nsync_budget_ms = 300 # 行末"));
        assert!(r.source.contains("future_key = 1"));
        assert!(r.source.contains("\"Ctrl+j\" = \"ToKatakana\""));
        assert!(r.source.contains("[learning]\nenabled = false"));
        let (config, _) = crate::parse(&r.source).unwrap();
        assert_eq!(config.conversion.sync_budget_ms, 300);
        assert!(!config.learning.enabled);
        // 読み戻すと explicit
        let s = read(Some(&r.source)).unwrap();
        let st = s
            .iter()
            .find(|s| s.def.key == "conversion.sync_budget_ms")
            .unwrap();
        assert!(st.explicit);
        assert_eq!(st.value, Some(SettingValue::Int(300)));
    }

    #[test]
    fn invalid_items_error_and_others_apply() {
        let r = apply_changes(
            None,
            &[
                set("conversion.sync_budget_ms", SettingValue::Int(1)),
                set("conversion.composing_chunk_len", SettingValue::Bool(true)),
                set("profile.mode", SettingValue::Text("turbo".into())),
                set("no.such_key", SettingValue::Int(1)),
                set("conversion.beam_width", SettingValue::Int(4)),
            ],
        )
        .unwrap();
        assert_eq!(r.applied, vec!["conversion.beam_width"]);
        assert_eq!(r.errors.len(), 4);
        assert!(!r.source.contains("sync_budget_ms"));
        assert!(r.source.contains("beam_width = 4"));
    }

    #[test]
    fn null_removes_explicit_value() {
        let src = "[conversion]\nbeam_width = 4\nsync_budget_ms = 200\n";
        let r = apply_changes(Some(src), &[("conversion.beam_width".into(), None)]).unwrap();
        assert!(!r.source.contains("beam_width"));
        assert!(r.source.contains("sync_budget_ms = 200"));
        let s = read(Some(&r.source)).unwrap();
        let b = s
            .iter()
            .find(|s| s.def.key == "conversion.beam_width")
            .unwrap();
        assert!(!b.explicit);
        assert_eq!(b.value, None);
    }

    #[test]
    fn broken_syntax_is_an_error() {
        assert!(apply_changes(Some("[conversion\n"), &[]).is_err());
        assert!(read(Some("= =")).is_err());
    }

    #[test]
    fn write_atomic_replaces_file() {
        let dir = std::env::temp_dir().join(format!("nagi-settings-{}", std::process::id()));
        let path = dir.join("sub").join("config.toml");
        write_atomic(&path, "a = 1\n").unwrap();
        write_atomic(&path, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
        assert!(!dir.join("sub").join("config.toml.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
