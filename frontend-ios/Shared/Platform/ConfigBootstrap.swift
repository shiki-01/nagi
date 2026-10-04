import Foundation

/// 初回に `config.toml` を書き出す。曖昧入力（`[fuzzy]`）をモバイルの既定で有効にするため。
/// 既にファイルがあれば上書きしない（ユーザーの設定が常に優先）。
/// コアの設定ファイルは `<user_dir>/config.toml` が既定の場所なので、`user_dir` を渡すだけで反映される
enum ConfigBootstrap {
    static let defaultConfig = """
    # Nagi iOS の既定設定（初回に自動生成されます。このファイルは以後上書きされません）
    # 全項目は config/default.toml を参照してください。

    [fuzzy]
    # 曖昧入力: タッチ位置の誤差・フリックの誤判定を補正候補として提示する
    enabled = true

    """

    enum Outcome: String {
        case created
        case exists
        case noUserDir = "no_user_dir"
        case failed
    }

    @discardableResult
    static func ensureConfig(userDir: String?) -> Outcome {
        guard let userDir = userDir else { return .noUserDir }
        let url = URL(fileURLWithPath: userDir).appendingPathComponent("config.toml")
        if FileManager.default.fileExists(atPath: url.path) { return .exists }
        do {
            try defaultConfig.write(to: url, atomically: true, encoding: .utf8)
            return .created
        } catch {
            return .failed
        }
    }
}
