import Foundation

/// `initialize` で申告する capabilities（純粋関数。単体テストの対象）。
///
/// - `input_method`: タッチ入力（コアが最初のプロファイルを決めるのに使う）
/// - `partial_suggestions`: 入力中の候補バーが「読みの接頭辞だけを置き換える候補」（`CandidateView.covers`）を
///   選べる。選ぶと `select_candidate` でその分だけ確定して、残りの読みは入力中のまま続く（H-58）。
///   宣言したときだけコアが接頭辞の候補（読みの末尾を削った部分予測）を返す
func makeClientCapabilities(environment: SetEnvironmentParams?) -> ClientCapabilities {
    ClientCapabilities(input_method: .touch, environment: environment, partial_suggestions: true)
}
