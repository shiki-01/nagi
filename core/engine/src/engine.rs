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
use nagi_romaji::{Composer, Piece, PieceKind, PunctuationStyle, RomajiTable, ShiftLetterMode};
use std::collections::HashMap;
use std::sync::Arc;

/// 推論・変換に渡す左文脈の最大文字数（Unicode スカラー）
const LEFT_CONTEXT_CHARS: usize = 40;
/// 文節 1 つ分に集める候補数の上限
const CANDIDATE_LIMIT: usize = 50;
/// 文全体の候補（sentence モード）の上限
const SENTENCE_CANDIDATE_LIMIT: usize = 8;

/// 候補ウィンドウの初期表示形式（`conversion.initial_view`。設計書 §4.4）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialView {
    /// 文全体の N-best を表示する（既定）
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
    /// 候補ウィンドウの初期表示形式（`conversion.initial_view`）
    pub initial_view: InitialView,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            shift_letter: ShiftLetterMode::LiteralContinue,
            punctuation: PunctuationStyle::Japanese,
            page_size: 9,
            initial_mode: InputMode::Hiragana,
            initial_view: InitialView::Sentence,
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
    /// preedit 全体がちょうど 1 回の `convert_sentence` 呼び出し（`start_converting` 時点）から
    /// 成るときだけ Some（(reading, left_context)）。文全体モードの候補取得に使う。
    /// Literal との混在などで複数ピースにまたがる入力では None（sentence モードは提供しない）
    whole: Option<(String, String)>,
    /// 文全体の候補（sentence モード）。遅延生成。`←/→` 等で操作したら None に戻す
    sentence: Option<SentenceCandidates>,
}

/// 文全体の N-best（候補ウィンドウの sentence モード）。1 候補 = 文全体の 1 つの切り方
#[derive(Clone)]
struct SentenceCandidates {
    /// 各候補の文節列。採用すると `Conv.segments` を丸ごと差し替える
    options: Vec<Vec<Segment>>,
    selected: usize,
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
        Engine {
            backend,
            config,
            keymap: KeyMap::default_preset(),
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
                    whole: None,
                    sentence: None,
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
        match self.keymap.resolve(ctx, sess.mode, key) {
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

    fn apply_converting(&self, sess: &mut Session, action: Action, commit: &mut String) {
        match action {
            Action::NextCand | Action::StartConvert => self.step_candidate(sess, 1),
            Action::PrevCand => self.step_candidate(sess, -1),
            Action::MoveFocus(dir) => self.move_focus(sess, dir),
            Action::ResizeSegment(dir) => self.resize_segment(sess, dir),
            Action::Commit => self.commit_all(sess, commit),
            Action::Cancel => self.back_to_composing(sess),
            Action::ToKatakana => self.set_focus_kana(sess, true),
            Action::ToHiragana => self.set_focus_kana(sess, false),
            Action::SelectCand(n) => self.select_candidate(sess, n, commit),
            Action::DeleteLearning => self.delete_learning(sess),
            // ExtendSelection（範囲選択）は Phase 3
            _ => {}
        }
    }

    // ---- 確定・遷移 ----

    /// 表示中の内容をすべて確定文字列へ移し、Direct に戻す
    fn commit_all(&self, sess: &mut Session, commit: &mut String) {
        match std::mem::replace(&mut sess.state, State::Direct) {
            State::Direct => {}
            State::Composing => {
                sess.composer.flush_pending();
                commit.push_str(&sess.composer.text());
            }
            State::Converting(conv) => {
                self.notify_commit(sess, &conv);
                for s in &conv.segments {
                    commit.push_str(&s.surface);
                }
            }
        }
        sess.composer.clear();
    }

    /// 変換を経て確定した文節を observer（学習など）へ通知する
    fn notify_commit(&self, sess: &Session, conv: &Conv) {
        let Some(observer) = &self.observer else {
            return;
        };
        let mut left = sess.left_context.clone();
        let mut segments = Vec::with_capacity(conv.segments.len());
        for seg in &conv.segments {
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
        conv.whole = None;
        conv.sentence = None; // sentence モードを抜ける（以後この Conv では二度と入らない）
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
        // preedit 全体がちょうど 1 つの Origin（1 回の convert_sentence 呼び出し）から成るときだけ、
        // 文全体モード（sentence モード）の候補取得に使う (reading, left_context) を残す
        let whole = match sess.origins.as_slice() {
            [origin] if origin.start == 0 && origin.count == segments.len() => {
                Some((origin.reading.clone(), origin.left.clone()))
            }
            _ => None,
        };
        sess.state = State::Converting(Conv {
            segments,
            focus: 0,
            visible: false,
            whole,
            sentence: None,
        });
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

    /// `conv.sentence` がまだ無く条件を満たせば、文全体の候補を取得して sentence モードに入る。
    /// 既に候補ウィンドウを開いた後（`conv.visible`）や、`conv.whole` が無い（複数ピースに
    /// またがる入力など）場合、`conversion.initial_view = segment` の場合は何もしない。
    /// バックエンドが 1 件しか返さない（代替が無い）ときも sentence モードには入らない
    fn try_sentence_mode(&self, conv: &mut Conv) {
        if self.config.initial_view != InitialView::Sentence {
            return;
        }
        let Some((reading, left)) = conv.whole.clone() else {
            return;
        };
        let sentences = self
            .backend
            .sentence_candidates(&reading, &left, SENTENCE_CANDIDATE_LIMIT);
        if sentences.len() < 2 {
            return;
        }
        let mut options: Vec<Vec<Segment>> = sentences
            .into_iter()
            .map(|s| {
                s.segments
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
            })
            .collect();
        // 現在表示中の切り方（学習・ユーザー辞書の反映済み）が候補に無ければ先頭に加える
        let joined =
            |segs: &[Segment]| -> String { segs.iter().map(|s| s.surface.as_str()).collect() };
        let current_joined = joined(&conv.segments);
        let selected = options
            .iter()
            .position(|o| joined(o) == current_joined)
            .unwrap_or_else(|| {
                options.insert(0, conv.segments.clone());
                0
            });
        conv.sentence = Some(SentenceCandidates { options, selected });
    }

    fn step_candidate(&self, sess: &mut Session, delta: isize) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        if conv.sentence.is_none() && !conv.visible {
            self.try_sentence_mode(conv);
        }
        if let Some(sc) = conv.sentence.as_mut() {
            let n = sc.options.len() as isize;
            sc.selected = ((sc.selected as isize + delta).rem_euclid(n)) as usize;
            conv.segments = sc.options[sc.selected].clone();
            conv.focus = conv.focus.min(conv.segments.len().saturating_sub(1));
            conv.visible = true;
            return;
        }
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
        conv.whole = None;
        conv.sentence = None; // sentence モードを抜けて、通常の文節候補に切り替える（以後戻らない）
        conv.visible = false;
        match dir {
            Direction::Left => conv.focus = conv.focus.saturating_sub(1),
            Direction::Right => conv.focus = (conv.focus + 1).min(conv.segments.len() - 1),
        }
    }

    fn select_candidate(&self, sess: &mut Session, n: usize, commit: &mut String) {
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
        if let Some(sc) = conv.sentence.as_mut() {
            let page_size = self.config.page_size.max(1);
            let index = (sc.selected / page_size) * page_size + n - 1;
            if index < sc.options.len() {
                sc.selected = index;
                conv.segments = sc.options[index].clone();
                conv.focus = conv.focus.min(conv.segments.len().saturating_sub(1));
                conv.visible = false;
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
        conv.whole = None;
        conv.sentence = None; // sentence モードを抜ける（以後この Conv では二度と入らない）
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

    /// 注目文節の区切りを reading 1 文字分だけ伸縮し、その文節と後続を再変換する
    fn resize_segment(&self, sess: &mut Session, dir: Direction) {
        let State::Converting(conv) = &mut sess.state else {
            return;
        };
        conv.whole = None;
        conv.sentence = None; // sentence モードを抜ける（以後この Conv では二度と入らない）
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
                let segments: Vec<Segment> = sess
                    .composer
                    .pieces()
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let kind = match p.kind {
                            PieceKind::Japanese => SegmentKind::Japanese,
                            PieceKind::Literal => SegmentKind::Literal,
                        };
                        Segment::plain(&p.text, kind, i as u32)
                    })
                    .collect();
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
                revision: sess.revision,
                ..Default::default()
            },
        }
    }

    fn build_candidates(&self, sess: &Session) -> CandidatesView {
        let page_size = self.config.page_size as u32;
        if let State::Converting(conv) = &sess.state
            && conv.visible
        {
            if let Some(sc) = &conv.sentence {
                return CandidatesView {
                    items: sc
                        .options
                        .iter()
                        .map(|o| CandidateView {
                            surface: o.iter().map(|s| s.surface.as_str()).collect(),
                            // 文全体候補は現状すべて LLM（ビームサーチ）由来
                            sources: vec![CandidateSource::Llm],
                        })
                        .collect(),
                    selected: sc.selected as u32,
                    page_size,
                    visible: true,
                };
            }
            if let Some(set) = &conv.segments[conv.focus].candidates {
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
            ..Default::default()
        };
        let mut changed = result.commit.is_some();
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
