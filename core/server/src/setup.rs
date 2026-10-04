//! サーバーの組み立て: 起動オプション・設定ファイル・辞書・モデル・学習・ユーザー辞書を読み込み、
//! 変換パイプラインと共有状態（[`Runtime`]）を作る。設定の再読み込みの状態もここで持つ。
//!
//! 設定の優先順位: コマンドライン引数 > 設定ファイル（`config.toml`）> 既定値。
//! どの段階で失敗しても入力は続けられる（設定が壊れていれば既定値、辞書・モデルが無ければ縮退）。

use crate::pipeline::Pipeline;
use crate::runtime::{Event, Runtime, Tunables};
use nagi_config::{Config, InitialView as ConfigInitialView, Issue, Severity, ShiftLetter};
use nagi_dict::{Dictionary, UserDictionary};
use nagi_engine::EngineConfig;
use nagi_engine::InitialView;
use nagi_engine::KeyContext;
use nagi_infer::ConversionModel;
use nagi_learn::{LearnConfig, Learner};
use nagi_protocol::methods::NotifyLevel;
use nagi_protocol::view::InputMode;
use nagi_romaji::{PunctuationStyle, ShiftLetterMode};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

/// 既定の変換モデルのファイル名（`<data_dir>/models/` 下。`cargo xtask fetch-model` の出力）
pub const DEFAULT_MODEL_FILE: &str = "zenz-v3.1-small-Q5_K_M.gguf";

/// 起動オプション
#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// 取得済みデータ（`dict/nagi-dict.bin`、`models/*.gguf`）のディレクトリ。`--data-dir` または `NAGI_DATA_DIR`
    pub data_dir: Option<PathBuf>,
    /// ユーザーデータ（`config.toml`・学習・ユーザー辞書）のディレクトリ。`--user-dir`。
    /// 未指定なら設定ディレクトリ（`NAGI_CONFIG_DIR` / OS の既定）
    pub user_dir: Option<PathBuf>,
    /// 設定ファイルのパス。`--config`。未指定なら `<user_dir>/config.toml`
    pub config_path: Option<PathBuf>,
    /// LLM を使うか（`--no-llm` で無効化）。feature `llama` が無い、またはモデルが無ければ使えない
    pub use_llm: bool,
    /// LLM の推論を待つ最長時間（ミリ秒、`--sync-budget-ms`）。指定すると設定ファイルより優先
    pub sync_budget_ms: Option<u64>,
    /// モデルの読み込みをバックグラウンドで行う（`imserver` の本番経路）。`build` は読み込みを待たずに
    /// 返り、読み込み中は辞書のみで動く。`false`（既定）なら読み込み完了まで `build` が返らない
    /// （`nagi-eval` など、起動直後からモデルを使う利用側向け）
    pub async_model_load: bool,
}

impl Default for ServerOptions {
    fn default() -> Self {
        ServerOptions {
            data_dir: None,
            user_dir: None,
            config_path: None,
            use_llm: true,
            sync_budget_ms: None,
            async_model_load: false,
        }
    }
}

/// 引数から起動オプションを作る（`--data-dir` `--user-dir` `--config` `--no-llm` `--sync-budget-ms`、
/// 環境変数 `NAGI_DATA_DIR`）
pub fn parse_args<I: Iterator<Item = String>>(mut args: I) -> Result<ServerOptions, String> {
    let mut options = ServerOptions {
        data_dir: std::env::var_os("NAGI_DATA_DIR").map(PathBuf::from),
        ..ServerOptions::default()
    };
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} に値がありません"))
        };
        match arg.as_str() {
            "--data-dir" => options.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--user-dir" => options.user_dir = Some(PathBuf::from(value("--user-dir")?)),
            "--config" => options.config_path = Some(PathBuf::from(value("--config")?)),
            "--no-llm" => options.use_llm = false,
            "--sync-budget-ms" => {
                let v = value("--sync-budget-ms")?;
                options.sync_budget_ms = Some(
                    v.parse()
                        .map_err(|_| format!("--sync-budget-ms が数値ではありません: {v}"))?,
                );
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    Ok(options)
}

/// 設定から engine の設定を作る。キーマップの上書きに解析できないものがあれば、その警告を
/// 通知イベントとして合わせて返す
pub fn engine_config_from(config: &Config) -> (EngineConfig, Vec<Event>) {
    let punct = &config.input.punctuation;
    let bindings: Vec<(KeyContext, String, String)> = config
        .keymap
        .composing
        .iter()
        .map(|(k, v)| (KeyContext::Composing, k.clone(), v.clone()))
        .chain(
            config
                .keymap
                .converting
                .iter()
                .map(|(k, v)| (KeyContext::Converting, k.clone(), v.clone())),
        )
        .collect();
    let (keymap, warnings) = nagi_engine::KeyMap::with_overrides(&bindings);
    let events = warnings
        .into_iter()
        .map(|message| Event {
            level: NotifyLevel::Warning,
            code: "keymap_warning",
            message,
        })
        .collect();
    (
        EngineConfig {
            shift_letter: match config.input.shift_letter {
                ShiftLetter::LiteralContinue => ShiftLetterMode::LiteralContinue,
                ShiftLetter::SingleUpper => ShiftLetterMode::SingleUpper,
            },
            punctuation: PunctuationStyle::from_config_str(&format!(
                "{}{}",
                punct.comma, punct.period
            ))
            .unwrap_or(PunctuationStyle::Japanese),
            page_size: config.conversion.num_candidates,
            initial_mode: InputMode::Hiragana,
            keymap,
            predict_enabled: config.predict.enabled,
            predict_min_chars: config.predict.min_chars,
            composing_chunk_len: config.conversion.composing_chunk_len,
            initial_view: match config.conversion.initial_view {
                ConfigInitialView::Sentence => InitialView::Sentence,
                ConfigInitialView::Segment => InitialView::Segment,
            },
            beam_width: config
                .conversion
                .beam_width
                .unwrap_or(EngineConfig::default().beam_width),
            fuzzy: fuzzy_engine_config(&config.fuzzy),
        },
        events,
    )
}

/// `[fuzzy]` をエンジンの設定に直す。プロファイル別の上限（`minimal` は `max_edits = 0` など。
/// 設計 §7）は F-1 では未対応で、設定値をそのまま使う
pub fn fuzzy_engine_config(f: &nagi_config::FuzzySection) -> nagi_engine::FuzzyEngineConfig {
    nagi_engine::FuzzyEngineConfig {
        enabled: f.enabled,
        search: nagi_fuzzy::FuzzyConfig {
            max_edits: u8::try_from(f.max_edits).unwrap_or(u8::MAX),
            window: f.window,
            beam: f.beam,
            max_cost: f.max_cost as f32,
            max_hypotheses: f.hypotheses,
            sigma: f.sigma as f32,
            max_distance: f.max_distance as f32,
            ..nagi_fuzzy::FuzzyConfig::default()
        },
        input_weight: f.input_weight as f32,
        ambiguity_margin: f.ambiguity_margin as f32,
        poor_coverage: f.poor_coverage as f32,
        promote_corrected: f.promote_corrected,
    }
}

/// 設定ファイルの問題を通知（`notify`）に直す
pub fn issue_events(path: Option<&Path>, issues: &[Issue]) -> Vec<Event> {
    let file = path
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("config.toml");
    issues
        .iter()
        .map(|i| {
            let place = if i.line > 0 {
                format!("{file} の {} 行目", i.line)
            } else {
                file.to_string()
            };
            match i.severity {
                Severity::Error => Event {
                    level: NotifyLevel::Error,
                    code: "config_error",
                    message: format!(
                        "設定エラー（{place}）: {}。以前の設定で動作します",
                        i.message
                    ),
                },
                Severity::Warning => Event {
                    level: NotifyLevel::Warning,
                    code: "config_warning",
                    message: format!("設定の警告（{place}）: {}", i.message),
                },
            }
        })
        .collect()
}

/// ファイルの更新時刻（存在しなければ `None`）
fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// ディレクトリ内のユーザー辞書ファイルの「変更の目印」（ファイル数と最新の更新時刻）
fn dir_stamp(dir: &Path) -> (usize, Option<SystemTime>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return (0, None);
    };
    let mut count = 0;
    let mut latest: Option<SystemTime> = None;
    for e in read.filter_map(Result::ok) {
        count += 1;
        if let Some(t) = e.metadata().ok().and_then(|m| m.modified().ok()) {
            latest = Some(latest.map_or(t, |l| l.max(t)));
        }
    }
    (count, latest)
}

/// 再読み込みに必要な状態（設定・ユーザー辞書の場所と、最後に読んだときの目印）
pub struct Reloadable {
    /// 設定ファイル
    pub config_path: Option<PathBuf>,
    /// ユーザー辞書のディレクトリ
    pub user_dict_dir: Option<PathBuf>,
    /// システム辞書（オーバーレイの品詞対応づけに使う）
    pub dict: Option<Arc<Dictionary>>,
    /// コマンドラインで上書きされた待ち時間
    pub cli_sync_budget_ms: Option<u64>,
    /// 起動時に使った変換モデルの指定（変更は次回起動時に反映）
    pub applied_model: Option<String>,
    config_stamp: Option<SystemTime>,
    dict_stamp: (usize, Option<SystemTime>),
    last_poll: Instant,
}

impl Reloadable {
    /// 何も監視しない（テスト用）
    pub fn detached() -> Self {
        Reloadable {
            config_path: None,
            user_dict_dir: None,
            dict: None,
            cli_sync_budget_ms: None,
            applied_model: None,
            config_stamp: None,
            dict_stamp: (0, None),
            last_poll: Instant::now(),
        }
    }

    /// 前回の読み込みから変化があったか。`poll_interval` 未満の間隔では確認しない
    pub fn changed(&mut self, poll_interval: std::time::Duration) -> bool {
        if self.last_poll.elapsed() < poll_interval {
            return false;
        }
        self.last_poll = Instant::now();
        let config = self.config_path.as_deref().and_then(mtime);
        let dict = self.user_dict_dir.as_deref().map_or((0, None), dir_stamp);
        let changed = config != self.config_stamp || dict != self.dict_stamp;
        self.config_stamp = config;
        self.dict_stamp = dict;
        changed
    }

    /// 現在の目印を記録し直す（読み込み直後）
    pub fn mark_read(&mut self) {
        self.config_stamp = self.config_path.as_deref().and_then(mtime);
        self.dict_stamp = self.user_dict_dir.as_deref().map_or((0, None), dir_stamp);
        self.last_poll = Instant::now();
    }
}

/// [`build`] の結果
pub struct Built {
    /// 変換パイプライン
    pub pipeline: Arc<Pipeline>,
    /// 共有状態
    pub runtime: Arc<Runtime>,
    /// 読み込んだ設定
    pub config: Config,
    /// 再読み込みの状態
    pub reload: Reloadable,
    /// 起動時の診断メッセージ（stderr 用。入力内容は含まない）
    pub notes: Vec<String>,
}

/// システム辞書を読み込む
pub fn load_dictionary(data_dir: &Path) -> Result<Arc<Dictionary>, String> {
    let path = data_dir.join("dict").join("nagi-dict.bin");
    if !path.exists() {
        return Err(format!("辞書が見つかりません: {}", path.display()));
    }
    // feature `mmap` ではファイルを mmap してゼロコピーで参照する（常駐メモリが小さい）。
    // mmap に失敗したら `fs::read` で全量をヒープに読む
    #[cfg(feature = "mmap")]
    match Dictionary::load_mmap(&path) {
        Ok(dict) => return Ok(Arc::new(dict)),
        // 形式の不正（バージョン違い等）は読み直しても同じなのでそのまま返す
        Err(e @ nagi_dict::DictError::Format(_)) => {
            return Err(format!("辞書の読み込みに失敗しました: {e}"));
        }
        Err(e) => eprintln!("辞書の mmap に失敗したため通常読み込みにします: {e}"),
    }
    Dictionary::load(&path)
        .map(Arc::new)
        .map_err(|e| format!("辞書の読み込みに失敗しました: {e}"))
}

#[cfg(feature = "llama")]
fn load_model(
    data_dir: &Path,
    model_setting: Option<&str>,
    n_threads: usize,
) -> Result<Box<dyn ConversionModel>, String> {
    use nagi_infer::llama::{LlamaModel, LlamaOptions};
    let path = match model_setting {
        Some(p) if Path::new(p).is_absolute() || p.ends_with(".gguf") => PathBuf::from(p),
        _ => data_dir.join("models").join(DEFAULT_MODEL_FILE),
    };
    if !path.exists() {
        return Err(format!("変換モデルが見つかりません: {}", path.display()));
    }
    LlamaModel::load(
        &path,
        LlamaOptions {
            n_threads,
            ..LlamaOptions::default()
        },
    )
    .map(|m| Box::new(m) as Box<dyn ConversionModel>)
    .map_err(|e| format!("変換モデルの読み込みに失敗しました: {e}"))
}

#[cfg(not(feature = "llama"))]
fn load_model(
    _data_dir: &Path,
    _model: Option<&str>,
    _n_threads: usize,
) -> Result<Box<dyn ConversionModel>, String> {
    Err("このビルドは LLM 推論（feature `llama`）を含みません".to_string())
}

/// モデルの読み込み処理（差し替え用の継ぎ目）。バックグラウンドスレッドで 1 回だけ呼ばれる
pub type ModelLoader = Box<dyn FnOnce() -> Result<Box<dyn ConversionModel>, String> + Send>;

/// 設定・辞書・モデル・学習・ユーザー辞書を読み込んで、パイプラインと共有状態を作る
pub fn build(options: &ServerOptions) -> Built {
    build_with_loader(options, None)
}

/// [`build`] のモデル読み込み処理を差し替えられる版（テスト用の継ぎ目）。`loader` が `None` なら
/// 既定（`data_dir/models/` の GGUF）。辞書が読めない・`use_llm` が偽なら `loader` は呼ばれない。
/// `options.async_model_load` が真なら、`loader` の実行とその後のウォームアップはバックグラウンドで行う
pub fn build_with_loader(options: &ServerOptions, loader: Option<ModelLoader>) -> Built {
    let mut notes = Vec::new();
    let user_dir = options.user_dir.clone().or_else(nagi_config::config_dir);
    let config_path = options
        .config_path
        .clone()
        .or_else(|| user_dir.as_ref().map(|d| d.join("config.toml")));

    // 設定（壊れていれば既定値で続ける）
    let mut events: Vec<Event> = Vec::new();
    let mut config = Config::default();
    let mut config_error = false;
    if let Some(path) = &config_path {
        match nagi_config::load_file(path) {
            Ok((c, issues)) => {
                config = c;
                events.extend(issue_events(Some(path), &issues));
            }
            Err(issues) => {
                config_error = true;
                events.extend(issue_events(Some(path), &issues));
            }
        }
    }
    if let Some(ms) = options.sync_budget_ms {
        config.conversion.sync_budget_ms = ms;
    }

    // システム辞書・変換モデル
    let mut dict = None;
    let mut model = None;
    let mut pending_loader: Option<ModelLoader> = None;
    match &options.data_dir {
        None => notes.push("データディレクトリが未指定のため辞書なしで起動します".to_string()),
        Some(dir) => match load_dictionary(dir) {
            Ok(d) => {
                let dict_path = dir.join("dict").join("nagi-dict.bin");
                let entry_count = d.entry_count();
                notes.push(format!(
                    "辞書: OK ({} entries) from {}",
                    entry_count,
                    dict_path.display()
                ));
                dict = Some(d);
                if options.use_llm {
                    let loader = loader.unwrap_or_else(|| {
                        let dir = dir.clone();
                        let setting = config.conversion.model.clone();
                        let n_threads = config.conversion.n_threads;
                        Box::new(move || load_model(&dir, setting.as_deref(), n_threads))
                    });
                    if options.async_model_load {
                        notes.push(
                            "変換モデルはバックグラウンドで読み込みます（読み込み中は辞書のみ）"
                                .to_string(),
                        );
                        pending_loader = Some(loader);
                    } else {
                        let started = Instant::now();
                        match loader() {
                            Ok(m) => {
                                eprintln!(
                                    "imserver: モデル読み込み完了 ({} ms)",
                                    started.elapsed().as_millis()
                                );
                                model = Some(m);
                            }
                            Err(e) => notes.push(format!("{e}（辞書のみで起動します）")),
                        }
                    }
                } else {
                    notes.push("LLM は無効です（辞書のみで起動します）".to_string());
                }
            }
            Err(e) => notes.push(format!("{e}（読みをそのまま返します）")),
        },
    }

    // 学習
    let learn_config = LearnConfig {
        max_entries: config.learning.max_entries,
        ..LearnConfig::default()
    };
    let mut learn_path = user_dir.as_ref().map(|d| d.join("learning.tsv"));
    let learner = match &learn_path {
        Some(path) => match Learner::load(path, learn_config.clone()) {
            Ok((l, report)) => {
                if report.skipped_lines > 0 {
                    events.push(Event {
                        level: NotifyLevel::Warning,
                        code: "learning_repaired",
                        message: format!(
                            "学習データの壊れた {} 行を読み飛ばしました（元のファイルは退避済み）",
                            report.skipped_lines
                        ),
                    });
                }
                l
            }
            Err(e) => {
                events.push(Event {
                    level: NotifyLevel::Warning,
                    code: "learning_unavailable",
                    message: format!("学習データを読み込めないため、学習を保存しません: {e}"),
                });
                learn_path = None;
                Learner::new(learn_config)
            }
        },
        None => Learner::new(learn_config),
    };

    let has_llm = model.is_some();
    // 非同期読み込みでは、読み込みに成功すれば LLM ありになる前提でキャリブレーションの記録を照合する
    let expect_llm = has_llm || pending_loader.is_some();
    let system = nagi_profile::detect();
    let backend = crate::pipeline::backend_name(dict.is_some(), expect_llm);
    let calibration_path = user_dir.as_ref().map(|d| d.join("calibration.json"));
    let cached_calibration_ms = calibration_path
        .as_deref()
        .and_then(crate::calibration::load)
        .filter(|record| record.matches(&system, backend))
        .map(|record| record.median_ms);
    let runtime = Runtime::new(
        system,
        has_llm,
        Tunables::from_config(&config),
        learner,
        learn_path,
    );
    runtime.set_calibration_path(calibration_path);
    if let Some(ms) = cached_calibration_ms {
        if has_llm {
            // 前回の実測値を即座に反映させる。直後の pipeline.warm_up() が新しい実測値で上書きする
            runtime.finish_calibration(ms);
        } else {
            // 非同期読み込み: モデルが入る（attach_model）まで LLM なし＝minimal のまま。
            // 実測値は置くだけにして、読み込み完了時の判定に使う
            runtime.set_calibration_hint(ms);
        }
    }
    runtime.set_config_state(config_path.clone(), config_error);

    // ユーザー辞書
    let user_dict_dir = user_dir.as_ref().map(|d| d.join("user_dict"));
    if let (Some(dir), Some(d)) = (&user_dict_dir, &dict) {
        let (user, issues) = UserDictionary::load_dir(dir);
        runtime.set_overlay(d.make_overlay(&user));
        events.extend(user_dict_events(&issues));
    }
    for e in events {
        runtime.push_event(e);
    }

    if config.update.auto_check {
        let runtime_for_update = runtime.clone();
        let url = config
            .update
            .check_url
            .clone()
            .unwrap_or_else(|| crate::update_check::DEFAULT_CHECK_URL.to_string());
        let state_path = user_dir.as_ref().map(|d| d.join("update_check.json"));
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let _ = std::thread::Builder::new()
            .name("nagi-update-check".into())
            .spawn(move || {
                if let Some(manifest) = crate::update_check::check_with_throttle(
                    &url,
                    &current_version,
                    state_path.as_deref(),
                ) {
                    let message = match &manifest.notes_url {
                        Some(u) => {
                            format!(
                                "新しいバージョン {} が利用可能です（{u}）",
                                manifest.version
                            )
                        }
                        None => format!("新しいバージョン {} が利用可能です", manifest.version),
                    };
                    runtime_for_update.push_event(Event {
                        level: NotifyLevel::Info,
                        code: "update_available",
                        message,
                    });
                }
            });
    }

    let applied_model = config.conversion.model.clone();
    let pipeline = Arc::new(Pipeline::new(dict.clone(), model, runtime.clone()));
    pipeline.warm_up();
    if let Some(loader) = pending_loader {
        spawn_model_loader(loader, pipeline.clone());
    }
    let mut reload = Reloadable {
        config_path,
        user_dict_dir,
        dict,
        cli_sync_budget_ms: options.sync_budget_ms,
        applied_model,
        ..Reloadable::detached()
    };
    reload.mark_read();
    Built {
        pipeline,
        runtime,
        config,
        reload,
        notes,
    }
}

/// モデルをバックグラウンドで読み込み、完了したらパイプラインに取り付ける。
/// 失敗（`Err`・パニック）しても辞書のみで動き続ける（理由は stderr。入力内容は含まない）
fn spawn_model_loader(loader: ModelLoader, pipeline: Arc<Pipeline>) {
    let spawned = std::thread::Builder::new()
        .name("nagi-model-load".into())
        .spawn(move || {
            let started = Instant::now();
            match loader() {
                Ok(model) => {
                    eprintln!(
                        "imserver: モデル読み込み完了 ({} ms)",
                        started.elapsed().as_millis()
                    );
                    pipeline.attach_model(model);
                }
                Err(e) => eprintln!("imserver: {e}（辞書のみで動作を続けます）"),
            }
        });
    if let Err(e) = spawned {
        eprintln!(
            "imserver: モデル読み込みスレッドを開始できませんでした: {e}（辞書のみで動作します）"
        );
    }
}

/// ユーザー辞書の読み込みの問題を通知に直す
pub fn user_dict_events(issues: &[nagi_dict::UserDictIssue]) -> Vec<Event> {
    issues
        .iter()
        .map(|i| Event {
            level: NotifyLevel::Warning,
            code: "user_dict_warning",
            message: if i.line > 0 {
                format!("ユーザー辞書 {} の {} 行目: {}", i.file, i.line, i.message)
            } else {
                format!("ユーザー辞書 {}: {}", i.file, i.message)
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_engine::{Action, parse_key};

    #[test]
    fn engine_config_from_applies_keymap_overrides_and_reports_warnings() {
        let mut config = Config::default();
        config
            .keymap
            .composing
            .insert("Ctrl+j".to_string(), "ToKatakana".to_string());
        config
            .keymap
            .converting
            .insert("Tab".to_string(), "NextCand".to_string());
        // 不正な上書き: 存在しないキー
        config
            .keymap
            .composing
            .insert("NotAKey".to_string(), "ToKatakana".to_string());

        let (engine_config, events) = engine_config_from(&config);

        assert_eq!(
            engine_config.keymap.resolve(
                KeyContext::Composing,
                InputMode::Hiragana,
                &parse_key("Ctrl+j").unwrap()
            ),
            Some(Action::ToKatakana)
        );
        assert_eq!(
            engine_config.keymap.resolve(
                KeyContext::Converting,
                InputMode::Hiragana,
                &parse_key("Tab").unwrap()
            ),
            Some(Action::NextCand)
        );
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].code, "keymap_warning");
        assert_eq!(events[0].level, NotifyLevel::Warning);
    }
}
