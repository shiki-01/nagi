import Foundation

// Mozc TSV の解析・書き出し（ユーザー辞書の形式）。ProductSession・SharedStore・コアに依存しない
// 純粋な Swift なので、単体テスト（Tests/MozcTSVTests.swift）の対象にしている。
// 形式と文字コードは core/dict/src/user.rs に厳密に合わせる:
//  - 1 行 `読み<TAB>単語<TAB>品詞[<TAB>コメント]`。空行と `#` `!` で始まる行は無視
//  - 読みはカタカナをひらがなに直す。各列は前後の空白を除く（スニペットの本文だけは除かない）
//  - 品詞が `スニペット` の行は、本文の `\n` `\t` `\\` をエスケープとして解釈する
//  - 文字コードは UTF-8（BOM 付き可）と UTF-16 LE/BE（BOM 付き）。書き出しは BOM なしの UTF-8

/// ユーザー辞書の 1 行
struct NagiUserDictEntry: Identifiable, Hashable {
    var id = UUID()
    var reading: String
    var surface: String
    var pos: String
    var comment: String
    /// 読み取り専用の行の出所のファイル名（編集対象のファイルでは空）
    var file: String = ""
    /// 取り込み時の元の行番号（1 始まり。画面で追加した行は 0）
    var line: Int = 0

    var isSnippet: Bool { pos == NagiMozcTSV.snippetPos }

    /// 重複の判定キー。コアも語は「読み + 単語」、スニペットは別枠で扱う
    var duplicateKey: String { (isSnippet ? "s" : "w") + "\t" + reading + "\t" + surface }
}

/// 読み込み時・取り込み時の問題（その行は無視される）
struct NagiUserDictIssue: Identifiable {
    let id = UUID()
    let file: String
    let line: Int
    let message: String

    var text: String {
        line > 0 ? "\(file) \(line) 行目: \(message)" : "\(file): \(message)"
    }
}

/// 読み込んだ結果
struct NagiUserDictSnapshot {
    var own: [NagiUserDictEntry] = []
    var ownIssues: [NagiUserDictIssue] = []
    /// 編集対象のファイルを読めなかった理由（このとき書き込まない。空にして上書きしてしまうのを防ぐ）
    var ownError: String?
    var readOnly: [NagiUserDictEntry] = []
    var otherIssues: [NagiUserDictIssue] = []
}

enum NagiMozcTSV {
    /// スニペットを表す品詞名（core/dict の `SNIPPET_POS`）
    static let snippetPos = "スニペット"
    /// スニペット本文の上限（文字数。docs/ios-settings.md §2.4）
    static let maxSnippetChars = 256
    /// 読み・単語（スニペット以外）の長さの上限（文字数）
    static let maxWordChars = 256

    // MARK: - 文字コード・エスケープ

    /// バイト列を文字列にする（core/dict の `decode_text` と同じ）
    static func decodeText(_ data: Data) -> String {
        let bytes = [UInt8](data)
        if bytes.count >= 2, bytes[0] == 0xFF, bytes[1] == 0xFE {
            return decodeUTF16(bytes.dropFirst(2), littleEndian: true)
        }
        if bytes.count >= 2, bytes[0] == 0xFE, bytes[1] == 0xFF {
            return decodeUTF16(bytes.dropFirst(2), littleEndian: false)
        }
        var body = bytes[...]
        if bytes.count >= 3, bytes[0] == 0xEF, bytes[1] == 0xBB, bytes[2] == 0xBF {
            body = bytes.dropFirst(3)
        }
        return String(decoding: body, as: UTF8.self)
    }

    private static func decodeUTF16(_ bytes: ArraySlice<UInt8>, littleEndian: Bool) -> String {
        let array = Array(bytes)
        var units: [UInt16] = []
        units.reserveCapacity(array.count / 2)
        var i = 0
        while i + 1 < array.count {
            let a = UInt16(array[i])
            let b = UInt16(array[i + 1])
            units.append(littleEndian ? (a | (b << 8)) : ((a << 8) | b))
            i += 2
        }
        return String(decoding: units, as: UTF16.self)
    }

    /// カタカナ → ひらがな（core/dict の `to_hiragana`。長音などはそのまま）
    static func toHiragana(_ s: String) -> String {
        var out = String.UnicodeScalarView()
        for u in s.unicodeScalars {
            if u.value >= 0x30A1 && u.value <= 0x30F6, let h = Unicode.Scalar(u.value - 0x60) {
                out.append(h)
            } else {
                out.append(u)
            }
        }
        return String(out)
    }

    /// スニペットの本文の `\n` `\t` `\\` を解釈する（core/dict の `unescape`）
    static func unescape(_ s: String) -> String {
        let scalars = Array(s.unicodeScalars)
        var out = String.UnicodeScalarView()
        var i = 0
        while i < scalars.count {
            let c = scalars[i]
            i += 1
            if c != "\\" {
                out.append(c)
                continue
            }
            if i >= scalars.count {
                out.append(c)
                break
            }
            let next = scalars[i]
            i += 1
            if next == "n" {
                out.append("\n")
            } else if next == "t" {
                out.append("\t")
            } else if next == "\\" {
                out.append("\\")
            } else {
                out.append(c)
                out.append(next)
            }
        }
        return String(out)
    }

    /// スニペットの本文の `\` 改行 タブをエスケープする（core/dict の `escape`）
    static func escape(_ s: String) -> String {
        var out = ""
        for u in s.unicodeScalars {
            if u == "\\" {
                out += "\\\\"
            } else if u == "\n" {
                out += "\\n"
            } else if u == "\t" {
                out += "\\t"
            } else {
                out.unicodeScalars.append(u)
            }
        }
        return out
    }

    /// 改行を `\n` にそろえる（CRLF・CR → LF）
    private static func normalizeNewlines(_ s: String) -> String {
        var out = ""
        var previousCR = false
        for u in s.unicodeScalars {
            if u == "\r" {
                out += "\n"
                previousCR = true
                continue
            }
            if u == "\n" && previousCR {
                previousCR = false
                continue
            }
            previousCR = false
            out.unicodeScalars.append(u)
        }
        return out
    }

    // MARK: - 解析・書き出し

    /// Mozc TSV のテキストを解析する。不正な行は問題として記録して読み飛ばす（core/dict の `add_tsv`）
    static func parse(_ text: String, file: String) -> (entries: [NagiUserDictEntry], issues: [NagiUserDictIssue]) {
        var entries: [NagiUserDictEntry] = []
        var issues: [NagiUserDictIssue] = []
        let all = Array(text.utf8)
        let lines = all.split(separator: 10, omittingEmptySubsequences: false)
        for (index, rawLine) in lines.enumerated() {
            var bytes = rawLine
            while bytes.last == 13 { bytes = bytes.dropLast() }
            if bytes.isEmpty { continue }
            if bytes.first == 35 || bytes.first == 33 { continue }
            let line = String(decoding: bytes, as: UTF8.self)
            if line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { continue }
            let cols = bytes
                .split(separator: 9, omittingEmptySubsequences: false)
                .map { String(decoding: $0, as: UTF8.self) }
            if cols.count < 3 {
                issues.append(NagiUserDictIssue(file: file, line: index + 1, message: "列が 3 つ未満です（読み・単語・品詞が必要です）"))
                continue
            }
            let reading = toHiragana(cols[0].trimmingCharacters(in: .whitespacesAndNewlines))
            let pos = cols[2].trimmingCharacters(in: .whitespacesAndNewlines)
            let surface = pos == snippetPos
                ? unescape(cols[1])
                : cols[1].trimmingCharacters(in: .whitespacesAndNewlines)
            if reading.isEmpty || surface.isEmpty {
                issues.append(NagiUserDictIssue(file: file, line: index + 1, message: "読みまたは単語が空です"))
                continue
            }
            let comment = cols.count > 3 ? cols[3].trimmingCharacters(in: .whitespacesAndNewlines) : ""
            entries.append(NagiUserDictEntry(reading: reading, surface: surface, pos: pos, comment: comment, line: index + 1))
        }
        return (entries, issues)
    }

    /// Mozc TSV にする（core/dict の `to_mozc_tsv`）
    static func serialize(_ entries: [NagiUserDictEntry]) -> String {
        var out = ""
        out.reserveCapacity(entries.count * 32)
        for e in entries {
            let surface = e.isSnippet ? escape(e.surface) : e.surface
            out += "\(e.reading)\t\(surface)\t\(e.pos)\t\(e.comment)\n"
        }
        return out
    }

    // MARK: - 検証

    /// 画面・取り込みの入力を検証して整える。問題があれば日本語のエラーを返す
    static func validate(_ entry: inout NagiUserDictEntry) -> String? {
        let reading = toHiragana(entry.reading.trimmingCharacters(in: .whitespacesAndNewlines))
        if reading.isEmpty { return "読みを入力してください" }
        if reading.count > maxWordChars { return "読みは \(maxWordChars) 文字以内にしてください（現在 \(reading.count) 文字）" }
        var bad: [String] = []
        for u in reading.unicodeScalars {
            let v = u.value
            let ok = (v >= 0x3041 && v <= 0x3096) || v == 0x30FC
            if !ok {
                let ch = String(Character(u))
                if !bad.contains(ch) { bad.append(ch) }
            }
        }
        if !bad.isEmpty {
            let shown = bad.prefix(5).map { $0 == " " || $0 == "\u{3000}" ? "空白" : "「\($0)」" }.joined(separator: " ")
            return "読みはひらがなで入力してください（使えない文字: \(shown)）"
        }

        let isSnippet = entry.isSnippet
        let pos = entry.pos.trimmingCharacters(in: .whitespacesAndNewlines)
        var surface = entry.surface
        if isSnippet {
            surface = normalizeNewlines(surface)
            if surface.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "本文を入力してください" }
            if surface.count > maxSnippetChars {
                return "本文は \(maxSnippetChars) 文字以内にしてください（現在 \(surface.count) 文字）"
            }
        } else {
            surface = surface.trimmingCharacters(in: .whitespacesAndNewlines)
            if surface.isEmpty { return "単語を入力してください" }
            if surface.count > maxWordChars { return "単語は \(maxWordChars) 文字以内にしてください（現在 \(surface.count) 文字）" }
            if surface.contains("\t") || surface.contains("\n") || surface.contains("\r") {
                return "単語に改行やタブは使えません（改行を含む定型文はスニペットにしてください）"
            }
            if pos.isEmpty { return "品詞を選んでください" }
            if pos.contains("\t") || pos.contains("\n") || pos.contains("\r") { return "品詞に改行やタブは使えません" }
        }
        let comment = entry.comment.trimmingCharacters(in: .whitespacesAndNewlines)
        if comment.contains("\t") || comment.contains("\n") || comment.contains("\r") {
            return "コメントに改行やタブは使えません"
        }
        entry.reading = reading
        entry.surface = surface
        entry.pos = pos
        entry.comment = comment
        return nil
    }
}
