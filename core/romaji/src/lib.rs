//! Nagi ローマ字変換: Mozc 互換 TSV テーブルと入力バッファ（Composer）。

use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 句読点の表記スタイル設定
pub enum PunctuationStyle {
    /// 日本語スタイル: 、。
    Japanese,
    /// 西欧スタイル: ，．
    Western,
    /// 混合スタイル: ，。
    Mixed,
}

impl PunctuationStyle {
    /// 設定文字列からスタイルを解析する
    pub fn from_config_str(s: &str) -> Option<Self> {
        match s {
            "、。" => Some(PunctuationStyle::Japanese),
            "，．" => Some(PunctuationStyle::Western),
            "，。" => Some(PunctuationStyle::Mixed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Error)]
#[error("ローマ字テーブルパースエラー")]
/// ローマ字テーブルパースエラー
pub enum TableError {
    /// 列数が不足しています
    #[error("行{line}: 列数が不足しています（{got}列、最小2列必要）")]
    InsufficientColumns {
        /// エラーが発生した行番号
        line: usize,
        /// 実際の列数
        got: usize,
    },

    /// 入力キーが重複しています
    #[error("行{line}: 入力が重複しています（'{input}'）")]
    DuplicateInput {
        /// エラーが発生した行番号
        line: usize,
        /// 重複した入力キー
        input: String,
    },
}

#[derive(Debug, Clone)]
/// ローマ字変換テーブル
pub struct RomajiTable {
    // 入力 -> (出力, 保留) のマッピング
    entries: HashMap<String, (String, String)>,
}

impl RomajiTable {
    /// 標準ローマ字テーブルを取得する（キャッシュ済み）
    pub fn standard() -> Arc<Self> {
        // lazy_static や once_cell のような仕組みがなければ毎回パースするが、
        // 本来なら Arc<OnceLock> などで初回のみパースする想定
        let tsv = include_str!("../../../config/romaji/standard.tsv");
        Self::from_tsv(tsv)
            .expect("標準ローマ字テーブルのパースに失敗しました")
            .into()
    }

    /// TSV 形式のテーブルをパースする
    pub fn from_tsv(src: &str) -> Result<Self, TableError> {
        let mut entries = HashMap::new();

        for (line_num, line) in src.lines().enumerate() {
            let line_idx = line_num + 1;

            // コメント行と空行をスキップ
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // タブで分割
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                return Err(TableError::InsufficientColumns {
                    line: line_idx,
                    got: parts.len(),
                });
            }

            let input = parts[0].to_string();
            let output = parts[1].to_string();
            let pending = parts.get(2).map(|s| s.to_string()).unwrap_or_default();

            // 入力の重複チェック
            if entries.contains_key(&input) {
                return Err(TableError::DuplicateInput {
                    line: line_idx,
                    input,
                });
            }

            entries.insert(input, (output, pending));
        }

        Ok(RomajiTable { entries })
    }

    /// 句読点スタイルを適用した新しいテーブルを生成する
    pub fn with_punctuation(&self, style: PunctuationStyle) -> Self {
        let mut entries = self.entries.clone();

        match style {
            PunctuationStyle::Japanese => {
                // デフォルト値、特に変更する必要なし（標準テーブルは日本語スタイル）
                entries.insert(",".to_string(), ("、".to_string(), "".to_string()));
                entries.insert(".".to_string(), ("。".to_string(), "".to_string()));
            }
            PunctuationStyle::Western => {
                entries.insert(",".to_string(), ("，".to_string(), "".to_string()));
                entries.insert(".".to_string(), ("．".to_string(), "".to_string()));
            }
            PunctuationStyle::Mixed => {
                entries.insert(",".to_string(), ("，".to_string(), "".to_string()));
                entries.insert(".".to_string(), ("。".to_string(), "".to_string()));
            }
        }

        RomajiTable { entries }
    }

    /// テーブルの全エントリ `(入力, 出力, 保留)` を返す（順序は不定）。
    /// 逆引き（かな → ローマ字）など、評価・診断ツール向け
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.entries
            .iter()
            .map(|(k, (out, pending))| (k.as_str(), out.as_str(), pending.as_str()))
    }

    /// テーブルから入力に対応する出力と保留を取得する
    fn get(&self, input: &str) -> Option<(String, String)> {
        self.entries.get(input).cloned()
    }

    /// 入力がテーブルのいずれかのエントリの接頭辞であり、かつそれより長いエントリが存在するかを検査
    fn has_longer_prefix(&self, input: &str) -> bool {
        self.entries
            .keys()
            .any(|k| k.starts_with(input) && k.len() > input.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ローマ字変換器の出力
pub enum Emit {
    /// テーブルで変換された仮名等
    Converted(String),
    /// テーブルに無く素通しした文字
    Raw(char),
}

#[derive(Debug)]
/// ローマ字→ひらがな変換器
pub struct RomajiConverter {
    table: Arc<RomajiTable>,
    buffer: String,
    /// 直前の `feed` の巻き戻し情報（G-49）。巻き戻せる `feed` の直後だけ Some
    last: Option<Rollback>,
}

#[derive(Debug, Clone)]
/// 直前の `feed` を取り消すための情報
struct Rollback {
    /// `feed` する前の保留
    prev_buffer: String,
    /// `feed` が出力した文字数（Unicode スカラー）
    emitted_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// `RomajiConverter::backspace` の結果
pub enum Backspace {
    /// 保留が無く、直前の `feed` の巻き戻しもできない（呼び出し側が確定済みの文字を消す）
    Nothing,
    /// 保留の末尾 1 文字を削除した
    Pending,
    /// 直前の `feed` を取り消した。保留は `feed` 前に戻ったので、
    /// 呼び出し側は直前の `feed` が出力した末尾の `retract` 文字を消すこと
    Rollback {
        /// 消すべき出力の文字数
        retract: usize,
    },
}

impl RomajiConverter {
    /// 新しい変換器を生成する
    pub fn new(table: Arc<RomajiTable>) -> Self {
        RomajiConverter {
            table,
            buffer: String::new(),
            last: None,
        }
    }

    /// 1 文字入力し、変換結果を返す（出力が無ければ空 Vec）
    ///
    /// 不一致で確定（Raw の素通し）が起きた場合と、確定の後に保留が残る場合
    /// （促音 `kk`→っ+k、`nk`→ん+k など）は、直後の `backspace` で `feed` 前の状態に戻せる。
    pub fn feed(&mut self, ch: char) -> Vec<Emit> {
        let prev_buffer = self.buffer.clone();
        self.last = None;
        self.buffer.push(ch);

        let mut result = Vec::new();

        // 最長一致アルゴリズム
        loop {
            // バッファと完全一致するエントリがあるか
            let exact_match = self.table.get(&self.buffer);
            // バッファを接頭辞とするより長いエントリがあるか
            let has_longer = self.table.has_longer_prefix(&self.buffer);

            if let Some((output, pending)) = exact_match.as_ref().filter(|_| !has_longer) {
                // 完全一致があり、より長い候補がない → emit
                result.push(Emit::Converted(output.clone()));
                self.buffer = pending.clone();
                break;
            }

            // 長い候補がまだあるなら待つ
            if has_longer {
                break;
            }

            // 完全一致がないし、接頭辞でもない → 先頭 1 文字を emit
            if self.buffer.is_empty() {
                break;
            }

            let first_char = self.buffer.chars().next().unwrap();
            result.push(Emit::Raw(first_char));
            self.buffer.remove(0);
        }

        let any_raw = result.iter().any(|e| matches!(e, Emit::Raw(_)));
        if any_raw || (!result.is_empty() && !self.buffer.is_empty()) {
            let emitted_chars = result
                .iter()
                .map(|e| match e {
                    Emit::Converted(t) => t.chars().count(),
                    Emit::Raw(_) => 1,
                })
                .sum();
            self.last = Some(Rollback {
                prev_buffer,
                emitted_chars,
            });
        }

        result
    }

    /// 表示用の保留ローマ字を取得
    pub fn pending(&self) -> &str {
        &self.buffer
    }

    /// 何かバッファにあるかを確認
    pub fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// 保留を確定させる（"n"→ん 等）
    pub fn flush(&mut self) -> Vec<Emit> {
        self.last = None;
        let mut result = Vec::new();

        if self.buffer == "n" {
            // 単独の n は ん に変換
            result.push(Emit::Converted("ん".to_string()));
            self.buffer.clear();
        } else if !self.buffer.is_empty() {
            // その他の保留は Raw で 1 文字ずつ emit
            for ch in self.buffer.chars() {
                result.push(Emit::Raw(ch));
            }
            self.buffer.clear();
        }

        result
    }

    /// Backspace: 直前のキー入力の取り消し（G-49）
    ///
    /// 直前の `feed` が巻き戻せる形（不一致の確定など）なら、保留を `feed` 前に戻す。
    /// それ以外は保留の末尾 1 文字を削除する。履歴は直前の 1 キー分のみで、使うと消える
    pub fn backspace(&mut self) -> Backspace {
        if let Some(last) = self.last.take() {
            self.buffer = last.prev_buffer;
            return Backspace::Rollback {
                retract: last.emitted_chars,
            };
        }
        if self.buffer.pop().is_some() {
            Backspace::Pending
        } else {
            Backspace::Nothing
        }
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.last = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Shift キー時の英字処理モード
pub enum ShiftLetterMode {
    /// Shift+英字以降の小文字も英字のまま（`Nagi`）
    LiteralContinue,
    /// Shift+英字のみ大文字、直後の小文字は日本語変換
    SingleUpper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// セグメントの種類
pub enum PieceKind {
    /// 日本語
    Japanese,
    /// 英数・記号・数字（変換しない）
    Literal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 入力バッファの単位
pub struct Piece {
    /// セグメントの種類
    pub kind: PieceKind,
    /// テキスト
    pub text: String,
}

/// 打鍵 1 回の記録（曖昧入力用。`Composer::key_log`）
///
/// 打たれた文字と、フロントが渡した代替（近傍のキー・フリックの方向ゾーン）を持つ。
/// 代替の距離はキー幅で正規化された値で、コストへの換算は使う側が行う
/// （この crate は曖昧入力の実装に依存しない）。
#[derive(Debug, Clone, PartialEq)]
pub struct KeyRecord {
    /// 打たれた文字（小文字 ASCII のローマ字キー、またはひらがな 1 文字）
    pub typed: char,
    /// 代替の `(文字, 距離)`（`typed` 自身は含まない）。距離の小さい順。代替なしなら空
    pub alts: Vec<(char, f32)>,
    /// フロントが代替に含めた、打たれたキー自身の距離（タッチ位置からそのキー中心まで）。
    /// 代替のコストは、これを基準にした相対値で換算する。送られなかったなら `None`
    pub typed_distance: Option<f32>,
}

/// 1 打鍵に保持する代替の最大数
const MAX_ALTS_PER_KEY: usize = 8;
/// `Composer::backspace` で、変換済みの文字 1 つを消したときに巻き戻す打鍵数の上限
/// （最長のローマ字綴り `xtsu` などを想定）
const MAX_KEYS_PER_CHAR: usize = 4;

/// 打鍵ログに載せるかな（ひらがな・長音）
fn is_loggable_kana(c: char) -> bool {
    ('ぁ'..='ゖ').contains(&c) || c == 'ー'
}

#[derive(Debug)]
/// ローマ字入力バッファ・カーソル管理
///
/// バッファは 1 文字ずつ種別（日本語 / Literal）を持つ平坦な列で、カーソルはその文字位置。
/// 保留中のローマ字はバッファに含めず、常にカーソル位置に付随する。
///
/// # 打鍵ログ（曖昧入力。既定は無効）
///
/// [`Composer::set_key_log_enabled`] で有効にすると、`chars` とは別に打鍵の列 [`KeyRecord`] を
/// 保持する（`chars` は変換後のかなで打鍵と 1 対 1 ではないため、並行する配列にはしない）。
/// ログは「この入力がすべて小文字 ASCII のローマ字キーと `insert_reading_char` のひらがなだけから成り、
/// 末尾への追記と Backspace だけで編集されている」間だけ有効で、次の操作で無効化（クリア）される:
/// 文中への挿入・`delete`・カーソル移動・大文字/数字/記号など Literal 化する入力・ひらがな以外の挿入・
/// `load_pieces`・保留の確定（`flush_pending`）の後に続けて打ったキー・Backspace 後に打鍵との対応が
/// 取れなくなったとき。無効化したログは入力が空になる（または `clear`）まで復活しない。
/// ログは `reading`・保留・Backspace の結果には一切影響しない。
pub struct Composer {
    converter: RomajiConverter,
    shift_letter: ShiftLetterMode,
    chars: Vec<(char, PieceKind)>,
    cursor: usize,
    literal_mode: bool, // Shift+英字で開始した literal モード
    key_log: Vec<KeyRecord>,
    key_log_enabled: bool,
    key_log_valid: bool,
    /// 保留を確定（`flush_pending`）してから、まだ打鍵のログに反映していない（ログの再生では再現できない）
    flushed_since_key: bool,
}

impl Composer {
    /// 新しい Composer を生成する
    pub fn new(table: Arc<RomajiTable>, shift_letter: ShiftLetterMode) -> Self {
        Composer {
            converter: RomajiConverter::new(table),
            shift_letter,
            chars: Vec::new(),
            cursor: 0,
            literal_mode: false,
            key_log: Vec::new(),
            key_log_enabled: false,
            key_log_valid: true,
            flushed_since_key: false,
        }
    }

    /// 打鍵ログの有効・無効を切り替える（既定は無効。無効の間は何も記録せず、コストもかからない）。
    ///
    /// すでに入力がある状態で有効にすると、それまでの打鍵は分からないので、
    /// 入力が空になるまでログは無効のままになる
    pub fn set_key_log_enabled(&mut self, enabled: bool) {
        if enabled == self.key_log_enabled {
            return;
        }
        self.key_log_enabled = enabled;
        self.key_log.clear();
        self.key_log_valid = !enabled || self.is_empty();
        self.flushed_since_key = false;
    }

    /// 有効な打鍵ログ。無効化されている・無効にしている・空なら `None`
    ///
    /// 戻り値の列を `RomajiConverter` に順に入力して最後に `flush` した結果が、
    /// この Composer の `text()` + 保留の確定と一致することが保証される
    /// （ひらがなの記録は、保留を確定してから挿入する）
    pub fn key_log(&self) -> Option<&[KeyRecord]> {
        (self.key_log_enabled && self.key_log_valid && !self.key_log.is_empty())
            .then_some(self.key_log.as_slice())
    }

    /// 設定の再読み込み用: Shift+英字の扱いと、ローマ字テーブルを差し替える。
    ///
    /// 保留中のローマ字は先に確定し、以降の入力から新しい設定を使う
    pub fn reconfigure(&mut self, table: Arc<RomajiTable>, shift_letter: ShiftLetterMode) {
        self.flush_pending();
        self.converter = RomajiConverter::new(table);
        self.shift_letter = shift_letter;
    }

    /// 1 文字を挿入する
    ///
    /// ASCII 大文字は Shift 押下で打たれたものとして Literal になる。
    pub fn insert_char(&mut self, ch: char) {
        self.insert_char_with_alts(ch, &[]);
    }

    /// [`Composer::insert_char`] に、打鍵の代替 `(文字, 距離)` を添える（曖昧入力用）。
    ///
    /// 代替は小文字 ASCII のローマ字キーのときだけ打鍵ログ（[`Composer::key_log`]）に記録する
    /// （小文字 ASCII 以外の代替は捨てる）。入力される文字・保留の扱いは `insert_char` と同じ
    pub fn insert_char_with_alts(&mut self, ch: char, alts: &[(char, f32)]) {
        if ch.is_control() {
            return;
        }
        if ch.is_ascii_uppercase() {
            self.invalidate_key_log();
            self.flush_pending();
            self.literal_mode = matches!(self.shift_letter, ShiftLetterMode::LiteralContinue);
            self.insert_at_cursor(ch, PieceKind::Literal);
        } else if ch.is_ascii_digit() || (self.literal_mode && ch.is_ascii_graphic()) {
            self.invalidate_key_log();
            self.flush_pending();
            self.insert_at_cursor(ch, PieceKind::Literal);
        } else if !ch.is_ascii() {
            self.invalidate_key_log();
            self.flush_pending();
            self.insert_at_cursor(ch, PieceKind::Japanese);
        } else {
            if ch.is_ascii_lowercase() {
                self.log_key(ch, alts, false);
            } else {
                // `-` `,` などの記号は小文字のローマ字キーではないので、補正の対象外にする
                self.invalidate_key_log();
            }
            let emits = self.converter.feed(ch);
            self.insert_emits(emits);
        }
    }

    /// ローマ字変換を経由せず、読みの 1 文字を種別つきでカーソル位置へ挿入する
    ///
    /// 保留中のローマ字は先に確定する（`n` → ん など）。タッチ UI の `insert_reading` 用。
    /// 制御文字は無視する
    pub fn insert_reading_char(&mut self, ch: char, kind: PieceKind) {
        self.insert_reading_char_with_alts(ch, kind, &[]);
    }

    /// [`Composer::insert_reading_char`] に、打鍵の代替 `(文字, 距離)` を添える（フリックの代替ひらがな。
    /// 曖昧入力用）。ひらがな（日本語）のときだけ打鍵ログに記録し、代替はひらがな・長音だけ残す
    pub fn insert_reading_char_with_alts(
        &mut self,
        ch: char,
        kind: PieceKind,
        alts: &[(char, f32)],
    ) {
        if ch.is_control() {
            return;
        }
        // ログの再生は「保留を確定してから挿入」なので、確定した結果は打鍵のログと一致する
        self.flush_pending();
        if kind == PieceKind::Japanese && is_loggable_kana(ch) {
            self.log_key(ch, alts, true);
        } else {
            self.invalidate_key_log();
        }
        self.insert_at_cursor(ch, kind);
    }

    /// 保留を確定させる（`n` → ん など）
    pub fn flush_pending(&mut self) {
        if self.converter.has_pending() {
            self.flushed_since_key = true;
        }
        let emits = self.converter.flush();
        self.insert_emits(emits);
    }

    /// literal_mode を解除する（変換・確定・ESC の時に呼ぶ）
    pub fn end_literal_run(&mut self) {
        self.literal_mode = false;
    }

    /// バックスペース（直前のキー入力の取り消し。G-49）
    ///
    /// 直前のキーで不一致の確定などが起きていれば、その出力を消して保留をキー入力前に戻す。
    /// そうでなければ保留の末尾 1 文字、保留も無ければカーソル直前の 1 文字を削除する
    pub fn backspace(&mut self) -> bool {
        let at_end = self.cursor == self.chars.len();
        let kind = self.converter.backspace();
        let changed = match kind {
            Backspace::Pending => true,
            Backspace::Rollback { retract } => {
                // 巻き戻せるのは直前の feed 直後だけ（カーソル移動等は履歴を破棄する）ので、
                // 出力はカーソル直前に並んでいる
                let n = retract.min(self.cursor);
                self.cursor -= n;
                self.chars.drain(self.cursor..self.cursor + n);
                true
            }
            Backspace::Nothing => {
                if self.cursor == 0 {
                    return false;
                }
                self.cursor -= 1;
                self.chars.remove(self.cursor);
                true
            }
        };
        if self.key_log_enabled && self.key_log_valid {
            self.rewind_key_log(kind, at_end);
        }
        changed
    }

    /// カーソル直後の 1 文字を削除する（保留は先に確定する）
    pub fn delete(&mut self) -> bool {
        self.invalidate_key_log();
        self.flush_pending();
        if self.cursor >= self.chars.len() {
            return false;
        }
        self.chars.remove(self.cursor);
        true
    }

    /// カーソルを左へ 1 文字移動する（保留は先に確定する）
    pub fn move_left(&mut self) -> bool {
        self.invalidate_key_log();
        self.flush_pending();
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    /// カーソルを右へ 1 文字移動する（保留は先に確定する）
    pub fn move_right(&mut self) -> bool {
        self.invalidate_key_log();
        self.flush_pending();
        if self.cursor >= self.chars.len() {
            return false;
        }
        self.cursor += 1;
        true
    }

    /// カーソルを先頭へ移動する
    pub fn move_home(&mut self) {
        self.invalidate_key_log();
        self.flush_pending();
        self.cursor = 0;
    }

    /// カーソルを末尾へ移動する
    pub fn move_end(&mut self) {
        self.invalidate_key_log();
        self.flush_pending();
        self.cursor = self.chars.len();
    }

    /// 保留を除いたバッファを、同種の連続ごとにまとめた Piece 列で返す
    pub fn pieces(&self) -> Vec<Piece> {
        let mut pieces: Vec<Piece> = Vec::new();
        for &(ch, kind) in &self.chars {
            match pieces.last_mut() {
                Some(last) if last.kind == kind => last.text.push(ch),
                _ => pieces.push(Piece {
                    kind,
                    text: ch.to_string(),
                }),
            }
        }
        pieces
    }

    /// バッファを Piece 列で置き換える（エンジン再起動後の復元・変換取消用）
    ///
    /// 保留と literal_mode はクリアされる。`cursor` は文字数単位で、範囲外は末尾に丸める。
    pub fn load_pieces(&mut self, pieces: &[Piece], cursor: usize) {
        // 復元した入力には打鍵の記録が無い
        self.invalidate_key_log();
        self.converter.clear();
        self.literal_mode = false;
        self.chars = pieces
            .iter()
            .flat_map(|p| p.text.chars().map(move |c| (c, p.kind)))
            .collect();
        self.cursor = cursor.min(self.chars.len());
    }

    /// 表示用の保留ローマ字
    pub fn pending(&self) -> &str {
        self.converter.pending()
    }

    /// 文字（Unicode スカラー）単位のカーソル位置（保留を除く）
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// バッファのテキスト（保留を除く）
    pub fn text(&self) -> String {
        self.chars.iter().map(|&(c, _)| c).collect()
    }

    /// バッファの文字数（保留を除く）
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// バッファも保留も空かどうか
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty() && !self.converter.has_pending()
    }

    /// 保留が残っているか
    pub fn has_pending(&self) -> bool {
        self.converter.has_pending()
    }

    /// literal_mode（Shift+英字で開始した英字継続）中か
    pub fn is_literal_mode(&self) -> bool {
        self.literal_mode
    }

    /// バッファをクリアする
    pub fn clear(&mut self) {
        self.chars.clear();
        self.converter.clear();
        self.cursor = 0;
        self.literal_mode = false;
        self.key_log.clear();
        self.key_log_valid = true;
        self.flushed_since_key = false;
    }

    /// 打鍵ログを無効化（クリア）する。入力が空になる（または `clear`）まで復活しない
    fn invalidate_key_log(&mut self) {
        self.key_log.clear();
        self.key_log_valid = false;
    }

    /// 打鍵を記録する（`typed` を入力する直前に呼ぶ）。記録できない状態なら無効化する
    fn log_key(&mut self, typed: char, alts: &[(char, f32)], is_kana: bool) {
        if !self.key_log_enabled {
            return;
        }
        // 入力が完全に空なら新しい入力の開始なので、無効化を解いてやり直す
        if self.chars.is_empty() && !self.converter.has_pending() {
            self.key_log.clear();
            self.key_log_valid = true;
            self.flushed_since_key = false;
        }
        if !self.key_log_valid {
            return;
        }
        // 文中への挿入、または保留の確定の後に続けたローマ字キーは、ログの再生と一致しない
        if self.cursor != self.chars.len() || (!is_kana && self.flushed_since_key) {
            self.invalidate_key_log();
            return;
        }
        // 打たれたキー自身の距離は代替と分けて持つ（代替の上限に数えない）
        let typed_distance = alts
            .iter()
            .filter(|&&(c, d)| c == typed && d.is_finite() && d >= 0.0)
            .map(|&(_, d)| d)
            .fold(None, |m: Option<f32>, d| Some(m.map_or(d, |m| m.min(d))));
        let mut alts: Vec<(char, f32)> = alts
            .iter()
            .copied()
            .filter(|&(c, d)| {
                c != typed
                    && d.is_finite()
                    && d >= 0.0
                    && if is_kana {
                        is_loggable_kana(c)
                    } else {
                        c.is_ascii_lowercase()
                    }
            })
            .collect();
        alts.sort_by(|a, b| a.1.total_cmp(&b.1));
        alts.truncate(MAX_ALTS_PER_KEY);
        self.key_log.push(KeyRecord {
            typed,
            alts,
            typed_distance,
        });
        self.flushed_since_key = false;
    }

    /// 打鍵ログを先頭 `n` 件だけ再生した結果（確定したテキスト, 保留）
    fn replay_key_log(&self, n: usize) -> (String, String) {
        let mut conv = RomajiConverter::new(self.converter.table.clone());
        let mut text = String::new();
        let mut push = |emits: Vec<Emit>| {
            for e in emits {
                match e {
                    Emit::Converted(t) => text.push_str(&t),
                    Emit::Raw(c) => text.push(c),
                }
            }
        };
        for rec in &self.key_log[..n] {
            if is_loggable_kana(rec.typed) {
                push(conv.flush());
                push(vec![Emit::Raw(rec.typed)]);
            } else {
                push(conv.feed(rec.typed));
            }
        }
        let pending = conv.pending().to_string();
        (text, pending)
    }

    /// 先頭 `n` 件のログが現在の入力（確定したテキストと保留）と一致するか
    fn key_log_matches(&self, n: usize) -> bool {
        let (text, pending) = self.replay_key_log(n);
        text == self.text() && pending == self.converter.pending()
    }

    /// Backspace の後に、打鍵ログを入力と対応させ直す。対応が取れなければ無効化する。
    ///
    /// 保留の 1 文字削除と `feed` の巻き戻しは直前の 1 打鍵に対応する。変換済みの文字を消した場合は、
    /// その文字を作った打鍵（`ka` なら 2 打鍵）を、再生結果が入力と一致するまで遡って捨てる
    fn rewind_key_log(&mut self, kind: Backspace, at_end: bool) {
        if !at_end {
            self.invalidate_key_log();
            return;
        }
        let len = self.key_log.len();
        let max = match kind {
            Backspace::Pending | Backspace::Rollback { .. } => 1,
            Backspace::Nothing => MAX_KEYS_PER_CHAR,
        };
        for k in 1..=max.min(len) {
            if self.key_log_matches(len - k) {
                self.key_log.truncate(len - k);
                return;
            }
        }
        self.invalidate_key_log();
    }

    fn insert_at_cursor(&mut self, ch: char, kind: PieceKind) {
        self.chars.insert(self.cursor, (ch, kind));
        self.cursor += 1;
    }

    fn insert_emits(&mut self, emits: Vec<Emit>) {
        for emit in emits {
            match emit {
                Emit::Converted(text) => {
                    for ch in text.chars() {
                        self.insert_at_cursor(ch, PieceKind::Japanese);
                    }
                }
                Emit::Raw(c) => {
                    let kind = if c.is_ascii() {
                        PieceKind::Literal
                    } else {
                        PieceKind::Japanese
                    };
                    self.insert_at_cursor(c, kind);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_composer() -> (Composer, Arc<RomajiTable>) {
        let table = RomajiTable::standard();
        let composer = Composer::new(table.clone(), ShiftLetterMode::LiteralContinue);
        (composer, table)
    }

    fn insert_string(composer: &mut Composer, s: &str) {
        for ch in s.chars() {
            composer.insert_char(ch);
        }
    }

    fn text_after_flush(composer: &mut Composer) -> String {
        composer.flush_pending();
        composer.text()
    }

    #[test]
    fn test_basic_hiragana() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "ka");
        assert_eq!(text_after_flush(&mut composer), "か");
    }

    #[test]
    fn test_kya() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kya");
        assert_eq!(text_after_flush(&mut composer), "きゃ");
    }

    #[test]
    fn test_double_consonant() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kka");
        assert_eq!(text_after_flush(&mut composer), "っか");
    }

    #[test]
    fn test_nn() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "nn");
        assert_eq!(text_after_flush(&mut composer), "ん");
    }

    #[test]
    fn test_n_vowel() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "na");
        assert_eq!(text_after_flush(&mut composer), "な");
    }

    #[test]
    fn test_nka() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "nka");
        assert_eq!(text_after_flush(&mut composer), "んか");
    }

    #[test]
    fn test_konnichiha() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "konnichiha");
        // Mozc 標準どおり nn で「ん」が確定するので「こんいちは」になる（G-48）
        assert_eq!(text_after_flush(&mut composer), "こんいちは");
    }

    #[test]
    fn test_shinnyuu() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "shinnyuu");
        // nn の後の y は保留のまま「ゆ」になる（G-48）。「しんにゅう」は shinnnyuu / shin'nyuu
        assert_eq!(text_after_flush(&mut composer), "しんゆう");
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "shinnnyuu");
        assert_eq!(text_after_flush(&mut composer), "しんにゅう");
    }

    #[test]
    fn test_kanna() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kanna");
        assert_eq!(text_after_flush(&mut composer), "かんあ");
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kannna");
        assert_eq!(text_after_flush(&mut composer), "かんな");
    }

    #[test]
    fn test_honn() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "honn");
        assert_eq!(text_after_flush(&mut composer), "ほん");
    }

    #[test]
    fn test_hon() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "hon");
        assert_eq!(text_after_flush(&mut composer), "ほん");
    }

    #[test]
    fn test_gakkou() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "gakkou");
        assert_eq!(text_after_flush(&mut composer), "がっこう");
    }

    #[test]
    fn test_nippon() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "nippon");
        assert_eq!(text_after_flush(&mut composer), "にっぽん");
    }

    #[test]
    fn test_sensei() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "sensei");
        assert_eq!(text_after_flush(&mut composer), "せんせい");
    }

    #[test]
    fn test_tenki() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "tenki");
        assert_eq!(text_after_flush(&mut composer), "てんき");
    }

    #[test]
    fn test_kyouha() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kyouha");
        assert_eq!(text_after_flush(&mut composer), "きょうは");
    }

    #[test]
    fn test_tocchi() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "tocchi");
        assert_eq!(text_after_flush(&mut composer), "とっち");
    }

    #[test]
    fn test_pending_display_k() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('k');
        assert_eq!(composer.pending(), "k");
    }

    #[test]
    fn test_pending_display_n() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('n');
        composer.insert_char('n');
        // nn は ん に変換され、保留は残らない（Mozc 原本どおり。G-48）
        assert_eq!(composer.pending(), "");
    }

    #[test]
    fn test_backspace_after_kya() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "ky");
        composer.backspace();
        assert_eq!(composer.pending(), "k");
    }

    #[test]
    fn test_backspace_after_ka() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "ka");
        composer.backspace();
        assert_eq!(text_after_flush(&mut composer), "");
    }

    #[test]
    fn test_unmatch_q() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('q');
        composer.flush_pending();
        let pieces = composer.pieces();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].kind, PieceKind::Literal);
        assert_eq!(pieces[0].text, "q");
    }

    #[test]
    fn test_unmatch_kq() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kq");
        composer.flush_pending();
        let pieces = composer.pieces();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].kind, PieceKind::Literal);
        assert_eq!(pieces[0].text, "kq");
    }

    #[test]
    fn test_shift_literal_continue() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('N');
        composer.insert_char('a');
        composer.insert_char('g');
        composer.insert_char('i');
        composer.flush_pending();
        let pieces = composer.pieces();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].kind, PieceKind::Literal);
        assert_eq!(pieces[0].text, "Nagi");
    }

    #[test]
    fn test_shift_single_upper() {
        let table = RomajiTable::standard();
        let mut composer = Composer::new(table, ShiftLetterMode::SingleUpper);
        composer.insert_char('N');
        composer.insert_char('a');
        composer.flush_pending();
        let pieces = composer.pieces();
        // N は Literal、a は Japanese "あ"（ a 単独では あ）
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].kind, PieceKind::Literal);
        assert_eq!(pieces[0].text, "N");
        assert_eq!(pieces[1].kind, PieceKind::Japanese);
        assert_eq!(pieces[1].text, "あ");
    }

    #[test]
    fn test_digit() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "3ji");
        assert_eq!(text_after_flush(&mut composer), "3じ");
    }

    #[test]
    fn test_cursor_move_left_right() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kaki");
        composer.flush_pending();
        // cursor is at end
        assert!(composer.move_left());
        assert_eq!(composer.cursor(), 1); // after "か"
        assert!(composer.move_left());
        assert_eq!(composer.cursor(), 0); // before "か"
        assert!(!composer.move_left()); // at start
        assert!(composer.move_right());
        assert_eq!(composer.cursor(), 1);
    }

    #[test]
    fn test_cursor_insert_middle() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kaki");
        composer.flush_pending();
        composer.move_left(); // cursor between "か" and "き"
        composer.insert_char('u');
        composer.flush_pending();
        assert_eq!(composer.text(), "かうき");
    }

    #[test]
    fn test_move_home_end() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kaki");
        composer.flush_pending();
        composer.move_left();
        composer.move_home();
        assert_eq!(composer.cursor(), 0);
        composer.move_end();
        assert_eq!(composer.cursor(), 2); // length of "かき"
    }

    #[test]
    fn test_delete_after_cursor() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kaki");
        composer.flush_pending();
        composer.move_left(); // cursor between "か" and "き"
        assert!(composer.delete());
        assert_eq!(composer.text(), "か"); // "き" is deleted
    }

    #[test]
    fn test_punctuation_japanese() {
        let table = RomajiTable::standard();
        let modified = table.with_punctuation(PunctuationStyle::Japanese);
        let mut composer = Composer::new(Arc::new(modified), ShiftLetterMode::LiteralContinue);
        composer.insert_char(',');
        composer.flush_pending();
        assert_eq!(composer.text(), "、");
    }

    #[test]
    fn test_punctuation_western() {
        let table = RomajiTable::standard();
        let modified = table.with_punctuation(PunctuationStyle::Western);
        let mut composer = Composer::new(Arc::new(modified), ShiftLetterMode::LiteralContinue);
        composer.insert_char(',');
        composer.flush_pending();
        assert_eq!(composer.text(), "，");
    }

    #[test]
    fn test_tsv_parse_error_insufficient_columns() {
        let tsv = "a\tb\na";
        let result = RomajiTable::from_tsv(tsv);
        assert!(matches!(
            result,
            Err(TableError::InsufficientColumns { .. })
        ));
    }

    #[test]
    fn test_tsv_parse_error_duplicate_input() {
        let tsv = "a\tb\na\tc";
        let result = RomajiTable::from_tsv(tsv);
        assert!(matches!(result, Err(TableError::DuplicateInput { .. })));
    }

    #[test]
    fn test_tsv_comment_and_empty_lines() {
        let tsv = "# comment\n\na\tb\n\nc\td";
        let result = RomajiTable::from_tsv(tsv);
        assert!(result.is_ok());
        let table = result.unwrap();
        assert!(table.get("a").is_some());
        assert!(table.get("c").is_some());
    }

    #[test]
    fn test_standard_table_parse() {
        let table = RomajiTable::standard();
        assert!(table.get("ka").is_some());
        assert!(table.get("kya").is_some());
        assert!(table.get("nn").is_some());
    }

    #[test]
    fn test_standard_table_no_duplicates() {
        let tsv = include_str!("../../../config/romaji/standard.tsv");
        let result = RomajiTable::from_tsv(tsv);
        assert!(result.is_ok(), "standard.tsv has parse errors");
    }

    #[test]
    fn test_empty_composer() {
        let (composer, _) = setup_composer();
        assert!(composer.is_empty());
        assert_eq!(composer.len(), 0);
    }

    #[test]
    fn test_has_pending() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('k');
        assert!(composer.has_pending());
        composer.flush_pending();
        assert!(!composer.has_pending());
    }

    #[test]
    fn test_clear() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kaki");
        composer.clear();
        assert_eq!(composer.text(), "");
        assert!(!composer.has_pending());
    }

    #[test]
    fn test_literal_mode_end() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('A');
        assert!(composer.is_literal_mode());
        composer.end_literal_run();
        assert!(!composer.is_literal_mode());
        composer.insert_char('a');
        composer.flush_pending();
        assert_eq!(composer.text(), "Aあ");
    }

    #[test]
    fn test_shi_and_ti_variants() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "shi");
        assert_eq!(text_after_flush(&mut composer), "し");

        let (mut composer2, _) = setup_composer();
        insert_string(&mut composer2, "si");
        assert_eq!(text_after_flush(&mut composer2), "し");

        let (mut composer3, _) = setup_composer();
        insert_string(&mut composer3, "ti");
        assert_eq!(text_after_flush(&mut composer3), "ち");

        let (mut composer4, _) = setup_composer();
        insert_string(&mut composer4, "chi");
        assert_eq!(text_after_flush(&mut composer4), "ち");
    }

    #[test]
    fn test_tsu_variants() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "tsu");
        assert_eq!(text_after_flush(&mut composer), "つ");

        let (mut composer2, _) = setup_composer();
        insert_string(&mut composer2, "tu");
        assert_eq!(text_after_flush(&mut composer2), "つ");

        let (mut composer3, _) = setup_composer();
        insert_string(&mut composer3, "xtu");
        assert_eq!(text_after_flush(&mut composer3), "っ");

        let (mut composer4, _) = setup_composer();
        insert_string(&mut composer4, "ltu");
        assert_eq!(text_after_flush(&mut composer4), "っ");
    }

    #[test]
    fn test_foreign_sounds_fa_va() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "fa");
        assert_eq!(text_after_flush(&mut composer), "ふぁ");

        let (mut composer2, _) = setup_composer();
        insert_string(&mut composer2, "va");
        assert_eq!(text_after_flush(&mut composer2), "ゔぁ");
    }

    #[test]
    fn test_small_kana_xa_la() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "xa");
        assert_eq!(text_after_flush(&mut composer), "ぁ");

        let (mut composer2, _) = setup_composer();
        insert_string(&mut composer2, "la");
        assert_eq!(text_after_flush(&mut composer2), "ぁ");
    }

    #[test]
    fn test_wo_mark() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "wo");
        assert_eq!(text_after_flush(&mut composer), "を");
    }

    #[test]
    fn test_bracket_marks() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('[');
        assert_eq!(text_after_flush(&mut composer), "「");

        let (mut composer2, _) = setup_composer();
        composer2.insert_char(']');
        assert_eq!(text_after_flush(&mut composer2), "」");
    }

    #[test]
    fn test_dash_mark() {
        let (mut composer, _) = setup_composer();
        composer.insert_char('-');
        assert_eq!(text_after_flush(&mut composer), "ー");
    }

    #[test]
    fn test_complex_kyouha() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kyouha");
        assert_eq!(text_after_flush(&mut composer), "きょうは");
    }

    #[test]
    fn test_complex_shinnyuu() {
        let (mut composer, _) = setup_composer();
        // n を3つ打つか n' で区切る（nn で「ん」が確定するため。G-48）
        insert_string(&mut composer, "shinnnyuu");
        assert_eq!(text_after_flush(&mut composer), "しんにゅう");
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "shin'nyuu");
        assert_eq!(text_after_flush(&mut composer), "しんにゅう");
    }

    #[test]
    fn test_complex_ryokou() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "ryokou");
        assert_eq!(text_after_flush(&mut composer), "りょこう");
    }

    #[test]
    fn test_complex_gyuunyuu() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "gyuunyuu");
        assert_eq!(text_after_flush(&mut composer), "ぎゅうにゅう");
    }

    #[test]
    fn test_complex_shashin() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "shashin");
        assert_eq!(text_after_flush(&mut composer), "しゃしん");
    }

    #[test]
    fn test_complex_jishin() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "jishin");
        assert_eq!(text_after_flush(&mut composer), "じしん");
    }

    #[test]
    fn test_complex_chotto() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "chotto");
        assert_eq!(text_after_flush(&mut composer), "ちょっと");
    }

    #[test]
    fn test_complex_ocha() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "ocha");
        assert_eq!(text_after_flush(&mut composer), "おちゃ");
    }

    #[test]
    fn test_complex_benkyou() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "benkyou");
        assert_eq!(text_after_flush(&mut composer), "べんきょう");
    }

    #[test]
    fn test_complex_konnichiha() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "kon'nichiha");
        assert_eq!(text_after_flush(&mut composer), "こんにちは");
    }

    #[test]
    fn test_complex_hannou() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "hannou");
        // nn で「ん」が確定するので o は単独の「お」（G-48）
        assert_eq!(text_after_flush(&mut composer), "はんおう");
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "hannnou");
        assert_eq!(text_after_flush(&mut composer), "はんのう");
    }

    #[test]
    fn test_complex_onnna() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "onna");
        assert_eq!(text_after_flush(&mut composer), "おんあ");
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "onnna");
        assert_eq!(text_after_flush(&mut composer), "おんな");
    }

    #[test]
    fn test_sha_direct_form() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "sha");
        assert_eq!(text_after_flush(&mut composer), "しゃ");
    }

    #[test]
    fn test_ja_direct_form() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "ja");
        assert_eq!(text_after_flush(&mut composer), "じゃ");
    }

    #[test]
    fn test_cha_direct_form() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "cha");
        assert_eq!(text_after_flush(&mut composer), "ちゃ");
    }

    /// 入力列を流し込み、（確定前の表示テキスト, 表示用の保留）を返す
    fn display_after(input: &str) -> (String, String) {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, input);
        (composer.text(), composer.pending().to_string())
    }

    #[test]
    fn test_n_sequences_do_not_duplicate_nn() {
        // (入力, 確定前の表示テキスト, 表示用の保留, flush 後のテキスト)
        let cases = [
            ("n", "", "n", "ん"),
            ("nn", "ん", "", "ん"),
            ("na", "な", "", "な"),
            ("nk", "ん", "k", "んk"),
            ("nka", "んか", "", "んか"),
            ("nnk", "ん", "k", "んk"),
            ("nnka", "んか", "", "んか"),
            ("nna", "んあ", "", "んあ"),
            ("nni", "んい", "", "んい"),
            ("nnei", "んえい", "", "んえい"),
            ("nnya", "んや", "", "んや"),
            ("nnna", "んな", "", "んな"),
            ("nnnya", "んにゃ", "", "んにゃ"),
            ("n'", "ん", "", "ん"),
            ("xn", "ん", "", "ん"),
            ("kon", "こ", "n", "こん"),
            ("konn", "こん", "", "こん"),
            ("konni", "こんい", "", "こんい"),
            ("konnichi", "こんいち", "", "こんいち"),
            ("konnnichi", "こんにち", "", "こんにち"),
            ("kanji", "かんじ", "", "かんじ"),
            ("kanj", "かん", "j", "かんj"),
            ("kon'", "こん", "", "こん"),
            ("kon'a", "こんあ", "", "こんあ"),
            ("nnnk", "んん", "k", "んんk"),
            ("nnnn", "んん", "", "んん"),
        ];
        for (input, text, pending, flushed) in cases {
            assert_eq!(
                display_after(input),
                (text.to_string(), pending.to_string()),
                "表示: {input}"
            );
            let (mut composer, _) = setup_composer();
            insert_string(&mut composer, input);
            assert_eq!(text_after_flush(&mut composer), flushed, "flush: {input}");
        }
    }

    /// `input` を打ち、Backspace を `backspaces` 回押した後の（確定前の表示テキスト, 保留）
    fn after_backspaces(input: &str, backspaces: usize) -> (String, String) {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, input);
        for _ in 0..backspaces {
            composer.backspace();
        }
        (composer.text(), composer.pending().to_string())
    }

    #[test]
    fn test_backspace_rolls_back_mismatch_flush() {
        // hennk + p: k が不一致で確定するが、Backspace で k が保留に戻る（G-49）
        assert_eq!(
            after_backspaces("hennkp", 0),
            ("へんk".to_string(), "p".to_string())
        );
        assert_eq!(
            after_backspaces("hennkp", 1),
            ("へん".to_string(), "k".to_string())
        );
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "hennkp");
        composer.backspace();
        composer.insert_char('a');
        assert_eq!(text_after_flush(&mut composer), "へんか");
        // 巻き戻しで Literal の k は残らない
        assert!(
            composer
                .pieces()
                .iter()
                .all(|p| p.kind == PieceKind::Japanese)
        );
    }

    #[test]
    fn test_backspace_rolls_back_sokuon_and_n_consonant() {
        // kk → っ + 保留 k。Backspace で保留 k のみに戻る
        assert_eq!(
            after_backspaces("kk", 0),
            ("っ".to_string(), "k".to_string())
        );
        assert_eq!(after_backspaces("kk", 1), (String::new(), "k".to_string()));
        // nk → ん + 保留 k。Backspace で保留 n に戻る
        assert_eq!(
            after_backspaces("nk", 0),
            ("ん".to_string(), "k".to_string())
        );
        assert_eq!(after_backspaces("nk", 1), (String::new(), "n".to_string()));
        // 巻き戻し後に続けて打てる
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "nk");
        composer.backspace();
        composer.insert_char('a');
        assert_eq!(text_after_flush(&mut composer), "な");
        // kkp: っ + 保留 k に p が来て k が確定 → 「っk」+ 保留 p。Backspace で「っ」+ 保留 k
        assert_eq!(
            after_backspaces("kkp", 0),
            ("っk".to_string(), "p".to_string())
        );
        assert_eq!(
            after_backspaces("kkp", 1),
            ("っ".to_string(), "k".to_string())
        );
        // 保留の途中（tch → っ + ch）も直前キー前（tc）に戻る
        assert_eq!(
            after_backspaces("tch", 1),
            (String::new(), "tc".to_string())
        );
    }

    #[test]
    fn test_backspace_history_is_last_key_only() {
        // 2 回目以降は従来どおり（保留があれば保留を 1 文字、無ければ確定済みの末尾）
        assert_eq!(
            after_backspaces("hennkp", 2),
            ("へん".to_string(), String::new())
        );
        assert_eq!(
            after_backspaces("hennkp", 3),
            ("へ".to_string(), String::new())
        );
        // kk の 2 連続: 保留 k を消す（っ は戻らず消えたまま）
        assert_eq!(after_backspaces("kk", 2), (String::new(), String::new()));
    }

    #[test]
    fn test_backspace_without_flush_keeps_conventional_behavior() {
        // 確定が起きなかった（保留に足しただけ）: 保留を 1 文字戻す
        assert_eq!(after_backspaces("ky", 1), (String::new(), "k".to_string()));
        // 確定して保留が残らない: 確定済みの文字を消す
        assert_eq!(after_backspaces("ka", 1), (String::new(), String::new()));
        assert_eq!(after_backspaces("nn", 1), (String::new(), String::new()));
        // 履歴が無い（保留も直前の確定も無い）: 確定済みの末尾を消す
        assert_eq!(
            after_backspaces("kaki", 1),
            ("か".to_string(), String::new())
        );
    }

    #[test]
    fn test_backspace_rollback_of_raw_emit_with_empty_pending() {
        // k + `-`: k が Raw、`-` が ー で確定して保留は空。直前キー前（保留 k）に戻る
        assert_eq!(
            after_backspaces("k-", 0),
            ("kー".to_string(), String::new())
        );
        assert_eq!(after_backspaces("k-", 1), (String::new(), "k".to_string()));
    }

    #[test]
    fn test_backspace_history_discarded_by_other_operations() {
        // カーソル移動: 履歴を破棄し、保留は確定済み（Raw）になる
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "hennkp");
        composer.move_left();
        composer.move_right();
        assert_eq!(composer.text(), "へんkp");
        composer.backspace();
        assert_eq!(composer.text(), "へんk");
        assert_eq!(composer.pending(), "");
        // 文字以外の入力（数字）・確定・clear
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "hennkp5");
        composer.backspace();
        assert_eq!(composer.text(), "へんkp");
        assert_eq!(composer.pending(), "");
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "hennkp");
        composer.flush_pending();
        composer.backspace();
        assert_eq!(composer.text(), "へんk");
        assert_eq!(composer.pending(), "");
        // 途中挿入でも巻き戻しはカーソル直前の出力だけを消す
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "aiu");
        composer.move_left();
        insert_string(&mut composer, "kk");
        composer.backspace();
        assert_eq!(composer.text(), "あいう");
        assert_eq!(composer.pending(), "k");
        assert_eq!(composer.cursor(), 2);
    }

    #[test]
    fn test_converter_backspace_outcomes() {
        let mut c = RomajiConverter::new(RomajiTable::standard());
        assert_eq!(c.backspace(), Backspace::Nothing);
        c.feed('k');
        c.feed('p');
        assert_eq!(c.pending(), "p");
        assert_eq!(c.backspace(), Backspace::Rollback { retract: 1 });
        assert_eq!(c.pending(), "k");
        // 履歴は 1 キー分のみ
        assert_eq!(c.backspace(), Backspace::Pending);
        assert_eq!(c.backspace(), Backspace::Nothing);
        // 「ん」（1 文字）+ 保留
        c.feed('n');
        c.feed('k');
        assert_eq!(c.backspace(), Backspace::Rollback { retract: 1 });
        assert_eq!(c.pending(), "n");
    }

    #[test]
    fn test_nk_mora_n() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "nk");
        composer.flush_pending();
        // nk は ん（音便 n）+ 保留 k の flush で k は Raw
        let pieces = composer.pieces();
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].kind, PieceKind::Japanese);
        assert_eq!(pieces[0].text, "ん");
        assert_eq!(pieces[1].kind, PieceKind::Literal);
        assert_eq!(pieces[1].text, "k");
    }

    #[test]
    fn test_nk_with_a() {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, "nka");
        assert_eq!(text_after_flush(&mut composer), "んか");
    }

    #[test]
    fn test_load_pieces_and_mixed_insert() {
        let (mut composer, _) = setup_composer();
        composer.load_pieces(
            &[
                Piece {
                    kind: PieceKind::Japanese,
                    text: "きょう".to_string(),
                },
                Piece {
                    kind: PieceKind::Literal,
                    text: "Nagi".to_string(),
                },
            ],
            3,
        );
        assert_eq!(composer.cursor(), 3);
        assert_eq!(composer.text(), "きょうNagi");
        // 日本語と Literal の境界に Literal を挿入しても Piece が分断されない
        composer.insert_char('5');
        let kinds: Vec<_> = composer.pieces().iter().map(|p| p.kind).collect();
        assert_eq!(kinds, vec![PieceKind::Japanese, PieceKind::Literal]);
        // 日本語の途中に Literal を挿入すると 3 分割される
        composer.move_left();
        composer.move_left();
        composer.insert_char('7');
        let texts: Vec<_> = composer.pieces().into_iter().map(|p| p.text).collect();
        assert_eq!(texts, vec!["きょ", "7", "う", "5Nagi"]);
        // 範囲外の cursor は末尾に丸められる
        composer.load_pieces(&[], 9);
        assert_eq!(composer.cursor(), 0);
        assert!(composer.is_empty());
    }

    #[test]
    fn test_reconfigure_flushes_pending_and_applies_new_settings() {
        let (mut composer, table) = setup_composer();
        insert_string(&mut composer, "kan");
        // 保留中の n は ん として確定してから差し替える
        composer.reconfigure(
            Arc::new(table.with_punctuation(PunctuationStyle::Western)),
            ShiftLetterMode::SingleUpper,
        );
        assert_eq!(composer.text(), "かん");
        assert_eq!(composer.pending(), "");
        insert_string(&mut composer, ",");
        assert_eq!(composer.text(), "かん，");
        // SingleUpper: 大文字の直後の小文字は日本語のまま
        insert_string(&mut composer, "Na");
        assert_eq!(composer.text(), "かん，Nあ");
    }

    /// 標準テーブルで `input` を打って flush した結果
    fn convert_standard(input: &str) -> String {
        let (mut composer, _) = setup_composer();
        insert_string(&mut composer, input);
        text_after_flush(&mut composer)
    }

    /// Mozc の romanji-hiragana.tsv と同じ結果になるべき入力（系統別）
    const MOZC_CASES: &[(&str, &str)] = &[
        // c 系
        ("ca", "か"),
        ("ci", "し"),
        ("cu", "く"),
        ("ce", "せ"),
        ("co", "こ"),
        ("cya", "ちゃ"),
        ("cyi", "ちぃ"),
        ("cyu", "ちゅ"),
        ("cye", "ちぇ"),
        ("cyo", "ちょ"),
        ("cha", "ちゃ"),
        ("chi", "ち"),
        ("chu", "ちゅ"),
        ("che", "ちぇ"),
        ("cho", "ちょ"),
        // 拗音
        ("kya", "きゃ"),
        ("kyi", "きぃ"),
        ("kyu", "きゅ"),
        ("kye", "きぇ"),
        ("kyo", "きょ"),
        ("gya", "ぎゃ"),
        ("gyi", "ぎぃ"),
        ("gyu", "ぎゅ"),
        ("gye", "ぎぇ"),
        ("gyo", "ぎょ"),
        ("sya", "しゃ"),
        ("syi", "しぃ"),
        ("syu", "しゅ"),
        ("sye", "しぇ"),
        ("syo", "しょ"),
        ("sha", "しゃ"),
        ("shi", "し"),
        ("shu", "しゅ"),
        ("she", "しぇ"),
        ("sho", "しょ"),
        ("ja", "じゃ"),
        ("ji", "じ"),
        ("ju", "じゅ"),
        ("je", "じぇ"),
        ("jo", "じょ"),
        ("jya", "じゃ"),
        ("jyi", "じぃ"),
        ("jyu", "じゅ"),
        ("jye", "じぇ"),
        ("jyo", "じょ"),
        ("zya", "じゃ"),
        ("zyi", "じぃ"),
        ("zyu", "じゅ"),
        ("zye", "じぇ"),
        ("zyo", "じょ"),
        ("tya", "ちゃ"),
        ("tyi", "ちぃ"),
        ("tyu", "ちゅ"),
        ("tye", "ちぇ"),
        ("tyo", "ちょ"),
        ("dya", "ぢゃ"),
        ("dyi", "ぢぃ"),
        ("dyu", "ぢゅ"),
        ("dye", "ぢぇ"),
        ("dyo", "ぢょ"),
        ("nya", "にゃ"),
        ("nyi", "にぃ"),
        ("nyu", "にゅ"),
        ("nye", "にぇ"),
        ("nyo", "にょ"),
        ("hya", "ひゃ"),
        ("hyi", "ひぃ"),
        ("hyu", "ひゅ"),
        ("hye", "ひぇ"),
        ("hyo", "ひょ"),
        ("bya", "びゃ"),
        ("byi", "びぃ"),
        ("byu", "びゅ"),
        ("bye", "びぇ"),
        ("byo", "びょ"),
        ("pya", "ぴゃ"),
        ("pyi", "ぴぃ"),
        ("pyu", "ぴゅ"),
        ("pye", "ぴぇ"),
        ("pyo", "ぴょ"),
        ("mya", "みゃ"),
        ("myi", "みぃ"),
        ("myu", "みゅ"),
        ("mye", "みぇ"),
        ("myo", "みょ"),
        ("rya", "りゃ"),
        ("ryi", "りぃ"),
        ("ryu", "りゅ"),
        ("rye", "りぇ"),
        ("ryo", "りょ"),
        ("fya", "ふゃ"),
        ("fyu", "ふゅ"),
        ("fyo", "ふょ"),
        // 小書き
        ("xya", "ゃ"),
        ("xyi", "ぃ"),
        ("xyu", "ゅ"),
        ("xye", "ぇ"),
        ("xyo", "ょ"),
        ("lya", "ゃ"),
        ("lyi", "ぃ"),
        ("lyu", "ゅ"),
        ("lye", "ぇ"),
        ("lyo", "ょ"),
        ("xtu", "っ"),
        ("ltu", "っ"),
        ("xtsu", "っ"),
        ("ltsu", "っ"),
        ("xa", "ぁ"),
        ("xi", "ぃ"),
        ("xu", "ぅ"),
        ("xe", "ぇ"),
        ("xo", "ぉ"),
        ("la", "ぁ"),
        ("li", "ぃ"),
        ("lu", "ぅ"),
        ("le", "ぇ"),
        ("lo", "ぉ"),
        ("xwa", "ゎ"),
        ("lwa", "ゎ"),
        ("xka", "ヵ"),
        ("xke", "ヶ"),
        ("lka", "ヵ"),
        ("lke", "ヶ"),
        // 外来音
        ("fa", "ふぁ"),
        ("fi", "ふぃ"),
        ("fu", "ふ"),
        ("fe", "ふぇ"),
        ("fo", "ふぉ"),
        ("hwa", "ふぁ"),
        ("hwi", "ふぃ"),
        ("hwe", "ふぇ"),
        ("hwo", "ふぉ"),
        ("hwyu", "ふゅ"),
        ("va", "ゔぁ"),
        ("vi", "ゔぃ"),
        ("vu", "ゔ"),
        ("ve", "ゔぇ"),
        ("vo", "ゔぉ"),
        ("vya", "ゔゃ"),
        ("vyi", "ゔぃ"),
        ("vyu", "ゔゅ"),
        ("vye", "ゔぇ"),
        ("vyo", "ゔょ"),
        ("tsa", "つぁ"),
        ("tsi", "つぃ"),
        ("tsu", "つ"),
        ("tse", "つぇ"),
        ("tso", "つぉ"),
        ("tha", "てゃ"),
        ("thi", "てぃ"),
        ("thu", "てゅ"),
        ("the", "てぇ"),
        ("tho", "てょ"),
        ("dha", "でゃ"),
        ("dhi", "でぃ"),
        ("dhu", "でゅ"),
        ("dhe", "でぇ"),
        ("dho", "でょ"),
        ("twa", "とぁ"),
        ("twi", "とぃ"),
        ("twu", "とぅ"),
        ("twe", "とぇ"),
        ("two", "とぉ"),
        ("dwa", "どぁ"),
        ("dwi", "どぃ"),
        ("dwu", "どぅ"),
        ("dwe", "どぇ"),
        ("dwo", "どぉ"),
        ("t'i", "てぃ"),
        ("d'i", "でぃ"),
        ("t'u", "とぅ"),
        ("d'u", "どぅ"),
        ("t'yu", "てゅ"),
        ("d'yu", "でゅ"),
        ("wi", "うぃ"),
        ("we", "うぇ"),
        ("wu", "う"),
        ("ye", "いぇ"),
        ("wyi", "ゐ"),
        ("wye", "ゑ"),
        ("wha", "うぁ"),
        ("whi", "うぃ"),
        ("whu", "う"),
        ("whe", "うぇ"),
        ("who", "うぉ"),
        ("kwa", "くぁ"),
        ("kwi", "くぃ"),
        ("kwu", "くぅ"),
        ("kwe", "くぇ"),
        ("kwo", "くぉ"),
        ("gwa", "ぐぁ"),
        ("gwi", "ぐぃ"),
        ("gwu", "ぐぅ"),
        ("gwe", "ぐぇ"),
        ("gwo", "ぐぉ"),
        ("swa", "すぁ"),
        ("swi", "すぃ"),
        ("swu", "すぅ"),
        ("swe", "すぇ"),
        ("swo", "すぉ"),
        ("zwa", "ずぁ"),
        ("zwi", "ずぃ"),
        ("zwu", "ずぅ"),
        ("zwe", "ずぇ"),
        ("zwo", "ずぉ"),
        ("qa", "くぁ"),
        ("qi", "くぃ"),
        ("qu", "く"),
        ("qe", "くぇ"),
        ("qo", "くぉ"),
        // 促音（末尾の保留子音は flush で素通しされるので、続けて母音を打つ）
        ("kka", "っか"),
        ("ssa", "っさ"),
        ("tta", "った"),
        ("cca", "っか"),
        ("ppa", "っぱ"),
        ("qqa", "っくぁ"),
        ("lla", "っぁ"),
        ("xxa", "っぁ"),
        ("yya", "っや"),
        ("vva", "っゔぁ"),
        ("tcha", "っちゃ"),
        ("tchi", "っち"),
        ("ttya", "っちゃ"),
        ("ssha", "っしゃ"),
        // 撥音
        ("nn", "ん"),
        ("n'", "ん"),
        ("xn", "ん"),
        ("kan", "かん"),
        ("nna", "んあ"),
        ("nnei", "んえい"),
        ("nnna", "んな"),
        ("nka", "んか"),
        ("nja", "んじゃ"),
        ("nsa", "んさ"),
        ("nla", "んぁ"),
        ("nxa", "んぁ"),
        ("na", "な"),
        ("nya", "にゃ"),
        ("konnichiha", "こんいちは"),
        ("konnnichiha", "こんにちは"),
        ("kon'nichiha", "こんにちは"),
        ("kanni", "かんい"),
        // 記号
        ("-", "ー"),
        (",", "、"),
        (".", "。"),
        ("[", "「"),
        ("]", "」"),
        ("~", "〜"),
        ("z/", "・"),
        ("z.", "…"),
        ("z,", "‥"),
        ("z-", "〜"),
        ("z[", "『"),
        ("z]", "』"),
        ("zh", "←"),
        ("zj", "↓"),
        ("zk", "↑"),
        ("zl", "→"),
        // 基本（既存系統の退行防止）
        ("za", "ざ"),
        ("zi", "じ"),
        ("zu", "ず"),
        ("ze", "ぜ"),
        ("zo", "ぞ"),
        ("ta", "た"),
        ("ti", "ち"),
        ("tu", "つ"),
        ("da", "だ"),
        ("di", "ぢ"),
        ("du", "づ"),
        ("hu", "ふ"),
        ("wa", "わ"),
        ("wo", "を"),
        ("ya", "や"),
        ("yu", "ゆ"),
        ("yo", "よ"),
    ];

    #[test]
    fn test_mozc_compatible_table_cases() {
        let mut failures = Vec::new();
        for (input, expected) in MOZC_CASES {
            let got = convert_standard(input);
            if got != *expected {
                failures.push(format!("{input}: expected {expected}, got {got}"));
            }
        }
        assert!(
            failures.is_empty(),
            "Mozc 互換でない入力:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn test_cya_row_explicit() {
        for (input, expected) in [
            ("cya", "ちゃ"),
            ("cyi", "ちぃ"),
            ("cyu", "ちゅ"),
            ("cye", "ちぇ"),
            ("cyo", "ちょ"),
        ] {
            let (mut composer, _) = setup_composer();
            insert_string(&mut composer, input);
            // 完成した時点で保留は残らない（flush 前に確定している）
            assert_eq!(composer.text(), expected, "{input}");
            assert_eq!(composer.pending(), "", "{input}");
        }
    }

    #[test]
    fn test_standard_table_entries_are_well_formed() {
        let table = RomajiTable::standard();
        for (input, (output, pending)) in &table.entries {
            assert!(!input.is_empty(), "空の入力キー");
            assert!(!output.is_empty(), "出力が空: {input}");
            assert!(
                !input.chars().any(char::is_whitespace),
                "入力に空白: {input:?}"
            );
            // 保留は入力の末尾（促音・撥音）か、テーブルのいずれかの接頭辞になっている
            if !pending.is_empty() {
                assert!(
                    input.ends_with(pending.as_str())
                        || table.has_longer_prefix(pending)
                        || table.get(pending).is_some(),
                    "保留が不正: {input} -> {output} / {pending}"
                );
            }
        }
    }

    #[test]
    fn test_standard_table_no_duplicate_keys_in_file() {
        // from_tsv は重複を弾くが、ファイル内の行数とエントリ数が一致することでも確認する
        let tsv = include_str!("../../../config/romaji/standard.tsv");
        let lines = tsv
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .count();
        assert_eq!(lines, RomajiTable::standard().entries.len());
    }

    // ---- 打鍵ログ（曖昧入力） ----

    fn logged_composer() -> Composer {
        let mut c = Composer::new(RomajiTable::standard(), ShiftLetterMode::LiteralContinue);
        c.set_key_log_enabled(true);
        c
    }

    fn typed_of(c: &Composer) -> String {
        c.key_log()
            .map(|l| l.iter().map(|r| r.typed).collect())
            .unwrap_or_default()
    }

    /// ログの再生結果が Composer の状態と一致する（`key_log` が Some のとき）
    fn assert_log_consistent(c: &Composer) {
        let Some(log) = c.key_log() else { return };
        let mut conv = RomajiConverter::new(RomajiTable::standard());
        let mut text = String::new();
        let mut push = |emits: Vec<Emit>| {
            for e in emits {
                match e {
                    Emit::Converted(t) => text.push_str(&t),
                    Emit::Raw(ch) => text.push(ch),
                }
            }
        };
        for r in log {
            if ('ぁ'..='ゖ').contains(&r.typed) || r.typed == 'ー' {
                push(conv.flush());
                push(vec![Emit::Raw(r.typed)]);
            } else {
                push(conv.feed(r.typed));
            }
        }
        // 保留を確定した結果どうしで比べる（Space 変換の前に flush されていても一致する）
        push(conv.flush());
        let mut expected = c.text();
        if c.pending() == "n" {
            expected.push('ん');
        } else {
            expected.push_str(c.pending());
        }
        assert_eq!(text, expected, "log = {log:?}");
    }

    #[test]
    fn key_log_is_off_by_default() {
        let (mut c, _) = setup_composer();
        insert_string(&mut c, "ka");
        assert!(c.key_log().is_none());
        // 無効のままでも alts 付き挿入は通常の挿入と同じ
        c.insert_char_with_alts('k', &[('l', 0.3)]);
        assert!(c.key_log().is_none());
        assert_eq!(c.pending(), "k");
    }

    #[test]
    fn key_log_records_keys_and_sorted_clean_alts() {
        let mut c = logged_composer();
        c.insert_char_with_alts(
            'k',
            &[
                ('l', 0.6),
                ('j', 0.2),
                ('K', 0.1), // 大文字は捨てる
                ('k', 0.0), // 打たれた文字自身は捨てる
                ('1', 0.1), // 数字は捨てる
                ('m', f32::NAN),
                ('o', -1.0),
            ],
        );
        c.insert_char('a');
        let log = c.key_log().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].typed, 'k');
        assert_eq!(log[0].alts, vec![('j', 0.2), ('l', 0.6)]);
        // 打たれた文字自身の距離は alts と分けて保持する
        assert_eq!(log[0].typed_distance, Some(0.0));
        assert!(log[1].alts.is_empty());
        assert_eq!(log[1].typed_distance, None);
        assert_log_consistent(&c);
    }

    #[test]
    fn key_log_keeps_typed_distance_outside_alt_limit() {
        let mut c = logged_composer();
        let mut alts: Vec<(char, f32)> = ('a'..='j')
            .filter(|&ch| ch != 'k')
            .map(|ch| (ch, 0.1 + (ch as u8 - b'a') as f32 * 0.01))
            .collect();
        alts.push(('k', 0.9));
        c.insert_char_with_alts('k', &alts);
        let log = c.key_log().unwrap();
        assert_eq!(log[0].alts.len(), MAX_ALTS_PER_KEY);
        assert_eq!(log[0].typed_distance, Some(0.9));
    }

    #[test]
    fn key_log_includes_hiragana_inserts() {
        let mut c = logged_composer();
        c.insert_reading_char_with_alts('か', PieceKind::Japanese, &[('け', 0.4), ('K', 0.1)]);
        c.insert_char('n');
        c.insert_reading_char('き', PieceKind::Japanese); // 保留の n を確定してから挿入
        assert_eq!(typed_of(&c), "かnき");
        assert_eq!(c.key_log().unwrap()[0].alts, vec![('け', 0.4)]);
        assert_eq!(c.text(), "かんき");
        assert_log_consistent(&c);
    }

    #[test]
    fn key_log_backspace_pops_per_keystroke() {
        let mut c = logged_composer();
        insert_string(&mut c, "ka");
        assert_eq!(typed_of(&c), "ka");
        // か（2 打鍵）を消すとログも 2 件戻る
        c.backspace();
        assert!(c.is_empty());
        assert!(c.key_log().is_none());

        // 保留の削除（1 打鍵）
        insert_string(&mut c, "sh");
        c.backspace();
        assert_eq!(typed_of(&c), "s");
        assert_log_consistent(&c);

        // G-49 の巻き戻し: `kk` → っ + 保留 k。Backspace で保留 k に戻る（1 打鍵）
        c.clear();
        insert_string(&mut c, "kk");
        assert_eq!(c.text(), "っ");
        c.backspace();
        assert_eq!(c.text(), "");
        assert_eq!(c.pending(), "k");
        assert_eq!(typed_of(&c), "k");
        assert_log_consistent(&c);
        c.backspace();
        assert!(c.is_empty());
    }

    #[test]
    fn key_log_survives_raw_literal_output() {
        // `nihn`: 不一致の h は Raw（Literal）として残るが、小文字キーだけの入力なのでログは有効
        let mut c = logged_composer();
        insert_string(&mut c, "nihn");
        assert_eq!(typed_of(&c), "nihn");
        assert_log_consistent(&c);
        // 巻き戻しも整合する
        c.backspace();
        assert_log_consistent(&c);
        c.backspace();
        assert_log_consistent(&c);
    }

    #[test]
    fn key_log_is_invalidated_by_non_append_edits() {
        type Op = fn(&mut Composer);
        let invalidated: Vec<(&str, Op)> = vec![
            ("uppercase", |c| c.insert_char('A')),
            ("digit", |c| c.insert_char('1')),
            ("symbol", |c| c.insert_char('-')),
            ("non-ascii", |c| c.insert_char('ｱ')),
            ("literal reading", |c| {
                c.insert_reading_char('a', PieceKind::Literal)
            }),
            ("non-kana reading", |c| {
                c.insert_reading_char('。', PieceKind::Japanese)
            }),
            ("delete", |c| {
                c.delete();
            }),
            ("move_left", |c| {
                c.move_left();
            }),
            ("move_right", |c| {
                c.move_right();
            }),
            ("move_home", |c| c.move_home()),
            ("move_end", |c| c.move_end()),
            ("load_pieces", |c| {
                let p = c.pieces();
                c.load_pieces(&p, 2)
            }),
        ];
        for (name, op) in invalidated {
            let mut c = logged_composer();
            insert_string(&mut c, "kak");
            assert!(c.key_log().is_some(), "{name}");
            op(&mut c);
            assert!(c.key_log().is_none(), "{name}: 無効化されること");
            // 以降の打鍵でも復活しない
            insert_string(&mut c, "a");
            assert!(c.key_log().is_none(), "{name}: 復活しないこと");
        }
    }

    #[test]
    fn key_log_is_invalidated_by_mid_insert_and_flushed_pending() {
        // 文中への挿入（カーソル移動で無効化済みだが、戻ってきても復活しない）
        let mut c = logged_composer();
        insert_string(&mut c, "ka");
        c.move_left();
        c.insert_char('s');
        assert!(c.key_log().is_none());

        // 保留 n を確定したあとに続けた打鍵はログの再生と一致しない
        let mut c = logged_composer();
        insert_string(&mut c, "n");
        c.flush_pending();
        c.insert_char('a');
        assert!(c.key_log().is_none());
        assert_eq!(c.text(), "んあ");

        // Space 変換相当（flush のみ）ではログは保たれる
        let mut c = logged_composer();
        insert_string(&mut c, "kan");
        c.flush_pending();
        assert_eq!(typed_of(&c), "kan");
        assert_log_consistent(&c);
    }

    #[test]
    fn key_log_revives_after_input_becomes_empty() {
        let mut c = logged_composer();
        insert_string(&mut c, "a");
        c.insert_char('Z');
        assert!(c.key_log().is_none());
        c.clear();
        insert_string(&mut c, "ka");
        assert_eq!(typed_of(&c), "ka");
        // 全部 Backspace で消して空になっても、新しい入力は有効
        let mut c = logged_composer();
        insert_string(&mut c, "a");
        c.delete();
        c.insert_char('u');
        assert!(c.key_log().is_none());
        c.backspace();
        c.backspace();
        assert!(c.is_empty());
        insert_string(&mut c, "i");
        assert_eq!(typed_of(&c), "i");
    }

    #[test]
    fn key_log_enabling_midway_stays_invalid_until_empty() {
        let (mut c, _) = setup_composer();
        insert_string(&mut c, "ka");
        c.set_key_log_enabled(true);
        insert_string(&mut c, "k");
        assert!(c.key_log().is_none());
        c.clear();
        insert_string(&mut c, "a");
        assert_eq!(typed_of(&c), "a");
        c.set_key_log_enabled(false);
        assert!(c.key_log().is_none());
    }

    /// 擬似乱数で追記・Backspace・その他の編集を混ぜ、(1) ログが有効なら常に再生結果と一致する
    /// (2) ログの有無で text / pending / cursor / pieces が 1 ビットも変わらない
    #[test]
    fn key_log_never_changes_composer_behavior() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let alphabet: Vec<char> = "kknasihtuoeyzj".chars().collect();
        for _ in 0..300 {
            let mut with_log = logged_composer();
            let mut without =
                Composer::new(RomajiTable::standard(), ShiftLetterMode::LiteralContinue);
            for _ in 0..40 {
                let r = next();
                let ch = alphabet[(r >> 8) as usize % alphabet.len()];
                match r % 24 {
                    0..=13 => {
                        with_log.insert_char_with_alts(ch, &[('l', 0.4)]);
                        without.insert_char(ch);
                    }
                    14..=18 => {
                        assert_eq!(with_log.backspace(), without.backspace());
                    }
                    19 => {
                        with_log.insert_reading_char('か', PieceKind::Japanese);
                        without.insert_reading_char('か', PieceKind::Japanese);
                    }
                    20 => {
                        with_log.flush_pending();
                        without.flush_pending();
                    }
                    21 => {
                        assert_eq!(with_log.move_left(), without.move_left());
                    }
                    22 => {
                        with_log.move_end();
                        without.move_end();
                    }
                    _ => {
                        with_log.insert_char('Q');
                        without.insert_char('Q');
                    }
                }
                assert_eq!(with_log.text(), without.text());
                assert_eq!(with_log.pending(), without.pending());
                assert_eq!(with_log.cursor(), without.cursor());
                assert_eq!(with_log.pieces(), without.pieces());
                assert_log_consistent(&with_log);
            }
        }
    }
}
