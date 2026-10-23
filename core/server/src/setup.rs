//! サーバーの組み立て: 起動オプション・設定ファイル・辞書・モデル・学習・ユーザー辞書を読み込み、
//! 変換パイプラインと共有状態（[`Runtime`]）を作る。設定の再読み込みの状態もここで持つ。
//!
//! 設定の優先順位: コマンドライン引数 > 設定ファイル（`config.toml`）> 既定値。
//! どの段階で失敗しても入力は続けられる（設定が壊れていれば既定値、辞書・モデルが無ければ縮退）。

use crate::pipeline::Pipeline;
use crate::runtime::{Event, Runtime, Tunables};
use nagi_config::{
    Config, DakutenCycle, InitialView as ConfigInitialView, Issue, Severity, ShiftLetter,
};
use nagi_dict::{Dictionary, UserDictionary};
use nagi_engine::InitialView;
use nagi_engine::KeyContext;
use nagi_engine::touch::DakutenOrder;
use nagi_engine::{AppClass, AppsConfig, AutoEnglishMode, EngineConfig};
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

/// LLM を読み込むために必要な、プロセスのメモリ上限（MB）の最小値（定義は `nagi_profile`）。これ未満なら
/// LLM を読み込まず辞書のみで動く
pub use nagi_profile::MIN_MEMORY_BUDGET_FOR_LLM_MB;

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
    /// このプロセスが OS から課されているメモリ上限（MB。`--memory-budget-mb`）。iOS のキーボード拡張など、
    /// 搭載メモリより上限がはるかに小さい環境で、フロントエンドが `phys_footprint + 残り` として渡す。
    /// [`MIN_MEMORY_BUDGET_FOR_LLM_MB`] 未満なら LLM を読み込まない。`None` なら上限なし（デスクトップなど）
    pub memory_budget_mb: Option<u64>,
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
            memory_budget_mb: None,
        }
    }
}

/// 引数から起動オプションを作る（`--data-dir` `--user-dir` `--config` `--no-llm` `--sync-budget-ms`
/// `--memory-budget-mb`、
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
            "--memory-budget-mb" => {
                let v = value("--memory-budget-mb")?;
                options.memory_budget_mb = Some(
                    v.parse()
                        .map_err(|_| format!("--memory-budget-mb が数値ではありません: {v}"))?,
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
            next_word_enabled: config.predict.next_word,
            next_word_count: config.predict.next_word_count,
            composing_chunk_len: config.conversion.composing_chunk_len,
            initial_view: match config.conversion.initial_view {
                ConfigInitialView::Sentence => InitialView::Sentence,
                ConfigInitialView::Segment => InitialView::Segment,
            },
            beam_width: config
                .conversion
                .beam_width
                .unwrap_or(EngineConfig::default().beam_width),
            dakuten_order: match config.input.dakuten_cycle {
                DakutenCycle::SmallFirst => DakutenOrder::SmallFirst,
                DakutenCycle::DakutenFirst => DakutenOrder::DakutenFirst,
            },
            romaji_carryover: config.input.romaji_carryover,
            fuzzy_input: config.input.fuzzy.enabled,
            auto_english: match config.input.auto_english.mode {
                nagi_config::AutoEnglishMode::Off => AutoEnglishMode::Off,
                nagi_config::AutoEnglishMode::Conservative => AutoEnglishMode::Conservative,
                nagi_config::AutoEnglishMode::Standard => AutoEnglishMode::Standard,
                nagi_config::AutoEnglishMode::Aggressive => AutoEnglishMode::Aggressive,
            },
            apps: apps_config_from(&config.apps),
        },
        events,
    )
}

/// `[apps]` の設定から engine のアプリ設定を作る
pub fn apps_config_from(apps: &nagi_config::AppsSection) -> AppsConfig {
    use nagi_config::{AppClass as C, AppDefaultMode as M};
    let mode = |m: M| match m {
        M::Keep => None,
        M::Hiragana => Some(InputMode::Hiragana),
        M::Alphanumeric => Some(InputMode::Alphanumeric),
    };
    AppsConfig {
        rules: apps
            .rules()
            .into_iter()
            .map(|(pattern, class)| {
                let class = match class {
                    C::Code => AppClass::Code,
                    C::Chat => AppClass::Chat,
                    C::General => AppClass::General,
                };
                (pattern, class)
            })
            .collect(),
        code_mode: mode(apps.code_mode),
        chat_mode: mode(apps.chat_mode),
        general_mode: mode(apps.general_mode),
        remember_mode: apps.remember_mode,
        // 素通しにできるのは ASCII の記号だけ（英数字を入れると通常の入力ができなくなる）
        passthrough_chars: if apps.passthrough {
            apps.passthrough_chars
                .chars()
                .filter(char::is_ascii_punctuation)
                .collect()
        } else {
            Vec::new()
        },
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
    Dictionary::load(&path)
        .map(Arc::new)
        .map_err(|e| format!("辞書の読み込みに失敗しました: {e}"))
}

/// 英語の語彙（`<data_dir>/english/words.fst`）を読み込む。ファイルが無ければ `None`
pub fn load_english_lexicon(
    data_dir: Option<&Path>,
) -> Option<Result<nagi_english::Lexicon, String>> {
    let path = data_dir?.join("english").join("words.fst");
    path.exists().then(|| {
        nagi_english::Lexicon::load(&path)
            .map_err(|e| format!("英語の語彙の読み込みに失敗しました: {e}"))
    })
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
                let llm_blocked_by_memory = options
                    .memory_budget_mb
                    .filter(|&mb| mb < MIN_MEMORY_BUDGET_FOR_LLM_MB);
                if let (true, Some(mb)) = (options.use_llm, llm_blocked_by_memory) {
                    notes.push(format!(
                        "メモリ上限 {mb}MB では LLM に {MIN_MEMORY_BUDGET_FOR_LLM_MB}MB 以上必要なため、\
                         LLM を読み込みません（辞書のみで起動します）"
                    ));
                } else if options.use_llm {
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
        strength: config.learning.strength,
        forget_days: config.learning.forget_days,
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
    let runtime = Runtime::new_with_budget(
        system,
        has_llm,
        Tunables::from_config(&config),
        learner,
        learn_path,
        options.memory_budget_mb,
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

    // feature `update-check` が無効（iOS 等）なら auto_check が true でも何もしない（通知もエラーも出さない）
    #[cfg(feature = "update-check")]
    if config.update.auto_check {
        let runtime_for_update = runtime.clone();
        let url = config
            .update
            .check_url
            .clone()
            .unwrap_or_else(crate::update_check::default_check_url);
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
    let mut pipeline = Pipeline::new(dict.clone(), model, runtime.clone());
    // 英語の自動判定の語彙（任意。`cargo xtask fetch-english`）。無くても特徴量だけで動く
    match load_english_lexicon(options.data_dir.as_deref()) {
        Some(Ok(lexicon)) => {
            notes.push(format!("英語の語彙: OK ({} words)", lexicon.len()));
            pipeline.set_english_lexicon(Some(Arc::new(lexicon)));
        }
        Some(Err(e)) => notes.push(format!("{e}（英語の自動判定は特徴量のみで動作します）")),
        None => notes.push(
            "英語の語彙が見つかりません（英語の自動判定は特徴量のみで動作します。\
             cargo xtask fetch-english で取得できます）"
                .to_string(),
        ),
    }
    let pipeline = Arc::new(pipeline);
    // メモリ逼迫・メモリ上限・`unload_model` でのモデルの解放と再読み込みの実行先
    let control: Arc<dyn crate::runtime::ModelControl> = pipeline.clone();
    runtime.set_model_control(Arc::downgrade(&control));
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
    fn apps_config_from_default_maps_table_modes_and_passthrough() {
        let apps = apps_config_from(&nagi_config::AppsSection::default());
        assert_eq!(apps.classify("com.apple.Terminal"), AppClass::Code);
        assert_eq!(apps.classify("com.jetbrains.goland"), AppClass::Code);
        assert_eq!(apps.classify("com.apple.Safari"), AppClass::General);
        assert_eq!(
            apps.default_mode(AppClass::Code),
            Some(InputMode::Alphanumeric)
        );
        assert_eq!(apps.default_mode(AppClass::Chat), None);
        assert_eq!(apps.default_mode(AppClass::General), None);
        assert!(apps.remember_mode);
        assert_eq!(apps.passthrough_chars, vec!['/', '$', '@', '!', ':', '#']);
    }

    #[test]
    fn apps_config_from_disabled_passthrough_and_non_symbol_chars() {
        let mut section = nagi_config::AppsSection {
            passthrough_chars: "/a1あ@".to_string(),
            ..Default::default()
        };
        assert_eq!(apps_config_from(&section).passthrough_chars, vec!['/', '@']);
        section.passthrough = false;
        assert!(apps_config_from(&section).passthrough_chars.is_empty());
    }

    #[test]
    fn engine_config_from_maps_dakuten_cycle() {
        let mut config = Config::default();
        assert_eq!(
            engine_config_from(&config).0.dakuten_order,
            DakutenOrder::SmallFirst
        );
        config.input.dakuten_cycle = DakutenCycle::DakutenFirst;
        assert_eq!(
            engine_config_from(&config).0.dakuten_order,
            DakutenOrder::DakutenFirst
        );
    }

    #[test]
    fn engine_config_from_maps_fuzzy_input() {
        let mut config = Config::default();
        assert!(!engine_config_from(&config).0.fuzzy_input);
        config.input.fuzzy.enabled = true;
        assert!(engine_config_from(&config).0.fuzzy_input);
    }

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
