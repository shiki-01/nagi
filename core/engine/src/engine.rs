//! 状態機械（Direct / Composing / Converting）と、セッションごとの入力処理。
//!
//! 状態遷移はすべて [`Action`] で駆動する。キーと Action の対応は [`KeyMap`] が解決する。
//! 状態機械は OS に一切依存しない純粋な Rust で、ヘッドレスにテストできる。
//!
//! `process_key` は同期で、応答に「前回フロントへ返した状態からの差分」（変化した
//! `preedit` / `candidates` / `mode` と、確定文字列）をまとめて返す。

use crate::action::{Action, Direction};
use crate::backend::{BackendSegment, Completion, ConversionBackend};
use crate::ir::{
    Candidate, CandidateSet, CandidateSource, CursorPos, Preedit, Segment, SegmentKind,
};
use crate::kana::{normalize_reading_char, to_katakana};
use crate::keymap::{KeyContext, KeyMap, parse_action};
use crate::observer::{CommitContext, CommitObserver, CommittedSegment};
use nagi_fuzzy::{FuzzyConfig, KeyAlternatives};
use nagi_protocol::key::{KeyEvent, KeyKind};
use nagi_protocol::methods::{
    InputAlternative, ProcessKeyResult, RestoreSessionParams, SetContextParams,
};
use nagi_protocol::utf16;
use nagi_protocol::view::{CandidateView, CandidatesView, InputMode, PreeditView};
use nagi_rewriter::Rewriter;
use nagi_romaji::{Composer, Piece, PieceKind, PunctuationStyle, RomajiTable, ShiftLetterMode};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

/// 推論・変換に渡す左文脈の最大文字数（Unicode スカラー）
const LEFT_CONTEXT_CHARS: usize = 40;
/// 文節 1 つ分に集める候補数の上限
const CANDIDATE_LIMIT: usize = 50;

/// 候補ウィンドウの初期表示（`conversion.initial_view`。設計書 §4.4）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitialView {
    /// 文全体の N-best を表示する（既定）
    #[default]
    Sentence,
    /// 先頭文節の候補を表示する（MS-IME / Mozc と同じ）
    Segment,
}

/// 曖昧入力の設定（`config.toml` の `[fuzzy]` に対応する部分。設計: `docs/fuzzy-input-design.md`）
#[derive(Debug, Clone)]
pub struct FuzzyEngineConfig {
    /// 曖昧入力（補正候補）を使うか。`false` なら打鍵の記録も補正の計算も一切行わない
    pub enabled: bool,
    /// 仮説探索・コスト換算の設定（`max_hypotheses` は第一仮説を含む仮説数）
    pub search: FuzzyConfig,
    /// 入力コストの重み（`total = conv_cost + input_weight * input_cost`）
    pub input_weight: f32,
    /// 曖昧さの閾値（nat）。追加仮説を走らせる条件と、補正候補を残す条件の両方に使う
    pub ambiguity_margin: f32,
    /// 第一仮説の変換で辞書に載らない割合がこれ以上なら追加仮説を走らせる
    pub poor_coverage: f32,
    /// 第一仮説が `poor_coverage` 以上のとき、補正候補を先頭に出す
    pub promote_corrected: bool,
}

impl Default for FuzzyEngineConfig {
    fn default() -> Self {
        FuzzyEngineConfig {
            enabled: false,
            search: FuzzyConfig::default(),
            input_weight: 1.0,
            ambiguity_margin: 1.5,
            poor_coverage: 0.3,
            promote_corrected: false,
        }
    }
}

/// エンジンの設定（`config.toml` の `[input]` `[conversion]` に対応する部分）
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Shift+英字の扱い（`input.shift_letter`）
    pub shift_letter: ShiftLetterMode,
    /// 句読点の表記（`input.punctuation`）
    pub punctuation: PunctuationStyle,
    /// 候補ウィンドウ 1 ページの候補数（`conversion.num_candidates`）
    pub page_size: usize,
    /// セッション開始時の入力モード
    pub initial_mode: InputMode,
    /// キーマップ（`[keymap]`。既定は内蔵の既定テーブルのみで上書きなし）
    pub keymap: KeyMap,
    /// 予測変換を行うか（`predict.enabled`）
    pub predict_enabled: bool,
    /// 予測変換を表示する最小の読みの文字数（`predict.min_chars`）
    pub predict_min_chars: usize,
    /// ライブ変換のチャンク長（`conversion.composing_chunk_len`）。この文字数を超える
    /// Composing 中の1つの日本語入力は、この文字数ごとに分けて変換する
    pub composing_chunk_len: usize,
    /// 候補ウィンドウの初期表示（`conversion.initial_view`）
    pub initial_view: InitialView,
    /// 文全体N-bestの候補数の起点（`conversion.beam_width`。既定値のみ。実行時のプロファイル
    /// 反映は `Effective.beam_width`（`nagi-server`）が別に持つ）
    pub beam_width: usize,
    /// 曖昧入力（`[fuzzy]`。既定は無効）
    pub fuzzy: FuzzyEngineConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            shift_letter: ShiftLetterMode::LiteralContinue,
            punctuation: PunctuationStyle::Japanese,
            page_size: 9,
            initial_mode: InputMode::Hiragana,
            keymap: KeyMap::default_preset(),
            predict_enabled: true,
            predict_min_chars: 1,
            composing_chunk_len: 30,
            initial_view: InitialView::Sentence,
            beam_width: 3,
            fuzzy: FuzzyEngineConfig::default(),
        }
    }
}

/// 状態の種類（設計書 §4.3）。`RangeSelecting` / `StyleAction` は Phase 3 / 4 まで到達しない
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    /// 未入力
    Direct,
    /// ローマ字入力中
    Composing,
    /// 変換中（注目文節あり）
    Converting,
    /// 2 文節以上の範囲選択（未実装）
    RangeSelecting,
    /// スタイル変換の実行中（未実装）
    StyleAction,
}

/// テスト・診断用のセッションの現在状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// 状態
    pub state: StateKind,
    /// 現在のプリエディット表示
    pub preedit: PreeditView,
    /// 現在の候補ウィンドウ表示
    pub candidates: CandidatesView,
    /// 入力モード
    pub mode: InputMode,
    /// リビジョン
    pub revision: u32,
}

#[derive(Clone)]
struct Conv {
    segments: Vec<Segment>,
    focus: usize,
    visible: bool,
    /// 範囲選択の反対側の端（`Some` の間は複数文節を選択中）。選択範囲は `focus` と
    /// `range_anchor` を含む最小区間。`focus == range_anchor` になったら選択解除（`None` に戻す）
    range_anchor: Option<usize>,
    /// 文全体N-bestの表示中、各候補ごとの文節分割（`segments[0].candidates` の index と対応）。
    /// 候補選択（`NextCand`/`PrevCand`/`SelectCand`/`select_candidate`）以外の操作が来たら、
    /// 選択中の候補の分割へ丸ごと置き換えて `None` にする（`ensure_segmented`）
    sentence_alternatives: Option<Vec<Vec<Segment>>>,
    /// 曖昧入力の補正仮説（`base_reading` が `Some` の 1 文節のときだけ使い、`ensure_candidates` が
    /// 候補の末尾に Corrected として足す）
    corrections: Vec<Correction>,
    /// `corrections` の元になった、入力全体の reading（打たれたとおり）。`Some` の間、先頭の文節で
    /// 補正候補を選ぶとその reading に、通常の候補へ戻すとこの reading に書き換わる
    base_reading: Option<String>,
    /// 複数文節（または Literal を含む）入力の補正仮説（入力全体の reading, 補正）。
    /// 表示構造（文節列・候補リスト）は第一仮説のまま変えず、注目文節の候補の末尾に文全体の補正候補
    /// （`Candidate::reading` が `Some`）を足す。選んだときだけ文全体が 1 文節（文全体候補）へ置き換わる
    sentence_corrections: Option<(String, Vec<Correction>)>,
}

/// 曖昧入力の補正仮説 1 つぶんの変換結果
#[derive(Debug, Clone)]
struct Correction {
    /// 補正後の読み
    reading: String,
    /// その読みの文全体変換
    segments: Vec<BackendSegment>,
    /// 変換結果の表記（`segments` の連結）
    surface: String,
}

/// 追加の読み仮説を変換した結果（`Engine::fuzzy_corrections`）
struct FuzzyOutcome {
    /// 補正候補（総合スコアの良い順）
    corrections: Vec<Correction>,
    /// 先頭に出す（`promote_corrected` かつ第一仮説が低品質）
    promote: bool,
}

enum State {
    Direct,
    Composing,
    Converting(Conv),
}

struct Sent {
    preedit: PreeditView,
    candidates: CandidatesView,
    mode: InputMode,
}

/// 非同期完了の差し替え対象。`convert_sentence` に渡された引数と同じ値を持つ
#[derive(Debug, Clone)]
struct Origin {
    /// Conv.segments 内の先頭位置
    start: usize,
    /// 文節数
    count: usize,
    /// `convert_sentence` に渡した reading
    reading: String,
    /// `convert_sentence` に渡した left_context
    left: String,
}

struct Session {
    id: String,
    mode: InputMode,
    state: State,
    composer: Composer,
    /// 直前のテキスト（`set_context` の Surrounding Text のカーソル手前）
    left_context: String,
    /// フロントが報告したアプリケーション識別子
    app_id: Option<String>,
    secure_input: bool,
    revision: u64,
    sent: Sent,
    /// 非同期の LLM 完了に対応する差し替え対象。未操作なら差し替える
    origins: Vec<Origin>,
    /// ライブ変換の結果（Composing 中のみ使う）。`source_reading` が現在の入力全体の reading と
    /// 一致する、または末尾への追記でその接頭辞になっている（カーソルが末尾）間だけ表示に使う
    /// （追記分は固定部分の後ろに未変換のかなとして足す。G-43）。それ以外は自然に無視される
    live_conversion: Option<LiveConversion>,
    /// ライブ変換の抑制。Backspace / DeleteForward / 変換の取り消し（Esc 等）でひらがな表示に
    /// 戻したあと、次に文字が入力（追記）されるまで `live_convert` の結果を反映しない（G-46）。
    /// Space 変換・Enter 確定など明示操作には影響しない
    live_suppressed: bool,
    /// 直前の確定の記憶（`Action::UndoCommit` 用）
    last_commit: Option<LastCommit>,
    /// このキー処理で確定取消が成功した場合の `delete_before`（`finish` が読んで消費する）
    pending_delete_before: Option<u32>,
    /// 処理中のキーに添えられた曖昧入力の代替 `(文字, 距離)`。`insert_char` が打鍵ログへ渡して消費する
    key_alts: Vec<(char, f32)>,
}

/// ライブ変換のキャッシュ
#[derive(Clone)]
struct LiveConversion {
    /// 計算時点の入力全体（Composing の全 piece を連結した）reading
    source_reading: String,
    /// 変換結果のセグメント列（reading を連結すると `source_reading` に一致する）
    segments: Vec<Segment>,
    /// 変換したチャンクのうち、バックエンドの結果が妥当だったものの `(reading, left_context)`。
    /// 予算超過で辞書寄りの結果になっていた場合に、LLM の遅延完了（`Completion`）で
    /// 全文を再変換して置き換えるための照合キー（G-47）
    chunks: Vec<(String, String)>,
}

/// 表示に使うライブ変換結果（`Engine::live_view`）
struct LiveView {
    /// 変換済みで固定された部分
    fixed: Vec<Segment>,
    /// 固定部分の後ろに追記された未変換の部分（追記が無ければ空）
    tail: Vec<Segment>,
}

/// 直前の確定の記憶（Ctrl+Backspace での確定取消に使う。確定のたびに置き換わり、
/// 確定取消以外の操作が来ると `handle_key` が破棄する。1回分だけ保持する）
#[derive(Clone)]
struct LastCommit {
    /// 確定前の入力（Composer に復元するための Piece 列。Converting だった場合は
    /// 各文節の reading から作る。変換結果は失われ、ひらがなに戻る（Escape での
    /// 変換取消と同じ扱い）
    pieces: Vec<Piece>,
    /// 確定した文字列の UTF-16 コード単位での長さ（`ProcessKeyResult.delete_before` に使う）
    committed_len_utf16: u32,
}

/// 変換の始め方
#[derive(Clone, Copy, PartialEq, Eq)]
enum Start {
    Convert,
    Katakana,
    Hiragana,
}

/// 状態機械エンジン
pub struct Engine {
    backend: Arc<dyn ConversionBackend>,
    config: EngineConfig,
    keymap: KeyMap,
    table: Arc<RomajiTable>,
    sessions: HashMap<String, Session>,
    observer: Option<Arc<dyn CommitObserver>>,
}

fn tail_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

/// Piece 列の reading の連結
fn pieces_reading(pieces: &[Piece]) -> String {
    pieces.iter().map(|p| p.text.as_str()).collect()
}

fn push_candidate(items: &mut Vec<Candidate>, surface: &str, sources: &[CandidateSource]) {
    if let Some(existing) = items.iter_mut().find(|c| c.surface == surface) {
        for s in sources {
            if !existing.sources.contains(s) {
                existing.sources.push(*s);
            }
        }
    } else {
        items.push(Candidate {
            surface: surface.to_string(),
            sources: sources.to_vec(),
            reading: None,
        });
    }
}

impl Engine {
    /// エンジンを作る
    pub fn new(backend: Arc<dyn ConversionBackend>, config: EngineConfig) -> Self {
        let table = Arc::new(RomajiTable::standard().with_punctuation(config.punctuation));
        let keymap = config.keymap.clone();
        Engine {
            backend,
            config,
            keymap,
            table,
            sessions: HashMap::new(),
            observer: None,
        }
    }

    /// 確定・学習削除の通知先を設定する（学習など）
    pub fn set_commit_observer(&mut self, observer: Arc<dyn CommitObserver>) {
        self.observer = Some(observer);
    }

    /// 設定を差し替える（設定ファイルの再読み込み）。既存のセッションにも、次の入力から反映される
    pub fn update_config(&mut self, config: EngineConfig) {
        self.table = Arc::new(RomajiTable::standard().with_punctuation(config.punctuation));
        for sess in self.sessions.values_mut() {
            sess.composer
                .reconfigure(self.table.clone(), config.shift_letter);
            sess.composer.set_key_log_enabled(config.fuzzy.enabled);
            sess.sent.candidates.page_size = config.page_size as u32;
        }
        self.keymap = config.keymap.clone();
        self.config = config;
    }

    /// 現在の設定
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// 使用中のバックエンド名（診断用）
    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    /// バックエンドの完了通知を取り出す
    pub fn backend_completions(&self) -> Vec<Completion> {
        self.backend.take_completions()
    }

    /// バックエンドの完了通知の waker を登録する
    pub fn set_backend_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.backend.set_waker(waker);
    }

    fn new_session(&self) -> Session {
        let mut composer = Composer::new(self.table.clone(), self.config.shift_letter);
        composer.set_key_log_enabled(self.config.fuzzy.enabled);
        Session {
            id: String::new(),
            mode: self.config.initial_mode,
            state: State::Direct,
            composer,
            left_context: String::new(),
            app_id: None,
            secure_input: false,
            revision: 0,
            sent: Sent {
                preedit: PreeditView::default(),
                candidates: CandidatesView {
                    page_size: self.config.page_size as u32,
                    ..Default::default()
                },
                mode: self.config.initial_mode,
            },
            origins: Vec::new(),
            live_conversion: None,
            live_suppressed: false,
            last_commit: None,
            pending_delete_before: None,
            key_alts: Vec::new(),
        }
    }

    fn take_session(&mut self, id: &str) -> Session {
        match self.sessions.remove(id) {
            Some(s) => s,
            None => {
                let mut s = self.new_session();
                s.id = id.to_string();
                s
            }
        }
    }

    /// キー 1 つを処理する
    pub fn process_key(&mut self, session_id: &str, key: &KeyEvent) -> ProcessKeyResult {
        self.process_key_with_alternatives(session_id, key, &[])
    }

    /// [`Engine::process_key`] に、タップされたキーの代替（近傍のキー。`distance` はキー幅で
    /// 正規化した距離）を添える。`ProcessKeyParams.alternatives` の入口。
    ///
    /// 代替は `fuzzy.enabled` のときだけ、ローマ字の文字入力（`Action::Insert`）の打鍵ログに記録する
    /// （検証は [`sanitize_alternatives`]）。無効なら `process_key` と完全に同じ
    pub fn process_key_with_alternatives(
        &mut self,
        session_id: &str,
        key: &KeyEvent,
        alternatives: &[InputAlternative],
    ) -> ProcessKeyResult {
        let mut sess = self.take_session(session_id);
        sess.origins.clear();
        if self.config.fuzzy.enabled && !alternatives.is_empty() {
            sess.key_alts = alternative_pairs(alternatives);
        }
        let mut commit = String::new();
        let consumed = self.handle_key(&mut sess, key, &mut commit);
        sess.key_alts.clear();
        let result = self.finish(&mut sess, consumed, commit);
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// ローマ字変換を経由せず `reading` を挿入する（フリック・12 キー等のタッチ UI 向け）。
    ///
    /// - ひらがな・長音・CJK 記号は日本語、カタカナはひらがなに正規化して日本語、
    ///   それ以外の文字は Literal として扱う（`kana::normalize_reading_char`）。制御文字は無視
    /// - 保留中のローマ字（`k` など）は、挿入の前に確定する（`Action::InsertText` と同じ
    ///   `Composer` の流儀。`n` → `ん`、確定できない子音はそのまま残る）
    /// - Direct からは Composing を開始する。Converting 中は変換結果を確定してから新規入力を始める
    ///   （`process_key` の文字入力と同じ）
    /// - 挿入できる文字が無ければ `consumed = false`（何もしない）
    /// - `alternatives`（曖昧入力の代替ひらがな）は検証（1 文字・distance が有限で 0 以上。
    ///   [`sanitize_alternatives`]）のうえ、`reading` が 1 文字のときだけ、その打鍵の代替として
    ///   `fuzzy.enabled` のときの打鍵ログに記録する（複数文字の挿入は 1 打鍵ではないので記録しない）
    pub fn insert_reading(
        &mut self,
        session_id: &str,
        reading: &str,
        alternatives: &[InputAlternative],
    ) -> ProcessKeyResult {
        let alternatives = alternative_pairs(alternatives);
        let mut sess = self.take_session(session_id);
        sess.origins.clear();
        // 文字入力も「確定直後のみ有効」の取消の対象外にする（process_key と同じ）
        sess.last_commit = None;
        let mut commit = String::new();
        let mut chars = reading
            .chars()
            .filter(|c| !c.is_control())
            .map(normalize_reading_char)
            .peekable();
        let consumed = chars.peek().is_some();
        if consumed {
            self.begin_insert(&mut sess, &mut commit);
            let chars: Vec<(char, bool)> = chars.collect();
            // 代替は 1 打鍵（1 文字の挿入）に対するものだけ
            let alts: &[(char, f32)] = if self.config.fuzzy.enabled && chars.len() == 1 {
                &alternatives
            } else {
                &[]
            };
            for (c, japanese) in chars {
                let kind = if japanese {
                    PieceKind::Japanese
                } else {
                    PieceKind::Literal
                };
                sess.composer.insert_reading_char_with_alts(c, kind, alts);
            }
            sess.state = State::Composing;
        }
        let result = self.finish(&mut sess, consumed, commit);
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// キーマップの Action 名（`Commit`、`MoveCursor:Left`、`SelectCand:2` など。
    /// [`parse_action`] と同じ）で Action を直接実行する（タッチ UI のボタン操作向け）。
    ///
    /// 状態遷移は `process_key` と同じ `apply` を通る。未知の名前（`Insert` / `InsertText` /
    /// `SetMode` も含む）は [`UnknownAction`] を返し、状態は変えない。
    /// Direct 状態では、入力中にだけ意味を持つ Action は `consumed = false`（ホストアプリに任せる。
    /// Backspace・Enter 等をタッチキーボードが素通しできるように）
    pub fn perform_action(
        &mut self,
        session_id: &str,
        action_name: &str,
    ) -> Result<ProcessKeyResult, UnknownAction> {
        let action = parse_action(action_name).map_err(UnknownAction)?;
        let mut sess = self.take_session(session_id);
        sess.origins.clear();
        let mut commit = String::new();
        let consumed = if matches!(sess.state, State::Direct)
            && !matches!(
                action,
                Action::ToggleMode | Action::UndoCommit | Action::NoOp
            ) {
            sess.last_commit = None;
            false
        } else {
            self.dispatch(&mut sess, action, &mut commit)
        };
        let result = self.finish(&mut sess, consumed, commit);
        self.sessions.insert(session_id.to_string(), sess);
        Ok(result)
    }

    /// フォーカスを得た（状態は変えない）
    pub fn focus_in(&mut self, session_id: &str) -> ProcessKeyResult {
        let mut sess = self.take_session(session_id);
        let result = self.finish(&mut sess, false, String::new());
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// フォーカスを失った: 表示中の surface を確定して Direct へ戻る
    pub fn focus_out(&mut self, session_id: &str) -> ProcessKeyResult {
        let mut sess = self.take_session(session_id);
        sess.origins.clear();
        let mut commit = String::new();
        self.commit_all(&mut sess, &mut commit);
        let result = self.finish(&mut sess, true, commit);
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// 未確定状態を破棄して Direct へ戻る（確定はしない）
    pub fn reset(&mut self, session_id: &str) -> ProcessKeyResult {
        let mut sess = self.take_session(session_id);
        sess.origins.clear();
        sess.state = State::Direct;
        sess.composer.clear();
        let result = self.finish(&mut sess, true, String::new());
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// `select_candidate` メソッド: 注目文節の候補一覧から絶対インデックスで選択する
    /// （マウス操作等。ページ相対の数字キー（`Action::SelectCand`）とは別経路）。
    /// Converting 状態でなければ何もしない。候補一覧が未生成なら作ってから選択する。
    /// index が範囲外なら選択を変えない
    pub fn select_candidate(&mut self, session_id: &str, index: u32) -> ProcessKeyResult {
        let mut sess = self.take_session(session_id);
        if let State::Converting(conv) = &mut sess.state {
            let f = conv.focus;
            self.ensure_candidates(&sess.left_context, conv, f);
            let index = index as usize;
            if Self::set_selected(conv, f, index) {
                sess.origins.clear();
            }
            conv.visible = false;
        }
        let result = self.finish(&mut sess, true, String::new());
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// 現在 Composing 状態なら、入力中の全体の reading（ローマ字変換済みのひらがな連結）を返す。
    /// Composing でない・未入力・ライブ変換の抑制中（G-46）なら `None`（サーバーがライブ変換のデバウンス判定に使う）
    pub fn composing_reading(&self, session_id: &str) -> Option<String> {
        let sess = self.sessions.get(session_id)?;
        let State::Composing = sess.state else {
            return None;
        };
        if sess.live_suppressed {
            return None;
        }
        let pieces = sess.composer.pieces();
        if pieces.is_empty() {
            return None;
        }
        Some(pieces.iter().map(|p| p.text.as_str()).collect())
    }

    /// ライブ変換の再変換を試みる（サーバーがデバウンス後に呼ぶ）。Composing 状態で、かつ
    /// 現在の入力全体の reading が `expected_reading` と一致する場合だけ変換を行い、Session に
    /// 反映する。一致しない（すでに別の入力になっている）場合や、変換しても表示が変わらない
    /// 場合は `None` を返す（フロントへ何も送らなくてよいという意味）
    pub fn live_convert(
        &mut self,
        session_id: &str,
        expected_reading: &str,
    ) -> Option<ProcessKeyResult> {
        let mut sess = self.take_session(session_id);
        let applied = self.try_apply_live_conversion(&mut sess, expected_reading);
        let result = if applied {
            let r = self.finish(&mut sess, false, String::new());
            r.preedit.is_some().then_some(r)
        } else {
            None
        };
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// `live_convert` の内部処理。反映できたら `true`
    fn try_apply_live_conversion(&self, sess: &mut Session, expected_reading: &str) -> bool {
        let State::Composing = sess.state else {
            return false;
        };
        if sess.live_suppressed {
            return false;
        }
        let pieces = sess.composer.pieces();
        if pieces.is_empty() {
            return false;
        }
        let full_reading: String = pieces.iter().map(|p| p.text.as_str()).collect();
        if full_reading != expected_reading {
            return false;
        }
        sess.live_conversion = Some(self.convert_live(&sess.left_context, &pieces, full_reading));
        true
    }

    /// 入力全体（`pieces`）を変換して `LiveConversion` にする。打鍵中の暫定表示（固定部分 + 追記の
    /// かな）ではなく、reading 全体を `composing_chunk_len` で分けて変換する（G-47）
    fn convert_live(
        &self,
        left_context: &str,
        pieces: &[Piece],
        full_reading: String,
    ) -> LiveConversion {
        let mut segments = Vec::new();
        let mut chunks = Vec::new();
        let mut context = left_context.to_string();
        for piece in pieces {
            match piece.kind {
                PieceKind::Japanese => {
                    self.convert_japanese_chunked(
                        &piece.text,
                        &mut context,
                        &mut segments,
                        &mut chunks,
                    );
                }
                PieceKind::Literal => {
                    context.push_str(&piece.text);
                    let id = segments.len() as u32;
                    segments.push(Segment::plain(&piece.text, SegmentKind::Literal, id));
                }
            }
        }
        LiveConversion {
            source_reading: full_reading,
            segments,
            chunks,
        }
    }

    /// ライブ変換の表示がひらがなの reading と異なる（変換結果が見えている）か。
    /// Esc で「ライブ変換の破棄」と「入力全体の破棄」を分けるのに使う（G-45）
    fn live_displays_conversion(sess: &Session) -> bool {
        let pieces = sess.composer.pieces();
        Self::live_view(sess, &pieces)
            .is_some_and(|v| v.fixed.iter().any(|s| s.surface != s.reading))
    }

    /// 現在の Composing で表示に使えるライブ変換結果。`source_reading` が現在の reading と一致すれば
    /// 全体が固定部分、末尾への追記で接頭辞になっていれば（カーソルが末尾のときだけ）固定部分 +
    /// 追記分（未変換のかな。`SegmentKind` は piece の種類どおり）を返す。それ以外は `None`
    fn live_view(sess: &Session, pieces: &[Piece]) -> Option<LiveView> {
        let lc = sess.live_conversion.as_ref()?;
        let full_reading: String = pieces.iter().map(|p| p.text.as_str()).collect();
        if lc.source_reading == full_reading {
            return Some(LiveView {
                fixed: lc.segments.clone(),
                tail: Vec::new(),
            });
        }
        let total = full_reading.chars().count();
        let fixed_chars = lc.source_reading.chars().count();
        if fixed_chars == 0
            || !full_reading.starts_with(&lc.source_reading)
            || sess.composer.cursor() != total
        {
            return None;
        }
        let mut tail = Vec::new();
        for piece in Self::pieces_after(pieces, fixed_chars) {
            let kind = match piece.kind {
                PieceKind::Japanese => SegmentKind::Japanese,
                PieceKind::Literal => SegmentKind::Literal,
            };
            let id = (lc.segments.len() + tail.len()) as u32;
            tail.push(Segment::plain(&piece.text, kind, id));
        }
        Some(LiveView {
            fixed: lc.segments.clone(),
            tail,
        })
    }

    /// `pieces` の先頭 `skip` 文字を取り除いた残りの piece 列
    fn pieces_after(pieces: &[Piece], skip: usize) -> Vec<Piece> {
        let mut skip = skip;
        let mut out = Vec::new();
        for p in pieces {
            let len = p.text.chars().count();
            if skip >= len {
                skip -= len;
                continue;
            }
            out.push(Piece {
                kind: p.kind,
                text: p.text.chars().skip(skip).collect(),
            });
            skip = 0;
        }
        out
    }

    /// 日本語の入力（ひらがなのみ）を `composing_chunk_len` 文字ずつのチャンクに分けて変換し、
    /// `segments` に追記する。`context` はチャンクをまたいで更新される左文脈（前のチャンクの
    /// 変換結果を含む）。1文字数が `composing_chunk_len` 以下なら1回のループで既存と同じ動作になる
    fn convert_japanese_chunked(
        &self,
        text: &str,
        context: &mut String,
        segments: &mut Vec<Segment>,
        chunks: &mut Vec<(String, String)>,
    ) {
        let chunk_len = self.config.composing_chunk_len.max(1);
        let chars: Vec<char> = text.chars().collect();
        for start in (0..chars.len()).step_by(chunk_len) {
            let end = (start + chunk_len).min(chars.len());
            let chunk: String = chars[start..end].iter().collect();
            let left = tail_chars(context, LEFT_CONTEXT_CHARS);
            let converted = self.backend.convert_sentence(&chunk, &left);
            let valid = !converted.is_empty()
                && converted
                    .iter()
                    .map(|s| s.reading.as_str())
                    .collect::<String>()
                    == chunk;
            if valid {
                chunks.push((chunk.clone(), left));
                for bs in converted {
                    context.push_str(&bs.surface);
                    let id = segments.len() as u32;
                    segments.push(Segment {
                        reading: bs.reading,
                        surface: bs.surface,
                        kind: SegmentKind::Japanese,
                        candidates: None,
                        chunk_id: id,
                    });
                }
            } else {
                context.push_str(&chunk);
                let id = segments.len() as u32;
                segments.push(Segment::plain(&chunk, SegmentKind::Japanese, id));
            }
        }
    }

    /// エンジン再起動後に、フロントが保持していた preedit を復元する
    pub fn restore_session(
        &mut self,
        session_id: &str,
        params: &RestoreSessionParams,
    ) -> ProcessKeyResult {
        let mut sess = self.take_session(session_id);
        sess.origins.clear();
        sess.composer.clear();
        sess.state = State::Direct;
        if !params.segments.is_empty() {
            if params.converting {
                let segments: Vec<Segment> = params
                    .segments
                    .iter()
                    .enumerate()
                    .map(|(i, s)| Segment {
                        reading: s.reading.clone(),
                        surface: s.surface.clone(),
                        kind: s.kind,
                        candidates: None,
                        chunk_id: i as u32,
                    })
                    .collect();
                let focus = (params.focus.unwrap_or(0) as usize).min(segments.len() - 1);
                sess.state = State::Converting(Conv {
                    segments,
                    focus,
                    visible: false,
                    range_anchor: None,
                    sentence_alternatives: None,
                    corrections: Vec::new(),
                    base_reading: None,
                    sentence_corrections: None,
                });
            } else {
                let pieces: Vec<Piece> = params
                    .segments
                    .iter()
                    .map(|s| Piece {
                        kind: match s.kind {
                            SegmentKind::Literal => PieceKind::Literal,
                            _ => PieceKind::Japanese,
                        },
                        text: s.reading.clone(),
                    })
                    .collect();
                sess.composer.load_pieces(&pieces, params.cursor as usize);
                sess.state = State::Composing;
            }
        }
        let result = self.finish(&mut sess, true, String::new());
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// 確定済みテキストの再変換要求（`reconvert` RPC。OS の再変換 API から）。
    /// 読みは辞書の逆引きで推定する（[`ConversionBackend::reconvert`]）。推定できなければ
    /// （バックエンドが空を返したら）Literal な1文節として扱う（reading = surface。
    /// 再変換はできないが表示・編集はできる）
    pub fn reconvert(&mut self, session_id: &str, text: &str) -> ProcessKeyResult {
        let mut sess = self.take_session(session_id);
        let mut commit = String::new();
        self.commit_all(&mut sess, &mut commit);
        if !text.is_empty() {
            let backend_segments = self.backend.reconvert(text);
            let segments: Vec<Segment> = if backend_segments.is_empty() {
                vec![Segment::plain(text, SegmentKind::Literal, 0)]
            } else {
                backend_segments
                    .into_iter()
                    .enumerate()
                    .map(|(i, bs)| Segment {
                        reading: bs.reading,
                        surface: bs.surface,
                        kind: SegmentKind::Japanese,
                        candidates: None,
                        chunk_id: i as u32,
                    })
                    .collect()
            };
            sess.state = State::Converting(Conv {
                segments,
                focus: 0,
                visible: false,
                range_anchor: None,
                sentence_alternatives: None,
                corrections: Vec::new(),
                base_reading: None,
                sentence_corrections: None,
            });
        }
        let result = self.finish(&mut sess, true, commit);
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// Surrounding Text・secure input などの文脈を保存する（secure input 中は文脈を持たない）
    pub fn set_context(&mut self, session_id: &str, ctx: &SetContextParams) {
        let mut sess = self.take_session(session_id);
        if let Some(app) = &ctx.app_id {
            sess.app_id = Some(app.clone());
        }
        if let Some(secure) = ctx.secure_input {
            sess.secure_input = secure;
            if secure {
                sess.left_context.clear();
            }
        }
        if !sess.secure_input
            && let Some(st) = &ctx.surrounding_text
        {
            let end = utf16::utf16_to_char_index(&st.text, st.cursor).unwrap_or(0);
            let before: String = st.text.chars().take(end).collect();
            sess.left_context = tail_chars(&before, LEFT_CONTEXT_CHARS);
        }
        self.sessions.insert(session_id.to_string(), sess);
    }

    /// セッションを破棄する
    pub fn end_session(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    /// 完了通知に対応する変換中セッションの未操作のプリエディットを、最新の変換結果に差し替える。
    /// 返り値は差分（`finish` の結果）があったセッションだけ
    pub fn apply_completions(&mut self, done: &[Completion]) -> Vec<(String, ProcessKeyResult)> {
        let ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| match s.state {
                State::Converting(_) => !s.origins.is_empty(),
                State::Composing => s
                    .live_conversion
                    .as_ref()
                    .is_some_and(|l| !l.chunks.is_empty()),
                State::Direct => false,
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut out = Vec::new();
        for id in ids {
            let mut sess = self.take_session(&id);
            let changed = if matches!(sess.state, State::Composing) {
                self.replace_live(&mut sess, done)
            } else {
                self.replace_origins(&mut sess, done)
            };
            if changed {
                let result = self.finish(&mut sess, false, String::new());
                if result.preedit.is_some() || result.candidates.is_some() {
                    out.push((id.clone(), result));
                }
            }
            self.sessions.insert(id, sess);
        }
        out
    }

    /// ライブ変換の LLM 遅延完了の取り込み。`done` が表示中のライブ変換のチャンクと一致し、
    /// 入力が変換時から変わっていなければ、全文を変換し直して置き換える（キャッシュに結果が
    /// あるので LLM 込みの結果になる）。入力が変わっていれば、デバウンス後の `live_convert` が
    /// 全文を変換するのでここでは何もしない。変更したら `true`（G-47）
    fn replace_live(&self, sess: &mut Session, done: &[Completion]) -> bool {
        if sess.live_suppressed {
            return false;
        }
        let Some(lc) = &sess.live_conversion else {
            return false;
        };
        let matched = lc
            .chunks
            .iter()
            .any(|(r, l)| done.iter().any(|c| &c.reading == r && &c.left_context == l));
        if !matched {
            return false;
        }
        let pieces = sess.composer.pieces();
        let full_reading: String = pieces.iter().map(|p| p.text.as_str()).collect();
        if full_reading != lc.source_reading {
            return false;
        }
        let fresh = self.convert_live(&sess.left_context, &pieces, full_reading);
        let same = lc
            .segments
            .iter()
            .map(|s| (&s.reading, &s.surface))
            .eq(fresh.segments.iter().map(|s| (&s.reading, &s.surface)));
        if same {
            return false;
        }
        sess.live_conversion = Some(fresh);
        true
    }

    /// `done` に一致する `Origin` の範囲を最新の変換結果で置き換える。変更したら `true`
    fn replace_origins(&self, sess: &mut Session, done: &[Completion]) -> bool {
        let State::Converting(conv) = &mut sess.state else {
            return false;
        };
        let mut changed = false;
        // 前の差し替えによる文節数の増減
        let mut delta: isize = 0;
        for origin in sess.origins.iter_mut() {
            origin.start = (origin.start as isize + delta) as usize;
            let matched = done
                .iter()
                .any(|c| c.reading == origin.reading && c.left_context == origin.left);
            let range = origin.start..origin.start + origin.count;
            if !matched || range.end > conv.segments.len() {
                continue;
            }
            let (fresh, valid) = self.convert_piece(&origin.reading, &origin.left);
            let same = conv.segments[range.clone()]
                .iter()
                .map(|s| (&s.reading, &s.surface))
                .eq(fresh.iter().map(|s| (&s.reading, &s.surface)));
            if !valid || same {
                continue;
            }
            delta += fresh.len() as isize - origin.count as isize;
            origin.count = fresh.len();
            conv.segments.splice(range, fresh);
            changed = true;
        }
        if changed {
            for (i, s) in conv.segments.iter_mut().enumerate() {
                s.chunk_id = i as u32;
            }
            conv.focus = conv.focus.min(conv.segments.len() - 1);
            // 差し替え前の sentence_alternatives は、もう conv.segments と対応しないので捨てる
            // （そのまま残すと、次の ensure_segmented で古い切り方に巻き戻ってしまう）
            conv.sentence_alternatives = None;
        }
        changed
    }

    /// セッションの現在状態（未作成なら None）
    pub fn snapshot(&self, session_id: &str) -> Option<SessionSnapshot> {
        let sess = self.sessions.get(session_id)?;
        Some(SessionSnapshot {
            state: match sess.state {
                State::Direct => StateKind::Direct,
                State::Composing => StateKind::Composing,
                State::Converting(ref conv) if conv.range_anchor.is_some() => {
                    StateKind::RangeSelecting
                }
                State::Converting(_) => StateKind::Converting,
            },
            preedit: self.build_preedit(sess).to_view(),
            candidates: self.build_candidates(sess),
            mode: sess.mode,
            revision: sess.revision as u32,
        })
    }

    // ---- キー処理 ----

    fn handle_key(&self, sess: &mut Session, key: &KeyEvent, commit: &mut String) -> bool {
        if key.kind == KeyKind::FlagsChanged {
            return false;
        }
        let ctx = match sess.state {
            State::Direct => KeyContext::Direct,
            State::Composing => KeyContext::Composing,
            State::Converting(_) => KeyContext::Converting,
        };
        let action = self.keymap.resolve(ctx, sess.mode, key);
        // 確定取消（Ctrl+Backspace）以外の操作が来たら、確定直後の記憶を捨てる。
        // 未割当キー（素通し）でも捨てる（「確定直後のみ有効」という制約はここで実現する）
        if !matches!(action, Some(Action::UndoCommit)) {
            sess.last_commit = None;
        }
        match action {
            Some(action) => self.dispatch(sess, action, commit),
            // 割当なし: 未入力なら素通し、入力中はキーを消費して何もしない
            None => !matches!(sess.state, State::Direct),
        }
    }

    /// 確定取消以外の操作では確定直後の記憶を捨ててから、Action を適用する
    /// （`handle_key` と `perform_action` の共通経路）。戻り値は「消費したか」
    fn dispatch(&self, sess: &mut Session, action: Action, commit: &mut String) -> bool {
        if !matches!(action, Action::UndoCommit) {
            sess.last_commit = None;
        }
        self.apply(sess, action, commit)
    }

    /// Action を適用する。戻り値は「キーを消費したか」
    fn apply(&self, sess: &mut Session, action: Action, commit: &mut String) -> bool {
        match action {
            Action::SetMode(mode) => {
                self.commit_all(sess, commit);
                sess.mode = mode;
                true
            }
            Action::ToggleMode => {
                self.commit_all(sess, commit);
                sess.mode = match sess.mode {
                    InputMode::Hiragana => InputMode::Alphanumeric,
                    InputMode::Alphanumeric => InputMode::Hiragana,
                };
                true
            }
            Action::CommitAndPassThrough => {
                self.commit_all(sess, commit);
                false
            }
            Action::UndoCommit => self.try_undo_commit(sess),
            Action::NoOp => true,
            Action::Insert(c) => {
                self.insert_char(sess, c, commit);
                true
            }
            Action::InsertText(text) => {
                // 代替は 1 キーの文字入力（Insert）に対するものなので、テキスト挿入には使わない
                sess.key_alts.clear();
                let mut chars = text.chars();
                if let Some(first) = chars.next() {
                    self.insert_char(sess, first, commit);
                    for c in chars {
                        sess.composer.insert_char(c);
                    }
                }
                true
            }
            action => {
                match sess.state {
                    State::Direct => {}
                    State::Composing => self.apply_composing(sess, action, commit),
                    State::Converting(_) => self.apply_converting(sess, action, commit),
                }
                true
            }
        }
    }

    fn insert_char(&self, sess: &mut Session, c: char, commit: &mut String) {
        self.begin_insert(sess, commit);
        let alts = std::mem::take(&mut sess.key_alts);
        sess.composer.insert_char_with_alts(c, &alts);
        sess.state = State::Composing;
    }

    /// 文字挿入の前処理（Converting なら確定、ライブ変換結果の破棄など）。
    /// `insert_char` と `insert_reading` で共通
    fn begin_insert(&self, sess: &mut Session, commit: &mut String) {
        if matches!(sess.state, State::Converting(_)) {
            // 変換結果を確定して新しい入力を始める
            self.commit_all(sess, commit);
        }
        // 文中編集は固定部分と対応が取れないので変換結果を捨てる（G-43）
        // 新規入力の開始でも古い結果を持ち越さない
        if sess.composer.cursor() != sess.composer.len() || matches!(sess.state, State::Direct) {
            sess.live_conversion = None;
        }
        // 文字の入力（追記）でライブ変換の抑制を解く（G-46）
        sess.live_suppressed = false;
    }

    fn apply_composing(&self, sess: &mut Session, action: Action, commit: &mut String) {
        match action {
            Action::Backspace => {
                // 表示中の変換（固定部分を含む）はすべて破棄し、ひらがなに戻してから1文字削除する
                sess.live_conversion = None;
                sess.live_suppressed = true;
                sess.composer.backspace();
                self.leave_if_empty(sess);
            }
            Action::DeleteForward => {
                sess.live_conversion = None;
                sess.live_suppressed = true;
                sess.composer.delete();
                self.leave_if_empty(sess);
            }
            Action::MoveCursor(Direction::Left) => {
                sess.composer.move_left();
            }
            Action::MoveCursor(Direction::Right) => {
                sess.composer.move_right();
            }
            Action::MoveCursorHome => sess.composer.move_home(),
            Action::MoveCursorEnd => sess.composer.move_end(),
            Action::Cancel => {
                if Self::live_displays_conversion(sess) {
                    // ライブ変換の表示中は、変換だけを破棄してひらがな表示に戻す（G-45）
                    sess.live_conversion = None;
                    sess.live_suppressed = true;
                } else {
                    sess.composer.clear();
                    sess.live_conversion = None;
                    sess.state = State::Direct;
                }
            }
            Action::Commit => self.commit_all(sess, commit),
            Action::StartConvert | Action::NextCand => self.start_converting(sess, Start::Convert),
            Action::Predict => self.predict(sess),
            Action::ToKatakana => self.start_converting(sess, Start::Katakana),
            Action::ToHiragana => self.start_converting(sess, Start::Hiragana),
            _ => {}
        }
    }

    fn leave_if_empty(&self, sess: &mut Session) {
        if sess.composer.is_empty() {
            sess.composer.clear();
            sess.state = State::Direct;
        }
    }

    /// 読みを `convert_sentence` で変換し、(文節列, バックエンドの結果が妥当か) を返す。
    /// 妥当でなければ読みそのままの 1 文節にする（`chunk_id` は呼び出し側で振る）
    fn convert_piece(&self, reading: &str, left: &str) -> (Vec<Segment>, bool) {
        let converted = self.backend.convert_sentence(reading, left);
        let valid = !converted.is_empty()
            && converted
                .iter()
                .map(|s| s.reading.as_str())
                .collect::<String>()
                == reading;
        let converted = if valid {
            converted
        } else {
            vec![BackendSegment {
                reading: reading.to_string(),
                surface: reading.to_string(),
                sources: vec![CandidateSource::Kana],
            }]
        };
        let segments: Vec<Segment> = converted
            .into_iter()
            .map(|bs| Segment {
                reading: bs.reading,
                surface: bs.surface,
                kind: SegmentKind::Japanese,
                candidates: None,
                chunk_id: 0, // あとで振り直す
            })
            .collect();
        (segments, valid)
    }

    /// 文全体N-best（`conv.sentence_alternatives`）を表示中に、候補選択以外の操作が来たら、
    /// 現在選択中の候補の文節分割へ丸ごと置き換える（以後は通常の文節編集として扱う）
    fn ensure_segmented(&self, conv: &mut Conv) {
        let Some(alternatives) = conv.sentence_alternatives.take() else {
            return;
        };
        let selected = conv
            .segments
            .first()
            .and_then(|s| s.candidates.as_ref())
            .map(|c| c.selected)
            .unwrap_or(0);
        if let Some(segments) = alternatives.into_iter().nth(selected) {
            conv.segments = segments;
            conv.focus = 0;
        }
    }

    fn apply_converting(&self, sess: &mut Session, action: Action, commit: &mut String) {
        if !matches!(
            action,
            Action::NextCand | Action::StartConvert | Action::PrevCand | Action::SelectCand(_)
        ) && let State::Converting(conv) = &mut sess.state
        {
            self.ensure_segmented(conv);
        }
        let selecting =
            matches!(&sess.state, State::Converting(conv) if conv.range_anchor.is_some());
        match action {
            Action::NextCand | Action::StartConvert | Action::PrevCand if selecting => {
                self.reconvert_selection(sess);
            }
            Action::NextCand | Action::StartConvert => self.step_candidate(sess, 1),
            Action::PrevCand => self.step_candidate(sess, -1),
            Action::MoveFocus(dir) => self.move_focus(sess, dir),
            Action::ResizeSegment(dir) => self.resize_segment(sess, dir),
            Action::ExtendSelection(dir) => self.extend_selection(sess, dir),
            Action::Commit => self.commit_all(sess, commit),
            Action::Cancel => self.cancel_converting(sess),
            Action::ToKatakana => self.set_focus_kana(sess, true),
            Action::ToHiragana => self.set_focus_kana(sess, false),
            Action::ToHankaku => self.set_focus_hankaku(sess),
            Action::SelectCand(n) => self.select_candidate_page_relative(sess, n, commit),
            Action::DeleteLearning => self.delete_learning(sess),
            _ => {}
        }
    }

    /// Escape: 複数文節選択中はまず選択だけ解除する。選択していなければ Composing へ戻る
    fn cancel_converting(&self, sess: &mut Session) {
        let cleared = if let State::Converting(conv) = &mut sess.state {
            conv.range_anchor.take().is_some()
        } else {
            false
        };
        if !cleared {
            self.back_to_composing(sess);
        }
    }

    /// `Ctrl+Shift+←/→`: 複数文節の選択を開始・伸縮する。`focus` を選択の可動端として動かし、
    /// 反対側の固定端を `range_anchor` に持つ。`focus` が `range_anchor` まで戻ったら選択を解除する
    fn extend_selection(&self, sess: &mut Session, dir: Direction) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        if conv.range_anchor.is_none() {
            conv.range_anchor = Some(conv.focus);
        }
        conv.visible = false;
        match dir {
            Direction::Left => conv.focus = conv.focus.saturating_sub(1),
            Direction::Right => conv.focus = (conv.focus + 1).min(conv.segments.len() - 1),
        }
        if conv.range_anchor == Some(conv.focus) {
            conv.range_anchor = None;
        }
    }

    /// 現在の選択範囲（セグメント index の半開区間）。選択していなければ `None`
    fn selection_range(conv: &Conv) -> Option<Range<usize>> {
        let anchor = conv.range_anchor?;
        let lo = anchor.min(conv.focus);
        let hi = anchor.max(conv.focus);
        Some(lo..hi + 1)
    }

    /// 複数文節選択中に Space（`NextCand`/`PrevCand`/`StartConvert`）が来たら、選択範囲全体を
    /// 1つの読みとして再変換する（設計書 §4.10「範囲のみ再変換（候補は範囲の reading に対して
    /// 生成）」）。結果は1つの文節に置き換わり、候補ウィンドウを開いた状態にする
    /// （そのまま Space で候補をめくれる。「選択状態の維持」の意図を、選択そのものではなく
    /// 再変換結果への候補アクセスの継続として実現する）
    fn reconvert_selection(&self, sess: &mut Session) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        let Some(range) = Self::selection_range(conv) else {
            return;
        };
        let merged_reading: String = conv.segments[range.clone()]
            .iter()
            .map(|s| s.reading.as_str())
            .collect();
        let left = Self::left_context_for(&sess.left_context, conv, range.start);
        let backend_candidates =
            self.backend
                .segment_candidates(&merged_reading, &left, CANDIDATE_LIMIT);
        let katakana = to_katakana(&merged_reading);
        let mut items: Vec<Candidate> = Vec::new();
        for c in &backend_candidates {
            push_candidate(&mut items, &c.surface, &c.sources);
        }
        push_candidate(&mut items, &merged_reading, &[CandidateSource::Kana]);
        push_candidate(&mut items, &katakana, &[CandidateSource::Kana]);
        let surface = items
            .first()
            .map(|c| c.surface.clone())
            .unwrap_or_else(|| merged_reading.clone());
        let new_focus = range.start;
        let new_segment = Segment {
            reading: merged_reading,
            surface,
            kind: SegmentKind::Japanese,
            candidates: Some(CandidateSet { items, selected: 0 }),
            chunk_id: range.start as u32,
        };
        conv.segments.splice(range, [new_segment]);
        for (i, s) in conv.segments.iter_mut().enumerate() {
            s.chunk_id = i as u32;
        }
        conv.focus = new_focus;
        conv.range_anchor = None;
        conv.visible = true;
    }

    // ---- 確定・遷移 ----

    /// 表示中の内容をすべて確定文字列へ移し、Direct に戻す
    fn commit_all(&self, sess: &mut Session, commit: &mut String) {
        let before_len = commit.len();
        let pieces = match std::mem::replace(&mut sess.state, State::Direct) {
            State::Direct => None,
            State::Composing => {
                // 表示中のライブ変換結果（`build_preedit` と同じ条件）。flush 前の reading で照合する
                let displayed_pieces = sess.composer.pieces();
                let displayed_reading: String =
                    displayed_pieces.iter().map(|p| p.text.as_str()).collect();
                let live = Self::live_view(sess, &displayed_pieces);
                sess.live_conversion = None;
                sess.composer.flush_pending();
                let pieces = sess.composer.pieces();
                let flushed_text = sess.composer.text();
                match live {
                    // 保留（単独の n 等）の flush が末尾にだけ足された場合は、その分を後ろに付ける。
                    // カーソルが途中で flush 分が途中に入った場合は対応を取れないので読みで確定する
                    Some(v) if flushed_text.starts_with(&displayed_reading) => {
                        // 学習するのは変換済みの固定部分だけ（追記分は未変換のかな）
                        self.notify_commit(sess, &v.fixed);
                        for s in v.fixed.iter().chain(&v.tail) {
                            commit.push_str(&s.surface);
                        }
                        commit.push_str(&flushed_text[displayed_reading.len()..]);
                    }
                    _ => commit.push_str(&flushed_text),
                }
                (!pieces.is_empty()).then_some(pieces)
            }
            State::Converting(conv) => {
                self.notify_commit(sess, &conv.segments);
                let pieces: Vec<Piece> = conv
                    .segments
                    .iter()
                    .map(|s| Piece {
                        kind: match s.kind {
                            SegmentKind::Literal => PieceKind::Literal,
                            _ => PieceKind::Japanese,
                        },
                        text: s.reading.clone(),
                    })
                    .collect();
                for s in &conv.segments {
                    commit.push_str(&s.surface);
                }
                (!pieces.is_empty()).then_some(pieces)
            }
        };
        sess.composer.clear();
        sess.last_commit = pieces.map(|pieces| LastCommit {
            pieces,
            committed_len_utf16: utf16::utf16_len(&commit[before_len..]),
        });
    }

    /// `Action::UndoCommit`（Ctrl+Backspace）の処理。確定直後で `last_commit` が残っている
    /// 場合だけ、確定前の入力状態（Composing）に戻す。戻り値は「キーを消費したか」
    /// （何も戻すものが無ければ消費せず素通しする。ホスト側の Ctrl+Backspace の既定動作に譲る）
    fn try_undo_commit(&self, sess: &mut Session) -> bool {
        let Some(last) = sess.last_commit.take() else {
            return false;
        };
        let len = last.pieces.iter().map(|p| p.text.chars().count()).sum();
        sess.composer.load_pieces(&last.pieces, len);
        sess.state = State::Composing;
        sess.pending_delete_before = Some(last.committed_len_utf16);
        true
    }

    /// 変換を経て確定した文節を observer（学習など）へ通知する
    fn notify_commit(&self, sess: &Session, committed: &[Segment]) {
        let Some(observer) = &self.observer else {
            return;
        };
        let mut left = sess.left_context.clone();
        let mut segments = Vec::with_capacity(committed.len());
        for seg in committed {
            segments.push(CommittedSegment {
                reading: seg.reading.clone(),
                surface: seg.surface.clone(),
                kind: seg.kind,
                left_context: tail_chars(&left, LEFT_CONTEXT_CHARS),
            });
            left.push_str(&seg.surface);
        }
        let ctx = CommitContext {
            session_id: &sess.id,
            app_id: sess.app_id.as_deref(),
            secure_input: sess.secure_input,
        };
        observer.on_commit(&ctx, &segments);
    }

    /// 注目文節の現在の表記の学習を忘れさせ、候補一覧を作り直す
    fn delete_learning(&self, sess: &mut Session) {
        let Some(observer) = self.observer.clone() else {
            return;
        };
        let Session {
            state,
            id,
            app_id,
            secure_input,
            left_context,
            ..
        } = sess;
        let State::Converting(conv) = state else {
            return;
        };
        let f = conv.focus;
        if conv.segments[f].kind != SegmentKind::Japanese {
            return;
        }
        let ctx = CommitContext {
            session_id: id,
            app_id: app_id.as_deref(),
            secure_input: *secure_input,
        };
        let (reading, surface) = (
            conv.segments[f].reading.clone(),
            conv.segments[f].surface.clone(),
        );
        if observer.on_forget(&ctx, &reading, &surface) {
            conv.segments[f].candidates = None;
            self.ensure_candidates(left_context, conv, f);
        }
    }

    /// `Tab`: Composing 中に学習キャッシュ・ユーザー辞書の前方一致（`ConversionBackend::predict`）
    /// から予測変換を行う。見つかれば Converting へ入り候補ウィンドウを開く
    /// （Mozc の `PredictAndConvert` と同じ考え方）。見つからなければ何もしない（Composing のまま）
    fn predict(&self, sess: &mut Session) {
        if !self.config.predict_enabled {
            return;
        }
        if !matches!(sess.state, State::Composing) {
            return;
        }
        sess.composer.flush_pending();
        let pieces = sess.composer.pieces();
        if pieces.is_empty() || pieces.iter().any(|p| p.kind != PieceKind::Japanese) {
            return;
        }
        let prefix: String = pieces.iter().map(|p| p.text.as_str()).collect();
        if prefix.chars().count() < self.config.predict_min_chars {
            return;
        }
        let left = tail_chars(&sess.left_context, LEFT_CONTEXT_CHARS);
        let backend_candidates = self.backend.predict(&prefix, &left, CANDIDATE_LIMIT);
        if backend_candidates.is_empty() {
            return;
        }
        let mut items: Vec<Candidate> = Vec::new();
        for c in &backend_candidates {
            push_candidate(&mut items, &c.surface, &c.sources);
        }
        let surface = items[0].surface.clone();
        sess.composer.clear();
        sess.state = State::Converting(Conv {
            segments: vec![Segment {
                reading: prefix,
                surface,
                kind: SegmentKind::Japanese,
                candidates: Some(CandidateSet { items, selected: 0 }),
                chunk_id: 0,
            }],
            focus: 0,
            visible: true,
            range_anchor: None,
            sentence_alternatives: None,
            corrections: Vec::new(),
            base_reading: None,
            sentence_corrections: None,
        });
    }

    fn start_converting(&self, sess: &mut Session, how: Start) {
        sess.composer.flush_pending();
        sess.composer.end_literal_run();
        let pieces = sess.composer.pieces();
        if pieces.is_empty() {
            sess.composer.clear();
            sess.state = State::Direct;
            return;
        }
        sess.origins.clear();
        // Space 変換は常に新しく変換する（古いライブ変換結果を持ち越さない。G-44）
        sess.live_conversion = None;
        // 曖昧入力の補正候補（`fuzzy.enabled` で打鍵ログが有効、かつ補正が必要なときだけ Some）。
        // 第一仮説（打たれたとおりの reading）の変換には一切影響しない
        let fuzzy = if how == Start::Convert {
            self.fuzzy_corrections(sess, &pieces)
        } else {
            None
        };
        if how == Start::Convert
            && self.config.initial_view == InitialView::Sentence
            && pieces.len() == 1
            && pieces[0].kind == PieceKind::Japanese
            && let Some(conv) =
                self.start_converting_sentence_n_best(sess, &pieces[0].text, fuzzy.as_ref())
        {
            // メガ文節も他の文節と同じく非同期のLLM完了差し替えの対象にする（G-32 系の統合）。
            // 予算超過で最初は辞書寄りの表示になっていても、遅延結果が届き次第 replace_origins
            // で差し替えられる
            sess.origins.push(Origin {
                start: 0,
                count: conv.segments.len(),
                reading: pieces[0].text.clone(),
                left: tail_chars(&sess.left_context, LEFT_CONTEXT_CHARS),
            });
            sess.state = State::Converting(conv);
            return;
        }
        let mut segments: Vec<Segment> = Vec::new();
        let mut context = sess.left_context.clone();
        let base = fuzzy.is_some().then(|| pieces_reading(&pieces));
        for piece in pieces {
            match (piece.kind, how) {
                (PieceKind::Japanese, Start::Convert) => {
                    let left_tail = tail_chars(&context, LEFT_CONTEXT_CHARS);
                    let (converted, valid) = self.convert_piece(&piece.text, &left_tail);
                    let start = segments.len();
                    let count = converted.len();
                    if valid {
                        sess.origins.push(Origin {
                            start,
                            count,
                            reading: piece.text.clone(),
                            left: left_tail,
                        });
                    }
                    for mut seg in converted {
                        context.push_str(&seg.surface);
                        seg.chunk_id = segments.len() as u32;
                        segments.push(seg);
                    }
                }
                (PieceKind::Japanese, Start::Katakana) => {
                    let surface = to_katakana(&piece.text);
                    context.push_str(&surface);
                    segments.push(Segment {
                        reading: piece.text,
                        surface,
                        kind: SegmentKind::Japanese,
                        candidates: None,
                        chunk_id: segments.len() as u32,
                    });
                }
                (PieceKind::Japanese, Start::Hiragana) | (PieceKind::Literal, _) => {
                    context.push_str(&piece.text);
                    let kind = match piece.kind {
                        PieceKind::Japanese => SegmentKind::Japanese,
                        PieceKind::Literal => SegmentKind::Literal,
                    };
                    let id = segments.len() as u32;
                    segments.push(Segment::plain(&piece.text, kind, id));
                }
            }
        }
        let mut conv = Conv {
            segments,
            focus: 0,
            visible: false,
            range_anchor: None,
            sentence_alternatives: None,
            corrections: Vec::new(),
            base_reading: None,
            sentence_corrections: None,
        };
        if let (Some(outcome), Some(base)) = (fuzzy, base) {
            self.attach_corrections(sess, &mut conv, outcome, base);
        }
        sess.state = State::Converting(conv);
    }

    /// 通常の変換結果（第一仮説）に曖昧入力の補正候補を添える。
    ///
    /// - 結果が日本語 1 文節で、reading が入力全体の場合: 補正仮説を `conv.corrections` に持ち、
    ///   `ensure_candidates` が文節の候補の末尾に Corrected として足す
    /// - それ以外（複数文節・Literal を含む入力。`nihn` の `h` のように、不一致のローマ字は
    ///   Literal として残る）: 構造は変えず `conv.sentence_corrections` に持ち、各文節の候補の末尾に
    ///   文全体の補正候補を足す。選んだときだけ文全体候補（`sentence_alternatives`）へ置き換わる
    ///   （`promote` のときだけは従来どおり最初から文全体候補にして補正を先頭に出す）
    fn attach_corrections(
        &self,
        sess: &mut Session,
        conv: &mut Conv,
        outcome: FuzzyOutcome,
        base: String,
    ) {
        if outcome.corrections.is_empty() {
            return;
        }
        let single = conv.segments.len() == 1
            && conv.segments[0].kind == SegmentKind::Japanese
            && conv.segments[0].reading == base;
        if single {
            conv.corrections = outcome.corrections;
            conv.base_reading = Some(base);
            if outcome.promote {
                self.ensure_candidates(&sess.left_context, conv, 0);
                Self::promote_first_correction(conv);
            }
            return;
        }
        if !outcome.promote {
            // 第一仮説の表示・文節構造・候補リストは変えない。補正は注目文節の候補の末尾に
            // 追加項目として出し、選ばれたときだけ文全体を置き換える（`restructure_to_correction`）
            conv.sentence_corrections = Some((base, outcome.corrections));
            return;
        }
        let first_segments = std::mem::take(&mut conv.segments);
        let first_surface: String = first_segments.iter().map(|s| s.surface.as_str()).collect();
        let first_source = if first_surface == base {
            CandidateSource::Kana
        } else {
            CandidateSource::SystemDict
        };
        let mut items = vec![Candidate {
            surface: first_surface,
            sources: vec![first_source],
            reading: None,
        }];
        let mut alternatives = vec![first_segments];
        Self::push_corrections(&mut items, &mut alternatives, &outcome.corrections);
        if outcome.promote && items.len() > 1 {
            items.swap(0, 1);
            alternatives.swap(0, 1);
        }
        sess.origins.clear();
        conv.segments = vec![Segment {
            reading: base,
            surface: items[0].surface.clone(),
            kind: SegmentKind::Japanese,
            candidates: Some(CandidateSet { items, selected: 0 }),
            chunk_id: 0,
        }];
        conv.sentence_alternatives = Some(alternatives);
    }

    /// 補正候補を文全体の候補（`items` と対応する `alternatives`）の末尾に足す。
    /// 表記が既存の候補と同じものは足さない
    fn push_corrections(
        items: &mut Vec<Candidate>,
        alternatives: &mut Vec<Vec<Segment>>,
        corrections: &[Correction],
    ) {
        for c in corrections {
            if items.iter().any(|i| i.surface == c.surface) {
                continue;
            }
            items.push(Candidate {
                surface: c.surface.clone(),
                sources: vec![CandidateSource::Corrected],
                reading: Some(c.reading.clone()),
            });
            alternatives.push(
                c.segments
                    .iter()
                    .enumerate()
                    .map(|(i, s)| Segment {
                        reading: s.reading.clone(),
                        surface: s.surface.clone(),
                        kind: SegmentKind::Japanese,
                        candidates: None,
                        chunk_id: i as u32,
                    })
                    .collect(),
            );
        }
    }

    /// 先頭の文節の候補のうち、最初の補正候補を先頭へ移して選択する（`promote_corrected`）
    fn promote_first_correction(conv: &mut Conv) {
        let Some(set) = conv.segments[0].candidates.as_mut() else {
            return;
        };
        let Some(pos) = set.items.iter().position(|c| c.reading.is_some()) else {
            return;
        };
        let item = set.items.remove(pos);
        set.items.insert(0, item);
        Self::set_selected(conv, 0, 0);
    }

    /// 曖昧入力の補正仮説を作って変換する（Space 変換のときだけ呼ぶ）。
    ///
    /// 次のいずれかを満たすときだけ `Some`（満たさなければ仮説探索 `expand` 自体を呼ばない）:
    /// 1. 第一仮説の変換が低品質（`quality.uncovered >= poor_coverage`）
    /// 2. 入力モデル上の曖昧さ: 打鍵の代替のうち、コストが `ambiguity_margin` 以内のものがある
    ///    （「`expand` の第 2 仮説の cost が margin 以内」を、探索せずに代替キーの cost で近似する。
    ///    タイポ編集のコストは既定で 2.0 以上なので margin 1.5 では編集由来の曖昧さは対象外）
    ///
    /// 打鍵ログが無効（文中編集・Literal の混入・復元など）なら `None`。補正仮説の順位は
    /// `total = conv_cost + input_weight * input_cost`。第一仮説の `conv_cost` より
    /// `ambiguity_margin` を超えて悪いものは、条件 1 の場合を除いて捨てる。最大 `hypotheses - 1` 件
    fn fuzzy_corrections(&self, sess: &Session, pieces: &[Piece]) -> Option<FuzzyOutcome> {
        let fz = &self.config.fuzzy;
        if !fz.enabled || fz.search.max_hypotheses <= 1 {
            return None;
        }
        let log = sess.composer.key_log()?;
        let oracle = self.backend.prefix_oracle()?;
        let base = pieces_reading(pieces);
        let quality = self.backend.quality(&base)?;
        let keys: Vec<KeyAlternatives> = log
            .iter()
            .map(|r| {
                // 打たれたキー自身の距離があれば、代替のコストはそれを基準にした相対値になる
                let mut alts = r.alts.clone();
                if let Some(d) = r.typed_distance {
                    alts.push((r.typed, d));
                }
                KeyAlternatives::from_distances(r.typed, &alts, &fz.search)
            })
            .collect();
        let poor = quality.uncovered >= fz.poor_coverage;
        let ambiguous = keys.iter().any(|k| {
            k.items()
                .get(1)
                .is_some_and(|a| a.cost <= fz.ambiguity_margin)
        });
        if !poor && !ambiguous {
            return None;
        }
        let hyps = nagi_fuzzy::expand(&keys, &self.table, oracle, &fz.search);
        // 打鍵ログの再生が入力と食い違う場合は補正しない（入力を失わない）
        if hyps.len() < 2 || hyps[0].reading != base {
            return None;
        }
        let left = tail_chars(&sess.left_context, LEFT_CONTEXT_CHARS);
        let limit = fz.search.max_hypotheses - 1;
        let scored = self.backend.convert_hypotheses(&hyps[1..], &left, limit);
        let mut ranked: Vec<(f32, Correction)> = Vec::new();
        for s in scored {
            let Some(h) = hyps.get(s.hypothesis_index + 1) else {
                continue;
            };
            let joined: String = s.segments.iter().map(|x| x.reading.as_str()).collect();
            if s.segments.is_empty() || joined != h.reading {
                continue;
            }
            let total = s.conv_cost + fz.input_weight * h.cost;
            if !poor && total > quality.conv_cost + fz.ambiguity_margin {
                continue;
            }
            let surface: String = s.segments.iter().map(|x| x.surface.as_str()).collect();
            ranked.push((
                total,
                Correction {
                    reading: h.reading.clone(),
                    segments: s.segments,
                    surface,
                },
            ));
        }
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        ranked.truncate(limit);
        let corrections: Vec<Correction> = ranked.into_iter().map(|(_, c)| c).collect();
        (!corrections.is_empty()).then_some(FuzzyOutcome {
            corrections,
            promote: fz.promote_corrected && poor,
        })
    }

    /// `initial_view = sentence` かつ入力全体が1つの日本語 Piece のときに、文全体のN-bestを
    /// 候補ウィンドウに載せた単一の「メガ文節」を作る。候補が2件未満（多様性が無い）なら
    /// `None` を返し、呼び出し側は通常の（1-best・文節ごとの）変換にフォールバックする
    ///
    /// `fuzzy`（曖昧入力の補正）があれば、補正候補を文全体の候補の末尾に足す（`promote` なら 2 番目へ
    /// ではなく先頭へ）。補正候補で確定すると、その仮説の文節列・reading に置き換わる
    fn start_converting_sentence_n_best(
        &self,
        sess: &Session,
        reading: &str,
        fuzzy: Option<&FuzzyOutcome>,
    ) -> Option<Conv> {
        let context = tail_chars(&sess.left_context, LEFT_CONTEXT_CHARS);
        let beam_width = self.config.beam_width.max(2);
        let candidates = self
            .backend
            .sentence_candidates(reading, &context, beam_width);
        let mut alternatives: Vec<Vec<Segment>> = Vec::new();
        let mut items: Vec<Candidate> = Vec::new();
        for bs in candidates {
            let joined_reading: String = bs.segments.iter().map(|s| s.reading.as_str()).collect();
            if joined_reading != reading {
                continue;
            }
            // 文節ごとに異なりうる出所（辞書のk-best・実LLMのビームサーチなど）の和集合を併記する
            let mut sources: Vec<CandidateSource> = Vec::new();
            for seg in &bs.segments {
                for s in &seg.sources {
                    if !sources.contains(s) {
                        sources.push(*s);
                    }
                }
            }
            if sources.is_empty() {
                sources.push(CandidateSource::SystemDict);
            }
            let segments: Vec<Segment> = bs
                .segments
                .into_iter()
                .enumerate()
                .map(|(i, s)| Segment {
                    reading: s.reading,
                    surface: s.surface,
                    kind: SegmentKind::Japanese,
                    candidates: None,
                    chunk_id: i as u32,
                })
                .collect();
            let surface: String = segments.iter().map(|s| s.surface.as_str()).collect();
            items.push(Candidate {
                surface,
                sources,
                reading: None,
            });
            alternatives.push(segments);
        }
        if alternatives.len() < 2 {
            return None;
        }
        if let Some(outcome) = fuzzy {
            Self::push_corrections(&mut items, &mut alternatives, &outcome.corrections);
            if outcome.promote
                && let Some(pos) = items.iter().position(|c| c.reading.is_some())
            {
                let item = items.remove(pos);
                items.insert(0, item);
                let alt = alternatives.remove(pos);
                alternatives.insert(0, alt);
            }
        }
        let mega = Segment {
            reading: reading.to_string(),
            surface: items[0].surface.clone(),
            kind: SegmentKind::Japanese,
            candidates: Some(CandidateSet { items, selected: 0 }),
            chunk_id: 0,
        };
        Some(Conv {
            segments: vec![mega],
            focus: 0,
            visible: true,
            range_anchor: None,
            sentence_alternatives: Some(alternatives),
            corrections: Vec::new(),
            base_reading: None,
            sentence_corrections: None,
        })
    }

    fn back_to_composing(&self, sess: &mut Session) {
        let State::Converting(conv) = &sess.state else {
            return;
        };
        let pieces: Vec<Piece> = conv
            .segments
            .iter()
            .map(|s| Piece {
                kind: match s.kind {
                    SegmentKind::Literal => PieceKind::Literal,
                    _ => PieceKind::Japanese,
                },
                text: s.reading.clone(),
            })
            .collect();
        let len = pieces.iter().map(|p| p.text.chars().count()).sum();
        sess.composer.load_pieces(&pieces, len);
        // 変換の取り消し。以前のライブ変換結果が残っていると Composing で変換表示に戻ってしまうので、
        // 必ずひらがな表示にする（G-44）
        sess.live_conversion = None;
        sess.live_suppressed = true;
        sess.state = State::Composing;
    }

    // ---- 変換中の操作 ----

    /// `idx` 番目の文節より左の文脈（Surrounding Text + 前の文節の表記）
    fn left_context_for(surrounding: &str, conv: &Conv, idx: usize) -> String {
        let mut s = surrounding.to_string();
        for seg in &conv.segments[..idx] {
            s.push_str(&seg.surface);
        }
        tail_chars(&s, LEFT_CONTEXT_CHARS)
    }

    /// 文節の候補一覧を（未生成なら）作る。先頭は現在の表記、末尾にひらがな・カタカナ
    fn ensure_candidates(&self, surrounding: &str, conv: &mut Conv, idx: usize) {
        if conv.segments[idx].candidates.is_some() {
            return;
        }
        let left = Self::left_context_for(surrounding, conv, idx);
        let seg = &conv.segments[idx];
        let mut items: Vec<Candidate> = Vec::new();
        if seg.kind == SegmentKind::Japanese {
            let from_backend =
                self.backend
                    .segment_candidates(&seg.reading, &left, CANDIDATE_LIMIT);
            let katakana = to_katakana(&seg.reading);
            let current_sources = from_backend
                .iter()
                .find(|c| c.surface == seg.surface)
                .map(|c| c.sources.clone())
                .unwrap_or_else(|| {
                    if seg.surface == seg.reading || seg.surface == katakana {
                        vec![CandidateSource::Kana]
                    } else {
                        vec![CandidateSource::SystemDict]
                    }
                });
            push_candidate(&mut items, &seg.surface, &current_sources);
            for c in &from_backend {
                push_candidate(&mut items, &c.surface, &c.sources);
            }
            push_candidate(&mut items, &seg.reading, &[CandidateSource::Kana]);
            push_candidate(&mut items, &katakana, &[CandidateSource::Kana]);
            // 曖昧入力の補正候補は通常の候補の後ろ。表記が既にある候補は足さない
            if idx == 0 && conv.base_reading.as_deref() == Some(seg.reading.as_str()) {
                for c in &conv.corrections {
                    if !items.iter().any(|i| i.surface == c.surface) {
                        items.push(Candidate {
                            surface: c.surface.clone(),
                            sources: vec![CandidateSource::Corrected],
                            reading: Some(c.reading.clone()),
                        });
                    }
                }
            }
        } else {
            push_candidate(&mut items, &seg.surface, &[]);
        }
        // 複数文節入力の文全体の補正候補は、どの文節の候補にも末尾に足す（選ぶと文全体が置き換わる）
        if let Some((_, corrections)) = &conv.sentence_corrections {
            for c in corrections {
                if !items.iter().any(|i| i.surface == c.surface) {
                    items.push(Candidate {
                        surface: c.surface.clone(),
                        sources: vec![CandidateSource::Corrected],
                        reading: Some(c.reading.clone()),
                    });
                }
            }
        }
        conv.segments[idx].candidates = Some(CandidateSet { items, selected: 0 });
    }

    /// 文節 `f` の候補 `index` を選択し、表記を反映する。曖昧入力の補正候補（`reading` を持つ候補）
    /// を含む 1 文節では、補正候補を選ぶとその文節の reading が補正後の読みに書き換わり、
    /// 通常の候補に戻すと元の reading（打たれたとおり）に戻る（確定・学習・取消は書き換え後の
    /// reading を使う。補正前への Undo は未対応）
    ///
    /// 複数文節入力の文全体の補正候補（`sentence_corrections`）を選ぶと、文全体が 1 文節の文全体候補へ
    /// 置き換わる。置き換えたら `true`（呼び出し側が `origins` を捨てる）
    fn set_selected(conv: &mut Conv, f: usize, index: usize) -> bool {
        let Some(set) = conv.segments[f].candidates.as_mut() else {
            return false;
        };
        let Some(cand) = set.items.get(index) else {
            return false;
        };
        let (surface, corrected) = (cand.surface.clone(), cand.reading.clone());
        if conv.sentence_corrections.is_some() && corrected.is_some() {
            return Self::restructure_to_correction(conv, &surface);
        }
        let ours = set.items.iter().any(|c| c.reading.is_some());
        set.selected = index;
        conv.segments[f].surface = surface;
        if f == 0
            && ours
            && let Some(base) = &conv.base_reading
        {
            conv.segments[0].reading = corrected.unwrap_or_else(|| base.clone());
        }
        false
    }

    /// 文全体の補正候補（表記 `surface`）の選択: 現在の文節列を第一仮説として保持したまま、文全体を
    /// 1 文節（候補は第一仮説と補正候補）に置き換え、補正候補を選択する。以後は文全体候補として
    /// 前後に移動でき、第一仮説に戻せる。置き換えできなければ何もせず `false`
    fn restructure_to_correction(conv: &mut Conv, surface: &str) -> bool {
        let Some((base, corrections)) = conv.sentence_corrections.as_ref() else {
            return false;
        };
        let joined: String = conv.segments.iter().map(|s| s.reading.as_str()).collect();
        if joined != *base {
            return false;
        }
        let base = base.clone();
        let first_segments = conv.segments.clone();
        let first_surface: String = first_segments.iter().map(|s| s.surface.as_str()).collect();
        let first_source = if first_surface == base {
            CandidateSource::Kana
        } else {
            CandidateSource::SystemDict
        };
        let mut items = vec![Candidate {
            surface: first_surface,
            sources: vec![first_source],
            reading: None,
        }];
        let mut alternatives = vec![first_segments];
        Self::push_corrections(&mut items, &mut alternatives, corrections);
        let Some(pos) = items
            .iter()
            .position(|c| c.reading.is_some() && c.surface == surface)
        else {
            return false;
        };
        conv.segments = vec![Segment {
            reading: base,
            surface: items[pos].surface.clone(),
            kind: SegmentKind::Japanese,
            candidates: Some(CandidateSet {
                items,
                selected: pos,
            }),
            chunk_id: 0,
        }];
        conv.focus = 0;
        conv.range_anchor = None;
        conv.sentence_alternatives = Some(alternatives);
        conv.sentence_corrections = None;
        true
    }

    fn step_candidate(&self, sess: &mut Session, delta: isize) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        let f = conv.focus;
        self.ensure_candidates(&sess.left_context, conv, f);
        conv.visible = true;
        if let Some(set) = conv.segments[f].candidates.as_ref() {
            let n = set.items.len() as isize;
            let next = ((set.selected as isize + delta).rem_euclid(n)) as usize;
            if Self::set_selected(conv, f, next) {
                sess.origins.clear();
            }
        }
    }

    fn move_focus(&self, sess: &mut Session, dir: Direction) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        conv.visible = false;
        conv.range_anchor = None;
        match dir {
            Direction::Left => conv.focus = conv.focus.saturating_sub(1),
            Direction::Right => conv.focus = (conv.focus + 1).min(conv.segments.len() - 1),
        }
    }

    /// `Action::SelectCand`（数字キー）: 候補ウィンドウ表示中のページ相対インデックス（1〜9）で選択する。
    /// `select_candidate`（public, RPC 用の絶対インデックス版）とは別物
    fn select_candidate_page_relative(&self, sess: &mut Session, n: usize, commit: &mut String) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        if !conv.visible {
            // 候補ウィンドウが出ていない間の数字キーは、確定して数字を入力する
            if let Some(c) = char::from_digit(n as u32, 10) {
                self.insert_char(sess, c, commit);
            }
            return;
        }
        let f = conv.focus;
        let page_size = self.config.page_size.max(1);
        if let Some(set) = conv.segments[f].candidates.as_ref() {
            let index = (set.selected / page_size) * page_size + n - 1;
            if index < set.items.len() {
                if Self::set_selected(conv, f, index) {
                    sess.origins.clear();
                }
                conv.visible = false;
            }
        }
    }

    fn set_focus_kana(&self, sess: &mut Session, katakana: bool) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        if let Some(range) = Self::selection_range(conv) {
            self.set_range_kana(conv, range, katakana);
            return;
        }
        let f = conv.focus;
        if conv.segments[f].kind != SegmentKind::Japanese {
            return;
        }
        self.ensure_candidates(&sess.left_context, conv, f);
        let reading = conv.segments[f].reading.clone();
        let target = if katakana {
            to_katakana(&reading)
        } else {
            reading
        };
        if let Some(set) = conv.segments[f].candidates.as_mut()
            && let Some(pos) = set.items.iter().position(|c| c.surface == target)
        {
            set.selected = pos;
        }
        conv.segments[f].surface = target;
        conv.visible = false;
    }

    /// 複数文節選択中の `ToKatakana`/`ToHiragana`: 選択範囲全体の reading をまとめてかな化し、
    /// 1つの文節に置き換える（選択は解除する。混在する非日本語文節があれば何もしない）
    fn set_range_kana(&self, conv: &mut Conv, range: Range<usize>, katakana: bool) {
        if conv.segments[range.clone()]
            .iter()
            .any(|s| s.kind != SegmentKind::Japanese)
        {
            return;
        }
        let merged_reading: String = conv.segments[range.clone()]
            .iter()
            .map(|s| s.reading.as_str())
            .collect();
        let surface = if katakana {
            to_katakana(&merged_reading)
        } else {
            merged_reading.clone()
        };
        let new_focus = range.start;
        let new_segment = Segment {
            reading: merged_reading,
            surface,
            kind: SegmentKind::Japanese,
            candidates: None,
            chunk_id: range.start as u32,
        };
        conv.segments.splice(range, [new_segment]);
        for (i, s) in conv.segments.iter_mut().enumerate() {
            s.chunk_id = i as u32;
        }
        conv.focus = new_focus;
        conv.range_anchor = None;
        conv.visible = false;
    }

    /// `ToHankaku`: 注目文節（複数文節選択中はその範囲）の表記を半角化する
    /// （カタカナ→半角カナ、または半角化可能な英数記号→半角。`nagi_rewriter` 経由）。
    /// どちらの変換も適用できなければ何もしない
    fn set_focus_hankaku(&self, sess: &mut Session) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        let range = Self::selection_range(conv).unwrap_or(conv.focus..conv.focus + 1);
        let merged_surface: String = conv.segments[range.clone()]
            .iter()
            .map(|s| s.surface.as_str())
            .collect();
        let merged_reading: String = conv.segments[range.clone()]
            .iter()
            .map(|s| s.reading.as_str())
            .collect();
        let hankaku = nagi_rewriter::HankakuKatakana
            .rewrite(&merged_surface)
            .into_iter()
            .next()
            .or_else(|| {
                nagi_rewriter::HankakuAlnum
                    .rewrite(&merged_surface)
                    .into_iter()
                    .next()
            });
        let Some(surface) = hankaku else {
            return;
        };
        let new_focus = range.start;
        let kind = conv.segments[range.start].kind;
        let new_segment = Segment {
            reading: merged_reading,
            surface,
            kind,
            candidates: None,
            chunk_id: range.start as u32,
        };
        conv.segments.splice(range, [new_segment]);
        for (i, s) in conv.segments.iter_mut().enumerate() {
            s.chunk_id = i as u32;
        }
        conv.focus = new_focus;
        conv.range_anchor = None;
        conv.visible = false;
    }

    /// 注目文節の区切りを reading 1 文字分だけ伸縮し、その文節と後続を再変換する
    fn resize_segment(&self, sess: &mut Session, dir: Direction) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        let f = conv.focus;
        if conv.segments[f].kind != SegmentKind::Japanese {
            return;
        }
        // focus を含む、Japanese セグメントの連続範囲 a..=b
        let mut a = f;
        while a > 0 && conv.segments[a - 1].kind == SegmentKind::Japanese {
            a -= 1;
        }
        let mut b = f;
        while b + 1 < conv.segments.len() && conv.segments[b + 1].kind == SegmentKind::Japanese {
            b += 1;
        }
        let run: Vec<char> = conv.segments[a..=b]
            .iter()
            .flat_map(|s| s.reading.chars())
            .collect();
        let head_len: usize = conv.segments[a..f]
            .iter()
            .map(|s| s.reading.chars().count())
            .sum();
        let cur_len = conv.segments[f].reading.chars().count();
        let new_len = match dir {
            Direction::Right => cur_len + 1,
            Direction::Left if cur_len > 1 => cur_len - 1,
            Direction::Left => return,
        };
        if head_len + new_len > run.len() {
            return;
        }
        let focus_reading: String = run[head_len..head_len + new_len].iter().collect();
        let tail: String = run[head_len + new_len..].iter().collect();

        let left = Self::left_context_for(&sess.left_context, conv, f);
        let focus_surface = self
            .backend
            .segment_candidates(&focus_reading, &left, 1)
            .into_iter()
            .next()
            .map(|c| c.surface)
            .unwrap_or_else(|| focus_reading.clone());
        let mut replacement = vec![Segment {
            reading: focus_reading,
            surface: focus_surface.clone(),
            kind: SegmentKind::Japanese,
            candidates: None,
            chunk_id: f as u32,
        }];
        if !tail.is_empty() {
            let tail_left = tail_chars(&format!("{left}{focus_surface}"), LEFT_CONTEXT_CHARS);
            let converted = self.backend.convert_sentence(&tail, &tail_left);
            let valid = !converted.is_empty()
                && converted
                    .iter()
                    .map(|s| s.reading.as_str())
                    .collect::<String>()
                    == tail;
            if valid {
                for bs in converted {
                    replacement.push(Segment {
                        reading: bs.reading,
                        surface: bs.surface,
                        kind: SegmentKind::Japanese,
                        candidates: None,
                        chunk_id: 0,
                    });
                }
            } else {
                replacement.push(Segment::plain(&tail, SegmentKind::Japanese, 0));
            }
        }
        conv.segments.splice(f..=b, replacement);
        for (i, s) in conv.segments.iter_mut().enumerate() {
            s.chunk_id = i as u32;
        }
        conv.visible = false;
    }

    // ---- 表示の組み立て ----

    fn build_preedit(&self, sess: &Session) -> Preedit {
        match &sess.state {
            State::Direct => Preedit::default(),
            State::Composing => {
                let pieces = sess.composer.pieces();
                let live = Self::live_view(sess, &pieces);
                let segments: Vec<Segment> = match live {
                    Some(mut v) => {
                        v.fixed.extend(v.tail);
                        v.fixed
                    }
                    None => pieces
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let kind = match p.kind {
                                PieceKind::Japanese => SegmentKind::Japanese,
                                PieceKind::Literal => SegmentKind::Literal,
                            };
                            Segment::plain(&p.text, kind, i as u32)
                        })
                        .collect(),
                };
                let mut remaining = sess.composer.cursor();
                let mut cursor = CursorPos::default();
                for (i, s) in segments.iter().enumerate() {
                    let len = s.reading.chars().count();
                    if remaining <= len {
                        cursor = CursorPos {
                            segment: i,
                            offset: remaining,
                        };
                        break;
                    }
                    remaining -= len;
                }
                Preedit {
                    segments,
                    cursor,
                    pending: sess.composer.pending().to_string(),
                    revision: sess.revision,
                    ..Default::default()
                }
            }
            State::Converting(conv) => Preedit {
                segments: conv.segments.clone(),
                focus: Some(conv.focus),
                selection: Self::selection_range(conv),
                revision: sess.revision,
                ..Default::default()
            },
        }
    }

    fn build_candidates(&self, sess: &Session) -> CandidatesView {
        let page_size = self.config.page_size as u32;
        if let State::Converting(conv) = &sess.state
            && conv.visible
            && let Some(set) = &conv.segments[conv.focus].candidates
        {
            return CandidatesView {
                items: set
                    .items
                    .iter()
                    .map(|c| CandidateView {
                        surface: c.surface.clone(),
                        sources: c.sources.clone(),
                        note: c.reading.clone(),
                    })
                    .collect(),
                selected: set.selected as u32,
                page_size,
                visible: true,
            };
        }
        CandidatesView {
            page_size,
            ..Default::default()
        }
    }

    /// 前回返した状態との差分を `ProcessKeyResult` にまとめる
    fn finish(&self, sess: &mut Session, consumed: bool, commit: String) -> ProcessKeyResult {
        let preedit = self.build_preedit(sess).to_view();
        let candidates = self.build_candidates(sess);
        let mut result = ProcessKeyResult {
            consumed,
            commit: (!commit.is_empty()).then_some(commit),
            delete_before: sess.pending_delete_before.take(),
            ..Default::default()
        };
        let mut changed = result.commit.is_some() || result.delete_before.is_some();
        if preedit != sess.sent.preedit {
            sess.sent.preedit = preedit.clone();
            result.preedit = Some(preedit);
            changed = true;
        }
        if candidates != sess.sent.candidates {
            sess.sent.candidates = candidates.clone();
            result.candidates = Some(candidates);
            changed = true;
        }
        if sess.mode != sess.sent.mode {
            sess.sent.mode = sess.mode;
            result.mode = Some(sess.mode);
            changed = true;
        }
        if changed {
            sess.revision += 1;
        }
        result.revision = sess.revision as u32;
        result
    }
}

/// 未知の Action 名（`perform_action`）
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct UnknownAction(pub String);

/// 検証済みの代替を `(文字, 距離)` に直す（`Composer` の打鍵ログの形）
fn alternative_pairs(alternatives: &[InputAlternative]) -> Vec<(char, f32)> {
    sanitize_alternatives(alternatives)
        .iter()
        .filter_map(|a| a.text.chars().next().map(|c| (c, a.distance)))
        .collect()
}

/// 曖昧入力の代替を検証する。1 文字でない、または `distance` が
/// 有限でない・負のものは捨てる
pub fn sanitize_alternatives(alternatives: &[InputAlternative]) -> Vec<InputAlternative> {
    alternatives
        .iter()
        .filter(|a| a.text.chars().count() == 1 && a.distance.is_finite() && a.distance >= 0.0)
        .cloned()
        .collect()
}
