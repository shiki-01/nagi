import Foundation

/// UTF-16 コード単位 ↔ Unicode スカラー単位の変換。
///
/// プロトコルの位置（cursor / range / selection）は UTF-16 コード単位。
/// Swift の `String.count`（拡張書記素クラスタ）や `unicodeScalars.count` とは異なるため、
/// 位置を扱うときは必ずここを経由する。ただし `RestoreSessionParams.cursor` だけは
/// 「reading 連結上の Unicode スカラー単位」（Protocol.swift のコメント参照）。
public enum UTF16Position {
    /// UTF-16 コード単位での長さ
    public static func length(_ s: String) -> UInt32 {
        UInt32(s.utf16.count)
    }

    /// UTF-16 offset までのスカラー数。サロゲートペアの途中を指す場合はそのスカラーを含めない（切り下げ）
    public static func scalarCount(inPrefixOf s: String, utf16Offset: UInt32) -> Int {
        var units = 0
        var count = 0
        let limit = Int(utf16Offset)
        for scalar in s.unicodeScalars {
            let width = scalar.value > 0xFFFF ? 2 : 1
            if units + width > limit { break }
            units += width
            count += 1
        }
        return count
    }

    /// 先頭から `scalarCount` 個のスカラーの UTF-16 長（文字列より多ければ全長）
    public static func utf16Offset(ofScalarCount scalarCount: Int, in s: String) -> UInt32 {
        var units = 0
        for (i, scalar) in s.unicodeScalars.enumerated() {
            if i >= scalarCount { break }
            units += scalar.value > 0xFFFF ? 2 : 1
        }
        return UInt32(units)
    }

    /// UTF-16 範囲 `[start, end)` の部分文字列。範囲外・サロゲート途中は nil
    public static func substring(of s: String, range: RangeU16) -> String? {
        let utf16 = s.utf16
        guard range.start <= range.end, Int(range.end) <= utf16.count,
            let from = utf16.index(utf16.startIndex, offsetBy: Int(range.start), limitedBy: utf16.endIndex),
            let to = utf16.index(utf16.startIndex, offsetBy: Int(range.end), limitedBy: utf16.endIndex),
            let f = from.samePosition(in: s), let t = to.samePosition(in: s)
        else { return nil }
        return String(s[f..<t])
    }
}

extension RestoreSessionParams {
    /// フロントが保持している最新の preedit から `restore_session` の引数を作る（設計書 §3.3）。
    ///
    /// - `cursor` は `PreeditView.cursor`（text 上の UTF-16）を、segments の reading 連結上の
    ///   Unicode スカラー位置に換算する。カーソルが文節内部にある場合は、その文節の surface 内の
    ///   位置をスカラー数で数え reading の長さに丸める（Composing では surface == reading）。
    ///   保留ローマ字（segments に含まれない部分）に掛かる位置は、直前の文節の末尾に丸める。
    public static func make(
        sessionId: String, preedit: PreeditView, converting: Bool
    ) -> RestoreSessionParams {
        let segments = preedit.segments.map {
            RestoreSegment(reading: $0.reading, surface: $0.surface, kind: $0.kind)
        }
        var scalars = 0
        for seg in preedit.segments {
            let readingCount = seg.reading.unicodeScalars.count
            if preedit.cursor >= seg.range.end {
                scalars += readingCount
            } else if preedit.cursor > seg.range.start {
                let inner = preedit.cursor - seg.range.start
                scalars += min(
                    UTF16Position.scalarCount(inPrefixOf: seg.surface, utf16Offset: inner), readingCount)
                break
            } else {
                break
            }
        }
        return RestoreSessionParams(
            session_id: sessionId, segments: segments, cursor: UInt32(scalars),
            focus: preedit.focus, converting: converting)
    }
}
