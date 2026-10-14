import Foundation
import NagiCore

/// NagiEngine: C ABI のラッパー
class NagiEngine {
    private var handle: OpaquePointer?
    private let queue = DispatchQueue(label: "com.shiki.nagi.engine")
    private var wakeBox: Unmanaged<WakeBox>?

    /// エンジンを生成。失敗時は nil を返す
    /// - memoryBudgetMB: このプロセスが OS から課されているメモリ上限（MB）。キーボード拡張では
    ///   `MemoryProbe.processBudgetMB()`。上限が小さい場合、コアは LLM を読み込まず辞書のみで動く
    init?(dataDir: String, userDir: String? = nil, noLLM: Bool = false, memoryBudgetMB: UInt64 = 0) {
        let noLLMFlag: Int32 = noLLM ? 1 : 0
        self.handle = dataDir.withCString { (dataDirPtr: UnsafePointer<CChar>) -> OpaquePointer? in
            if let userDir = userDir {
                return userDir.withCString { (userDirPtr: UnsafePointer<CChar>) -> OpaquePointer? in
                    nagi_server_new_with_budget(dataDirPtr, userDirPtr, noLLMFlag, memoryBudgetMB)
                }
            }
            return nagi_server_new_with_budget(dataDirPtr, nil, noLLMFlag, memoryBudgetMB)
        }

        guard handle != nil else { return nil }
    }

    /// JSON-RPC 行を送信して応答行の配列を取得。コアのハンドルは同時に触れないため、
    /// すべての呼び出し（送信・通知の取り出し・解放）を 1 本の直列キューで行う。
    /// 通知のハンドラの中から呼ぶとデッドロックするので、ハンドラは別のキューに積み直すこと
    func send(line: String) -> [String] {
        queue.sync { sendLocked(line: line) }
    }

    private func sendLocked(line: String) -> [String] {
        guard let handle = handle else { return [] }

        return line.withCString { (cLine: UnsafePointer<CChar>) -> [String] in
            guard let responsePtr = nagi_handle_line(handle, cLine) else { return [String]() }
            defer { nagi_string_free(responsePtr) }

            let responseString = String(cString: responsePtr)
            return responseString.split(separator: "\n").map(String.init)
        }
    }

    /// 通知の受け取りを開始する。LLM が同期の待ち時間を超えた分の完了（`update_preedit` /
    /// `update_candidates`）やライブ変換の結果など、次のリクエストを待たずに届く通知を、
    /// `handler` にまとめて渡す（メインキューで呼ぶ）。`initialize` の後に呼ぶこと
    func startNotifications(_ handler: @escaping ([String]) -> Void) {
        queue.sync {
            guard let handle = handle else { return }
            stopNotificationsLocked()
            let box = WakeBox(engine: self, handler: handler)
            let unmanaged = Unmanaged.passRetained(box)
            wakeBox = unmanaged
            // コールバックは任意のスレッドから呼ばれる。ここでは engine のキューに積み直すだけにする
            nagi_set_waker(handle, { userData in
                guard let userData = userData else { return }
                let box = Unmanaged<WakeBox>.fromOpaque(userData).takeUnretainedValue()
                box.fire()
            }, unmanaged.toOpaque())
        }
    }

    /// 通知の受け取りを止める
    func stopNotifications() {
        queue.sync { stopNotificationsLocked() }
    }

    private func stopNotificationsLocked() {
        if let handle = handle {
            nagi_set_waker(handle, nil, nil)
        }
        wakeBox?.release()
        wakeBox = nil
    }

    /// キューの上で通知を取り出し、あればハンドラをメインキューで呼ぶ
    fileprivate func pollAndDeliver(_ handler: @escaping ([String]) -> Void) {
        queue.async { [weak self] in
            guard let self = self, let handle = self.handle else { return }
            guard let ptr = nagi_poll_notifications(handle) else { return }
            defer { nagi_string_free(ptr) }
            let text = String(cString: ptr)
            if text.isEmpty { return }
            let lines = text.split(separator: "\n").map(String.init)
            DispatchQueue.main.async { handler(lines) }
        }
    }

    /// ビルド時の注記（プロファイル名など）を取得
    var notes: String {
        guard let handle = handle else { return "" }
        guard let notesPtr = nagi_last_notes(handle) else { return "" }
        defer { nagi_string_free(notesPtr) }
        return String(cString: notesPtr)
    }

    deinit {
        if let handle = handle {
            // 先にコールバックを外す（解放後は nagi_server_free が呼ばれないことを保証する）
            nagi_set_waker(handle, nil, nil)
            nagi_server_free(handle)
        }
        wakeBox?.release()
    }

    /// エンジンのバージョンを取得
    static var version: String {
        guard let versionPtr = nagi_version() else { return "unknown" }
        return String(cString: versionPtr)
    }
}

/// コアのコールバック（任意のスレッド）から engine を起こすための入れ物。engine は弱参照にして、
/// コールバックが engine の寿命を延ばさないようにする
private final class WakeBox {
    private weak var engine: NagiEngine?
    private let handler: ([String]) -> Void

    init(engine: NagiEngine, handler: @escaping ([String]) -> Void) {
        self.engine = engine
        self.handler = handler
    }

    func fire() {
        engine?.pollAndDeliver(handler)
    }
}
