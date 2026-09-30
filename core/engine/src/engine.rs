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
use crate::kana::to_katakana;
use crate::keymap::{KeyContext, KeyMap};
use crate::observer::{CommitContext, CommitObserver, CommittedSegment};
use nagi_protocol::key::{KeyEvent, KeyKind};
use nagi_protocol::methods::{ProcessKeyResult, RestoreSessionParams, SetContextParams};
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
    /// 一致する間だけ表示に使う。一致しなくなったら（次の入力があった等）自然に無視される
    live_conversion: Option<LiveConversion>,
    /// 直前の確定の記憶（`Action::UndoCommit` 用）
    last_commit: Option<LastCommit>,
    /// このキー処理で確定取消が成功した場合の `delete_before`（`finish` が読んで消費する）
    pending_delete_before: Option<u32>,
}

/// ライブ変換のキャッシュ
#[derive(Clone)]
struct LiveConversion {
    /// 計算時点の入力全体（Composing の全 piece を連結した）reading
    source_reading: String,
    /// 変換結果のセグメント列（reading を連結すると `source_reading` に一致する）
    segments: Vec<Segment>,
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
        Session {
            id: String::new(),
            mode: self.config.initial_mode,
            state: State::Direct,
            composer: Composer::new(self.table.clone(), self.config.shift_letter),
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
            last_commit: None,
            pending_delete_before: None,
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
        let mut sess = self.take_session(session_id);
        sess.origins.clear();
        let mut commit = String::new();
        let consumed = self.handle_key(&mut sess, key, &mut commit);
        let result = self.finish(&mut sess, consumed, commit);
        self.sessions.insert(session_id.to_string(), sess);
        result
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
            if let Some(set) = conv.segments[f].candidates.as_mut()
                && index < set.items.len()
            {
                set.selected = index;
                let surface = set.items[index].surface.clone();
                conv.segments[f].surface = surface;
            }
            conv.visible = false;
        }
        let result = self.finish(&mut sess, true, String::new());
        self.sessions.insert(session_id.to_string(), sess);
        result
    }

    /// 現在 Composing 状態なら、入力中の全体の reading（ローマ字変換済みのひらがな連結）を返す。
    /// Composing でない・未入力なら `None`（サーバーがライブ変換のデバウンス判定に使う）
    pub fn composing_reading(&self, session_id: &str) -> Option<String> {
        let sess = self.sessions.get(session_id)?;
        let State::Composing = sess.state else {
            return None;
        };
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
        let pieces = sess.composer.pieces();
        if pieces.is_empty() {
            return false;
        }
        let full_reading: String = pieces.iter().map(|p| p.text.as_str()).collect();
        if full_reading != expected_reading {
            return false;
        }
        let mut segments: Vec<Segment> = Vec::new();
        let mut context = sess.left_context.clone();
        for piece in &pieces {
            match piece.kind {
                PieceKind::Japanese => {
                    self.convert_japanese_chunked(&piece.text, &mut context, &mut segments);
                }
                PieceKind::Literal => {
                    context.push_str(&piece.text);
                    let id = segments.len() as u32;
                    segments.push(Segment::plain(&piece.text, SegmentKind::Literal, id));
                }
            }
        }
        sess.live_conversion = Some(LiveConversion {
            source_reading: full_reading,
            segments,
        });
        true
    }

    /// 日本語の入力（ひらがなのみ）を `composing_chunk_len` 文字ずつのチャンクに分けて変換し、
    /// `segments` に追記する。`context` はチャンクをまたいで更新される左文脈（前のチャンクの
    /// 変換結果を含む）。1文字数が `composing_chunk_len` 以下なら1回のループで既存と同じ動作になる
    fn convert_japanese_chunked(
        &self,
        text: &str,
        context: &mut String,
        segments: &mut Vec<Segment>,
    ) {
        let chunk_len = self.config.composing_chunk_len.max(1);
        let chars: Vec<char> = text.chars().collect();
        for start in (0..chars.len()).step_by(chunk_len) {
            let end = (start + chunk_len).min(chars.len());
            let chunk: String = chars[start..end].iter().collect();
            let converted = self
                .backend
                .convert_sentence(&chunk, &tail_chars(context, LEFT_CONTEXT_CHARS));
            let valid = !converted.is_empty()
                && converted
                    .iter()
                    .map(|s| s.reading.as_str())
                    .collect::<String>()
                    == chunk;
            if valid {
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
            .filter(|(_, s)| !s.origins.is_empty() && matches!(s.state, State::Converting(_)))
            .map(|(id, _)| id.clone())
            .collect();
        let mut out = Vec::new();
        for id in ids {
            let mut sess = self.take_session(&id);
            if self.replace_origins(&mut sess, done) {
                let result = self.finish(&mut sess, false, String::new());
                if result.preedit.is_some() || result.candidates.is_some() {
                    out.push((id.clone(), result));
                }
            }
            self.sessions.insert(id, sess);
        }
        out
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
            Some(action) => self.apply(sess, action, commit),
            // 割当なし: 未入力なら素通し、入力中はキーを消費して何もしない
            None => !matches!(sess.state, State::Direct),
        }
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
        if matches!(sess.state, State::Converting(_)) {
            // 変換結果を確定して新しい入力を始める
            self.commit_all(sess, commit);
        }
        sess.composer.insert_char(c);
        sess.state = State::Composing;
    }

    fn apply_composing(&self, sess: &mut Session, action: Action, commit: &mut String) {
        match action {
            Action::Backspace => {
                sess.composer.backspace();
                self.leave_if_empty(sess);
            }
            Action::DeleteForward => {
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
                sess.composer.clear();
                sess.state = State::Direct;
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
                let displayed_reading: String = sess
                    .composer
                    .pieces()
                    .iter()
                    .map(|p| p.text.as_str())
                    .collect();
                let live = sess
                    .live_conversion
                    .take()
                    .filter(|lc| lc.source_reading == displayed_reading);
                sess.composer.flush_pending();
                let pieces = sess.composer.pieces();
                let flushed_text = sess.composer.text();
                match live {
                    // 保留（単独の n 等）の flush が末尾にだけ足された場合は、その分を後ろに付ける。
                    // カーソルが途中で flush 分が途中に入った場合は対応を取れないので読みで確定する
                    Some(lc) if flushed_text.starts_with(&displayed_reading) => {
                        self.notify_commit(sess, &lc.segments);
                        for s in &lc.segments {
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
        if how == Start::Convert
            && self.config.initial_view == InitialView::Sentence
            && pieces.len() == 1
            && pieces[0].kind == PieceKind::Japanese
            && let Some(conv) = self.start_converting_sentence_n_best(sess, &pieces[0].text)
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
        sess.state = State::Converting(Conv {
            segments,
            focus: 0,
            visible: false,
            range_anchor: None,
            sentence_alternatives: None,
        });
    }

    /// `initial_view = sentence` かつ入力全体が1つの日本語 Piece のときに、文全体のN-bestを
    /// 候補ウィンドウに載せた単一の「メガ文節」を作る。候補が2件未満（多様性が無い）なら
    /// `None` を返し、呼び出し側は通常の（1-best・文節ごとの）変換にフォールバックする
    fn start_converting_sentence_n_best(&self, sess: &Session, reading: &str) -> Option<Conv> {
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
            items.push(Candidate { surface, sources });
            alternatives.push(segments);
        }
        if alternatives.len() < 2 {
            return None;
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
        } else {
            push_candidate(&mut items, &seg.surface, &[]);
        }
        conv.segments[idx].candidates = Some(CandidateSet { items, selected: 0 });
    }

    fn step_candidate(&self, sess: &mut Session, delta: isize) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        let f = conv.focus;
        self.ensure_candidates(&sess.left_context, conv, f);
        conv.visible = true;
        if let Some(set) = conv.segments[f].candidates.as_mut() {
            let n = set.items.len() as isize;
            set.selected = ((set.selected as isize + delta).rem_euclid(n)) as usize;
            let surface = set.items[set.selected].surface.clone();
            conv.segments[f].surface = surface;
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
        if let Some(set) = conv.segments[f].candidates.as_mut() {
            let index = (set.selected / page_size) * page_size + n - 1;
            if index < set.items.len() {
                set.selected = index;
                let surface = set.items[index].surface.clone();
                conv.segments[f].surface = surface;
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
                let full_reading: String = pieces.iter().map(|p| p.text.as_str()).collect();
                let live = sess
                    .live_conversion
                    .as_ref()
                    .filter(|lc| lc.source_reading == full_reading);
                let segments: Vec<Segment> = match live {
                    Some(lc) => lc.segments.clone(),
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
