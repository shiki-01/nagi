#if os(macOS)
import Foundation
import NagiClient
import SwiftUI

/// 設定画面の状態管理。コアの記述子（`SettingsFormState`）を保持し、変更をデバウンスして `update_settings` へ送る。
/// メインスレッドからのみ使う。
@MainActor
final class SettingsModel: ObservableObject {
    @Published private(set) var state = SettingsFormState()
    /// エンジンに接続できない（または要求に失敗した）
    @Published private(set) var connectionFailed = false
    @Published private(set) var loaded = false

    private let supervisor: EngineSupervisor
    private var flushTask: Task<Void, Never>?
    private var inFlight = false

    /// 数値・テキスト入力のデバウンス
    private static let debounce: TimeInterval = 0.5

    init(supervisor: EngineSupervisor) {
        self.supervisor = supervisor
    }

    /// 設定を読み込み直す
    func load() {
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let snapshot = try await self.supervisor.getSettings()
                self.state.apply(snapshot: snapshot)
                self.connectionFailed = false
            } catch {
                self.connectionFailed = true
            }
            self.loaded = true
        }
    }

    // MARK: 編集（即時: Toggle / Picker、デバウンス: 数値・テキスト）

    func setBool(_ key: String, _ value: Bool) {
        state.edit(key: key, bool: value)
        scheduleFlush(after: 0)
    }

    func setChoice(_ key: String, _ value: String) {
        state.edit(key: key, text: value)
        scheduleFlush(after: 0)
    }

    func setInt(_ key: String, _ value: Int) {
        state.edit(key: key, int: value)
        scheduleFlush(after: Self.debounce)
    }

    func setFloat(_ key: String, _ value: Double) {
        state.edit(key: key, float: value)
        scheduleFlush(after: Self.debounce)
    }

    func setText(_ key: String, _ value: String) {
        state.edit(key: key, text: value)
        scheduleFlush(after: Self.debounce)
    }

    func reset(_ key: String) {
        state.reset(key: key)
        scheduleFlush(after: 0)
    }

    // MARK: 送信

    private func scheduleFlush(after delay: TimeInterval) {
        flushTask?.cancel()
        flushTask = Task { @MainActor [weak self] in
            if delay > 0 {
                try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
                if Task.isCancelled { return }
            }
            self?.flush()
        }
    }

    /// 送信中でなければ、溜まった変更を 1 回の `update_settings` にまとめて送る。
    /// 送信中の編集は完了後に続けて送る
    private func flush() {
        guard !inFlight, state.hasStagedChanges else { return }
        let changes = state.takeStagedChanges()
        inFlight = true
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let result = try await self.supervisor.updateSettings(changes: changes)
                self.state.apply(result: result)
                self.connectionFailed = false
            } catch {
                // 送れなかった。表示をコアの現在値に戻すため読み直す
                self.connectionFailed = true
                self.load()
            }
            self.inFlight = false
            if self.state.hasStagedChanges { self.flush() }
        }
    }
}

/// コアが返す記述子から汎用フォームを描く。項目ごとのコードは持たない。
struct SettingsView: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        VStack(spacing: 0) {
            if model.connectionFailed {
                banner("エンジンに接続できません", color: .red)
            }
            if let error = model.state.snapshotError {
                banner("設定ファイルを読み込めませんでした（既定値を表示中・変更は保存されません）: \(error)", color: .orange)
            }
            if model.loaded && model.state.items.isEmpty && !model.connectionFailed {
                Text("設定項目がありません").foregroundColor(.secondary).padding()
            }
            Form {
                ForEach(model.state.groups, id: \.title) { group in
                    Section(header: Text(group.title)) {
                        ForEach(group.items, id: \.key) { item in
                            row(item)
                        }
                    }
                }
            }
            .formStyle(.grouped)
        }
        .frame(minWidth: 480, minHeight: 360)
    }

    private func banner(_ text: String, color: Color) -> some View {
        Text(text)
            .foregroundColor(.white)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(8)
            .background(color)
    }

    private func row(_ item: SettingItem) -> some View {
        let key = item.key
        return VStack(alignment: .leading, spacing: 4) {
            control(item)
            Text(item.description).font(.caption).foregroundColor(.secondary)
            if item.requires_restart {
                Text("変更の反映にはエンジンの再起動（ログアウト等）が必要です")
                    .font(.caption).foregroundColor(.orange)
            }
            if let message = model.state.error(forKey: key) {
                Text(message).font(.caption).foregroundColor(.red)
            }
            if model.state.canReset(forKey: key) {
                Button("既定に戻す") { model.reset(key) }
                    .buttonStyle(.link)
                    .font(.caption)
            }
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private func control(_ item: SettingItem) -> some View {
        let key = item.key
        switch item.kind {
        case .bool:
            Toggle(
                item.label,
                isOn: Binding(
                    get: { model.state.boolValue(forKey: key) }, set: { model.setBool(key, $0) }))
        case .int(let range):
            let lo = Int(range.min)
            let hi = max(Int(range.max), lo)
            HStack {
                Text(item.label)
                Spacer()
                Text(model.state.isUnspecified(forKey: key) ? "自動" : "\(model.state.intValue(forKey: key))")
                    .foregroundColor(.secondary)
                Stepper(
                    "",
                    value: Binding(
                        get: { model.state.intValue(forKey: key) }, set: { model.setInt(key, $0) }),
                    in: lo...hi, step: max(Int(range.step), 1)
                )
                .labelsHidden()
            }
        case .float(let range):
            HStack {
                Text(item.label)
                Spacer()
                Text(model.state.isUnspecified(forKey: key) ? "自動" : "\(model.state.floatValue(forKey: key))")
                    .foregroundColor(.secondary)
                Stepper(
                    "",
                    value: Binding(
                        get: { model.state.floatValue(forKey: key) }, set: { model.setFloat(key, $0) }),
                    in: range.min...max(range.max, range.min), step: range.step > 0 ? range.step : 0.1
                )
                .labelsHidden()
            }
        case .choice(let options):
            Picker(
                item.label,
                selection: Binding(
                    get: { model.state.textValue(forKey: key) }, set: { model.setChoice(key, $0) })
            ) {
                ForEach(options.options, id: \.value) { option in
                    Text(option.label).tag(option.value)
                }
            }
        case .text:
            HStack {
                Text(item.label)
                TextField(
                    "",
                    text: Binding(
                        get: { model.state.textValue(forKey: key) }, set: { model.setText(key, $0) }))
            }
        }
    }
}
#endif
