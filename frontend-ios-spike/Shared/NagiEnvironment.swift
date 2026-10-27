import Foundation

/// OS の実行環境（低電力モード・サーマル状態・メモリ逼迫・タイムゾーン）を集めて、コアの `set_environment` の形にする。
/// 電源・熱は OS の通知（`NSProcessInfoPowerStateDidChange` / `thermalStateDidChangeNotification`）で変化を
/// 検出する。メモリ逼迫は OS の警告（`didReceiveMemoryWarning`）を `noteMemoryWarning` で受ける
/// （メモリ上限そのものは、コアの生成時に `nagi_server_new_with_budget` で渡す。docs/mobile-roadmap.md D-4）。
/// OS への問い合わせはここ（フロント）に置き、コアは報告された値だけで判定する
final class NagiEnvironmentReporter {
    /// 電源・熱が変化したとき（メインキューで呼ぶ）。呼び出し側が `snapshot()` をコアへ送る
    var onChange: (() -> Void)?

    private var observers: [NSObjectProtocol] = []
    private var memoryPressure: MemoryPressureState = .normal
    private var lastMemoryWarning: Date?
    /// メモリ逼迫を「通常」へ戻すまでの猶予（秒）。警告のあと、この間は報告を維持する
    private let memoryPressureHold: TimeInterval = 10
    /// 逼迫を解く予約の世代（新しい警告で古い予約を無効にする）
    private var releaseGeneration = 0

    func start() {
        guard observers.isEmpty else { return }
        let center = NotificationCenter.default
        let handler: (Notification) -> Void = { [weak self] _ in self?.onChange?() }
        observers.append(center.addObserver(
            forName: .NSProcessInfoPowerStateDidChange, object: nil, queue: .main, using: handler))
        observers.append(center.addObserver(
            forName: ProcessInfo.thermalStateDidChangeNotification, object: nil, queue: .main, using: handler))
        // タイムゾーン（夏時間の切替を含む）の変更。日付・時刻の候補に使う UTC からのずれを再送する
        observers.append(center.addObserver(
            forName: .NSSystemTimeZoneDidChange, object: nil, queue: .main, using: handler))
    }

    func stop() {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers.removeAll()
    }

    deinit { stop() }

    /// OS のメモリ警告を記録する。`didReceiveMemoryWarning` はプロセスが強制終了に近いという OS の警告なので、
    /// コアには常に `critical` で伝える（コアが LLM を解放する。H-16）。`memoryPressureHold` 秒後に自動で
    /// 通常へ戻し、`onChange` を呼ぶ（呼び出し側が `normal` をコアへ送り、空きメモリが足りれば `reload_model` で即座に読み直す。H-26）
    func noteMemoryWarning() {
        lastMemoryWarning = Date()
        memoryPressure = .critical
        releaseGeneration += 1
        let generation = releaseGeneration
        DispatchQueue.main.asyncAfter(deadline: .now() + memoryPressureHold + 1) { [weak self] in
            guard let self = self, self.releaseGeneration == generation else { return }
            self.onChange?()
        }
    }

    /// 現在の環境。`on_battery` は報告しない（コアは前回の値を維持する。iOS では低電力モードで代表させる）
    func snapshot() -> SetEnvironmentParams {
        // 警告から一定時間たったら通常へ戻す（戻さないと、一度の警告で辞書のみのまま固定されてしまう）
        if memoryPressure != .normal,
           let last = lastMemoryWarning, Date().timeIntervalSince(last) > memoryPressureHold {
            memoryPressure = .normal
        }
        return Self.systemSnapshot(memoryPressure: memoryPressure)
    }

    /// OS から読んだ低電力モード・サーマル状態と、このプロセスのメモリ上限の推定（`initialize` の
    /// `capabilities` と `set_environment` で共通）
    static func systemSnapshot(memoryPressure: MemoryPressureState) -> SetEnvironmentParams {
        let info = ProcessInfo.processInfo
        let thermal: ThermalState
        switch info.thermalState {
        case .nominal: thermal = .nominal
        case .fair: thermal = .fair
        case .serious: thermal = .serious
        case .critical: thermal = .critical
        @unknown default: thermal = .nominal
        }
        return SetEnvironmentParams(
            on_battery: nil,
            low_power_mode: info.isLowPowerModeEnabled,
            thermal: thermal,
            memory_pressure: memoryPressure,
            memory_budget_mb: UInt32(clamping: MemoryProbe.processBudgetMB()),
            utc_offset_minutes: Int32(TimeZone.autoupdatingCurrent.secondsFromGMT() / 60)
        )
    }

    /// ログ用の要約（入力内容は含まない）
    static func describe(_ params: SetEnvironmentParams) -> String {
        "lowPower=\(params.low_power_mode.map { "\($0)" } ?? "-") thermal=\(params.thermal?.rawValue ?? "-") "
            + "memory=\(params.memory_pressure?.rawValue ?? "-") budgetMB=\(params.memory_budget_mb.map { "\($0)" } ?? "-")"
    }
}
