//! 確定済みテキストの再変換の入口の判定と、`ITfFnReconversion::GetReconversion` の候補（OS 非依存）。
//! 入口の判定は macOS 版 `reconvertSelection` と同じ規約。

use nagi_protocol::ProcessKeyResult;

/// 再変換の対象にする選択の最大長（UTF-16 コード単位。macOS 版の `reconvertMaxLength` と同じ）
pub const RECONVERT_MAX_UTF16: usize = 200;

/// 再変換にかけるテキスト。空・長すぎる選択は対象外（何もしない）
pub fn eligible_text(text: &str) -> Option<&str> {
    if text.is_empty() {
        return None;
    }
    let len: usize = text.chars().map(char::len_utf16).sum();
    (len <= RECONVERT_MAX_UTF16).then_some(text)
}

/// 取得時に「最大長 + 1」だけ読めば長すぎるかを判定できる。読む範囲の上限（UTF-16）
pub const RECONVERT_READ_LIMIT: usize = RECONVERT_MAX_UTF16 + 1;

/// `GetReconversion` が返す候補の最大数（候補一覧を出すアプリの表示に収まる程度。実測なし）
pub const MAX_RANGE_CANDIDATES: usize = 30;

/// 再変換の応答から、**範囲全体を置き換える**候補文字列を作る。
///
/// `ITfFnReconversion` の候補は対象の範囲全体の代わりになる文字列だが、コアの候補は注目文節
/// （`preedit.focus`）の代わりなので、そのまま渡すと複数文節のとき範囲の残りを失う。
/// そこで、先頭に変換結果の全文を置き、続けて「注目文節だけを各候補に替えた全文」を並べる。
/// 重複と空は除く。応答に preedit が無ければ空。
pub fn range_candidates(result: &ProcessKeyResult) -> Vec<String> {
    let Some(preedit) = &result.preedit else {
        return Vec::new();
    };
    let text = &preedit.text;
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !s.is_empty() && out.len() < MAX_RANGE_CANDIDATES && !out.contains(&s) {
            out.push(s);
        }
    };
    push(text.clone());
    let focused = preedit
        .focus
        .and_then(|i| preedit.segments.get(i as usize))
        .map(|seg| seg.range);
    if let (Some(range), Some(view)) = (focused, &result.candidates) {
        let units: Vec<u16> = text.encode_utf16().collect();
        let (start, end) = (range.start as usize, range.end as usize);
        if start <= end && end <= units.len() {
            for item in &view.items {
                let mut joined = units[..start].to_vec();
                joined.extend(item.surface.encode_utf16());
                joined.extend_from_slice(&units[end..]);
                // 分割位置がサロゲートペアの途中にあるなど不正な場合は捨てる
                if let Ok(s) = String::from_utf16(&joined) {
                    push(s);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::{
        CandidateSource, CandidateView, CandidatesView, PreeditView, RangeU16, SegmentKind,
        SegmentView,
    };

    fn segment(surface: &str, start: u32, end: u32) -> SegmentView {
        SegmentView {
            surface: surface.into(),
            reading: String::new(),
            kind: SegmentKind::Japanese,
            range: RangeU16::new(start, end),
        }
    }

    fn item(surface: &str) -> CandidateView {
        CandidateView {
            surface: surface.into(),
            sources: vec![CandidateSource::SystemDict],
            covers: None,
            reading: None,
        }
    }

    fn result(
        text: &str,
        segments: Vec<SegmentView>,
        focus: Option<u32>,
        items: &[&str],
    ) -> ProcessKeyResult {
        ProcessKeyResult {
            preedit: Some(PreeditView {
                text: text.into(),
                segments,
                focus,
                ..Default::default()
            }),
            candidates: Some(CandidatesView {
                items: items.iter().map(|s| item(s)).collect(),
                visible: true,
                page_size: 9,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn candidates_replace_only_the_focused_segment_and_keep_the_rest() {
        // 「今日は」「晴れ」の先頭文節の候補を、範囲全体の文字列にする
        let r = result(
            "今日は晴れ",
            vec![segment("今日は", 0, 3), segment("晴れ", 3, 5)],
            Some(0),
            &["今日は", "京は", "強は"],
        );
        assert_eq!(range_candidates(&r), ["今日は晴れ", "京は晴れ", "強は晴れ"]);
    }

    #[test]
    fn focus_on_a_later_segment_and_surrogate_pairs_use_utf16_offsets() {
        // 絵文字（UTF-16 で 2 単位）の後ろの文節
        let r = result(
            "😀はれ",
            vec![segment("😀", 0, 2), segment("はれ", 2, 4)],
            Some(1),
            &["晴れ"],
        );
        assert_eq!(range_candidates(&r), ["😀はれ", "😀晴れ"]);
    }

    #[test]
    fn no_preedit_or_no_focus_degrades_safely() {
        assert!(range_candidates(&ProcessKeyResult::default()).is_empty());
        let r = result("今日", vec![segment("今日", 0, 2)], None, &["強"]);
        assert_eq!(range_candidates(&r), ["今日"]);
        // 範囲外の segment は捨てる（候補で壊れた文字列を作らない）
        let r = result("今日", vec![segment("今日", 0, 9)], Some(0), &["強"]);
        assert_eq!(range_candidates(&r), ["今日"]);
        // 分割位置がサロゲートペアの途中
        let r = result("😀", vec![segment("x", 0, 1)], Some(0), &["a"]);
        assert_eq!(range_candidates(&r), ["😀"]);
    }

    #[test]
    fn candidates_are_deduplicated_and_capped() {
        let items: Vec<String> = (0..50).map(|i| format!("候補{i}")).collect();
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        let r = result("あ", vec![segment("あ", 0, 1)], Some(0), &refs);
        let got = range_candidates(&r);
        assert_eq!(got.len(), MAX_RANGE_CANDIDATES);
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), got.len());
        // 全文と同じ候補は重複させない
        let r = result("あ", vec![segment("あ", 0, 1)], Some(0), &["あ", "亜"]);
        assert_eq!(range_candidates(&r), ["あ", "亜"]);
    }

    #[test]
    fn empty_and_too_long_are_not_eligible() {
        assert_eq!(eligible_text(""), None);
        assert_eq!(eligible_text("今日"), Some("今日"));
        let max = "あ".repeat(RECONVERT_MAX_UTF16);
        assert!(eligible_text(&max).is_some());
        assert!(eligible_text(&format!("{max}あ")).is_none());
    }

    #[test]
    fn counts_utf16_units_not_chars() {
        // サロゲートペアは 2 単位
        let s = "😀".repeat(RECONVERT_MAX_UTF16 / 2);
        assert!(eligible_text(&s).is_some());
        assert!(eligible_text(&format!("{s}😀")).is_none());
    }
}
