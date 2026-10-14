import Foundation

/// コアから届いた候補の種類
enum CandidateKind: Equatable {
    /// 候補なし（空、または `visible = false`）
    case hidden
    /// 入力中（変換前）・変換中の候補
    case conversion
    /// 確定直後の次語予測（`predict.next_word`。Direct 状態で届く。選ぶとそのまま挿入される）
    case nextWord
}

/// 候補の差分の振り分け（純粋関数。単体テストの対象）。
///
/// コアの仕様（`docs/decisions.md` H-19）: 次語候補は入力中の読みが空の Direct 状態で、`visible = true`、
/// 出所が `learned` のみ、`covers` なしで届く。入力が始まる・確定のない操作が来ると `visible = false` で消える。
/// 入力中の読みがあるときに届いた候補は、出所にかかわらず通常の候補として扱う
/// （古い次語候補の通知が入力中に遅れて届いても、次語候補として出さない）
func classifyCandidates(_ view: CandidatesView, hasPreedit: Bool) -> CandidateKind {
    guard view.visible, !view.items.isEmpty else { return .hidden }
    if hasPreedit { return .conversion }
    let allNextWord = view.items.allSatisfy { $0.covers == nil && $0.sources == [.learned] }
    return allNextWord ? .nextWord : .conversion
}
