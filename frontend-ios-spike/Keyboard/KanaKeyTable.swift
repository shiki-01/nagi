import Foundation

/// 12 キーのキー名（`flick` の `kana` フィールド）とコアの `KanaKey`、および押下ポップアップに出す
/// 5 つの文字（中央・左・上・右・下の順）の対応。
///
/// 文字の表はコアの `get_key_table`（`touch.rs` の表が単一の情報源）の結果を `install` で受け取って使う。
/// 取得できなかったとき（コアが応答しない・旧コア）のために、同じ内容の複製表をフォールバックとして残している
/// （表示と、コアが使えないときの直接入力にだけ使う。入力される文字の決定は常にコアが行う）。
/// 呼び出しはメインスレッドのみ
enum KanaKeyTable {
    struct Entry {
        let key: KanaKey
        /// 中央（タップ）・左・上・右・下
        let chars: [String]
    }

    /// `get_key_table` から取得した表（取得前・失敗時は nil）
    nonisolated(unsafe) private static var fetched: [KanaKey: Entry]?

    /// 取得を試みたか（成否を問わない。起動時に 1 回だけ取得する）
    nonisolated(unsafe) private(set) static var fetchAttempted = false

    /// キー名（先頭の 1 文字）と `KanaKey` の対応。句読点キーは「、」（または「、。？！」など先頭が「、」の名前）で指定する
    private static let names: [Character: KanaKey] = [
        "あ": .a, "か": .ka, "さ": .sa, "た": .ta, "な": .na, "は": .ha, "ま": .ma, "や": .ya, "ら": .ra, "わ": .wa,
        "、": .punct,
    ]

    /// フォールバックの複製表（`core/engine/src/touch.rs` の `key_chars` と同じ内容）
    private static let fallback: [KanaKey: Entry] = [
        .a: Entry(key: .a, chars: ["あ", "い", "う", "え", "お"]),
        .ka: Entry(key: .ka, chars: ["か", "き", "く", "け", "こ"]),
        .sa: Entry(key: .sa, chars: ["さ", "し", "す", "せ", "そ"]),
        .ta: Entry(key: .ta, chars: ["た", "ち", "つ", "て", "と"]),
        .na: Entry(key: .na, chars: ["な", "に", "ぬ", "ね", "の"]),
        .ha: Entry(key: .ha, chars: ["は", "ひ", "ふ", "へ", "ほ"]),
        .ma: Entry(key: .ma, chars: ["ま", "み", "む", "め", "も"]),
        .ya: Entry(key: .ya, chars: ["や", "（", "ゆ", "）", "よ"]),
        .ra: Entry(key: .ra, chars: ["ら", "り", "る", "れ", "ろ"]),
        .wa: Entry(key: .wa, chars: ["わ", "を", "ん", "ー", "〜"]),
        .punct: Entry(key: .punct, chars: ["、", "。", "？", "！", "…"]),
    ]

    /// コアから取得した表を使う。`result` が空なら何もしない（フォールバックのまま）
    static func install(_ result: KeyTableResult?) {
        fetchAttempted = true
        guard let result = result, !result.keys.isEmpty else { return }
        var table: [KanaKey: Entry] = [:]
        for item in result.keys {
            table[item.key] = Entry(key: item.key, chars: [item.center, item.left, item.up, item.right, item.down])
        }
        fetched = table
    }

    /// コアの `KanaKey` から項目を返す（コアが使えないときに、押されたキーの文字を直接入れるため）
    static func entry(for key: KanaKey) -> Entry? {
        fetched?[key] ?? fallback[key]
    }

    /// ジェスチャに対応する文字（`chars` は 中央・左・上・右・下 の順）
    static func char(for key: KanaKey, gesture: KeyGesture) -> String? {
        guard let chars = entry(for: key)?.chars, chars.count == 5 else { return nil }
        switch gesture {
        case .tap, .toggle: return chars[0]
        case .flickLeft: return chars[1]
        case .flickUp: return chars[2]
        case .flickRight: return chars[3]
        case .flickDown: return chars[4]
        }
    }

    /// キー名（先頭の 1 文字で引く）から項目を返す
    static func entry(forName name: String) -> Entry? {
        guard let first = name.first, let key = names[first] else { return nil }
        return entry(for: key)
    }
}
