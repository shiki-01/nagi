import Foundation
import Darwin

/// コアの学習ファイルの書き出し・読み込み（`core/learn`。`<path>.lock` を `flock(LOCK_EX)`）と同じ
/// ロックを、Swift 側から取るための補助。キーボード拡張とホストアプリが同じ App Group の
/// 学習ファイルを触るとき、ホストが直接ファイルを操作する場合にだけ使う（docs/mobile-roadmap.md D-5）
enum NagiFileLock {
    /// `lockPath` の排他ロックを取って `body` を実行する。ロックを取れなければ `body` は実行せず false。
    /// ロックファイルは消さない（コアは使い終わりに消すが、使用中の別プロセスと食い違う原因になるため）
    static func withExclusive(path lockPath: String, _ body: () -> Bool) -> Bool {
        let fd = open(lockPath, O_CREAT | O_RDWR, 0o644)
        guard fd >= 0 else { return false }
        defer { close(fd) }
        guard flock(fd, LOCK_EX) == 0 else { return false }
        defer { _ = flock(fd, LOCK_UN) }
        return body()
    }
}
