//! エンジンの応答（描画差分）を TSF の編集操作に変換する、OS 非依存の部分。
//!
//! ロジックは持たず変換のみ（CLAUDE.md の分離の原則）。何を確定・表示するかはエンジンが決め、
//! ここは「応答のどのフィールドをどの順で TSF に反映するか」だけを表す。
//! COM を触る側（`edit` モジュール）はこの [`EditPlan`] を実行するだけにして、Linux でもテストできるようにする。

use nagi_protocol::{PreeditView, ProcessKeyResult};

/// 表示中の preedit の更新
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreeditUpdate {
    /// 変化なし（応答に `preedit` が無い）
    Keep,
    /// 空にして変換を終える
    Clear,
    /// この内容に置き換える
    Set {
        /// 表示文字列全体（保留ローマ字を含む）
        text: String,
        /// text 上のカーソル位置（UTF-16 コード単位。text の長さに丸め済み）
        cursor: u32,
        /// 装飾の範囲（UTF-16）。表示属性（下線など）の割り当てに使う
        spans: Vec<DisplaySpan>,
    },
}

/// 装飾の種別。実際の表示属性（下線の太さ・色）へは TSF 層が割り当てる
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStyle {
    /// 入力中・未変換（細い点線の下線）
    Input,
    /// 変換済みの文節（細い実線の下線）
    Converted,
    /// 注目文節（太い下線）
    Focused,
}

/// 装飾する範囲（UTF-16 コード単位、半開区間）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySpan {
    /// 開始
    pub start: u32,
    /// 終了
    pub end: u32,
    /// 種別
    pub style: SpanStyle,
}

/// 1 回の応答で TSF に行う編集。適用順は「直前の確定の削除 → 確定 → preedit の更新」
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPlan {
    /// ホストアプリのテキストから削除する直前の確定文字列の長さ（UTF-16）。確定取消
    pub delete_before: Option<u32>,
    /// 確定する文字列。現在の composition があればそれを確定文字列に置き換えて終了する
    pub commit: Option<String>,
    /// preedit の更新
    pub preedit: PreeditUpdate,
}

impl EditPlan {
    /// 何も反映しなくてよいか
    pub fn is_empty(&self) -> bool {
        self.delete_before.is_none()
            && self.commit.as_deref().is_none_or(str::is_empty)
            && self.preedit == PreeditUpdate::Keep
    }
}

/// 文字列の UTF-16 コード単位列
pub fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn spans_for(p: &PreeditView) -> Vec<DisplaySpan> {
    let len = u32::try_from(p.text.encode_utf16().count()).unwrap_or(u32::MAX);
    let mut spans: Vec<DisplaySpan> = Vec::new();
    for (i, seg) in p.segments.iter().enumerate() {
        let style = match p.focus {
            Some(f) if usize::try_from(f) == Ok(i) => SpanStyle::Focused,
            Some(_) => SpanStyle::Converted,
            None => SpanStyle::Input,
        };
        let (start, end) = (seg.range.start.min(len), seg.range.end.min(len));
        if start < end {
            spans.push(DisplaySpan { start, end, style });
        }
    }
    // 保留ローマ字（segments の隙間）は入力中の装飾にする
    if let Some(pending) = p.pending {
        let (start, end) = (pending.start.min(len), pending.end.min(len));
        if start < end {
            spans.push(DisplaySpan {
                start,
                end,
                style: SpanStyle::Input,
            });
        }
    }
    spans.sort_by_key(|s| s.start);
    spans
}

/// 応答から編集計画を作る。`candidates` / `mode` / `reconvert_selection` はここでは扱わない
/// （候補ウィンドウ・言語バー・`ITfFnReconversion` は未実装。`lib.rs` の TODO 参照）
pub fn plan_from_result(result: &ProcessKeyResult) -> EditPlan {
    EditPlan {
        delete_before: result.delete_before.filter(|n| *n > 0),
        commit: result.commit.clone().filter(|c| !c.is_empty()),
        preedit: preedit_update(result.preedit.as_ref()),
    }
}

/// 非同期通知 `update_preedit` の preedit だけを反映する計画
pub fn plan_from_preedit(preedit: &PreeditView) -> EditPlan {
    EditPlan {
        delete_before: None,
        commit: None,
        preedit: preedit_update(Some(preedit)),
    }
}

fn preedit_update(preedit: Option<&PreeditView>) -> PreeditUpdate {
    match preedit {
        None => PreeditUpdate::Keep,
        Some(p) if p.text.is_empty() => PreeditUpdate::Clear,
        Some(p) => {
            let len = u32::try_from(p.text.encode_utf16().count()).unwrap_or(u32::MAX);
            PreeditUpdate::Set {
                text: p.text.clone(),
                cursor: p.cursor.min(len),
                spans: spans_for(p),
            }
        }
    }
}

/// `OnTestKeyDown` で呼んだエンジンの応答を、直後の `OnKeyDown` に引き渡すための 1 件キャッシュ。
///
/// `process_key` は状態を進めるので、同じキーで 2 回呼べない。TSF は
/// `OnTestKeyDown`（食うか）→ `OnKeyDown`（実行）の順に同じキーで呼ぶため、前者で結果を取って後者で使う。
/// 食い違う（別のキーが来た）場合、古い結果は捨てる。
#[derive(Debug)]
pub struct HandoffCache<T> {
    slot: Option<(u64, T)>,
}

impl<T> Default for HandoffCache<T> {
    fn default() -> Self {
        Self { slot: None }
    }
}

impl<T> HandoffCache<T> {
    /// 空のキャッシュ
    pub fn new() -> Self {
        Self::default()
    }

    /// キー識別子 `id` の結果を保存する（既にあれば置き換える）
    pub fn store(&mut self, id: u64, value: T) {
        self.slot = Some((id, value));
    }

    /// `id` に一致する結果を取り出す。一致しなければ古い結果は捨てて `None`
    pub fn take(&mut self, id: u64) -> Option<T> {
        match self.slot.take() {
            Some((stored, v)) if stored == id => Some(v),
            _ => None,
        }
    }

    /// 破棄する（フォーカス移動・リセット時）
    pub fn clear(&mut self) {
        self.slot = None;
    }
}

/// キー識別子。仮想キーと lParam（リピート回数・スキャンコード・フラグ）から作る
pub fn key_id(vk: u16, lparam: isize) -> u64 {
    (u64::from(vk) << 48) ^ (lparam as u64 & 0x0000_FFFF_FFFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::{RangeU16, SegmentKind, SegmentView};

    fn seg(surface: &str, start: u32) -> SegmentView {
        let end = start + surface.encode_utf16().count() as u32;
        SegmentView {
            surface: surface.into(),
            reading: surface.into(),
            kind: SegmentKind::Japanese,
            range: RangeU16::new(start, end),
        }
    }

    fn preedit(segs: Vec<SegmentView>, cursor: u32, focus: Option<u32>) -> PreeditView {
        PreeditView {
            text: segs.iter().map(|s| s.surface.as_str()).collect(),
            segments: segs,
            cursor,
            focus,
            ..Default::default()
        }
    }

    #[test]
    fn passthrough_is_empty_plan() {
        let plan = plan_from_result(&ProcessKeyResult::passthrough(1));
        assert!(plan.is_empty());
        assert_eq!(plan.preedit, PreeditUpdate::Keep);
    }

    #[test]
    fn commit_only() {
        let r = ProcessKeyResult {
            consumed: true,
            commit: Some("確定".into()),
            ..Default::default()
        };
        let plan = plan_from_result(&r);
        assert_eq!(plan.commit.as_deref(), Some("確定"));
        assert!(!plan.is_empty());
    }

    #[test]
    fn empty_commit_is_ignored() {
        let r = ProcessKeyResult {
            consumed: true,
            commit: Some(String::new()),
            ..Default::default()
        };
        assert!(plan_from_result(&r).is_empty());
    }

    #[test]
    fn empty_preedit_clears() {
        let r = ProcessKeyResult {
            consumed: true,
            preedit: Some(PreeditView::default()),
            ..Default::default()
        };
        assert_eq!(plan_from_result(&r).preedit, PreeditUpdate::Clear);
    }

    #[test]
    fn commit_and_new_preedit_in_one_response() {
        let r = ProcessKeyResult {
            consumed: true,
            commit: Some("今日".into()),
            preedit: Some(preedit(vec![seg("は", 0)], 1, None)),
            ..Default::default()
        };
        let plan = plan_from_result(&r);
        assert_eq!(plan.commit.as_deref(), Some("今日"));
        assert!(
            matches!(plan.preedit, PreeditUpdate::Set { ref text, cursor: 1, .. } if text == "は")
        );
    }

    #[test]
    fn cursor_is_clamped_to_text_length() {
        let r = ProcessKeyResult {
            consumed: true,
            preedit: Some(preedit(vec![seg("あ", 0)], 99, None)),
            ..Default::default()
        };
        let PreeditUpdate::Set { cursor, .. } = plan_from_result(&r).preedit else {
            panic!("Set expected");
        };
        assert_eq!(cursor, 1);
    }

    #[test]
    fn spans_follow_focus() {
        let p = preedit(
            vec![seg("今日", 0), seg("は", 2), seg("晴れ", 3)],
            5,
            Some(1),
        );
        let spans = spans_for(&p);
        let styles: Vec<_> = spans.iter().map(|s| s.style).collect();
        assert_eq!(
            styles,
            [
                SpanStyle::Converted,
                SpanStyle::Focused,
                SpanStyle::Converted
            ]
        );
        assert_eq!((spans[1].start, spans[1].end), (2, 3));
    }

    #[test]
    fn spans_without_focus_are_input_and_pending_is_included() {
        let mut p = preedit(vec![seg("あ", 0)], 2, None);
        p.text = "あk".into();
        p.pending = Some(RangeU16::new(1, 2));
        let spans = spans_for(&p);
        assert_eq!(spans.len(), 2);
        assert!(spans.iter().all(|s| s.style == SpanStyle::Input));
    }

    #[test]
    fn utf16_spans_count_surrogate_pairs() {
        let p = preedit(vec![seg("𠮷", 0), seg("あ", 2)], 3, Some(1));
        let spans = spans_for(&p);
        assert_eq!((spans[0].start, spans[0].end), (0, 2));
        assert_eq!((spans[1].start, spans[1].end), (2, 3));
    }

    #[test]
    fn async_update_plan_only_touches_preedit() {
        let plan = plan_from_preedit(&preedit(vec![seg("あ", 0)], 1, None));
        assert!(plan.commit.is_none() && plan.delete_before.is_none());
        assert!(matches!(plan.preedit, PreeditUpdate::Set { .. }));
    }

    #[test]
    fn delete_before_zero_is_ignored() {
        let r = ProcessKeyResult {
            consumed: true,
            delete_before: Some(0),
            ..Default::default()
        };
        assert!(plan_from_result(&r).is_empty());
        let r = ProcessKeyResult {
            consumed: true,
            delete_before: Some(3),
            ..Default::default()
        };
        assert_eq!(plan_from_result(&r).delete_before, Some(3));
    }

    #[test]
    fn handoff_cache_matches_by_id_and_drops_stale() {
        let mut c = HandoffCache::new();
        c.store(key_id(0x41, 0x1E0001), "a");
        assert_eq!(c.take(key_id(0x41, 0x1E0001)), Some("a"));
        assert_eq!(c.take(key_id(0x41, 0x1E0001)), None);
        c.store(key_id(0x41, 1), "a");
        assert_eq!(c.take(key_id(0x42, 1)), None);
        assert_eq!(c.take(key_id(0x41, 1)), None, "stale entry is dropped");
    }

    #[test]
    fn key_id_distinguishes_vk_and_lparam() {
        assert_ne!(key_id(0x41, 1), key_id(0x42, 1));
        assert_ne!(key_id(0x41, 1), key_id(0x41, 2));
    }
}
