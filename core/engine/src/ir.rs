//! IR（中間表現）: プリエディットのセグメント構造。
//!
//! 設計書 §4.1。`reading`（ひらがな）が常に真実で、`surface` はその射影。位置の内部表現は
//! reading 基準（セグメント index + reading 内の文字位置）で、プロトコルへは surface 上の
//! UTF-16 offset として出す（[`Preedit::to_view`]）。

use nagi_protocol::utf16;
pub use nagi_protocol::view::{CandidateSource, SegmentKind};
use nagi_protocol::view::{PreeditView, RangeU16, SegmentView};

/// 1 つの候補
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 表示文字列
    pub surface: String,
    /// 出所（複数の供給元が同じ表記を出した場合は複数）
    pub sources: Vec<CandidateSource>,
}

/// 文節ごとの候補セット（遅延生成）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSet {
    /// 候補（先頭が変換開始時の表記）
    pub items: Vec<Candidate>,
    /// 選択中の候補 index
    pub selected: usize,
}

/// セグメント（文節相当）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// ひらがな読み（正）。Literal では入力した文字そのもの
    pub reading: String,
    /// 現在の表示（変換結果、または読みそのまま）
    pub surface: String,
    /// 種別
    pub kind: SegmentKind,
    /// 候補セット（フォーカスされるまで生成しない）
    pub candidates: Option<CandidateSet>,
    /// ライブ変換の再変換単位（Phase 2 まで連番）
    pub chunk_id: u32,
}

impl Segment {
    /// 読みと表記が同じセグメントを作る（Composing 用）
    pub fn plain(text: &str, kind: SegmentKind, chunk_id: u32) -> Segment {
        Segment {
            reading: text.to_string(),
            surface: text.to_string(),
            kind,
            candidates: None,
            chunk_id,
        }
    }
}

/// カーソル位置（セグメント index + そのセグメントの reading 内の文字位置）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorPos {
    /// セグメント index
    pub segment: usize,
    /// セグメントの reading 内の文字（Unicode スカラー）位置
    pub offset: usize,
}

/// プリエディット
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preedit {
    /// セグメント列
    pub segments: Vec<Segment>,
    /// カーソル位置（Composing で意味を持つ。Converting では末尾扱い）
    pub cursor: CursorPos,
    /// Converting の注目文節
    pub focus: Option<usize>,
    /// 2 文節以上の選択範囲（セグメント index の半開区間。RangeSelecting）
    pub selection: Option<std::ops::Range<usize>>,
    /// 未確定のローマ字（設計書 §4.1 に無い追加フィールド。カーソル位置に付随する）
    pub pending: String,
    /// リビジョン
    pub revision: u64,
}

impl Preedit {
    /// surface の連結
    pub fn surface_text(&self) -> String {
        self.segments.iter().map(|s| s.surface.as_str()).collect()
    }

    /// reading の連結
    pub fn reading_text(&self) -> String {
        self.segments.iter().map(|s| s.reading.as_str()).collect()
    }

    /// プロトコル用の表示型へ変換する
    ///
    /// `text` は surface の連結にカーソル位置へ `pending` を挟んだもの。各セグメントの
    /// `range` は `text` 上の UTF-16 範囲で、pending より後ろは pending の長さだけずれ、
    /// pending がセグメントの途中に入る場合はそのセグメントの範囲が pending を跨ぐ。
    /// Converting（`focus` あり）ではカーソルは末尾。
    pub fn to_view(&self) -> PreeditView {
        // pending なしでの各セグメントの範囲
        let mut starts = Vec::with_capacity(self.segments.len());
        let mut ends = Vec::with_capacity(self.segments.len());
        let mut pos = 0u32;
        for s in &self.segments {
            starts.push(pos);
            pos += utf16::utf16_len(&s.surface);
            ends.push(pos);
        }
        let total = pos;

        // カーソルの text 上の位置（pending を挟む前）
        let cursor_before = if self.focus.is_some() {
            total
        } else if let Some(seg) = self.segments.get(self.cursor.segment) {
            // cursor.offset は reading の文字数。surface が reading と異なる（変換済み）文節では
            // 同じ数値を surface に当てられないので、文節の端に寄せる（先頭 or 末尾）。
            // surface == reading なら文字数どおり
            let reading_len = seg.reading.chars().count();
            let offset = self.cursor.offset;
            if seg.surface == seg.reading {
                let head: String = seg.surface.chars().take(offset).collect();
                starts[self.cursor.segment] + utf16::utf16_len(&head)
            } else if offset * 2 < reading_len {
                starts[self.cursor.segment]
            } else {
                ends[self.cursor.segment]
            }
        } else {
            total
        };

        let pending_len = utf16::utf16_len(&self.pending);
        let has_pending = pending_len > 0;
        let shift = |p: u32, is_end: bool| -> u32 {
            // 挿入位置より後ろは pending 分ずらす。start が挿入位置ちょうどなら後ろ側（ずらす）、
            // end が挿入位置ちょうどなら前側（ずらさない）
            if has_pending && (p > cursor_before || (!is_end && p == cursor_before)) {
                p + pending_len
            } else {
                p
            }
        };

        let mut text = self.surface_text();
        let mut inserted = !has_pending;
        if has_pending {
            // text の UTF-16 位置 cursor_before に pending を挿入する
            let mut acc = 0u32;
            let mut out = String::with_capacity(text.len() + self.pending.len());
            for ch in text.chars() {
                if !inserted && acc == cursor_before {
                    out.push_str(&self.pending);
                    inserted = true;
                }
                out.push(ch);
                acc += ch.len_utf16() as u32;
            }
            if !inserted {
                out.push_str(&self.pending);
            }
            text = out;
        }

        let segments: Vec<SegmentView> = self
            .segments
            .iter()
            .enumerate()
            .map(|(i, s)| SegmentView {
                surface: s.surface.clone(),
                reading: s.reading.clone(),
                kind: s.kind,
                range: RangeU16::new(shift(starts[i], false), shift(ends[i], true)),
            })
            .collect();

        let pending =
            has_pending.then(|| RangeU16::new(cursor_before, cursor_before + pending_len));
        let selection = self.selection.as_ref().and_then(|r| {
            let first = segments.get(r.start)?;
            let last = segments.get(r.end.checked_sub(1)?)?;
            Some(RangeU16::new(first.range.start, last.range.end))
        });

        PreeditView {
            text,
            segments,
            cursor: cursor_before + pending_len,
            pending,
            focus: self.focus.map(|f| f as u32),
            selection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(reading: &str, surface: &str, kind: SegmentKind) -> Segment {
        Segment {
            reading: reading.into(),
            surface: surface.into(),
            kind,
            candidates: None,
            chunk_id: 0,
        }
    }

    fn preedit(segments: Vec<Segment>, cursor: (usize, usize), pending: &str) -> Preedit {
        Preedit {
            segments,
            cursor: CursorPos {
                segment: cursor.0,
                offset: cursor.1,
            },
            pending: pending.into(),
            ..Default::default()
        }
    }

    #[test]
    fn empty_preedit() {
        let v = Preedit::default().to_view();
        assert_eq!(v, PreeditView::default());
    }

    #[test]
    fn text_readings_and_ranges() {
        let p = preedit(
            vec![
                seg("きょう", "今日", SegmentKind::Japanese),
                seg("は", "は", SegmentKind::Japanese),
            ],
            (1, 1),
            "",
        );
        assert_eq!(p.surface_text(), "今日は");
        assert_eq!(p.reading_text(), "きょうは");
        let v = p.to_view();
        assert_eq!(v.text, "今日は");
        assert_eq!(v.segments[0].range, RangeU16::new(0, 2));
        assert_eq!(v.segments[1].range, RangeU16::new(2, 3));
        assert_eq!(v.cursor, 3);
        assert_eq!(v.pending, None);
    }

    #[test]
    fn pending_at_end_extends_nothing() {
        let p = preedit(vec![seg("か", "か", SegmentKind::Japanese)], (0, 1), "k");
        let v = p.to_view();
        assert_eq!(v.text, "かk");
        assert_eq!(v.segments[0].range, RangeU16::new(0, 1));
        assert_eq!(v.pending, Some(RangeU16::new(1, 2)));
        assert_eq!(v.cursor, 2);
    }

    #[test]
    fn pending_between_segments() {
        let p = preedit(
            vec![
                seg("か", "か", SegmentKind::Japanese),
                seg("N", "N", SegmentKind::Literal),
            ],
            (0, 1),
            "k",
        );
        let v = p.to_view();
        assert_eq!(v.text, "かkN");
        assert_eq!(v.segments[0].range, RangeU16::new(0, 1));
        assert_eq!(v.segments[1].range, RangeU16::new(2, 3));
        assert_eq!(v.pending, Some(RangeU16::new(1, 2)));
        assert_eq!(v.cursor, 2);
    }

    #[test]
    fn pending_inside_segment_spans_it() {
        let p = preedit(
            vec![seg("かき", "かき", SegmentKind::Japanese)],
            (0, 1),
            "k",
        );
        let v = p.to_view();
        assert_eq!(v.text, "かkき");
        assert_eq!(v.segments[0].range, RangeU16::new(0, 3));
        assert_eq!(v.pending, Some(RangeU16::new(1, 2)));
    }

    #[test]
    fn pending_at_start() {
        let p = preedit(vec![seg("か", "か", SegmentKind::Japanese)], (0, 0), "k");
        let v = p.to_view();
        assert_eq!(v.text, "kか");
        assert_eq!(v.segments[0].range, RangeU16::new(1, 2));
        assert_eq!(v.cursor, 1);
    }

    #[test]
    fn surrogate_pairs_and_combining_marks() {
        // 絵文字（UTF-16 で 2 単位）と結合文字（か + U+3099、2 スカラー）
        let p = preedit(
            vec![
                seg("😀", "😀", SegmentKind::Japanese),
                seg("か\u{3099}", "か\u{3099}", SegmentKind::Japanese),
            ],
            (1, 2),
            "",
        );
        let v = p.to_view();
        assert_eq!(v.segments[0].range, RangeU16::new(0, 2));
        assert_eq!(v.segments[1].range, RangeU16::new(2, 4));
        assert_eq!(v.cursor, 4);
        // カーソルが絵文字の直後（セグメント 0 の末尾）にあり pending がある場合
        let p = preedit(
            vec![
                seg("😀", "😀", SegmentKind::Japanese),
                seg("あ", "あ", SegmentKind::Japanese),
            ],
            (0, 1),
            "k",
        );
        let v = p.to_view();
        assert_eq!(v.text, "😀kあ");
        assert_eq!(v.pending, Some(RangeU16::new(2, 3)));
        assert_eq!(v.segments[1].range, RangeU16::new(3, 4));
        assert_eq!(v.cursor, 3);
    }

    #[test]
    fn converting_cursor_is_at_end_and_focus_selection_map() {
        let mut p = preedit(
            vec![
                seg("きょう", "今日", SegmentKind::Japanese),
                seg("は", "は", SegmentKind::Japanese),
                seg("いい", "良い", SegmentKind::Japanese),
            ],
            (0, 0),
            "",
        );
        p.focus = Some(1);
        p.selection = Some(1..3);
        let v = p.to_view();
        assert_eq!(v.cursor, 5);
        assert_eq!(v.focus, Some(1));
        assert_eq!(v.selection, Some(RangeU16::new(2, 5)));
    }
}
