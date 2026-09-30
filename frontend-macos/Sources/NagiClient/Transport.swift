import Foundation

/// エンジンとの通信路（1 接続 = 1 プロセス）。テストではモックに差し替える。
///
/// 1 メッセージ = 改行を含まない 1 行の JSON。改行の付与・分割は実装側の責務。
public protocol EngineConnection: AnyObject {
    /// 受信・終了のハンドラを設定する（`start()` より前に呼ばれる）。
    /// ハンドラは任意のスレッドから呼ばれうる
    /// - Parameters:
    ///   - onLine: 1 行（改行を除く）を受信した
    ///   - onClose: 通信路が閉じた（EOF・プロセス終了）。高々 1 回
    func setHandlers(onLine: @escaping (Data) -> Void, onClose: @escaping () -> Void)
    /// 通信を開始する（プロセスの場合は起動）
    func start() throws
    /// 1 メッセージを送る（改行なしの JSON）。失敗時は throw
    func send(_ message: Data) throws
    /// 通信路を閉じ、プロセスなら終了させる。呼び出し後の `onClose` は呼んでも呼ばなくてもよい
    func terminate()
}

/// 接続の生成器。再起動のたびに新しい接続を作る
public protocol EngineLauncher: AnyObject {
    /// - Parameter minimal: true なら minimal プロファイル（LLM なし）で起動する
    func makeConnection(minimal: Bool) throws -> EngineConnection
}

/// 遅延実行。再起動のバックオフ待ちに使い、テストでは即時実行に差し替える
public protocol EngineScheduler: AnyObject {
    func schedule(after delay: TimeInterval, _ work: @escaping () -> Void)
}

public final class DispatchEngineScheduler: EngineScheduler {
    private let queue: DispatchQueue
    public init(queue: DispatchQueue = DispatchQueue(label: "com.shiki.nagi.scheduler")) {
        self.queue = queue
    }
    public func schedule(after delay: TimeInterval, _ work: @escaping () -> Void) {
        queue.asyncAfter(deadline: .now() + delay, execute: work)
    }
}

/// 受信バイト列を改行区切りの行に分割する。チャンク境界をまたぐ行・複数行を扱う
public struct LineBuffer {
    private var buffer = Data()
    public init() {}

    /// バイト列を追加し、完成した行（改行を除く・空行は捨てる）を返す。
    /// 0x0A は UTF-8 の継続バイトに現れないため、バイト単位で分割してよい
    public mutating func append(_ data: Data) -> [Data] {
        buffer.append(data)
        var lines: [Data] = []
        while let idx = buffer.firstIndex(of: 0x0A) {
            var line = buffer[buffer.startIndex..<idx]
            if line.last == 0x0D { line = line.dropLast() }
            if !line.isEmpty { lines.append(Data(line)) }
            buffer = Data(buffer[buffer.index(after: idx)...])
        }
        return lines
    }
}
