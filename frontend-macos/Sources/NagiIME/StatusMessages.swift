import Foundation
import NagiClient

/// 利用者に見せる状態の文言（OS 非依存。テスト可能）。入力内容は含めない
public enum StatusMessages {
    /// エンジンのイベントのうち、利用者に知らせるべきものの文言。nil なら知らせない
    public static func toast(for event: EngineEvent) -> String? {
        switch event {
        case .notify(let n):
            // info は頻繁に来うるため出さない（ログのみ）
            return n.level == .info ? nil : n.message
        case .crashed(let attempt, _):
            return attempt == 1 ? "変換エンジンが停止しました。再起動しています" : nil
        case .enteredMinimalMode(let restarts):
            return "変換エンジンが \(restarts) 回続けて停止したため、辞書のみで動作しています"
        case .launchFailed:
            return "変換エンジンを起動できません。再試行しています"
        case .restoreFailed:
            return "変換エンジンの再起動で入力中の文字列を復元できなかったため、そのまま確定しました"
        case .started, .restored, .updatePreedit, .updateCandidates:
            return nil
        }
    }

    /// 入力メニューに出す状態の行（選択不可の項目として並べる）
    public static func menuLines(for status: StatusResult) -> [String] {
        var lines = ["プロファイル: \(status.profile)（\(status.profile_reason)）"]
        lines.append(status.llm ? "変換: LLM + 辞書" : "変換: 辞書のみ")
        lines.append(status.learning ? "学習: 有効（\(status.learned_entries) 件）" : "学習: 無効")
        lines.append("ユーザー辞書: \(status.user_dict_entries) 語")
        if status.config_error {
            lines.append("設定ファイルの読み込みに失敗しました（以前の設定で動作中）")
        }
        return lines
    }

    /// 状態を取得できなかったときの行
    public static let unavailable = "変換エンジンに接続していません"
}

/// 同じ文言を短時間に繰り返し出さないための間引き（OS 非依存。テスト可能）
public struct ToastThrottle {
    public var interval: TimeInterval
    private var lastShown: [String: Date] = [:]

    public init(interval: TimeInterval = 30) {
        self.interval = interval
    }

    /// `message` を今出してよいか。出してよければ記録する
    public mutating func shouldShow(_ message: String, now: Date = Date()) -> Bool {
        if let last = lastShown[message], now.timeIntervalSince(last) < interval { return false }
        lastShown[message] = now
        return true
    }
}
