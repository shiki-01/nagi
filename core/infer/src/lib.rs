//! Nagi 推論層: 変換モデルを trait で隠蔽し、llama.cpp（GGUF）を feature で提供する。
//!
//! # 設計
//!
//! - [`ConversionModel`] - 読み → 表記を返すトレイト（実装者依存）
//! - [`PromptAdapter`] - モデルごとのプロンプト形式を吸収する
//! - [`ZenzV3Adapter`] - Zenzai v3.1 形式の実装例
//! - [`MockModel`] - テスト用（feature OFF でも使用可）
//! - [`LlamaModel`] - feature `llama` で有効な llama.cpp バックエンド
//!
//! # 使用例
//!
//! ```
//! use nagi_infer::{ConvertRequest, MockModel, ConversionModel};
//!
//! let mut model = MockModel::new();
//! model.insert("こんにちは".to_string(), "こんにちは".to_string());
//!
//! let req = ConvertRequest {
//!     reading: "こんにちは".to_string(),
//!     left_context: String::new(),
//!     profile: None,
//!     max_new_tokens: 128,
//! };
//!
//! let result = model.convert(&req);
//! assert!(result.is_ok());
//! ```

pub mod gguf;
use std::collections::HashMap;

/// 変換要求
#[derive(Debug, Clone)]
pub struct ConvertRequest {
    /// 入力読み（ひらがな）
    pub reading: String,
    /// 左文脈（前の文節の surface など）
    pub left_context: String,
    /// 環境プロファイル（`quality` / `balanced` / `light` / `minimal` など）
    pub profile: Option<String>,
    /// 生成トークンの最大数
    pub max_new_tokens: usize,
}

/// 変換仮説（生成結果）
#[derive(Debug, Clone, PartialEq)]
pub struct Hypothesis {
    /// 生成された表記（漢字変換されたもの）
    pub surface: String,
    /// 累積対数確率スコア
    pub score: f32,
}

/// 推論層の エラー
#[derive(Debug, Clone)]
pub enum InferError {
    /// モデルのロード失敗
    ModelLoad(String),
    /// トークン化エラー
    Tokenize(String),
    /// デコードエラー（推論中の問題）
    Decode(String),
    /// 非対応の機能
    Unsupported(String),
    /// その他のエラー
    Other(String),
}

impl std::fmt::Display for InferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelLoad(msg) => write!(f, "ModelLoad: {}", msg),
            Self::Tokenize(msg) => write!(f, "Tokenize: {}", msg),
            Self::Decode(msg) => write!(f, "Decode: {}", msg),
            Self::Unsupported(msg) => write!(f, "Unsupported: {}", msg),
            Self::Other(msg) => write!(f, "Other: {}", msg),
        }
    }
}

impl std::error::Error for InferError {}

/// 変換モデルのトレイト
pub trait ConversionModel: Send {
    /// モデルの識別子（デバッグ・ログ用）
    fn id(&self) -> &str;

    /// 読み → 表記を変換する。結果は複数の仮説を返す場合もある（将来のビームサーチ対応）
    fn convert(&mut self, req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError>;

    /// 与えた各候補（表記）の対数確率スコアを返す（大きいほど尤もらしい。文脈込みの再採点用）。
    ///
    /// 既定実装は非対応。
    fn score(
        &mut self,
        req: &ConvertRequest,
        candidates: &[String],
    ) -> Result<Vec<f32>, InferError> {
        let _ = (req, candidates);
        Err(InferError::Unsupported("score".to_string()))
    }

    /// 推論のスレッド数のヒントを変更する。対応しないモデルは無視してよい（既定は無視）
    fn set_n_threads(&mut self, _n_threads: usize) {}

    /// モデルをアクティブにするかどうかを切り替える。`false` ならメモリ上の重みを解放してよい
    /// （対応しないモデルは無視して常に成功を返す）。`true` で再度使う前に、内部で必要なら再読み込みする
    fn set_active(&mut self, _active: bool) -> Result<(), InferError> {
        Ok(())
    }
}

/// プロンプト形式をモデルごとに吸収するトレイト
pub trait PromptAdapter: Send + Sync {
    /// アダプタの識別子
    fn id(&self) -> &str;

    /// 要求からプロンプト文字列を構築する。非対応機能の場合は警告文字列も返す
    fn build_prompt_checked(&self, req: &ConvertRequest) -> (String, Vec<String>);

    /// プロファイル欄をサポートするか
    fn supports_profile(&self) -> bool;
}

/// ひらがなをカタカナに変換する
///
/// # 例
///
/// ```
/// use nagi_infer::hiragana_to_katakana;
///
/// assert_eq!(hiragana_to_katakana("ひらがな"), "ヒラガナ");
/// assert_eq!(hiragana_to_katakana("ひらがな123"), "ヒラガナ123");
/// assert_eq!(hiragana_to_katakana(""), "");
/// ```
pub fn hiragana_to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| {
            // ひらがな範囲: U+3041..=U+3096
            // カタカナ範囲: U+30A1..=U+30F6
            // 差: 0x60
            if ('\u{3041}'..='\u{3096}').contains(&c) {
                char::from_u32(c as u32 + 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// Zenzai v3.1 の プロンプトアダプタ
///
/// プロンプト形式: `[\u{EE03}<profile末尾25文字>][\u{EE02}<左文脈末尾40文字>]\u{EE00}<読みをカタカナ化>\u{EE01}`
pub struct ZenzV3Adapter;

impl ZenzV3Adapter {
    /// 文字列の末尾 N 文字を取る（Unicode スカラー値単位）
    fn take_last_n_chars(s: &str, n: usize) -> String {
        s.chars()
            .rev()
            .take(n)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    }
}

impl PromptAdapter for ZenzV3Adapter {
    fn id(&self) -> &str {
        "zenz-v3.1"
    }

    fn build_prompt_checked(&self, req: &ConvertRequest) -> (String, Vec<String>) {
        let warnings = Vec::new();

        let mut prompt = String::new();

        // profile 欄（オプション、末尾 25 文字）
        if let Some(ref profile) = req.profile {
            let truncated = Self::take_last_n_chars(profile, 25);
            prompt.push('\u{EE03}');
            prompt.push_str(&truncated);
        }

        // 左文脈欄（オプション、末尾 40 文字）
        if !req.left_context.is_empty() {
            let truncated = Self::take_last_n_chars(&req.left_context, 40);
            prompt.push('\u{EE02}');
            prompt.push_str(&truncated);
        }

        // 入力読み（ひらがな → カタカナに変換）
        prompt.push('\u{EE00}');
        prompt.push_str(&hiragana_to_katakana(&req.reading));
        prompt.push('\u{EE01}');

        (prompt, warnings)
    }

    fn supports_profile(&self) -> bool {
        true
    }
}

/// テスト用の 簡易 MockModel（HashMap ベース）
///
/// 読み → 表記の固定マッピングを使用。テストおよび feature OFF のビルド検証用。
pub struct MockModel {
    table: HashMap<String, String>,
}

impl MockModel {
    /// 新規インスタンスを作成
    pub fn new() -> Self {
        Self {
            table: HashMap::new(),
        }
    }

    /// マッピングを追加
    pub fn insert(&mut self, reading: String, surface: String) {
        self.table.insert(reading, surface);
    }
}

impl Default for MockModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversionModel for MockModel {
    fn id(&self) -> &str {
        "mock"
    }

    fn convert(&mut self, req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError> {
        self.table
            .get(&req.reading)
            .map(|surface| {
                vec![Hypothesis {
                    surface: surface.clone(),
                    score: 0.0,
                }]
            })
            .ok_or_else(|| InferError::Other(format!("読み {} が未登録", req.reading)))
    }

    fn score(
        &mut self,
        req: &ConvertRequest,
        candidates: &[String],
    ) -> Result<Vec<f32>, InferError> {
        // テスト用: 登録済みの表記と一致する候補を最良（0）、他は文字数に比例して低くする
        let best = self.table.get(&req.reading);
        Ok(candidates
            .iter()
            .map(|c| {
                if Some(c) == best {
                    0.0
                } else {
                    -(c.chars().count() as f32) - 1.0
                }
            })
            .collect())
    }
}
/// llama.cpp による変換モデル（feature `llama` で有効）
///
/// 語彙とトークナイザは [`gguf::CharBpeTokenizer`] で自前処理する（同梱の llama.cpp は zenz の
/// pre-tokenizer 名 `gpt2-small-japanese-char` を拒否するため、ロード時だけ `default` に上書きする）。
#[cfg(feature = "llama")]
pub mod llama {
    use super::gguf::{CharBpeTokenizer, read_vocab};
    use super::*;
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::LlamaModel as LlamaModelCore;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::params::kv_overrides::ParamOverrideValue;
    use llama_cpp_2::token::LlamaToken;
    use std::ffi::CString;
    use std::num::NonZeroU32;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, OnceLock};
    use std::time::Instant;

    /// プロセスで 1 回だけ初期化する llama.cpp のバックエンド（複数のモデルを読み込んでも共有する）
    fn shared_backend() -> Result<Arc<LlamaBackend>, InferError> {
        static BACKEND: OnceLock<Result<Arc<LlamaBackend>, String>> = OnceLock::new();
        BACKEND
            .get_or_init(|| {
                let mut backend =
                    LlamaBackend::init().map_err(|e| format!("llama backend 初期化失敗: {e}"))?;
                backend.void_logs();
                Ok(Arc::new(backend))
            })
            .clone()
            .map_err(InferError::ModelLoad)
    }

    /// llama.cpp の初期化オプション
    #[derive(Debug, Clone)]
    pub struct LlamaOptions {
        /// スレッド数（0 = 自動判定）
        pub n_threads: usize,
        /// コンテキスト長（既定 512）
        pub n_ctx: u32,
        /// GPU 層数（既定 0 = CPU のみ）
        pub gpu_layers: u32,
    }

    impl Default for LlamaOptions {
        fn default() -> Self {
            Self {
                n_threads: 0,
                n_ctx: 512,
                gpu_layers: 0,
            }
        }
    }

    /// 直近の変換の計測値
    #[derive(Debug, Clone, Copy, Default)]
    pub struct InferStats {
        /// プロンプトのトークン数
        pub prompt_tokens: usize,
        /// 生成したトークン数（停止トークンを除く）
        pub generated_tokens: usize,
        /// コンテキスト作成 + プロンプト処理までの時間（ms）
        pub prefill_ms: f64,
        /// 生成にかかった時間（ms）
        pub decode_ms: f64,
        /// 全体の時間（ms）
        pub total_ms: f64,
    }

    /// llama.cpp バックエンドによる変換モデル
    pub struct LlamaModel {
        /// 読み込んだモデル本体。`None` なら解放済み（`minimal` プロファイル・メモリ逼迫時）
        model: Option<LlamaModelCore>,
        model_path: PathBuf,
        backend: Arc<LlamaBackend>,
        adapter: Box<dyn PromptAdapter>,
        options: LlamaOptions,
        tokenizer: CharBpeTokenizer,
        stop_ids: Vec<u32>,
        stats: Option<InferStats>,
    }

    impl LlamaModel {
        /// モデルをファイルから読み込む（mmap）
        pub fn load(path: impl AsRef<Path>, options: LlamaOptions) -> Result<Self, InferError> {
            let path = path.as_ref();
            if !path.exists() {
                return Err(InferError::ModelLoad(format!(
                    "モデルファイルが見つかりません: {}",
                    path.display()
                )));
            }
            let vocab = read_vocab(path)?;
            let tokenizer = CharBpeTokenizer::new(&vocab);
            let mut stop_ids: Vec<u32> = vocab.eos_id.into_iter().collect();
            stop_ids.extend(
                vocab
                    .tokens
                    .iter()
                    .position(|t| t == "</s>")
                    .map(|i| i as u32),
            );

            let backend = shared_backend()?;
            let model = Self::load_core(&backend, path, &options)?;
            Ok(Self {
                model: Some(model),
                model_path: path.to_path_buf(),
                backend,
                adapter: Box::new(ZenzV3Adapter),
                options,
                tokenizer,
                stop_ids,
                stats: None,
            })
        }

        /// モデル本体だけを読み込む（`load` と `set_active(true)` の再読み込みの両方から使う）
        fn load_core(
            backend: &Arc<LlamaBackend>,
            path: &Path,
            options: &LlamaOptions,
        ) -> Result<LlamaModelCore, InferError> {
            let mut params = LlamaModelParams::default().with_n_gpu_layers(options.gpu_layers);
            let key = CString::new("tokenizer.ggml.pre").expect("NUL を含まない");
            let mut value = [0 as std::os::raw::c_char; 128];
            for (dst, src) in value.iter_mut().zip(b"default") {
                *dst = *src as std::os::raw::c_char;
            }
            Pin::new(&mut params)
                .append_kv_override(key.as_c_str(), ParamOverrideValue::Str(value));

            LlamaModelCore::load_from_file(backend, path, &params)
                .map_err(|e| InferError::ModelLoad(format!("モデルロード失敗: {e}")))
        }

        /// 直近の [`ConversionModel::convert`] の計測値
        pub fn last_stats(&self) -> Option<InferStats> {
            self.stats
        }
    }

    impl ConversionModel for LlamaModel {
        fn id(&self) -> &str {
            "llama-zenz"
        }

        fn convert(&mut self, req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError> {
            self.set_active(true)?;
            let started = Instant::now();
            let (prompt, _warnings) = self.adapter.build_prompt_checked(req);
            let prompt_ids = self.tokenizer.encode(&prompt);
            if prompt_ids.is_empty() {
                return Err(InferError::Tokenize("トークン化結果が空".to_string()));
            }
            let n_ctx = self.options.n_ctx as usize;
            if prompt_ids.len() >= n_ctx {
                return Err(InferError::Tokenize(format!(
                    "プロンプトが長すぎます（{} トークン、n_ctx={}）",
                    prompt_ids.len(),
                    n_ctx
                )));
            }

            let mut ctx_params =
                LlamaContextParams::default().with_n_ctx(NonZeroU32::new(self.options.n_ctx));
            if self.options.n_threads > 0 {
                ctx_params = ctx_params.with_n_threads(self.options.n_threads as i32);
                ctx_params = ctx_params.with_n_threads_batch(self.options.n_threads as i32);
            }
            // `set_active(true)` 直後なので必ず Some
            let model = self
                .model
                .as_ref()
                .expect("set_active(true) 直後なので必ず Some");
            let mut ctx = model
                .new_context(&self.backend, ctx_params)
                .map_err(|e| InferError::ModelLoad(format!("コンテキスト作成失敗: {e}")))?;

            // プロンプト処理（最後のトークンだけロジットを取る）
            let mut batch = LlamaBatch::new(n_ctx, 1);
            let last = prompt_ids.len() - 1;
            for (pos, &id) in prompt_ids.iter().enumerate() {
                batch
                    .add(LlamaToken(id as i32), pos as i32, &[0], pos == last)
                    .map_err(|e| InferError::Decode(format!("batch 追加失敗: {e}")))?;
            }
            ctx.decode(&mut batch)
                .map_err(|e| InferError::Decode(format!("prefill 失敗: {e}")))?;
            let prefill_ms = started.elapsed().as_secs_f64() * 1000.0;

            // 貪欲デコード
            let decode_started = Instant::now();
            let mut generated: Vec<u32> = Vec::new();
            let mut score = 0.0f32;
            let mut logits_index = batch.n_tokens() - 1;
            let mut pos = prompt_ids.len();
            while generated.len() < req.max_new_tokens && pos < n_ctx {
                let logits = ctx.get_logits_ith(logits_index);
                let (best, best_logit) =
                    logits
                        .iter()
                        .enumerate()
                        .fold((0usize, f32::NEG_INFINITY), |acc, (i, &l)| {
                            if l > acc.1 { (i, l) } else { acc }
                        });
                let log_sum = logits
                    .iter()
                    .map(|&l| (l - best_logit).exp())
                    .sum::<f32>()
                    .ln()
                    + best_logit;
                let id = best as u32;
                if self.stop_ids.contains(&id) || model.is_eog_token(LlamaToken(best as i32)) {
                    break;
                }
                score += best_logit - log_sum;
                generated.push(id);

                batch.clear();
                batch
                    .add(LlamaToken(best as i32), pos as i32, &[0], true)
                    .map_err(|e| InferError::Decode(format!("batch 追加失敗: {e}")))?;
                ctx.decode(&mut batch)
                    .map_err(|e| InferError::Decode(format!("デコード失敗: {e}")))?;
                logits_index = 0;
                pos += 1;
            }

            self.stats = Some(InferStats {
                prompt_tokens: prompt_ids.len(),
                generated_tokens: generated.len(),
                prefill_ms,
                decode_ms: decode_started.elapsed().as_secs_f64() * 1000.0,
                total_ms: started.elapsed().as_secs_f64() * 1000.0,
            });
            Ok(vec![Hypothesis {
                surface: self.tokenizer.decode(&generated),
                score,
            }])
        }

        fn score(
            &mut self,
            req: &ConvertRequest,
            candidates: &[String],
        ) -> Result<Vec<f32>, InferError> {
            self.set_active(true)?;
            let (prompt, _warnings) = self.adapter.build_prompt_checked(req);
            let prompt_ids = self.tokenizer.encode(&prompt);
            let n_ctx = self.options.n_ctx as usize;
            if prompt_ids.is_empty() || prompt_ids.len() >= n_ctx {
                return Err(InferError::Tokenize(
                    "プロンプトの長さが不正です".to_string(),
                ));
            }
            let mut ctx_params =
                LlamaContextParams::default().with_n_ctx(NonZeroU32::new(self.options.n_ctx));
            if self.options.n_threads > 0 {
                ctx_params = ctx_params.with_n_threads(self.options.n_threads as i32);
                ctx_params = ctx_params.with_n_threads_batch(self.options.n_threads as i32);
            }
            // `set_active(true)` 直後なので必ず Some
            let model = self
                .model
                .as_ref()
                .expect("set_active(true) 直後なので必ず Some");
            let mut ctx = model
                .new_context(&self.backend, ctx_params)
                .map_err(|e| InferError::ModelLoad(format!("コンテキスト作成失敗: {e}")))?;

            // プロンプトは 1 回だけ処理し、候補ごとにその続きだけをデコードして KV キャッシュから外す
            let mut batch = LlamaBatch::new(n_ctx, 1);
            let last = prompt_ids.len() - 1;
            for (pos, &id) in prompt_ids.iter().enumerate() {
                batch
                    .add(LlamaToken(id as i32), pos as i32, &[0], pos == last)
                    .map_err(|e| InferError::Decode(format!("batch 追加失敗: {e}")))?;
            }
            ctx.decode(&mut batch)
                .map_err(|e| InferError::Decode(format!("prefill 失敗: {e}")))?;
            let first_logits: Vec<f32> = ctx.get_logits_ith(batch.n_tokens() - 1).to_vec();

            let mut scores = Vec::with_capacity(candidates.len());
            for cand in candidates {
                let ids = self.tokenizer.encode(cand);
                if prompt_ids.len() + ids.len() >= n_ctx {
                    scores.push(f32::NEG_INFINITY);
                    continue;
                }
                if ids.is_empty() {
                    scores.push(log_prob_any(&first_logits, &self.stop_ids));
                    continue;
                }
                let mut total = log_prob_any(&first_logits, &ids[..1]);
                batch.clear();
                for (k, &id) in ids.iter().enumerate() {
                    batch
                        .add(
                            LlamaToken(id as i32),
                            (prompt_ids.len() + k) as i32,
                            &[0],
                            true,
                        )
                        .map_err(|e| InferError::Decode(format!("batch 追加失敗: {e}")))?;
                }
                ctx.decode(&mut batch)
                    .map_err(|e| InferError::Decode(format!("候補のデコード失敗: {e}")))?;
                for k in 0..ids.len() {
                    let logits = ctx.get_logits_ith(k as i32);
                    total += match ids.get(k + 1) {
                        Some(next) => log_prob_any(logits, std::slice::from_ref(next)),
                        None => log_prob_any(logits, &self.stop_ids),
                    };
                }
                ctx.kv_cache_seq_rm(0, Some(prompt_ids.len() as u32), None)
                    .map_err(|e| InferError::Decode(format!("KV キャッシュの巻き戻し失敗: {e}")))?;
                scores.push(total);
            }
            Ok(scores)
        }

        fn set_n_threads(&mut self, n_threads: usize) {
            self.options.n_threads = n_threads;
        }

        fn set_active(&mut self, active: bool) -> Result<(), InferError> {
            if active {
                if self.model.is_none() {
                    self.model = Some(Self::load_core(
                        &self.backend,
                        &self.model_path,
                        &self.options,
                    )?);
                }
            } else {
                self.model = None;
            }
            Ok(())
        }
    }

    /// `logits` のソフトマックスで、`ids` のいずれかが選ばれる確率の対数
    fn log_prob_any(logits: &[f32], ids: &[u32]) -> f32 {
        let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let total: f32 = logits.iter().map(|&l| (l - max).exp()).sum();
        let hit: f32 = ids
            .iter()
            .filter_map(|&i| logits.get(i as usize))
            .map(|&l| (l - max).exp())
            .sum();
        if hit <= 0.0 {
            f32::NEG_INFINITY
        } else {
            hit.ln() - total.ln()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hiragana_to_katakana_basic() {
        assert_eq!(hiragana_to_katakana("ひらがな"), "ヒラガナ");
        assert_eq!(
            hiragana_to_katakana("きょうはいいてんきです"),
            "キョウハイイテンキデス"
        );
    }

    #[test]
    fn test_hiragana_to_katakana_mixed() {
        // 英数字・記号は変わらない
        assert_eq!(hiragana_to_katakana("ひらがな123"), "ヒラガナ123");
        assert_eq!(hiragana_to_katakana("あ-い"), "ア-イ");
    }

    #[test]
    fn test_hiragana_to_katakana_empty() {
        assert_eq!(hiragana_to_katakana(""), "");
    }

    #[test]
    fn test_zenz_adapter_without_context() {
        let adapter = ZenzV3Adapter;
        let req = ConvertRequest {
            reading: "こんにちは".to_string(),
            left_context: String::new(),
            profile: None,
            max_new_tokens: 128,
        };
        let (prompt, warnings) = adapter.build_prompt_checked(&req);
        // \u{EE00}<カタカナ読み>\u{EE01}
        assert!(prompt.contains('\u{EE00}'));
        assert!(prompt.contains('\u{EE01}'));
        assert!(prompt.contains("コンニチハ"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_zenz_adapter_with_context() {
        let adapter = ZenzV3Adapter;
        let req = ConvertRequest {
            reading: "です".to_string(),
            left_context: "私の名前は".to_string(),
            profile: None,
            max_new_tokens: 128,
        };
        let (prompt, _warnings) = adapter.build_prompt_checked(&req);
        assert!(prompt.contains('\u{EE02}'));
        assert!(prompt.contains("私の名前は"));
        assert!(prompt.contains("デス"));
    }

    #[test]
    fn test_zenz_adapter_with_profile() {
        let adapter = ZenzV3Adapter;
        let req = ConvertRequest {
            reading: "です".to_string(),
            left_context: String::new(),
            profile: Some("balanced".to_string()),
            max_new_tokens: 128,
        };
        let (prompt, _warnings) = adapter.build_prompt_checked(&req);
        assert!(prompt.contains('\u{EE03}'));
        assert!(prompt.contains("balanced"));
    }

    #[test]
    fn test_zenz_adapter_profile_truncation() {
        let adapter = ZenzV3Adapter;
        let long_profile = "a".repeat(50);
        let req = ConvertRequest {
            reading: "a".to_string(),
            left_context: String::new(),
            profile: Some(long_profile),
            max_new_tokens: 128,
        };
        let (prompt, _warnings) = adapter.build_prompt_checked(&req);
        // 最後の 25 文字だけ入るはず
        let profile_part = prompt
            .split('\u{EE03}')
            .nth(1)
            .map(|s| s.split('\u{EE00}').next().unwrap_or(""))
            .unwrap_or("");
        assert!(profile_part.len() <= 25);
    }

    #[test]
    fn test_mock_model_basic() {
        let mut model = MockModel::new();
        model.insert("こんにちは".to_string(), "こんにちは".to_string());

        let req = ConvertRequest {
            reading: "こんにちは".to_string(),
            left_context: String::new(),
            profile: None,
            max_new_tokens: 128,
        };

        let result = model.convert(&req);
        assert!(result.is_ok());
        let hypotheses = result.unwrap();
        assert_eq!(hypotheses.len(), 1);
        assert_eq!(hypotheses[0].surface, "こんにちは");
    }

    #[test]
    fn test_mock_model_not_found() {
        let mut model = MockModel::new();
        let req = ConvertRequest {
            reading: "unknown".to_string(),
            left_context: String::new(),
            profile: None,
            max_new_tokens: 128,
        };

        let result = model.convert(&req);
        assert!(result.is_err());
    }

    #[test]
    fn default_set_active_is_a_no_op_success() {
        // トレイトの既定実装（no-op）が Ok(()) を返すことを確認する
        let mut model = MockModel::new();
        assert!(model.set_active(false).is_ok());
        assert!(model.set_active(true).is_ok());
        model.set_n_threads(4); // no-op。パニックしないことだけ確認
    }

    #[test]
    fn test_zenz_adapter_id() {
        let adapter = ZenzV3Adapter;
        assert_eq!(adapter.id(), "zenz-v3.1");
    }

    #[test]
    fn test_zenz_adapter_supports_profile() {
        let adapter = ZenzV3Adapter;
        assert!(adapter.supports_profile());
    }

    #[test]
    fn test_mock_model_multiple_mappings() {
        let mut model = MockModel::new();
        model.insert("あ".to_string(), "あ".to_string());
        model.insert("い".to_string(), "い".to_string());

        let req1 = ConvertRequest {
            reading: "あ".to_string(),
            left_context: String::new(),
            profile: None,
            max_new_tokens: 128,
        };

        let result1 = model.convert(&req1);
        assert!(result1.is_ok());
        assert_eq!(result1.unwrap()[0].surface, "あ");

        let req2 = ConvertRequest {
            reading: "い".to_string(),
            left_context: String::new(),
            profile: None,
            max_new_tokens: 128,
        };

        let result2 = model.convert(&req2);
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap()[0].surface, "い");
    }

    /// 実モデルでの読み→表記の疎通確認（モデル未取得なら skip）
    #[cfg(feature = "llama")]
    #[test]
    fn test_llama_model_convert() {
        use crate::llama::{LlamaModel, LlamaOptions};
        use std::path::Path;

        let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/models/zenz-v3.1-small-Q5_K_M.gguf");
        if !model_path.exists() {
            eprintln!("skip: モデル未取得（cargo xtask fetch-model）");
            return;
        }
        let load_started = std::time::Instant::now();
        let mut model =
            LlamaModel::load(&model_path, LlamaOptions::default()).expect("モデルロード");
        eprintln!(
            "load: {:.0}ms",
            load_started.elapsed().as_secs_f64() * 1000.0
        );

        let cases = [
            ("きょうはいいてんきです", ""),
            ("なかのです", "私の名前は"),
            ("にほんごをにゅうりょくする", ""),
            ("とうきょうにいく", ""),
            ("かんじへんかんのてすと", ""),
            ("ぼくはがっこうにいく", ""),
            ("しゅうかつのめんせつ", ""),
        ];
        for (reading, context) in cases {
            let req = ConvertRequest {
                reading: reading.to_string(),
                left_context: context.to_string(),
                profile: None,
                max_new_tokens: 64,
            };
            let first = model.convert(&req).expect("変換");
            let stats = model.last_stats().expect("統計");
            eprintln!(
                "{reading} (ctx={context:?}) -> {} [{:.0}ms total, prefill {:.0}ms, {} prompt / {} gen tokens, score {:.2}]",
                first[0].surface,
                stats.total_ms,
                stats.prefill_ms,
                stats.prompt_tokens,
                stats.generated_tokens,
                first[0].score
            );
            assert!(!first[0].surface.is_empty());
            assert_ne!(first[0].surface, hiragana_to_katakana(reading));
            let second = model.convert(&req).expect("変換");
            assert_eq!(first[0].surface, second[0].surface, "決定的であること");
        }
    }

    /// `set_active(false)` でモデルを解放し、`set_active(true)` で再読み込みできること
    /// （モデル未取得なら skip）
    #[cfg(feature = "llama")]
    #[test]
    fn test_llama_model_set_active_reloads() {
        use crate::llama::{LlamaModel, LlamaOptions};
        use std::path::Path;

        let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/models/zenz-v3.1-small-Q5_K_M.gguf");
        if !model_path.exists() {
            eprintln!("skip: モデル未取得（cargo xtask fetch-model）");
            return;
        }
        let mut model =
            LlamaModel::load(&model_path, LlamaOptions::default()).expect("モデルロード");
        model.set_active(false).expect("解放");
        model.set_active(true).expect("再読み込み");
        let req = ConvertRequest {
            reading: "きょうはいいてんきです".to_string(),
            left_context: String::new(),
            profile: None,
            max_new_tokens: 64,
        };
        let result = model.convert(&req).expect("再読み込み後の変換");
        assert!(!result[0].surface.is_empty());
    }

    /// 文脈込みの再採点（モデル未取得なら skip）
    #[cfg(feature = "llama")]
    #[test]
    fn test_llama_score_prefers_contextual_candidate() {
        use crate::llama::{LlamaModel, LlamaOptions};
        use std::path::Path;

        let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/models/zenz-v3.1-small-Q5_K_M.gguf");
        if !model_path.exists() {
            eprintln!("skip: モデル未取得（cargo xtask fetch-model）");
            return;
        }
        let mut model =
            LlamaModel::load(&model_path, LlamaOptions::default()).expect("モデルロード");
        let candidates: Vec<String> = ["公園", "講演", "後援", "こうえん"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for (context, expected) in [("近所の", "公園"), ("大学の先生が", "講演")] {
            let req = ConvertRequest {
                reading: "こうえん".to_string(),
                left_context: context.to_string(),
                profile: None,
                max_new_tokens: 16,
            };
            let started = std::time::Instant::now();
            let scores = model.score(&req, &candidates).expect("score");
            let elapsed = started.elapsed();
            let best = scores
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(i, _)| candidates[i].as_str())
                .unwrap();
            eprintln!("ctx={context:?} scores={scores:?} best={best} ({elapsed:?})");
            assert_eq!(best, expected, "文脈 {context:?}");
            assert!(scores.iter().all(|s| s.is_finite() && *s <= 0.0));
        }
    }
}
