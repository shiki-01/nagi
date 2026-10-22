import Foundation
import NagiClient

/// OS の値 → `set_environment` の値（OS 非依存の変換。テスト可能）
public enum EnvironmentMapping {
    /// `IOPSGetProvidingPowerSourceType` の値（`kIOPMBatteryPowerKey` = "Battery Power"）から、電池駆動かを判定する
    public static func onBattery(providingPowerSourceType type: String?) -> Bool? {
        guard let type else { return nil }
        return type == "Battery Power"
    }

    public static func thermal(_ state: ProcessInfo.ThermalState) -> ThermalState {
        switch state {
        case .nominal: return .nominal
        case .fair: return .fair
        case .serious: return .serious
        case .critical: return .critical
        @unknown default: return .nominal
        }
    }

    /// メモリ逼迫イベント（`DispatchSource.MemoryPressureEvent`）
    public static func memoryPressure(_ event: DispatchSource.MemoryPressureEvent) -> MemoryPressureState {
        if event.contains(.critical) { return .critical }
        if event.contains(.warning) { return .warning }
        return .normal
    }
}

#if os(macOS)
import IOKit.ps

/// OS の実行環境（電源・低電力モード・サーマル状態・メモリ逼迫）を集めて、コアの `set_environment` の形にする。
/// OS への問い合わせはここ（フロント）に置き、コアは報告された値だけでプロファイルを切り替える（設計書 §4.7）
final class EnvironmentReporter {
    /// 環境が変化したとき（メインキューで呼ぶ）。呼び出し側が `snapshot()` をコアへ送る
    var onChange: (() -> Void)?

    private var observers: [NSObjectProtocol] = []
    private var powerSource: CFRunLoopSource?
    private var memorySource: DispatchSourceMemoryPressure?
    private var memoryPressure: MemoryPressureState = .normal

    func start() {
        guard observers.isEmpty else { return }
        let center = NotificationCenter.default
        let handler: (Notification) -> Void = { [weak self] _ in self?.onChange?() }
        observers.append(center.addObserver(
            forName: .NSProcessInfoPowerStateDidChange, object: nil, queue: .main, using: handler))
        observers.append(center.addObserver(
            forName: ProcessInfo.thermalStateDidChangeNotification, object: nil, queue: .main,
            using: handler))

        // 電源（AC / 電池）の切替。コールバックはメインのランループで呼ばれる
        let context = Unmanaged.passUnretained(self).toOpaque()
        if let source = IOPSNotificationCreateRunLoopSource(
            { context in
                guard let context else { return }
                Unmanaged<EnvironmentReporter>.fromOpaque(context).takeUnretainedValue().onChange?()
            }, context)?.takeRetainedValue()
        {
            CFRunLoopAddSource(CFRunLoopGetMain(), source, .defaultMode)
            powerSource = source
        }

        let memory = DispatchSource.makeMemoryPressureSource(
            eventMask: [.normal, .warning, .critical], queue: .main)
        memory.setEventHandler { [weak self, weak memory] in
            guard let self, let memory else { return }
            self.memoryPressure = EnvironmentMapping.memoryPressure(memory.data)
            self.onChange?()
        }
        memory.resume()
        memorySource = memory
    }

    func stop() {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers.removeAll()
        if let powerSource { CFRunLoopRemoveSource(CFRunLoopGetMain(), powerSource, .defaultMode) }
        powerSource = nil
        memorySource?.cancel()
        memorySource = nil
    }

    deinit { stop() }

    /// 現在の環境。メモリ上限（`memory_budget_mb`）は macOS では報告しない
    func snapshot() -> SetEnvironmentParams {
        let info = ProcessInfo.processInfo
        return SetEnvironmentParams(
            on_battery: EnvironmentMapping.onBattery(providingPowerSourceType: Self.providingPowerSourceType()),
            low_power_mode: info.isLowPowerModeEnabled,
            thermal: EnvironmentMapping.thermal(info.thermalState),
            memory_pressure: memoryPressure,
            memory_budget_mb: nil)
    }

    private static func providingPowerSourceType() -> String? {
        guard let info = IOPSCopyPowerSourcesInfo()?.takeRetainedValue(),
            let type = IOPSGetProvidingPowerSourceType(info)?.takeUnretainedValue()
        else { return nil }
        return type as String
    }
}
#endif
