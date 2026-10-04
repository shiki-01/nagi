import Foundation

/// 12 キーの「小゛゜」キー用: かなの 濁点・半濁点・小書きの循環
enum KanaTransform {
    /// 循環する文字の並び（先頭から順に次の文字へ進み、末尾の次は先頭に戻る）
    private static let cycles: [[Character]] = {
        let strings: [String] = [
            "あぁ", "いぃ", "うぅゔ", "えぇ", "おぉ",
            "かが", "きぎ", "くぐ", "けげ", "こご",
            "さざ", "しじ", "すず", "せぜ", "そぞ",
            "ただ", "ちぢ", "つっづ", "てで", "とど",
            "はばぱ", "ひびぴ", "ふぶぷ", "へべぺ", "ほぼぽ",
            "やゃ", "ゆゅ", "よょ", "わゎ",
        ]
        return strings.map { Array($0) }
    }()

    /// `c` の次の形。循環に含まれない文字は nil
    static func next(after c: Character) -> Character? {
        for cycle in cycles {
            if let i = cycle.firstIndex(of: c) {
                return cycle[(i + 1) % cycle.count]
            }
        }
        return nil
    }
}

enum TextMath {
    /// 末尾が `text` である確定済みテキストから、UTF-16 コード単位で `utf16Length` 分を取り消すために
    /// `deleteBackward` を呼ぶ回数（`deleteBackward` は 1 文字ずつ消すため）。
    /// `text` が足りないときは、不足分を 1 単位 = 1 回として数える
    static func deleteCount(utf16Length: Int, endingOf text: String) -> Int {
        guard utf16Length > 0 else { return 0 }
        var units = 0
        var count = 0
        for ch in text.reversed() {
            if units >= utf16Length { break }
            units += String(ch).utf16.count
            count += 1
        }
        if units >= utf16Length { return count }
        return count + (utf16Length - units)
    }
}
