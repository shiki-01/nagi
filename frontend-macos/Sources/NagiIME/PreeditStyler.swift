import Foundation
import NagiClient

/// マークテキスト上の下線スタイル種別（実際の属性への対応は AppKit 側）
public enum MarkStyle: Equatable {
    case normal
    /// Converting の注目文節
    case focused
    /// RangeSelecting の選択範囲内
    case selected
    /// 部分確定済み
    case fixed
    /// スタイル変換結果
    case styled
    /// 保留中のローマ字
    case pending
}

public struct StyleRun: Equatable {
    /// UTF-16 単位（NSAttributedString にそのまま使える）
    public let range: NSRange
    public let style: MarkStyle
}

/// `PreeditView` → 装飾区間。セグメントに覆われない部分は呼び出し側で `normal` を下地にする
public enum PreeditStyler {
    public static func runs(for preedit: PreeditView) -> [StyleRun] {
        let total = preedit.text.utf16.count
        func clamp(_ r: RangeU16) -> NSRange? {
            let s = min(Int(r.start), total)
            let e = min(Int(r.end), total)
            return e > s ? NSRange(location: s, length: e - s) : nil
        }
        var runs: [StyleRun] = []
        for (i, seg) in preedit.segments.enumerated() {
            guard let range = clamp(seg.range) else { continue }
            let style: MarkStyle
            if let focus = preedit.focus, Int(focus) == i {
                style = .focused
            } else if let sel = preedit.selection, seg.range.start >= sel.start, seg.range.end <= sel.end {
                style = .selected
            } else {
                switch seg.kind {
                case .fixed: style = .fixed
                case .styled: style = .styled
                case .japanese, .literal: style = .normal
                }
            }
            runs.append(StyleRun(range: range, style: style))
        }
        if let pending = preedit.pending, let range = clamp(pending) {
            runs.append(StyleRun(range: range, style: .pending))
        }
        return runs.sorted { $0.range.location < $1.range.location }
    }
}
