import Foundation
import NagiClient

/// IMKit の `handle(_:client:)` は同期的に true/false を返す必要があるため、
/// async な RPC を上限時間つきで同期待ちする。上限は `sync_budget_ms` + 要求タイムアウトの範囲に収める。
public enum SyncBridge {
    private final class Box<T>: @unchecked Sendable {
        private let lock = NSLock()
        private var value: Result<T, Error>?
        func set(_ v: Result<T, Error>) {
            lock.lock()
            value = v
            lock.unlock()
        }
        func get() -> Result<T, Error>? {
            lock.lock()
            defer { lock.unlock() }
            return value
        }
    }

    /// `operation` の完了を最大 `timeout` 秒待つ。超過時は `.failure(NagiClientError.timeout)`。
    /// 呼び出し元スレッド（メイン）を塞ぐので、`operation` 側は MainActor に依存してはならない
    public static func wait<T>(
        timeout: TimeInterval, _ operation: @escaping @Sendable () async throws -> T
    ) -> Result<T, Error> {
        let box = Box<T>()
        let semaphore = DispatchSemaphore(value: 0)
        let task = Task.detached {
            do {
                box.set(.success(try await operation()))
            } catch {
                box.set(.failure(error))
            }
            semaphore.signal()
        }
        if semaphore.wait(timeout: .now() + timeout) == .timedOut {
            task.cancel()
            return .failure(NagiClientError.timeout)
        }
        return box.get() ?? .failure(NagiClientError.timeout)
    }
}
