import SwiftUI

/// ホームタブの「追加データ」: 英語の語彙・カタカナ語の原語データの状態と、ダウンロード。
/// 入っていなくてもキーボードは動く（該当の機能が働かないだけ）。キーボード拡張は App Group の
/// data/english と data/loanword を mmap で読む
struct NagiOptionalDataSection: View {
    /// データが増えたあとに呼ぶ（ホームの状況を読み直す）
    let onChanged: () -> Void

    @StateObject private var installer = OptionalDataInstaller()
    @State private var sources = OptionalDataInstaller.loadSources()
    @State private var reloadToken = 0

    private var dataDir: URL? {
        SharedStore.shared.containerURL?.appendingPathComponent("data", isDirectory: true)
    }

    var body: some View {
        Section("追加データ（英語・カタカナ語の原語）") {
            ForEach(OptionalDataKind.allCases, id: \.self) { kind in
                let installed = isInstalled(kind)
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text(kind.title)
                        Spacer()
                        Image(systemName: installed ? "checkmark.circle.fill" : "xmark.circle.fill")
                            .foregroundColor(installed ? .green : .red)
                        Text(OptionalData.statusText(sources[kind], in: dataDir))
                            .foregroundColor(.secondary)
                    }
                    if !installed {
                        Text("入っていないと、\(kind.missingEffect)")
                            .font(.caption)
                            .foregroundColor(.orange)
                    }
                }
            }
            if let dataDir {
                if installer.running {
                    HStack {
                        ProgressView()
                        Text(installer.message ?? "").font(.caption)
                    }
                } else {
                    Button("ダウンロード（取得済みのものは飛ばします）") {
                        Task {
                            let changed = await installer.installMissing(into: dataDir)
                            reloadToken += 1
                            if changed { onChanged() }
                        }
                    }
                    .buttonStyle(.borderless)
                    if let message = installer.message {
                        Text(message).font(.caption).foregroundColor(.secondary)
                    }
                }
            } else {
                Text("App Group コンテナが取得できていないため、キーボード拡張と共有できません")
                    .font(.caption)
                    .foregroundColor(.orange)
            }
            Text("配布元から直接取得し、SHA-256 を確かめてからこの端末の中で索引を組み立てます（通信するのはこのアプリだけで、入力内容は送りません）。英語の語彙は SCOWL、原語データは JMdict（EDRDG）が元です。")
                .font(.caption)
                .foregroundColor(.secondary)
        }
    }

    private func isInstalled(_ kind: OptionalDataKind) -> Bool {
        guard let source = sources[kind], let dataDir else { return false }
        return OptionalData.isInstalled(source, in: dataDir)
    }
}
