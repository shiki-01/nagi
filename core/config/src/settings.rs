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

/// 設定画面の表示レベル（シンプル / 詳細。`docs/ios-settings.md` §1 のモード列）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingLevel {
    /// シンプルモード（詳細モードにも表示する）
    Simple,
    /// 詳細モードのみ
    Detail,
}

impl SettingLevel {
    /// プロトコル上の文字列
    pub fn as_str(self) -> &'static str {
        match self {
            SettingLevel::Simple => "simple",
            SettingLevel::Detail => "detail",
        }
    }
}

/// 項目を出すプラットフォーム
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingPlatforms {
    /// デスクトップ（macOS / Windows）のみ
    Desktop,
    /// iOS のみ
    Ios,
    /// 両方
    Both,
}

impl SettingPlatforms {
    /// プロトコル上の文字列
    pub fn as_str(self) -> &'static str {
        match self {
            SettingPlatforms::Desktop => "desktop",
            SettingPlatforms::Ios => "ios",
            SettingPlatforms::Both => "both",
        }
    }
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
    /// 表示レベル
    pub level: SettingLevel,
    /// 出すプラットフォーム
    pub platforms: SettingPlatforms,
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

fn apps_mode_choice() -> SettingKind {
    choice(&[
        ("keep", "前回のまま"),
        ("hiragana", "ひらがな"),
        ("alphanumeric", "英数"),
    ])
}

fn int(min: i32, max: i32, step: i32) -> SettingKind {
    SettingKind::Int { min, max, step }
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
        level: SettingLevel::Detail,
        platforms: SettingPlatforms::Both,
    }
}

impl SettingDef {
    /// シンプルモードにも出す項目にする
    fn simple(mut self) -> Self {
        self.level = SettingLevel::Simple;
        self
    }

    /// 出すプラットフォームを限る
    fn platforms(mut self, platforms: SettingPlatforms) -> Self {
        self.platforms = platforms;
        self
    }

    /// 反映に再起動が要る項目にする
    fn restart(mut self) -> Self {
        self.requires_restart = true;
        self
    }
}

/// GUI から変更できる設定項目の一覧（表示順）
pub fn definitions() -> Vec<SettingDef> {
    let update_check = def(
        "update.auto_check",
        "アップデート",
        "アップデートを自動で確認",
        "起動時に新しいバージョンがないか確認し、あれば通知します。ダウンロードやインストールは行いません。",
        SettingKind::Bool,
    )
    .simple()
    .restart();
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
        )
        .simple(),
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
        )
        .simple(),
        def(
            "conversion.live_conversion_delay_ms",
            "変換",
            "ライブ変換の待ち時間（ミリ秒）",
            "入力が止まってから変換するまでの時間です。短いほど反応が速い代わりに負荷が上がります。",
            int(0, 5000, 10),
        ),
        def(
            "conversion.live_conversion_min_key_length",
            "変換",
            "ライブ変換を始める最小の読みの長さ（文字数）",
            "読みがこの文字数に達するまでは、ライブ変換せずひらがなのまま表示します。",
            int(1, 20, 1),
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
        )
        .simple(),
        def(
            "conversion.model",
            "変換",
            "変換モデル",
            "使用する LLM のモデル ID または GGUF ファイルのパスです。未指定ならプロファイルに従います。変更は次回起動時に反映されます。",
            SettingKind::Text,
        )
        .restart(),
        def(
            "conversion.n_threads",
            "変換",
            "推論スレッド数",
            "LLM 推論に使うスレッド数です。0 なら自動で決めます。多いほど速くなりますが発熱・負荷が増えます。変更は次回起動時に反映されます。",
            int(0, 8, 1),
        )
        .restart(),
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
            "conversion.max_latency_ms",
            "変換",
            "推論の最大レイテンシ（ミリ秒）",
            "LLM 推論の待ち時間の上限（タイムアウト）です。通常は変更する必要はありません。",
            int(10, 10_000, 10),
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
            "conversion.max_context_length",
            "変換",
            "文脈の最大文字数",
            "変換に使う前後の文脈の長さです。小さいほどメモリと時間を節約できます。未指定ならプロファイルに従います。",
            int(1, 200, 1),
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
        )
        .simple(),
        def(
            "input.dakuten_cycle",
            "入力",
            "濁点・小書きキーの順序",
            "フリック入力の「゛゜小」キーを押したときに文字が変わる順序です（タッチ入力のみ）。「つ」「う」で「っ」「ぅ」と「づ」「ゔ」のどちらを先に出すかが変わります。",
            choice(&[
                ("small_first", "小書きを先に（つ→っ→づ）"),
                ("dakuten_first", "濁点を先に（つ→づ→っ）"),
            ]),
        )
        .platforms(SettingPlatforms::Ios),
        def(
            "input.romaji_carryover",
            "入力",
            "消したローマ字を引き継ぐ",
            "文字を消したあとに残った未変換のローマ字（例: kypu の pu を消したときの ky）を入力中の状態に戻し、続けて入力できるようにします（例: ou で「きょう」）。",
            SettingKind::Bool,
        ),
        def(
            "input.auto_english.mode",
            "入力",
            "英語の自動判定",
            "ローマ字のまま打った英単語（例: typescript）を、日本語入力を切り替えずに英語として入力できるようにします。判定が高いとき、Space で出る候補の上位に英字が出ます。入力は自動では変わりません。「標準」「積極的」は今後の段階（先頭の候補を英字にする・入力中に切り替える）の予約で、現状は「慎重」と同じ動作です。",
            choice(&[
                ("off", "オフ"),
                ("conservative", "慎重"),
                ("standard", "標準（予約・現状は慎重と同じ）"),
                ("aggressive", "積極的（予約・現状は慎重と同じ）"),
            ]),
        )
        .simple(),
        def(
            "input.fuzzy.enabled",
            "入力",
            "タイプミスの補正候補（曖昧入力）",
            "キー配置から考えられる打ち間違い（隣のキーの押し間違い・打ち忘れ・打ち順の入れ替え・濁点の付け忘れなど）を、「もしかして」の候補として変換候補に混ぜます。入力中の文字は打った通りのままで、自動では置き換えません。",
            SettingKind::Bool,
        )
        .simple(),
        def(
            "input.fuzzy.strength",
            "入力",
            "補正の強さ",
            "どこまでの打ち間違いを補正の候補にするかです。強いほど拾える代わりに、不要な候補も増えます。",
            choice(&[
                ("gentle", "控えめ"),
                ("normal", "標準"),
                ("aggressive", "強め"),
            ]),
        )
        .simple(),
        def(
            "input.fuzzy.max_candidates",
            "入力",
            "補正候補の最大数",
            "変換候補に混ぜる「もしかして」候補の最大数です。",
            int(1, 5, 1),
        ),
        def(
            "input.fuzzy.kinds",
            "入力",
            "補正する打ち間違いの種類",
            "カンマ区切りで指定します: substitute（隣のキー）, delete（打ち忘れ）, insert（余計な打鍵）, transpose（打ち順の入れ替え）, voicing（濁点・小書きの付け忘れ）。",
            SettingKind::Text,
        ),
        def(
            "input.fuzzy.use_touch_coords",
            "入力",
            "タッチ位置を補正に使う",
            "画面のキーを押した位置（隣のキーとの境界に近いか）やフリックの角度を、タイプミスの補正の手がかりにします。境界に近い打鍵ほど補正候補が出やすくなります。",
            SettingKind::Bool,
        )
        .platforms(SettingPlatforms::Ios),
        def(
            "learning.enabled",
            "学習",
            "学習する",
            "確定した変換結果を記録し、次回以降の候補の順位に反映します。",
            SettingKind::Bool,
        )
        .simple(),
        def(
            "learning.max_entries",
            "学習",
            "学習データの最大件数",
            "これを超えると古いものから忘れます。",
            int(100, 1_000_000, 100),
        ),
        def(
            "learning.strength",
            "学習",
            "学習の強さ",
            "確定した変換結果を候補の順位に反映する度合いです。0 で順位に反映せず、大きいほど強く反映します。",
            SettingKind::Float {
                min: 0.0,
                max: 2.0,
                step: 0.1,
            },
        ),
        def(
            "learning.forget_days",
            "学習",
            "学習を忘れるまでの日数",
            "最後に使ってからこの日数が過ぎた学習を忘れます。0 なら忘れません。",
            int(0, 3650, 30),
        ),
        def(
            "predict.enabled",
            "予測変換",
            "予測変換",
            "入力中の読みから、学習履歴やユーザー辞書の候補を先回りして表示します。",
            SettingKind::Bool,
        )
        .simple(),
        def(
            "predict.min_chars",
            "予測変換",
            "予測を始める文字数",
            "読みがこの文字数以上になったら予測を表示します。",
            int(1, 20, 1),
        ),
        def(
            "predict.next_word",
            "予測変換",
            "確定直後の次語予測",
            "語を確定した直後に、これまでの入力から次に来そうな語を候補に出します。学習が有効なときだけ働きます。",
            SettingKind::Bool,
        ),
        def(
            "predict.next_word_count",
            "予測変換",
            "次語予測の候補数",
            "確定直後に出す次語の候補の最大数です。",
            int(1, 10, 1),
        ),
        def(
            "rewriter.emoji",
            "表記の候補",
            "絵文字の候補",
            "読みに合う絵文字（「わらう」→😄 など）を変換候補の後ろに加えます。",
            SettingKind::Bool,
        ),
        def(
            "rewriter.kaomoji",
            "表記の候補",
            "顔文字の候補",
            "読みに合う顔文字（「えがお」→(^_^) など）を変換候補の後ろに加えます。",
            SettingKind::Bool,
        ),
        def(
            "convert.calculator",
            "表記の候補",
            "電卓候補",
            "「1+2」「(1+2)*3」のような式に、計算結果（3、9）を変換候補として加えます。",
            SettingKind::Bool,
        ),
        def(
            "convert.loanword.enabled",
            "表記の候補",
            "カタカナ語の原語候補",
            "「コンピューター」に computer、「アルバイト」に Arbeit のような、カタカナ語の原語（英語・ドイツ語・フランス語など）を変換候補に加えます。原語のデータをダウンロードしてあるときだけ働きます。",
            SettingKind::Bool,
        ),
        def(
            "userdict.enabled",
            "ユーザー辞書",
            "ユーザー辞書を使う",
            "登録した単語を変換・予測の候補に使います。オフにしても登録内容は消えません。",
            SettingKind::Bool,
        ),
        def(
            "snippet.enabled",
            "ユーザー辞書",
            "スニペットを使う",
            "短い読みから定型文を候補に出します。オフにしても登録内容は消えません。",
            SettingKind::Bool,
        ),
        def(
            "apps.code_mode",
            "アプリごとの設定",
            "コードエディタ・ターミナルの既定モード",
            "コードエディタ・ターミナルに入ったときの入力モードです。「前回のまま」なら切り替えません。どのアプリがこれに当たるかは config.toml の [apps.classes] で追加できます。",
            apps_mode_choice(),
        ),
        def(
            "apps.chat_mode",
            "アプリごとの設定",
            "チャットアプリの既定モード",
            "チャット・メッセンジャーに入ったときの入力モードです。",
            apps_mode_choice(),
        ),
        def(
            "apps.general_mode",
            "アプリごとの設定",
            "そのほかのアプリの既定モード",
            "上記以外のアプリに入ったときの入力モードです。",
            apps_mode_choice(),
        ),
        def(
            "apps.remember_mode",
            "アプリごとの設定",
            "アプリごとにモードを覚える",
            "手で切り替えた入力モードをアプリごとに覚え、次にそのアプリに入ったときその状態で始めます。覚えた内容は再起動で消えます。",
            SettingKind::Bool,
        ),
        def(
            "apps.passthrough",
            "アプリごとの設定",
            "記号で始まる語を変換しない",
            "何も入力していない状態で下の記号を打つと、その語（スペースまで）は日本語に変換せず、そのまま入力します。/help や @name、$HOME の入力用です。",
            SettingKind::Bool,
        ),
        def(
            "apps.passthrough_chars",
            "アプリごとの設定",
            "変換しない先頭の記号",
            "ASCII の記号を並べて指定します（例: /$@!:#）。",
            SettingKind::Text,
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
        "conversion.live_conversion_min_key_length" => {
            Some(n(config.conversion.live_conversion_min_key_length))
        }
        "conversion.composing_chunk_len" => Some(n(config.conversion.composing_chunk_len)),
        "conversion.model" => config.conversion.model.clone().map(Text),
        "conversion.n_threads" => Some(n(config.conversion.n_threads)),
        "conversion.max_latency_ms" => Some(Int(
            i32::try_from(config.conversion.max_latency_ms).unwrap_or(i32::MAX)
        )),
        "conversion.max_context_length" => config.conversion.max_context_length.map(n),
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
        "input.dakuten_cycle" => Some(Text(config.input.dakuten_cycle.as_str().to_string())),
        "input.romaji_carryover" => Some(Bool(config.input.romaji_carryover)),
        "input.auto_english.mode" => {
            Some(Text(config.input.auto_english.mode.as_str().to_string()))
        }
        "input.fuzzy.enabled" => Some(Bool(config.input.fuzzy.enabled)),
        "input.fuzzy.strength" => Some(Text(config.input.fuzzy.strength.as_str().to_string())),
        "input.fuzzy.max_candidates" => Some(n(config.input.fuzzy.max_candidates)),
        "input.fuzzy.kinds" => Some(Text(config.input.fuzzy.kinds.clone())),
        "input.fuzzy.use_touch_coords" => Some(Bool(config.input.fuzzy.use_touch_coords)),
        "rewriter.emoji" => Some(Bool(config.rewriter.emoji)),
        "rewriter.kaomoji" => Some(Bool(config.rewriter.kaomoji)),
        "convert.calculator" => Some(Bool(config.convert.calculator)),
        "convert.loanword.enabled" => Some(Bool(config.convert.loanword.enabled)),
        "userdict.enabled" => Some(Bool(config.userdict.enabled)),
        "snippet.enabled" => Some(Bool(config.snippet.enabled)),
        "learning.enabled" => Some(Bool(config.learning.enabled)),
        "learning.max_entries" => Some(n(config.learning.max_entries)),
        "learning.strength" => Some(Float(config.learning.strength)),
        "learning.forget_days" => Some(Int(
            i32::try_from(config.learning.forget_days).unwrap_or(i32::MAX)
        )),
        "predict.enabled" => Some(Bool(config.predict.enabled)),
        "predict.min_chars" => Some(n(config.predict.min_chars)),
        "predict.next_word" => Some(Bool(config.predict.next_word)),
        "predict.next_word_count" => Some(n(config.predict.next_word_count)),
        "update.auto_check" => Some(Bool(config.update.auto_check)),
        "apps.code_mode" => Some(Text(config.apps.code_mode.as_str().to_string())),
        "apps.chat_mode" => Some(Text(config.apps.chat_mode.as_str().to_string())),
        "apps.general_mode" => Some(Text(config.apps.general_mode.as_str().to_string())),
        "apps.remember_mode" => Some(Bool(config.apps.remember_mode)),
        "apps.passthrough" => Some(Bool(config.apps.passthrough)),
        "apps.passthrough_chars" => Some(Text(config.apps.passthrough_chars.clone())),
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

/// `section.key` に分ける。セクションは入れ子でもよい（`input.fuzzy.enabled` → `input.fuzzy` と `enabled`）
fn split_key(key: &str) -> Option<(&str, &str)> {
    key.rsplit_once('.')
}

/// 入れ子のテーブルを（無ければ作りながら）辿る。途中がテーブルでなければ `None`
fn table_at_or_create<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    path: &str,
) -> Option<&'a mut toml_edit::Table> {
    let mut table = doc.as_table_mut();
    for segment in path.split('.') {
        table = table
            .entry(segment)
            .or_insert_with(toml_edit::table)
            .as_table_mut()?;
    }
    Some(table)
}

/// 入れ子のテーブルを（作らずに）辿る
fn table_at<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    path: &str,
) -> Option<&'a mut toml_edit::Table> {
    let mut table = doc.as_table_mut();
    for segment in path.split('.') {
        table = table.get_mut(segment)?.as_table_mut()?;
    }
    Some(table)
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
                .and_then(|(s, k)| {
                    let mut item = doc.as_item();
                    for segment in s.split('.') {
                        item = item.get(segment)?;
                    }
                    item.get(k)
                })
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
                let Some(table) = table_at_or_create(&mut doc, section) else {
                    errors.push(fail("config.toml のセクションの形式が不正です"));
                    continue;
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
                if let Some(table) = table_at(&mut doc, section) {
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
    fn new_toml_items_are_editable() {
        let r = apply_changes(
            None,
            &[
                set("conversion.model", SettingValue::Text("m.gguf".into())),
                set("conversion.n_threads", SettingValue::Int(4)),
                set("conversion.max_context_length", SettingValue::Int(50)),
                set("conversion.max_latency_ms", SettingValue::Int(200)),
                set(
                    "conversion.live_conversion_min_key_length",
                    SettingValue::Int(3),
                ),
                set("conversion.n_threads", SettingValue::Int(9)),
            ],
        )
        .unwrap();
        assert_eq!(r.applied.len(), 5);
        assert_eq!(r.errors.len(), 1);
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(c.conversion.model.as_deref(), Some("m.gguf"));
        assert_eq!(c.conversion.n_threads, 4);
        assert_eq!(c.conversion.max_context_length, Some(50));
        assert_eq!(c.conversion.max_latency_ms, 200);
        assert_eq!(c.conversion.live_conversion_min_key_length, 3);
    }

    #[test]
    fn level_and_platforms_attributes() {
        let defs = definitions();
        let level = |k: &str| defs.iter().find(|d| d.key == k).unwrap().level;
        assert_eq!(level("conversion.live_conversion"), SettingLevel::Simple);
        assert_eq!(level("conversion.n_threads"), SettingLevel::Detail);
        assert_eq!(SettingLevel::Simple.as_str(), "simple");
        assert_eq!(SettingPlatforms::Both.as_str(), "both");
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
    fn learning_strength_and_forget_days_definitions() {
        let defs = definitions();
        for key in ["learning.strength", "learning.forget_days"] {
            let d = defs.iter().find(|d| d.key == key).unwrap();
            assert_eq!(d.level, SettingLevel::Detail, "{key}");
            assert_eq!(d.platforms, SettingPlatforms::Both, "{key}");
        }
        let strength = defs.iter().find(|d| d.key == "learning.strength").unwrap();
        assert!(validate_value(strength, &SettingValue::Float(2.0)).is_ok());
        assert!(validate_value(strength, &SettingValue::Float(2.5)).is_err());
        let r = apply_changes(
            None,
            &[
                set("learning.strength", SettingValue::Float(1.5)),
                set("learning.forget_days", SettingValue::Int(90)),
            ],
        )
        .unwrap();
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(c.learning.strength, 1.5);
        assert_eq!(c.learning.forget_days, 90);
    }

    #[test]
    fn userdict_snippet_dakuten_definitions() {
        let defs = definitions();
        let get = |k: &str| defs.iter().find(|d| d.key == k).unwrap();
        for key in ["userdict.enabled", "snippet.enabled"] {
            assert_eq!(get(key).level, SettingLevel::Detail, "{key}");
            assert_eq!(get(key).platforms, SettingPlatforms::Both, "{key}");
        }
        assert_eq!(get("input.dakuten_cycle").platforms, SettingPlatforms::Ios);
        let r = apply_changes(
            None,
            &[
                set("userdict.enabled", SettingValue::Bool(false)),
                set("snippet.enabled", SettingValue::Bool(false)),
                set(
                    "input.dakuten_cycle",
                    SettingValue::Text("dakuten_first".into()),
                ),
                set("input.dakuten_cycle", SettingValue::Text("bogus".into())),
            ],
        )
        .unwrap();
        assert_eq!(r.applied.len(), 3);
        assert_eq!(r.errors.len(), 1);
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(!c.userdict.enabled && !c.snippet.enabled);
        assert_eq!(c.input.dakuten_cycle, crate::DakutenCycle::DakutenFirst);
    }

    #[test]
    fn auto_english_definition_and_editing() {
        let defs = definitions();
        let d = defs
            .iter()
            .find(|d| d.key == "input.auto_english.mode")
            .unwrap();
        assert!(matches!(&d.kind, SettingKind::Choice { options } if options.len() == 4));
        assert_eq!(d.level, SettingLevel::Simple);
        let r = apply_changes(
            None,
            &[set(
                "input.auto_english.mode",
                SettingValue::Text("off".into()),
            )],
        )
        .unwrap();
        let (c, _) = crate::parse(&r.source).unwrap();
        assert_eq!(c.input.auto_english.mode, crate::AutoEnglishMode::Off);
        let bad = apply_changes(
            None,
            &[set(
                "input.auto_english.mode",
                SettingValue::Text("x".into()),
            )],
        )
        .unwrap();
        assert_eq!(bad.errors.len(), 1);
        assert!(bad.applied.is_empty());
    }

    #[test]
    fn fuzzy_definitions_and_nested_toml_editing() {
        let defs = definitions();
        let get = |k: &str| defs.iter().find(|d| d.key == k).unwrap();
        for (key, level) in [
            ("input.fuzzy.enabled", SettingLevel::Simple),
            ("input.fuzzy.strength", SettingLevel::Simple),
            ("input.fuzzy.max_candidates", SettingLevel::Detail),
            ("input.fuzzy.kinds", SettingLevel::Detail),
        ] {
            assert_eq!(get(key).level, level, "{key}");
            assert_eq!(get(key).platforms, SettingPlatforms::Both, "{key}");
        }
        assert_eq!(get("input.fuzzy.kinds").kind, SettingKind::Text);
        // 既定は読み出せて、オフ
        let states = read(None).unwrap();
        let enabled = states
            .iter()
            .find(|s| s.def.key == "input.fuzzy.enabled")
            .unwrap();
        assert_eq!(enabled.value, Some(SettingValue::Bool(false)));

        // 入れ子のテーブルへ書き込み、コメントを保ち、読み戻せる
        let src = "[input]\n# 句読点\npunctuation = \"、。\"\n\n[input.fuzzy]\n# 補正\nenabled = false # 行末\n";
        let r = apply_changes(
            Some(src),
            &[
                set("input.fuzzy.enabled", SettingValue::Bool(true)),
                set("input.fuzzy.strength", SettingValue::Text("gentle".into())),
                set("input.fuzzy.max_candidates", SettingValue::Int(9)),
            ],
        )
        .unwrap();
        assert_eq!(r.applied.len(), 2);
        assert_eq!(r.errors.len(), 1);
        assert!(
            r.source.contains("# 補正")
                && r.source.contains("# 行末")
                && r.source.contains("# 句読点")
        );
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(c.input.fuzzy.enabled);
        assert_eq!(c.input.fuzzy.strength, crate::FuzzyStrength::Gentle);
        let states = read(Some(&r.source)).unwrap();
        let st = states
            .iter()
            .find(|s| s.def.key == "input.fuzzy.strength")
            .unwrap();
        assert!(st.explicit);
        assert_eq!(st.value, Some(SettingValue::Text("gentle".into())));

        // 無い状態から書く・明示指定の削除
        let r = apply_changes(
            None,
            &[set(
                "input.fuzzy.kinds",
                SettingValue::Text("voicing".into()),
            )],
        )
        .unwrap();
        let (c, _) = crate::parse(&r.source).unwrap();
        assert_eq!(c.input.fuzzy.kinds, "voicing");
        let r = apply_changes(Some(&r.source), &[("input.fuzzy.kinds".to_string(), None)]).unwrap();
        let (c, _) = crate::parse(&r.source).unwrap();
        assert_eq!(c.input.fuzzy.kinds, crate::FUZZY_KINDS_ALL);
    }

    #[test]
    fn next_word_definitions() {
        let defs = definitions();
        let get = |k: &str| defs.iter().find(|d| d.key == k).unwrap();
        for key in ["predict.next_word", "predict.next_word_count"] {
            assert_eq!(get(key).level, SettingLevel::Detail, "{key}");
            assert_eq!(get(key).platforms, SettingPlatforms::Both, "{key}");
        }
        let c = crate::Config::default();
        assert!(!c.predict.next_word);
        assert_eq!(c.predict.next_word_count, 5);
        assert!(validate_value(get("predict.next_word_count"), &SettingValue::Int(10)).is_ok());
        assert!(validate_value(get("predict.next_word_count"), &SettingValue::Int(11)).is_err());
        assert!(validate_value(get("predict.next_word_count"), &SettingValue::Int(0)).is_err());
        let r = apply_changes(
            None,
            &[
                set("predict.next_word", SettingValue::Bool(true)),
                set("predict.next_word_count", SettingValue::Int(3)),
            ],
        )
        .unwrap();
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(c.predict.next_word);
        assert_eq!(c.predict.next_word_count, 3);
    }

    #[test]
    fn rewriter_emoji_definitions() {
        let defs = definitions();
        let get = |k: &str| defs.iter().find(|d| d.key == k).unwrap();
        for key in ["rewriter.emoji", "rewriter.kaomoji"] {
            assert_eq!(get(key).level, SettingLevel::Detail, "{key}");
            assert_eq!(get(key).platforms, SettingPlatforms::Both, "{key}");
        }
        let c = crate::Config::default();
        assert!(c.rewriter.emoji && c.rewriter.kaomoji);
        let r = apply_changes(
            None,
            &[
                set("rewriter.emoji", SettingValue::Bool(false)),
                set("rewriter.kaomoji", SettingValue::Bool(false)),
            ],
        )
        .unwrap();
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(!c.rewriter.emoji && !c.rewriter.kaomoji);
    }

    #[test]
    fn apps_definitions_round_trip() {
        let r = apply_changes(
            None,
            &[
                set("apps.code_mode", SettingValue::Text("hiragana".into())),
                set("apps.remember_mode", SettingValue::Bool(false)),
                set("apps.passthrough_chars", SettingValue::Text("/@".into())),
            ],
        )
        .unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(c.apps.code_mode, crate::AppDefaultMode::Hiragana);
        assert!(!c.apps.remember_mode);
        assert_eq!(c.apps.passthrough_chars, "/@");
        let bad = apply_changes(
            None,
            &[set("apps.chat_mode", SettingValue::Text("x".into()))],
        )
        .unwrap();
        assert_eq!(bad.errors.len(), 1);
    }

    #[test]
    fn convert_calculator_definition() {
        let defs = definitions();
        let d = defs.iter().find(|d| d.key == "convert.calculator").unwrap();
        assert_eq!(d.level, SettingLevel::Detail);
        assert_eq!(d.platforms, SettingPlatforms::Both);
        assert!(crate::Config::default().convert.calculator);
        let r = apply_changes(
            None,
            &[set("convert.calculator", SettingValue::Bool(false))],
        )
        .unwrap();
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(!c.convert.calculator);
    }

    #[test]
    fn convert_loanword_definition() {
        let defs = definitions();
        let d = defs
            .iter()
            .find(|d| d.key == "convert.loanword.enabled")
            .unwrap();
        assert_eq!(d.level, SettingLevel::Detail);
        assert!(crate::Config::default().convert.loanword.enabled);
        let r = apply_changes(
            None,
            &[set("convert.loanword.enabled", SettingValue::Bool(false))],
        )
        .unwrap();
        let (c, issues) = crate::parse(&r.source).unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        assert!(!c.convert.loanword.enabled);
        let (_, issues) = crate::parse("[convert.loanword]\nbogus = 1\n").unwrap();
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("convert.loanword.bogus"))
        );
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
