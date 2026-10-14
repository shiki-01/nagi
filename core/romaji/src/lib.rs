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

#[derive(Debug)]
/// ローマ字入力バッファ・カーソル管理
///
/// バッファは 1 文字ずつ種別（日本語 / Literal）を持つ平坦な列で、カーソルはその文字位置。
/// 保留中のローマ字はバッファに含めず、常にカーソル位置に付随する。
pub struct Composer {
    converter: RomajiConverter,
    shift_letter: ShiftLetterMode,
    chars: Vec<(char, PieceKind)>,
    cursor: usize,
    literal_mode: bool, // Shift+英字で開始した literal モード
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
        }
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
        if ch.is_control() {
            return;
        }
        if ch.is_ascii_uppercase() {
            self.flush_pending();
            self.literal_mode = matches!(self.shift_letter, ShiftLetterMode::LiteralContinue);
            self.insert_at_cursor(ch, PieceKind::Literal);
        } else if ch.is_ascii_digit() || (self.literal_mode && ch.is_ascii_graphic()) {
            self.flush_pending();
            self.insert_at_cursor(ch, PieceKind::Literal);
        } else if !ch.is_ascii() {
            self.flush_pending();
            self.insert_at_cursor(ch, PieceKind::Japanese);
        } else {
            let emits = self.converter.feed(ch);
            self.insert_emits(emits);
        }
    }

    /// 先頭から `n` 文字を取り除く（部分変換で先頭だけを確定したあと、残りを入力中に残す）。
    /// カーソルはその分だけ前へ動く。保留中のローマ字は先に確定する
    pub fn remove_front(&mut self, n: usize) {
        self.flush_pending();
        let n = n.min(self.chars.len());
        self.chars.drain(..n);
        self.cursor = self.cursor.saturating_sub(n);
    }

    /// カーソル直前の 1 文字（保留中のローマ字がある場合は `None`）
    pub fn char_before_cursor(&self) -> Option<char> {
        if self.has_pending() {
            return None;
        }
        self.cursor
            .checked_sub(1)
            .and_then(|i| self.chars.get(i))
            .map(|&(c, _)| c)
    }

    /// カーソル直前の 1 文字を、日本語の文字 `ch` に置き換える（タッチ入力のトグル・濁点循環用）。
    /// 置き換えられたら `true`
    pub fn replace_before_cursor(&mut self, ch: char) -> bool {
        self.flush_pending();
        let Some(i) = self.cursor.checked_sub(1) else {
            return false;
        };
        let Some(slot) = self.chars.get_mut(i) else {
            return false;
        };
        *slot = (ch, PieceKind::Japanese);
        true
    }

    /// 保留を確定させる（`n` → ん など）
    pub fn flush_pending(&mut self) {
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
        match self.converter.backspace() {
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
        }
    }

    /// カーソル直後の 1 文字を削除する（保留は先に確定する）
    pub fn delete(&mut self) -> bool {
        self.flush_pending();
        if self.cursor >= self.chars.len() {
            return false;
        }
        self.chars.remove(self.cursor);
        true
    }

    /// カーソルを左へ 1 文字移動する（保留は先に確定する）
    pub fn move_left(&mut self) -> bool {
        self.flush_pending();
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    /// カーソルを右へ 1 文字移動する（保留は先に確定する）
    pub fn move_right(&mut self) -> bool {
        self.flush_pending();
        if self.cursor >= self.chars.len() {
            return false;
        }
        self.cursor += 1;
        true
    }

    /// カーソルを先頭へ移動する
    pub fn move_home(&mut self) {
        self.flush_pending();
        self.cursor = 0;
    }

    /// カーソルを末尾へ移動する
    pub fn move_end(&mut self) {
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
}
