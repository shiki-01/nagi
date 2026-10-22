import Foundation

/// `imserver --install-data` の進捗（標準出力の 1 行 1 JSON）。OS 非依存（テスト可能）
public enum DataInstallProgress: Equatable {
    case dictFetch(file: String, index: Int, count: Int)
    case dictBuild
    case modelFetch(bytes: Int64, total: Int64)
    case done
    case error(String)

    private struct Line: Decodable {
        var stage: String
        var file: String?
        var index: Int?
        var count: Int?
        var bytes: Int64?
        var total: Int64?
        var message: String?
    }

    /// 1 行を解釈する。進捗の行でなければ nil
    public static func parse(_ line: String) -> DataInstallProgress? {
        guard let data = line.data(using: .utf8),
            let l = try? JSONDecoder().decode(Line.self, from: data)
        else { return nil }
        switch l.stage {
        case "dict_fetch": return .dictFetch(file: l.file ?? "", index: l.index ?? 0, count: l.count ?? 0)
        case "dict_build": return .dictBuild
        case "model_fetch": return .modelFetch(bytes: l.bytes ?? 0, total: l.total ?? 0)
        case "done": return .done
        case "error": return .error(l.message ?? "不明なエラー")
        default: return nil
        }
    }

    /// 利用者に見せる文言。細かすぎる進捗（モデルの途中経過）は 25% 刻みでだけ返す
    public func message(lastModelQuarter: inout Int) -> String? {
        switch self {
        case .dictFetch(_, let index, let count):
            return index == 1 ? "辞書をダウンロードしています（\(count) ファイル）" : nil
        case .dictBuild:
            return "辞書を組み立てています"
        case .modelFetch(let bytes, let total):
            guard total > 0 else { return nil }
            let quarter = Int(bytes * 4 / total)
            guard quarter != lastModelQuarter else { return nil }
            lastModelQuarter = quarter
            return "変換モデルをダウンロードしています（\(min(quarter * 25, 100))%）"
        case .done:
            return "辞書とモデルを入れました。変換エンジンを再起動します"
        case .error(let message):
            return "辞書とモデルを入れられませんでした: \(message)"
        }
    }
}
