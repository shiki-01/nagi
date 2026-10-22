//! セッションごとの最新の preedit を保持する（設計書 §3.3「入力を失わない」）。
//!
//! エンジン再起動時の `restore_session` の材料と、復元失敗時の「表示中の文字列の確定」に使う。
//! macOS 版 `SessionStore.swift` / `UTF16.swift` の `RestoreSessionParams.make` と同じ責務。
//! [`SessionStore::restore_params`] を
//! [`EngineSupervisor::set_restore_provider`](crate::supervisor::EngineSupervisor::set_restore_provider)
//! に渡す。

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use nagi_protocol::{PreeditView, RestoreSegment, RestoreSessionParams};

/// UTF-16 位置 → Unicode スカラー数の換算。
/// `utf16_offset` までのスカラー数（サロゲートペアの途中は切り下げ）
fn scalar_count_in_prefix(s: &str, utf16_offset: u32) -> usize {
    let limit = utf16_offset as usize;
    let mut units = 0;
    let mut count = 0;
    for c in s.chars() {
        let w = c.len_utf16();
        if units + w > limit {
            break;
        }
        units += w;
        count += 1;
    }
    count
}

/// フロントが保持している最新の preedit から `restore_session` の引数を作る（設計書 §3.3）。
///
/// `cursor` は `PreeditView.cursor`（text 上の UTF-16）を、segments の reading 連結上の
/// Unicode スカラー位置に換算する。カーソルが文節内部にある場合は、その文節の surface 内の
/// 位置をスカラー数で数え reading の長さに丸める（Composing では surface == reading）。
/// 保留ローマ字（segments に含まれない部分）に掛かる位置は、直前の文節の末尾に丸める。
pub fn make_restore_params(
    session_id: &str,
    preedit: &PreeditView,
    converting: bool,
) -> RestoreSessionParams {
    let segments = preedit
        .segments
        .iter()
        .map(|s| RestoreSegment {
            reading: s.reading.clone(),
            surface: s.surface.clone(),
            kind: s.kind,
        })
        .collect();
    let mut scalars = 0usize;
    for seg in &preedit.segments {
        let reading_count = seg.reading.chars().count();
        if preedit.cursor >= seg.range.end {
            scalars += reading_count;
        } else if preedit.cursor > seg.range.start {
            let inner = preedit.cursor - seg.range.start;
            scalars += scalar_count_in_prefix(&seg.surface, inner).min(reading_count);
            break;
        } else {
            break;
        }
    }
    RestoreSessionParams {
        session_id: session_id.to_string(),
        segments,
        cursor: u32::try_from(scalars).unwrap_or(u32::MAX),
        focus: preedit.focus,
        converting,
    }
}

/// セッション ID → 最新の preedit。スレッドセーフ（エンジンの通知スレッドから参照される）
#[derive(Debug, Default)]
pub struct SessionStore {
    preedits: Mutex<HashMap<String, PreeditView>>,
}

impl SessionStore {
    /// 空のストアを作る
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, PreeditView>> {
        self.preedits.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 最新の preedit を記録する。`None` と空の preedit（text が空）は「未確定なし」として削除する
    pub fn update(&self, session_id: &str, preedit: Option<&PreeditView>) {
        let mut map = self.lock();
        match preedit {
            Some(p) if !p.text.is_empty() => {
                map.insert(session_id.to_string(), p.clone());
            }
            _ => {
                map.remove(session_id);
            }
        }
    }

    /// 保持している preedit
    pub fn preedit(&self, session_id: &str) -> Option<PreeditView> {
        self.lock().get(session_id).cloned()
    }

    /// セッションを削除する
    pub fn remove(&self, session_id: &str) {
        self.update(session_id, None);
    }

    /// `EngineSupervisor::set_restore_provider` に渡す。
    /// segments が空のもの（保留ローマ字のみ）は復元対象外。順序は session_id 昇順（決定的にするため）
    pub fn restore_params(&self) -> Vec<RestoreSessionParams> {
        let map = self.lock();
        let mut out: Vec<RestoreSessionParams> = map
            .iter()
            .filter(|(_, p)| !p.segments.is_empty())
            .map(|(id, p)| make_restore_params(id, p, p.focus.is_some()))
            .collect();
        out.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::{RangeU16, SegmentKind, SegmentView};

    fn seg(surface: &str, reading: &str, start: u32) -> SegmentView {
        let len = surface.encode_utf16().count() as u32;
        SegmentView {
            surface: surface.into(),
            reading: reading.into(),
            kind: SegmentKind::Japanese,
            range: RangeU16::new(start, start + len),
        }
    }

    fn preedit(segs: Vec<SegmentView>, cursor: u32, focus: Option<u32>) -> PreeditView {
        let text: String = segs.iter().map(|s| s.surface.as_str()).collect();
        PreeditView {
            text,
            segments: segs,
            cursor,
            focus,
            ..Default::default()
        }
    }

    #[test]
    fn update_get_remove() {
        let store = SessionStore::new();
        let p = preedit(vec![seg("かな", "かな", 0)], 2, None);
        store.update("a", Some(&p));
        assert_eq!(store.preedit("a"), Some(p));
        store.remove("a");
        assert_eq!(store.preedit("a"), None);
    }

    #[test]
    fn empty_preedit_and_none_delete() {
        let store = SessionStore::new();
        store.update("a", Some(&preedit(vec![seg("あ", "あ", 0)], 1, None)));
        store.update("a", Some(&PreeditView::default()));
        assert_eq!(store.preedit("a"), None);
        store.update("a", Some(&preedit(vec![seg("あ", "あ", 0)], 1, None)));
        store.update("a", None);
        assert_eq!(store.preedit("a"), None);
    }

    #[test]
    fn restore_params_skip_pending_only_and_sort() {
        let store = SessionStore::new();
        store.update("b", Some(&preedit(vec![seg("い", "い", 0)], 1, None)));
        store.update("a", Some(&preedit(vec![seg("あ", "あ", 0)], 1, Some(0))));
        // 保留ローマ字のみ（segments 空、text あり）
        store.update(
            "c",
            Some(&PreeditView {
                text: "k".into(),
                ..Default::default()
            }),
        );
        let params = store.restore_params();
        let ids: Vec<_> = params.iter().map(|p| p.session_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(params[0].converting);
        assert_eq!(params[0].focus, Some(0));
        assert!(!params[1].converting);
    }

    #[test]
    fn cursor_at_end_counts_reading_scalars() {
        let p = preedit(
            vec![seg("今日", "きょう", 0), seg("は", "は", 2)],
            3,
            Some(0),
        );
        let r = make_restore_params("s", &p, true);
        assert_eq!(r.cursor, 4);
        assert_eq!(r.segments.len(), 2);
        assert_eq!(r.segments[0].reading, "きょう");
    }

    #[test]
    fn cursor_inside_segment_uses_surface_scalars_clamped() {
        // Composing: surface == reading。「あいう」の 2 文字目の後
        let p = preedit(vec![seg("あいう", "あいう", 0)], 2, None);
        assert_eq!(make_restore_params("s", &p, false).cursor, 2);
        // 変換済み文節の内部は reading 長に丸める
        let p = preedit(vec![seg("今日今日今日", "き", 0)], 5, None);
        assert_eq!(make_restore_params("s", &p, false).cursor, 1);
    }

    #[test]
    fn cursor_with_surrogate_pairs_counts_scalars() {
        // 𠮷（U+20BB7）は UTF-16 で 2 単位。「𠮷あ」の先頭直後は 1 スカラー
        let p = preedit(vec![seg("𠮷あ", "𠮷あ", 0)], 2, None);
        assert_eq!(make_restore_params("s", &p, false).cursor, 1);
        // サロゲートペアの途中は切り下げ
        let p = preedit(vec![seg("𠮷あ", "𠮷あ", 0)], 1, None);
        assert_eq!(make_restore_params("s", &p, false).cursor, 0);
        let p = preedit(vec![seg("𠮷あ", "𠮷あ", 0)], 3, None);
        assert_eq!(make_restore_params("s", &p, false).cursor, 2);
    }

    #[test]
    fn cursor_in_pending_rounds_to_previous_segment_end() {
        // text = "あk"、segments は「あ」のみ、カーソルは保留の後（2）
        let mut p = preedit(vec![seg("あ", "あ", 0)], 2, None);
        p.text = "あk".into();
        assert_eq!(make_restore_params("s", &p, false).cursor, 1);
    }

    #[test]
    fn works_as_restore_provider() {
        use std::sync::Arc;
        let store = Arc::new(SessionStore::new());
        store.update("a", Some(&preedit(vec![seg("あ", "あ", 0)], 1, None)));
        let s2 = Arc::clone(&store);
        let provider = move || s2.restore_params();
        assert_eq!(provider().len(), 1);
    }
}
