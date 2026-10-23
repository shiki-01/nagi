import Foundation

/// 入力欄（セッション）ごとに、画面へ反映済みの revision を覚えて、古い更新を捨てる。
///
/// `EngineSupervisor` の `RevisionGate` は通知の受信時に古さを確かめるが、その後メインスレッドへ渡してから
/// 実際に反映するまでの間に、キー処理の同期応答（確定など）が先に反映されることがある。
/// その場合、確定より前に計算された `update_preedit` / `update_candidates` が確定のあとに反映され、
/// 確定した文字が入力中の文字として復活する（取り残し）。反映する直前にここで確かめて防ぐ
public struct AppliedRevision: Equatable {
    /// 反映済みの同期応答の revision
    public private(set) var sync: UInt32 = 0
    /// 反映済みの非同期更新の revision
    public private(set) var async: UInt32 = 0

    public init() {}

    /// 同期応答（描画差分）を反映した。巻き戻さない
    public mutating func applied(sync revision: UInt32) {
        sync = max(sync, revision)
    }

    /// 非同期通知を反映してよいか。反映済みの同期応答以前のものと、反映済みの非同期更新より古いものは false。
    /// コアはプリエディットと候補を同じ revision で続けて送ることがあるので、非同期どうしは同じ値を許す
    public mutating func accept(async revision: UInt32) -> Bool {
        guard revision > sync, revision >= async else { return false }
        async = revision
        return true
    }

    /// エンジンの再起動（revision が 0 から始まり直す）
    public mutating func reset() {
        sync = 0
        async = 0
    }
}
