import Foundation
import Darwin

/// メモリ統計情報
struct MemoryStats {
    /// 物理フットプリント (MB)
    let physFootprintMB: Double
    /// 利用可能なメモリ (MB)
    let availableMemoryMB: Double
    /// タイムスタンプ
    let timestamp: Date = Date()
}

/// メモリプローブの機能
class MemoryProbe {
    /// 物理フットプリント (phys_footprint) を取得（MB）
    static func getPhysFootprintMB() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info>.size / MemoryLayout<integer_t>.size)

        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { ptr in
                task_info(
                    mach_task_self_,
                    task_flavor_t(TASK_VM_INFO),
                    ptr,
                    &count
                )
            }
        }

        guard kr == KERN_SUCCESS else { return 0.0 }
        return Double(info.phys_footprint) / 1024.0 / 1024.0
    }

    /// 利用可能なメモリを取得（MB）
    static func getAvailableMemoryMB() -> Double {
        let availableMemory = os_proc_available_memory()
        return Double(availableMemory) / 1024.0 / 1024.0
    }

    /// このプロセスが OS から課されているメモリ上限（MB）の推定: 現在の footprint + 残り。
    /// コアの `nagi_server_new_with_budget` に渡す（キーボード拡張は約 177MB、ホストアプリは数 GB）
    static func processBudgetMB() -> UInt64 {
        UInt64(max(0, getPhysFootprintMB() + getAvailableMemoryMB()))
    }

    /// メモリ統計を取得
    static func getStats() -> MemoryStats {
        return MemoryStats(
            physFootprintMB: getPhysFootprintMB(),
            availableMemoryMB: getAvailableMemoryMB()
        )
    }
}

/// プロセス起動直後のフットプリント（MB）。辞書等をロードして解放した後もヒープが OS に返らず、
/// 直前の測定との差分が 0 に見えるのを避けるため、起動時点からの増分を別途記録する。
/// `LaunchBaseline.capture()` をアプリ/拡張の起動時に 1 度呼ぶ。
enum LaunchBaseline {
    private static var captured: Double?

    static func capture() {
        if captured == nil { captured = MemoryProbe.getPhysFootprintMB() }
    }

    static var footprintMB: Double { captured ?? MemoryProbe.getPhysFootprintMB() }
}
