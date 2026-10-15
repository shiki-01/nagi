import Foundation

// 実機での確認作業を楽にするテスト素材（ホストアプリのテストビルド専用 UI が使う）。
// 画面・ファイル保存先に依存しない純粋な部分なので、単体テスト（Tests/TestAidsTests.swift）の対象にしている。

// MARK: - ユーザー辞書のサンプル

enum NagiTestSamples {
    /// `user_dict/` に作る読み取り専用サンプルのファイル名（`nagi-user.tsv` 以外は読み取り専用として扱われる）
    static let readonlySampleFileName = "readonly-sample.tsv"

    enum DictSample: CaseIterable {
        case utf8, utf16LE, invalidLines, duplicates

        var fileName: String {
            switch self {
            case .utf8: return "sample-utf8.tsv"
            case .utf16LE: return "sample-utf16le-msime.txt"
            case .invalidLines: return "sample-invalid-lines.tsv"
            case .duplicates: return "sample-duplicates.tsv"
            }
        }

        var title: String {
            switch self {
            case .utf8: return "UTF-8"
            case .utf16LE: return "UTF-16LE（BOM 付き・MS-IME 風）"
            case .invalidLines: return "不正行入り"
            case .duplicates: return "重複入り"
            }
        }

        /// 取り込んだときに期待する結果
        var expectation: String {
            switch self {
            case .utf8: return "5 件を取り込む（スニペット 1 件を含む）。読めなかった行は 0 件"
            case .utf16LE: return "3 件を取り込む（文字化けしない。! で始まるヘッダ行は無視）"
            case .invalidLines: return "3 件を取り込み、読めなかった行 3 件（列不足・単語が空・読みに漢字）。カタカナの読みはひらがなに直って取り込まれる"
            case .duplicates: return "2 件を取り込み、重複のため除外 3 件"
            }
        }
    }

    static func dictSampleData(_ sample: DictSample) -> Data {
        switch sample {
        case .utf8:
            return Data(utf8Text.utf8)
        case .utf16LE:
            return utf16LEData(msimeText)
        case .invalidLines:
            return Data(invalidText.utf8)
        case .duplicates:
            return Data(duplicatesText.utf8)
        }
    }

    /// UTF-8、BOM なし。コメント行と、スニペット（本文の `\n` は改行）を含む
    static let utf8Text = [
        "# Nagi テスト用 UTF-8",
        "てすと\tテスト\t名詞\tコメント付き",
        "きょうと\t京都\t地名",
        "とうきょう\t東京\t地名",
        "さとう\t佐藤\t人名",
        "ぎもん\tお世話になっております。\\n株式会社〇〇\tスニペット",
        "",
    ].joined(separator: "\n")

    /// MS-IME のエクスポート風（! のヘッダ行、読み・単語・品詞、CRLF）
    static let msimeText = [
        "!Microsoft IME Dictionary Tool",
        "!Version: 98.",
        "!Format:WORDLIST",
        "てすと\tテスト\t名詞",
        "やまだ\t山田\t人名",
        "おおさか\t大阪\t地名",
        "",
    ].joined(separator: "\r\n")

    /// 1 行目はコメント。3 行目は列不足、5 行目は単語が空、6 行目は読みに漢字（取り込み時の検証で弾かれる）。
    /// 4 行目のカタカナの読みはひらがなに直るので有効
    static let invalidText = [
        "# 不正行入り",
        "ただしい\t正しい\t名詞",
        "れつぶそく\t列不足",
        "カタカナ\tかたかなの読み\t名詞",
        "からたん\t\t名詞",
        "かんじ漢字\t漢字\t名詞",
        "ふつう\t普通\t名詞",
        "",
    ].joined(separator: "\n")

    /// 読み・単語が同じ行（品詞違い・カタカナの読み違いも重複）。有効な行は 5 行で、重複を除くと 2 件
    static let duplicatesText = [
        "ふくせい\t複製\t名詞",
        "ふくせい\t複製\t名詞",
        "フクセイ\t複製\t名詞",
        "ふくせい\t複製\t固有名詞",
        "べつ\t別\t名詞",
        "",
    ].joined(separator: "\n")

    /// BOM 付き UTF-16 LE にする
    static func utf16LEData(_ text: String) -> Data {
        var bytes: [UInt8] = [0xFF, 0xFE]
        for unit in text.utf16 {
            bytes.append(UInt8(unit & 0xFF))
            bytes.append(UInt8(unit >> 8))
        }
        return Data(bytes)
    }

    /// 読み取り専用サンプルの内容
    static let readonlySampleText = [
        "# Nagi テスト用: 読み取り専用ファイル（user_dict/ の nagi-user.tsv 以外）",
        "りーどおんりー\t読み取り専用サンプル\t名詞",
        "ろーどく\t読み取り専用の二つ目\t名詞",
        "",
    ].joined(separator: "\n")

    // MARK: 大量データ

    private static let digitKana: [Character] = ["あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ"]

    /// i 番目の読み（`てすと` + 10 進の各桁をひらがなにしたもの。i ごとに一意）
    static func bulkReading(_ index: Int) -> String {
        var text = ""
        for digit in String(index) {
            if let v = digit.wholeNumberValue { text.append(digitKana[v]) }
        }
        return "てすと" + text
    }

    /// i 番目の 1 行（品詞は名詞）
    static func bulkLine(_ index: Int) -> String {
        "\(bulkReading(index))\tテスト項目\(index)\t名詞\tbulk\n"
    }

    /// `count` 件の Mozc TSV を作る。`progress` は 0...1 を呼ぶ（バックグラウンドで使う想定）
    static func bulkTSV(count: Int, progress: ((Double) -> Void)? = nil) -> String {
        var out = ""
        out.reserveCapacity(count * 48)
        let step = max(1, count / 20)
        for i in 0..<max(0, count) {
            out += bulkLine(i)
            if let progress = progress, i % step == 0 { progress(Double(i) / Double(count)) }
        }
        progress?(1)
        return out
    }
}

// MARK: - テスト用レイアウト

struct NagiTestLayoutCase: Identifiable {
    enum Expectation {
        case success
        /// 取り込みの理由に含まれるはずの文字列
        case error(String)
    }

    /// バンドル内のファイル名（拡張子なし）
    let id: String
    let title: String
    let expectation: Expectation
    /// 取り込み後に割り当てられるモード（有効なレイアウトのみ）
    let assignMode: LayoutMode?
    /// ファイル内のレイアウト id（有効なレイアウトのみ。取り込み後の割り当てに使う）
    var layoutId: String? = nil

    var expectationText: String {
        switch expectation {
        case .success: return "成功"
        case .error(let reason): return "エラー: \(reason)"
        }
    }

    var isValid: Bool {
        if case .success = expectation { return true }
        return false
    }
}

enum NagiTestLayouts {
    /// バンドル内のフォルダ名（`config/keyboard/test-layouts`）
    static let folder = "test-layouts"

    static let cases: [NagiTestLayoutCase] = [
        NagiTestLayoutCase(id: "valid-toggle-kana", title: "かな toggle（longPress・（）・←→・layout 切替）",
                           expectation: .success, assignMode: .kana, layoutId: "test-toggle-kana"),
        NagiTestLayoutCase(id: "valid-toggle-alpha", title: "英字 toggle（longPress・←→・layout 切替）",
                           expectation: .success, assignMode: .alpha, layoutId: "test-toggle-alpha"),
        NagiTestLayoutCase(id: "valid-shortcut", title: "shortcut 専用（（）「」【】、複数手順）",
                           expectation: .success, assignMode: .alpha, layoutId: "test-shortcut"),
        NagiTestLayoutCase(id: "invalid-rows-9", title: "9 行（rows は 8 行まで）",
                           expectation: .error("rows は 1〜8 行"), assignMode: nil),
        NagiTestLayoutCase(id: "invalid-height-sum", title: "高さ比の合計 5（4.5 超）",
                           expectation: .error("行の高さの比の合計"), assignMode: nil),
        NagiTestLayoutCase(id: "invalid-id", title: "id 形式違反",
                           expectation: .error("id は英小文字"), assignMode: nil),
        NagiTestLayoutCase(id: "invalid-no-name", title: "name なし",
                           expectation: .error("name"), assignMode: nil),
        NagiTestLayoutCase(id: "invalid-label-17", title: "label 17 文字",
                           expectation: .error("label は 16 文字以内"), assignMode: nil),
        NagiTestLayoutCase(id: "invalid-unknown-kana", title: "未知の kana 名",
                           expectation: .error("未知のキー名"), assignMode: nil),
        NagiTestLayoutCase(id: "invalid-missing-target", title: "存在しない layout.target",
                           expectation: .error("存在するレイアウトでも組み込み名でもありません"), assignMode: nil),
        NagiTestLayoutCase(id: "invalid-steps-both", title: "steps の action / text 両指定",
                           expectation: .error("どちらか一方だけ"), assignMode: nil),
    ]

    /// 巨大 data の期待結果（ファイル / 貼り付け経由と、URL 経由で文言が違う）
    static let hugeExpectationImport = "エラー: JSON が大きすぎます（512 KB まで）"
    static let hugeExpectationURL = "エラー: data が大きすぎます（1024 KB まで）"

    static func url(for testCase: NagiTestLayoutCase, in bundle: Bundle = .main) -> URL? {
        bundle.url(forResource: testCase.id, withExtension: "json", subdirectory: folder)
    }

    /// 巨大な JSON を作る。有効なレイアウトに、未知の項目 `padding` を足して `bytes` バイト以上にする
    /// （未知の項目は無視されるので、大きさの検査だけが働く）
    static func hugeLayoutJSON(bytes: Int = 800 * 1024) -> Data {
        let head = #"{"schema":1,"id":"test-huge","name":"巨大","mode":"kana","rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]}],"padding":""#
        let tail = #""}"#
        let fill = String(repeating: "x", count: max(0, bytes - head.utf8.count - tail.utf8.count))
        return Data((head + fill + tail).utf8)
    }
}
