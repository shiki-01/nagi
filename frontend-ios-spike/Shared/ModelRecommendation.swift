import Foundation

/// 端末のメモリ予算から、載せて余裕のある最大のモデルを推奨する（docs/decisions.md H-22）。
///
/// 実測（H-9、iPhone 16e の拡張）: 上限は約 177MB。small（約 70MB）併用のピークは約 78MB 以下、
/// xsmall（約 20MB）は約 70MB 以下で、どちらも強制終了なし。この余裕を端末ごとの予算に当てはめる。
///
/// 余裕の式: `予算 − (モデルのサイズ + 辞書・実行時の見込み) ≥ 安全マージン`。
/// 純粋な計算だけを持ち、メモリの取得（`MemoryProbe`）には依存しない（単体テスト可能）
enum NagiModelRecommendation {
    /// 辞書（mmap）・推論の作業領域・拡張本体など、モデル以外の見込み（MB）。
    /// 実測のピーク（small で約 78MB）はモデル全体（約 70MB）を含めても 20MB 程度の上乗せに収まるため、
    /// 余裕を見て 20MB とする。モデルは mmap で触れたページだけが計上されるので、サイズ全体を足すのは安全側
    static let runtimeOverheadMB: Double = 20

    /// 上限に対して残しておく安全マージン（MB）。メモリ警告・キーボードの描画・ホストの状況による揺れを吸収する。
    /// 実測の「上限までの残り最小」は small で約 100MB なので、その半分以下の 40MB を最低ラインにする
    static let safetyMarginMB: Double = 40

    /// 拡張の予算が取れないとき（まだ拡張が起動していない場面のホストアプリ）に仮定する値（MB）。
    /// H-6 の実測（iPhone 16e の拡張の上限）
    static let assumedExtensionBudgetMB: UInt64 = 177

    /// このモデルを載せるのに必要な予算（MB）。`ModelFile.sizeBytes` が 0（不明）ならオーバーヘッドとマージンのみ
    static func requiredBudgetMB(for file: DataPaths.ModelFile) -> Double {
        sizeMB(file) + runtimeOverheadMB + safetyMarginMB
    }

    /// 余裕を持って載せられるか
    static func fits(_ file: DataPaths.ModelFile, budgetMB: UInt64) -> Bool {
        Double(budgetMB) - (sizeMB(file) + runtimeOverheadMB) >= safetyMarginMB
    }

    /// 推奨モデル: 余裕を持って載せられるうち、最大のもの（精度優先。H-17）。
    /// どれも載らなければ nil（呼び出し側は辞書のみ、またはもっとも小さいモデルにフォールバックする）。
    /// 同じサイズなら名前の昇順で先のもの（結果を安定させる）
    static func recommend(budgetMB: UInt64, available: [DataPaths.ModelFile]) -> DataPaths.ModelFile? {
        available
            .filter { fits($0, budgetMB: budgetMB) }
            .min { lhs, rhs in
                if lhs.sizeBytes != rhs.sizeBytes { return lhs.sizeBytes > rhs.sizeBytes }
                return lhs.name < rhs.name
            }
    }

    /// 画面に出す説明（「推奨: small（この端末の予算 177MB に対して余裕あり）」）
    static func summary(budgetMB: UInt64, available: [DataPaths.ModelFile]) -> String {
        guard !available.isEmpty else { return "" }
        if let hit = recommend(budgetMB: budgetMB, available: available) {
            return "推奨: \(label(hit.name))（この端末の予算 \(budgetMB)MB に対して余裕あり）"
        }
        return "推奨なし（この端末の予算 \(budgetMB)MB では、どのモデルも余裕を持って載せられない見込み）"
    }

    private static func sizeMB(_ file: DataPaths.ModelFile) -> Double {
        Double(file.sizeBytes) / 1024.0 / 1024.0
    }

    private static func label(_ fileName: String) -> String {
        let lower = fileName.lowercased()
        if lower.contains("xsmall") { return "xsmall" }
        if lower.contains("small") { return "small" }
        return fileName
    }
}
