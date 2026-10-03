import Foundation
import NagiClient

public struct CandidateRow: Equatable {
    /// ページ内の番号（1 始まり。数字キー選択の表示用）
    public let number: Int
    public let surface: String
    /// 出所ラベル（例: 「学習」「AI」）
    public let label: String
    public let isSelected: Bool
}

public struct CandidatePage: Equatable {
    public let rows: [CandidateRow]
    /// 0 始まり
    public let pageIndex: Int
    public let pageCount: Int
    public let total: Int
}

public enum CandidatePaging {
    /// 出所アノテーションの表示名。アイコンの割り当てはフロントの責務（ここでは短い文字ラベル）
    public static func label(for source: CandidateSource) -> String {
        switch source {
        case .learned: return "学習"
        case .userDict: return "ユーザー"
        case .llm: return "AI"
        case .systemDict: return "辞書"
        case .kana: return "かな"
        case .rewriter: return "変換"
        case .corrected: return "補正"
        }
    }

    public static func label(for sources: [CandidateSource]) -> String {
        var seen: [String] = []
        for s in sources {
            let l = label(for: s)
            if !seen.contains(l) { seen.append(l) }
        }
        return seen.joined(separator: "・")
    }

    /// 非表示・候補なしは nil（ウィンドウを閉じる）
    public static func page(for view: CandidatesView) -> CandidatePage? {
        guard view.visible, !view.items.isEmpty else { return nil }
        let total = view.items.count
        let size = max(Int(view.page_size), 1)
        let selected = min(Int(view.selected), total - 1)
        let pageIndex = selected / size
        let start = pageIndex * size
        let end = min(start + size, total)
        let rows = (start..<end).map { i in
            CandidateRow(
                number: i - start + 1, surface: view.items[i].surface,
                label: label(for: view.items[i].sources), isSelected: i == selected)
        }
        return CandidatePage(
            rows: rows, pageIndex: pageIndex, pageCount: (total + size - 1) / size, total: total)
    }
}
