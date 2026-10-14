import SwiftUI

// データタブ。辞書・モデルの有無とサイズ・場所、メモリ状況、LLM が使える見込みを表示する。
// 取り込み操作はホームタブの既存機能（辞書のコピー・モデルのインポート）にあり、ここでは重複して持たず誘導する。
// 学習データ・ユーザー辞書の場所と全削除は設定タブ側にある。

/// バイト数の表示用整形（ContentView の private な formatBytes と重複させない別名）
enum NagiFormat {
    static func bytes(_ bytes: Int64) -> String {
        ByteCountFormatter().string(fromByteCount: bytes)
    }

    static func megabytes(_ mb: Double) -> String {
        String(format: "%.0f MB", mb)
    }
}

/// 辞書・モデルの配置状況のスナップショット（オンボーディングとデータタブで共有）
struct NagiDataSnapshot {
    var dictPath: String?
    var dictSource: String
    var dictSizeBytes: Int64
    var modelStatus: DataPaths.ModelStatus
    var modelFiles: [DataPaths.ModelFile]
    var usedModel: DataPaths.ModelFile?
    var hasAppGroupContainer: Bool

    var hasDict: Bool { dictPath != nil }
    var hasModel: Bool { !modelFiles.isEmpty }

    static func load() -> NagiDataSnapshot {
        let store = SharedStore.shared
        let (dictPath, dictSource) = store.dictPath()
        let dictSize: Int64 = dictPath.flatMap {
            (try? FileManager.default.attributesOfItem(atPath: $0))?[.size] as? Int64
        } ?? 0
        let status = DataPaths.modelStatus(appGroupId: store.appGroupId)
        var files: [DataPaths.ModelFile] = []
        var used: DataPaths.ModelFile?
        if case .found(_, let found, _) = status {
            files = found
            used = DataPaths.selectModel(
                from: found,
                preferredName: DataPaths.preferredModelName(appGroupId: store.appGroupId)
            )
        }
        return NagiDataSnapshot(
            dictPath: dictPath,
            dictSource: dictSource,
            dictSizeBytes: dictSize,
            modelStatus: status,
            modelFiles: files,
            usedModel: used,
            hasAppGroupContainer: store.containerURL != nil
        )
    }

    /// ファイル名からモデルの種類を推定する（zenz-v3.1-xsmall / zenz-v3.1-small）
    static func modelKind(fileName: String) -> String {
        let lower = fileName.lowercased()
        if lower.contains("xsmall") { return "xsmall" }
        if lower.contains("small") { return "small" }
        return "不明（ファイル名から判別できません）"
    }
}

struct NagiDataStatusView: View {
    /// ホームタブ（辞書・モデルの取り込み機能がある）へ切り替える
    let onOpenImport: () -> Void

    /// LLM を読み込める最低限のメモリ上限（MB）の目安
    private static let llmMinBudgetMB: UInt64 = 150

    @State private var snapshot = NagiDataSnapshot.load()
    @State private var stats = MemoryProbe.getStats()
    @State private var budgetMB = MemoryProbe.processBudgetMB()

    var body: some View {
        NavigationStack {
            List {
                dictSection
                modelSection
                importSection
                memorySection
                llmSection
            }
            .navigationTitle("データ")
            .onAppear { reload() }
            .refreshable { reload() }
        }
    }

    // MARK: - セクション

    private var dictSection: some View {
        Section("システム辞書") {
            if let path = snapshot.dictPath {
                statusRow("状態", "あり", ok: true)
                statusRow("サイズ", NagiFormat.bytes(snapshot.dictSizeBytes))
                statusRow("読み込み元", sourceLabel(snapshot.dictSource))
                pathText(path)
            } else {
                statusRow("状態", "なし", ok: false)
                Text("辞書がないと読みをそのまま返します（LLM も読み込まれません）")
                    .font(.caption)
                    .foregroundColor(.orange)
            }
        }
    }

    private var modelSection: some View {
        Section("変換モデル") {
            if snapshot.modelFiles.isEmpty {
                statusRow("状態", "なし", ok: false)
                Text(modelMissingReason())
                    .font(.caption)
                    .foregroundColor(.orange)
            } else {
                statusRow("状態", "あり（\(snapshot.modelFiles.count) 個）", ok: true)
                ForEach(snapshot.modelFiles, id: \.name) { file in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Text(file.name).font(.callout)
                            Spacer()
                            if file.name == snapshot.usedModel?.name {
                                Text("使用").font(.caption).foregroundColor(.green)
                            }
                        }
                        Text("種類: \(NagiDataSnapshot.modelKind(fileName: file.name)) / \(NagiFormat.bytes(file.sizeBytes))")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
                if case .found(let dir, _, let source) = snapshot.modelStatus {
                    statusRow("読み込み元", sourceLabel(source))
                    pathText(dir)
                    if source == "local" {
                        Text("App Group が使えないため、このアプリ専用の領域に保存されています。キーボード拡張からは見えません")
                            .font(.caption)
                            .foregroundColor(.orange)
                    }
                }
            }
        }
    }

    private var importSection: some View {
        Section("取り込み方法") {
            Text("辞書とモデルは、ホームタブの「データ」から取り込みます。モデルは「ファイル」アプリなどに置いた .gguf を選んでください。辞書はアプリに同梱のものをコピーできます。")
                .font(.caption)
            Button("ホームの取り込み機能を開く") {
                onOpenImport()
            }
            .buttonStyle(.borderless)
        }
    }

    private var memorySection: some View {
        Section("メモリ状況（このアプリ）") {
            statusRow("使用中", NagiFormat.megabytes(stats.physFootprintMB))
            statusRow("あと使える量", NagiFormat.megabytes(stats.availableMemoryMB))
            statusRow("上限の推定", "\(budgetMB) MB")
            Button("再計測") { reload() }
                .buttonStyle(.borderless)
            Text("キーボード拡張はこのアプリより厳しい上限（実機で約 177MB）で動きます。拡張側の実測値は診断ログや実機計測で確認してください。")
                .font(.caption)
                .foregroundColor(.secondary)
        }
    }

    private var llmSection: some View {
        let verdict = llmVerdict()
        return Section("LLM が使える見込み") {
            HStack(alignment: .top) {
                Image(systemName: verdict.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                    .foregroundColor(verdict.ok ? .green : .orange)
                Text(verdict.message).font(.callout)
            }
            Text("判定: 辞書あり・モデルあり・メモリ上限の推定が \(Self.llmMinBudgetMB) MB 以上。実際に使えるかは、拡張が起動時に空きメモリを見て決めます。フルアクセスがオフだと拡張は辞書・モデルを読めず、ひらがな・カタカナ変換のみになります。")
                .font(.caption)
                .foregroundColor(.secondary)
        }
    }

    // MARK: - 部品

    private func statusRow(_ title: String, _ value: String, ok: Bool? = nil) -> some View {
        HStack {
            Text(title)
            Spacer()
            if let ok = ok {
                Image(systemName: ok ? "checkmark.circle.fill" : "xmark.circle.fill")
                    .foregroundColor(ok ? .green : .red)
            }
            Text(value).foregroundColor(.secondary)
        }
    }

    private func pathText(_ path: String) -> some View {
        Text(path)
            .font(.system(.caption2, design: .monospaced))
            .foregroundColor(.secondary)
            .textSelection(.enabled)
    }

    private func sourceLabel(_ source: String) -> String {
        switch source {
        case "appgroup": return "App Group（拡張と共有）"
        case "bundle": return "アプリに同梱"
        case "local": return "このアプリ専用の領域"
        default: return source
        }
    }

    private func modelMissingReason() -> String {
        switch snapshot.modelStatus {
        case .noContainer:
            return "App Group コンテナが取得できません。ホームの「モデルをインポート」でこのアプリ専用の領域には置けますが、拡張からは見えません"
        case .noModelsDir(let path):
            return "\(path) がありません。ホームの「モデルをインポート」で .gguf を選んでください"
        case .noGGUF(let path):
            return "\(path) に .gguf がありません。ホームの「モデルをインポート」で選んでください"
        case .found:
            return ""
        }
    }

    private func llmVerdict() -> (ok: Bool, message: String) {
        if !snapshot.hasDict {
            return (false, "辞書がないため LLM は読み込まれません。先に辞書を取り込んでください")
        }
        if !snapshot.hasModel {
            return (false, "モデルがありません。辞書のみで変換します")
        }
        if budgetMB < Self.llmMinBudgetMB {
            return (false, "メモリ上限の推定が \(budgetMB) MB で、\(Self.llmMinBudgetMB) MB に届きません。LLM は使えない見込みです（辞書のみで変換します）")
        }
        return (true, "使える見込みです（上限の推定 \(budgetMB) MB）")
    }

    private func reload() {
        snapshot = NagiDataSnapshot.load()
        stats = MemoryProbe.getStats()
        budgetMB = MemoryProbe.processBudgetMB()
    }
}
