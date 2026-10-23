import Foundation

/// `imserver --install-data` の進捗（標準出力の 1 行 1 JSON）。OS 非依存（テスト可能）
public enum DataInstallProgress: Equatable {
    /// `skipped`: 取得済み（SHA-256 一致）で飛ばした
    case dictFetch(file: String, index: Int, count: Int, skipped: Bool)
    case dictBuild
    case modelFetch(bytes: Int64, total: Int64, skipped: Bool)
    /// 英語の語彙（自動判定用）。`bytes == 0` が取得の開始
    case englishFetch(bytes: Int64, total: Int64, skipped: Bool)
    case englishBuild
    /// カタカナ語の原語データ（JMdict 由来）。`bytes == 0` が取得の開始
    case loanwordFetch(bytes: Int64, total: Int64, skipped: Bool)
    case loanwordBuild
    /// 任意のデータを入れられなかった（辞書とモデルは入っている）。変換には影響しない
    case warning(String)
    /// `changed`: 取得・組み立てを 1 つでも行ったか（false ならすべて取得済みだった）
    case done(changed: Bool)
    case error(String)

    private struct Line: Decodable {
        var stage: String
        var file: String?
        var index: Int?
        var count: Int?
        var bytes: Int64?
        var total: Int64?
        var message: String?
        var skipped: Bool?
        var changed: Bool?
    }

    /// 1 行を解釈する。進捗の行でなければ nil
    public static func parse(_ line: String) -> DataInstallProgress? {
        guard let data = line.data(using: .utf8),
            let l = try? JSONDecoder().decode(Line.self, from: data)
        else { return nil }
        switch l.stage {
        case "dict_fetch":
            return .dictFetch(
                file: l.file ?? "", index: l.index ?? 0, count: l.count ?? 0,
                skipped: l.skipped ?? false)
        case "dict_build": return .dictBuild
        case "model_fetch":
            return .modelFetch(bytes: l.bytes ?? 0, total: l.total ?? 0, skipped: l.skipped ?? false)
        case "english_fetch":
            return .englishFetch(bytes: l.bytes ?? 0, total: l.total ?? 0, skipped: l.skipped ?? false)
        case "english_build": return .englishBuild
        case "loanword_fetch":
            return .loanwordFetch(bytes: l.bytes ?? 0, total: l.total ?? 0, skipped: l.skipped ?? false)
        case "loanword_build": return .loanwordBuild
        case "warning": return .warning(l.message ?? "不明なエラー")
        case "done": return .done(changed: l.changed ?? true)
        case "error": return .error(l.message ?? "不明なエラー")
        default: return nil
        }
    }

    /// 利用者に見せる文言。取得済みで飛ばしたものは出さず（最後にまとめて知らせる）、
    /// 細かすぎる進捗（モデルの途中経過）は 25% 刻みでだけ返す
    public func message(lastModelQuarter: inout Int) -> String? {
        switch self {
        case .dictFetch(_, _, _, true), .modelFetch(_, _, true),
            .englishFetch(_, _, true), .loanwordFetch(_, _, true):
            return nil
        case .dictFetch(let file, let index, let count, false):
            return "辞書をダウンロードしています（\(index)/\(count): \(file)）"
        case .dictBuild:
            return "辞書を組み立てています"
        case .modelFetch(let bytes, let total, false):
            guard total > 0 else { return nil }
            let quarter = Int(bytes * 4 / total)
            guard quarter != lastModelQuarter else { return nil }
            lastModelQuarter = quarter
            return "変換モデルをダウンロードしています（\(min(quarter * 25, 100))%）"
        case .englishFetch(let bytes, _, false):
            // 小さいので始まりだけ知らせる（途中経過は出さない）
            return bytes == 0 ? "英語の語彙をダウンロードしています" : nil
        case .englishBuild:
            return "英語の語彙を組み立てています"
        case .loanwordFetch(let bytes, _, false):
            return bytes == 0 ? "カタカナ語の原語データをダウンロードしています" : nil
        case .loanwordBuild:
            return "カタカナ語の原語データを組み立てています"
        case .warning(let message):
            return "辞書とモデルは入れましたが、一部のデータを入れられませんでした: \(message)"
        case .done(true):
            return "辞書・モデル・語彙を入れました。変換エンジンを再起動します"
        case .done(false):
            return "辞書・モデル・語彙は最新です（取得済みのため、ダウンロードはしませんでした）"
        case .error(let message):
            return "辞書とモデルを入れられませんでした: \(message)"
        }
    }
}
