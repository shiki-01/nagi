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

/// コアの打鍵ログが代替として受け付ける文字（ひらがなと長音「ー」の 1 文字）の判定
enum KanaText {
    /// ひらがな 1 文字（ぁ U+3041 〜 ゖ U+3096）、または長音「ー」（U+30FC）
    static func isLoggableKana(_ text: String) -> Bool {
        let scalars = text.unicodeScalars
        guard scalars.count == 1, let scalar = scalars.first else { return false }
        return (scalar.value >= 0x3041 && scalar.value <= 0x3096) || scalar.value == 0x30FC
    }
}

/// 直前のフリックでコアへ送った確定かなと代替（`insert_reading` の引数）。
/// 「小゛゜」で次の形に変えるとき、代替を同じ変換で写して引き継ぐために覚えておく
struct LastFlick: Equatable {
    /// 確定したかな
    let reading: String
    /// 送った代替（確定したかな自身の距離を含む）
    let alternatives: [AltCandidate]

    /// 「小゛゜」（`KanaTransform` の循環）で次の形にしたもの。
    /// 代替の文字は同じ変換で写し、距離はそのまま引き継ぐ（変換後のかなの距離は、元の確定ゾーンの距離と同じ）。
    /// 変換が定義されない代替は落とす。`reading` が 1 文字でない、または変換が定義されなければ nil
    func transformed() -> LastFlick? {
        guard reading.count == 1,
              let c = reading.first,
              let next = KanaTransform.next(after: c)
        else { return nil }
        var mapped: [AltCandidate] = []
        for alt in alternatives {
            guard alt.text.count == 1,
                  let altChar = alt.text.first,
                  let altNext = KanaTransform.next(after: altChar)
            else { continue }
            mapped.append(AltCandidate(text: String(altNext), distance: alt.distance))
        }
        return LastFlick(reading: String(next), alternatives: mapped)
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
