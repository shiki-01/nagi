//! preedit の表示属性の「描画計画」（OS 非依存）。macOS 版 `PreeditStyler.swift` に相当する。
//!
//! 1. [`runs`]: 各セグメントの状態（通常・注目文節・選択・確定済み…）を [`MarkStyle`] の区間にする
//!    （macOS 版と同じ判定。セグメントに覆われない部分は含まない）
//! 2. [`plan`]: 全文を隙間なく覆う区間列にし（隙間は [`MarkStyle::Normal`]）、
//!    [`DisplayAttr`]（下線の種類・太さ）を割り当てる。TSF 層はこれを `ITfDisplayAttributeInfo` /
//!    `GUID_PROP_ATTRIBUTE` に写すだけにする
//!
//! アプリによっては属性が無視されるため、色や背景には頼らず下線の種類と太さだけで区別する。
//! 区間はすべて UTF-16 コード単位の半開区間。

use nagi_protocol::{PreeditView, RangeU16, SegmentKind};

/// 下線スタイルの種別（macOS 版 `MarkStyle` と同じ）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkStyle {
    /// 通常
    Normal,
    /// Converting の注目文節
    Focused,
    /// RangeSelecting の選択範囲内
    Selected,
    /// 部分確定済み
    Fixed,
    /// スタイル変換結果
    Styled,
    /// 保留中のローマ字
    Pending,
}

/// 装飾する区間（UTF-16 コード単位、半開区間）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleRun {
    /// 開始
    pub start: u32,
    /// 終了
    pub end: u32,
    /// 種別
    pub style: MarkStyle,
}

/// 下線の線種（`TF_DISPLAYATTRIBUTE.lsStyle` に対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderlineKind {
    /// 実線（`TF_LS_SOLID`）
    Solid,
    /// 点線（`TF_LS_DOT`）
    Dot,
    /// 破線（`TF_LS_DASH`）
    Dash,
}

/// 表示属性。色は指定しない（ホストの既定の色に従う）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayAttr {
    /// 下線の線種
    pub underline: UnderlineKind,
    /// 太線か（`fBoldLine`）
    pub bold: bool,
}

/// 属性を割り当てた区間
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttrRun {
    /// 開始
    pub start: u32,
    /// 終了
    pub end: u32,
    /// 種別
    pub style: MarkStyle,
    /// 表示属性
    pub attr: DisplayAttr,
}

/// 種別 → 表示属性。macOS 版（`NagiInputController.attributes(for:)`）と同じ対応:
/// 通常 = 細い実線、注目・選択 = 太い実線、確定済み・保留 = 点線、スタイル変換結果 = 破線
pub fn attr_for(style: MarkStyle) -> DisplayAttr {
    let (underline, bold) = match style {
        MarkStyle::Normal => (UnderlineKind::Solid, false),
        MarkStyle::Focused | MarkStyle::Selected => (UnderlineKind::Solid, true),
        MarkStyle::Fixed | MarkStyle::Pending => (UnderlineKind::Dot, false),
        MarkStyle::Styled => (UnderlineKind::Dash, false),
    };
    DisplayAttr { underline, bold }
}

/// 表示属性の取りうる値（`attr_for` の像）。TSF では属性ごとに GUID を 1 つ割り当てて登録する。
/// 並びが GUID の割り当て順になる（変えると登録済みの属性と対応が変わる）
pub const DISPLAY_ATTRS: [DisplayAttr; 4] = [
    DisplayAttr {
        underline: UnderlineKind::Solid,
        bold: false,
    },
    DisplayAttr {
        underline: UnderlineKind::Solid,
        bold: true,
    },
    DisplayAttr {
        underline: UnderlineKind::Dot,
        bold: false,
    },
    DisplayAttr {
        underline: UnderlineKind::Dash,
        bold: false,
    },
];

/// `DISPLAY_ATTRS` 内の位置
pub fn attr_index(attr: DisplayAttr) -> usize {
    DISPLAY_ATTRS
        .iter()
        .position(|a| *a == attr)
        .unwrap_or_default()
}

/// 範囲 `[0, total)` を覆う composition の範囲から、区間 `[start, end)` の範囲を作るための移動量。
/// TSF の `ShiftStart(start)` と `ShiftEnd(-(total - end))`
pub fn range_shifts(total: u32, start: u32, end: u32) -> (i32, i32) {
    let end = end.min(total);
    let start = start.min(end);
    (
        i32::try_from(start).unwrap_or(i32::MAX),
        -i32::try_from(total - end).unwrap_or(i32::MAX),
    )
}

fn text_len(preedit: &PreeditView) -> u32 {
    u32::try_from(preedit.text.encode_utf16().count()).unwrap_or(u32::MAX)
}

fn clamp(r: RangeU16, total: u32) -> Option<(u32, u32)> {
    let (s, e) = (r.start.min(total), r.end.min(total));
    (e > s).then_some((s, e))
}

/// `PreeditView` → 装飾区間。セグメントに覆われない部分は含まない。開始位置の昇順
pub fn runs(preedit: &PreeditView) -> Vec<StyleRun> {
    let total = text_len(preedit);
    let mut out = Vec::new();
    for (i, seg) in preedit.segments.iter().enumerate() {
        let Some((start, end)) = clamp(seg.range, total) else {
            continue;
        };
        let focused = preedit.focus.is_some_and(|f| usize::try_from(f) == Ok(i));
        let selected = preedit
            .selection
            .is_some_and(|sel| seg.range.start >= sel.start && seg.range.end <= sel.end);
        let style = if focused {
            MarkStyle::Focused
        } else if selected {
            MarkStyle::Selected
        } else {
            match seg.kind {
                SegmentKind::Fixed => MarkStyle::Fixed,
                SegmentKind::Styled => MarkStyle::Styled,
                SegmentKind::Japanese | SegmentKind::Literal => MarkStyle::Normal,
            }
        };
        out.push(StyleRun { start, end, style });
    }
    if let Some((start, end)) = preedit.pending.and_then(|p| clamp(p, total)) {
        out.push(StyleRun {
            start,
            end,
            style: MarkStyle::Pending,
        });
    }
    out.sort_by_key(|r| r.start);
    out
}

/// 全文を覆う描画計画。区間は重ならず、隙間なく `[0, 文字数)` を覆う。
/// 重なる指定は後のもの（保留ローマ字）が優先。同じ種別が連続する区間は 1 つにまとめる。
/// 文字列が空なら空の計画
pub fn plan(preedit: &PreeditView) -> Vec<AttrRun> {
    let total = text_len(preedit);
    if total == 0 {
        return Vec::new();
    }
    let mut styles = vec![MarkStyle::Normal; total as usize];
    // runs は開始順に並べ替え済みだが、保留を後勝ちにするため元の優先度（セグメント → 保留）で塗る
    let mut ordered = runs(preedit);
    ordered.sort_by_key(|r| u8::from(r.style == MarkStyle::Pending));
    for r in ordered {
        for s in &mut styles[r.start as usize..r.end as usize] {
            *s = r.style;
        }
    }
    let mut out: Vec<AttrRun> = Vec::new();
    for (i, style) in styles.into_iter().enumerate() {
        let pos = i as u32;
        match out.last_mut() {
            Some(last) if last.style == style => last.end = pos + 1,
            _ => out.push(AttrRun {
                start: pos,
                end: pos + 1,
                style,
                attr: attr_for(style),
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::SegmentView;

    fn seg(surface: &str, kind: SegmentKind, start: u32) -> SegmentView {
        SegmentView {
            surface: surface.into(),
            reading: surface.into(),
            kind,
            range: RangeU16::new(start, start + surface.encode_utf16().count() as u32),
        }
    }

    fn preedit(segs: Vec<SegmentView>) -> PreeditView {
        PreeditView {
            text: segs.iter().map(|s| s.surface.as_str()).collect(),
            cursor: 0,
            segments: segs,
            pending: None,
            focus: None,
            selection: None,
        }
    }

    fn styles(p: &PreeditView) -> Vec<MarkStyle> {
        runs(p).iter().map(|r| r.style).collect()
    }

    #[test]
    fn every_style_maps_into_the_attr_table() {
        for style in [
            MarkStyle::Normal,
            MarkStyle::Focused,
            MarkStyle::Selected,
            MarkStyle::Fixed,
            MarkStyle::Styled,
            MarkStyle::Pending,
        ] {
            let attr = attr_for(style);
            assert_eq!(DISPLAY_ATTRS[attr_index(attr)], attr, "{style:?}");
        }
        // 重複が無い
        for (i, a) in DISPLAY_ATTRS.iter().enumerate() {
            assert_eq!(attr_index(*a), i);
        }
    }

    #[test]
    fn range_shifts_cover_the_run() {
        assert_eq!(range_shifts(10, 0, 10), (0, 0));
        assert_eq!(range_shifts(10, 2, 5), (2, -5));
        // 範囲外は丸める
        assert_eq!(range_shifts(4, 3, 99), (3, 0));
        assert_eq!(range_shifts(4, 9, 9), (4, 0));
    }

    #[test]
    fn plain_segments_are_normal() {
        let p = preedit(vec![
            seg("きょう", SegmentKind::Japanese, 0),
            seg("abc", SegmentKind::Literal, 3),
        ]);
        assert_eq!(styles(&p), [MarkStyle::Normal, MarkStyle::Normal]);
    }

    #[test]
    fn focus_selection_fixed_styled() {
        let mut p = preedit(vec![
            seg("あ", SegmentKind::Japanese, 0),
            seg("い", SegmentKind::Fixed, 1),
            seg("う", SegmentKind::Styled, 2),
            seg("え", SegmentKind::Japanese, 3),
        ]);
        assert_eq!(
            styles(&p),
            [
                MarkStyle::Normal,
                MarkStyle::Fixed,
                MarkStyle::Styled,
                MarkStyle::Normal
            ]
        );
        p.focus = Some(0);
        assert_eq!(styles(&p)[0], MarkStyle::Focused);
        p.focus = None;
        p.selection = Some(RangeU16::new(1, 3));
        assert_eq!(
            styles(&p),
            [
                MarkStyle::Normal,
                MarkStyle::Selected,
                MarkStyle::Selected,
                MarkStyle::Normal
            ]
        );
        // 注目は選択より優先
        p.focus = Some(1);
        assert_eq!(styles(&p)[1], MarkStyle::Focused);
    }

    #[test]
    fn pending_is_appended_and_sorted() {
        let mut p = preedit(vec![seg("かん", SegmentKind::Japanese, 0)]);
        p.text.push_str("ky");
        p.pending = Some(RangeU16::new(2, 4));
        let r = runs(&p);
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[1].start, r[1].end, r[1].style),
            (2, 4, MarkStyle::Pending)
        );
    }

    #[test]
    fn out_of_range_runs_are_clamped_or_dropped() {
        let mut p = preedit(vec![seg("あい", SegmentKind::Japanese, 0)]);
        p.segments[0].range = RangeU16::new(0, 99);
        p.pending = Some(RangeU16::new(50, 60));
        let r = runs(&p);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].end, 2);
    }

    #[test]
    fn plan_covers_text_without_gaps_and_merges() {
        let mut p = preedit(vec![
            seg("あ", SegmentKind::Japanese, 0),
            seg("い", SegmentKind::Japanese, 1),
            seg("う", SegmentKind::Japanese, 2),
        ]);
        p.text.push_str("ky");
        p.pending = Some(RangeU16::new(3, 5));
        p.focus = Some(1);
        let plan = plan(&p);
        let got: Vec<_> = plan.iter().map(|r| (r.start, r.end, r.style)).collect();
        assert_eq!(
            got,
            [
                (0, 1, MarkStyle::Normal),
                (1, 2, MarkStyle::Focused),
                (2, 3, MarkStyle::Normal),
                (3, 5, MarkStyle::Pending),
            ]
        );
        assert_eq!(
            plan[1].attr,
            DisplayAttr {
                underline: UnderlineKind::Solid,
                bold: true
            }
        );
    }

    #[test]
    fn plan_fills_uncovered_text_as_normal_and_merges_neighbours() {
        let p = PreeditView {
            text: "abcd".into(),
            ..PreeditView::default()
        };
        let plan = plan(&p);
        assert_eq!(plan.len(), 1);
        assert_eq!(
            (plan[0].start, plan[0].end, plan[0].style),
            (0, 4, MarkStyle::Normal)
        );
    }

    #[test]
    fn plan_uses_utf16_units() {
        let p = preedit(vec![
            seg("😀", SegmentKind::Japanese, 0),
            seg("あ", SegmentKind::Fixed, 2),
        ]);
        let plan = plan(&p);
        assert_eq!((plan[0].start, plan[0].end), (0, 2));
        assert_eq!(
            (plan[1].start, plan[1].end, plan[1].style),
            (2, 3, MarkStyle::Fixed)
        );
    }

    #[test]
    fn empty_text_has_empty_plan() {
        assert!(plan(&PreeditView::default()).is_empty());
    }

    #[test]
    fn attribute_mapping_matches_macos() {
        let a = |s| attr_for(s);
        assert_eq!(
            a(MarkStyle::Normal),
            DisplayAttr {
                underline: UnderlineKind::Solid,
                bold: false
            }
        );
        assert_eq!(a(MarkStyle::Selected), a(MarkStyle::Focused));
        assert!(a(MarkStyle::Focused).bold);
        assert_eq!(a(MarkStyle::Fixed).underline, UnderlineKind::Dot);
        assert_eq!(a(MarkStyle::Pending).underline, UnderlineKind::Dot);
        assert_eq!(a(MarkStyle::Styled).underline, UnderlineKind::Dash);
    }
}
