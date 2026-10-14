import Foundation

/// 12 キーのキー名（`flick` の `kana` フィールド）とコアの `KanaKey`、および押下ポップアップに出す
/// 5 つの文字（中央・左・上・右・下の順）の対応。
///
/// **重複**: 文字の表は `core/engine/src/touch.rs` の `key_chars` と同じ内容を表示のためだけに複製している。
/// 入力される文字の決定はコアが行う（ここの表は画面表示にのみ使う）。コアが表を返すメソッドを
/// 持つようになれば、これを置き換える（要望は報告に記載）。
enum KanaKeyTable {
    struct Entry {
        let key: KanaKey
        /// 中央（タップ）・左・上・右・下
        let chars: [String]
    }

    private static let entries: [Character: Entry] = [
        "あ": Entry(key: .a, chars: ["あ", "い", "う", "え", "お"]),
        "か": Entry(key: .ka, chars: ["か", "き", "く", "け", "こ"]),
        "さ": Entry(key: .sa, chars: ["さ", "し", "す", "せ", "そ"]),
        "た": Entry(key: .ta, chars: ["た", "ち", "つ", "て", "と"]),
        "な": Entry(key: .na, chars: ["な", "に", "ぬ", "ね", "の"]),
        "は": Entry(key: .ha, chars: ["は", "ひ", "ふ", "へ", "ほ"]),
        "ま": Entry(key: .ma, chars: ["ま", "み", "む", "め", "も"]),
        "や": Entry(key: .ya, chars: ["や", "（", "ゆ", "）", "よ"]),
        "ら": Entry(key: .ra, chars: ["ら", "り", "る", "れ", "ろ"]),
        "わ": Entry(key: .wa, chars: ["わ", "を", "ん", "ー", "〜"]),
        // 句読点キーは「、」（または「、。？！」など先頭が「、」の名前）で指定する
        "、": Entry(key: .punct, chars: ["、", "。", "？", "！", "…"]),
    ]

    /// キー名（先頭の 1 文字で引く）から項目を返す
    static func entry(forName name: String) -> Entry? {
        guard let first = name.first else { return nil }
        return entries[first]
    }
}
