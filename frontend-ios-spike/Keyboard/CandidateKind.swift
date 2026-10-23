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

/// 曖昧入力（タイプミス補正）の候補に付ける「もしかして」の表示。出所に `fuzzy` を含まない候補は nil。
/// 補正後の読み（`CandidateView.reading`）があれば「もしかして: かんじ」の形にする。
/// 候補の種類の判定（`classifyCandidates`）には影響しない（補正候補は入力中の変換候補として扱う）
func fuzzyHint(sources: [CandidateSource], reading: String?) -> String? {
    guard sources.contains(.fuzzy) else { return nil }
    guard let reading = reading, !reading.isEmpty else { return "もしかして" }
    return "もしかして: \(reading)"
}

/// 候補バーの作り直しの鍵（純粋関数）。候補の一覧が入れ替わったら変わる（先頭へスクロールを戻すため）。
/// 件数と先頭の表記が同じなら同じ鍵（選択位置の変化など、一覧そのものが同じ更新では戻さない）
func candidateBarIdentity(_ surfaces: [String]) -> String {
    "\(surfaces.count)|\(surfaces.first ?? "")|\(surfaces.last ?? "")"
}
