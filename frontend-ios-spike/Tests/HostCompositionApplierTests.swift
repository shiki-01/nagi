import UIKit
import XCTest

/// `UITextView`（UITextInput）を `KeyboardHost` として使うアダプタ。
/// 実機の `textDocumentProxy` と同じ操作（setMarkedText / unmarkText / insertText / deleteBackward /
/// adjustTextPosition 相当）を、UIKit の実際のテキスト入力の実装に対して行う
private final class TextViewHost: KeyboardHost {
    let window: UIWindow
    let textView: UITextView

    init(initialText: String = "") {
        window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        textView = UITextView(frame: window.bounds)
        window.addSubview(textView)
        window.makeKeyAndVisible()
        // first responder でないと marked text が動かないことがあるので、ウィンドウに載せて取得する
        _ = textView.becomeFirstResponder()
        if !initialText.isEmpty {
            textView.text = initialText
            textView.selectedRange = NSRange(location: initialText.utf16.count, length: 0)
        }
    }

    var text: String { textView.text ?? "" }
    var hasMarkedText: Bool { textView.markedTextRange != nil }

    /// 変換中の範囲の文字列（なければ nil）
    var markedString: String? {
        guard let range = textView.markedTextRange else { return nil }
        return textView.text(in: range)
    }

    /// キャレット位置（UTF-16）
    var caret: Int { textView.selectedRange.location }

    func hostInsertText(_ text: String) { textView.insertText(text) }
    func hostDeleteBackward() { textView.deleteBackward() }

    func hostSetMarkedText(_ text: String, cursor: Int) {
        let position = min(max(cursor, 0), text.utf16.count)
        textView.setMarkedText(text, selectedRange: NSRange(location: position, length: 0))
    }

    func hostClearMarkedText() {
        textView.setMarkedText("", selectedRange: NSRange(location: 0, length: 0))
        textView.unmarkText()
    }

    func hostUnmarkText() { textView.unmarkText() }

    func hostAdjustCursor(by offset: Int) {
        let length = (textView.text ?? "").utf16.count
        let target = min(max(textView.selectedRange.location + offset, 0), length)
        textView.selectedRange = NSRange(location: target, length: 0)
    }

    func hostAdvanceToNextInputMode() {}
    func hostDismissKeyboard() {}

    func hostContextBeforeInput() -> String? {
        let utf16 = Array(text.utf16)
        let end = min(textView.selectedRange.location, utf16.count)
        return String(decoding: utf16[0..<end], as: UTF16.self)
    }

    func hostIsSecure() -> Bool { false }
}

final class HostCompositionApplierTests: XCTestCase {
    private func makeApplier(_ host: TextViewHost, deferResume: Bool = false) -> HostCompositionApplier {
        HostCompositionApplier(host: host, deferResumeAfterCommit: deferResume, verifyDelay: nil)
    }

    /// 次のメインキューの周回まで進める（確定直後の再開の遅延を待つ）
    private func spinMainQueue() {
        // RunLoop.run(until:) はソースが無いとすぐ戻るため、メインキューへ積んだ式の完了を待つ。
        // キューは先入れ先出しなので、先に積まれた再開の処理が終わってから fulfill される
        let done = expectation(description: "main queue drained")
        DispatchQueue.main.async { done.fulfill() }
        wait(for: [done], timeout: 2)
    }

    // MARK: (a) 部分確定のあとの全文確定

    private func runPartialThenFullCommit(deferResume: Bool, initialText: String = "") {
        let host = TextViewHost(initialText: initialText)
        let applier = makeApplier(host, deferResume: deferResume)

        applier.setMarked("今日はいい天気ですね", cursor: 10)
        XCTAssertEqual(host.text, initialText + "今日はいい天気ですね")
        XCTAssertEqual(host.markedString, "今日はいい天気ですね")

        // 部分確定: 「今日は」を確定して、残りを変換中にする
        applier.commit("今日は")
        applier.setMarked("いいてんきですね", cursor: 8)
        if deferResume {
            spinMainQueue()
        }
        XCTAssertEqual(host.text, initialText + "今日はいいてんきですね")
        XCTAssertEqual(host.markedString, "いいてんきですね")
        XCTAssertTrue(applier.verifyTail(committed: "今日は"))

        // 全文確定: 残りを「いい天気ですね」に置き換えて確定する（実機で二重になった手順）
        applier.commit("いい天気ですね")
        XCTAssertEqual(host.text, initialText + "今日はいい天気ですね")
        XCTAssertFalse(host.hasMarkedText, "確定後に下線（変換中）が残らない")
        XCTAssertEqual(applier.shownMarked, "")
        XCTAssertEqual(host.caret, (initialText + "今日はいい天気ですね").utf16.count)
    }

    func testPartialCommitThenFullCommitDoesNotDuplicate() {
        runPartialThenFullCommit(deferResume: false)
    }

    func testPartialCommitThenFullCommitWithDeferredResume() {
        runPartialThenFullCommit(deferResume: true)
    }

    func testPartialCommitThenFullCommitAfterExistingText() {
        runPartialThenFullCommit(deferResume: false, initialText: "こんにちは。")
    }

    func testRepeatedPartialAndFullCommitsDoNotAccumulate() {
        let host = TextViewHost()
        let applier = makeApplier(host)
        for _ in 0..<3 {
            applier.setMarked("今日はいい天気ですね", cursor: 10)
            applier.commit("今日は")
            applier.setMarked("いいてんきですね", cursor: 8)
            applier.commit("いい天気ですね")
        }
        XCTAssertEqual(host.text, String(repeating: "今日はいい天気ですね", count: 3))
        XCTAssertFalse(host.hasMarkedText)
    }

    func testCommitBeforeDeferredResumeFlushesItFirst() {
        let host = TextViewHost()
        let applier = makeApplier(host, deferResume: true)
        applier.setMarked("今日はいい天気ですね", cursor: 10)
        applier.commit("今日は")
        applier.setMarked("いいてんきですね", cursor: 8) // 遅延される
        // メインキューが周回する前に次の確定が来ても、残りの変換中を先にホストへ反映してから消す
        applier.commit("いい天気ですね")
        XCTAssertEqual(host.text, "今日はいい天気ですね")
        XCTAssertFalse(host.hasMarkedText)
        spinMainQueue()
        XCTAssertEqual(host.text, "今日はいい天気ですね")
    }

    func testClearBeforeDeferredResumeDropsIt() {
        let host = TextViewHost()
        let applier = makeApplier(host, deferResume: true)
        applier.setMarked("あいう", cursor: 3)
        applier.commit("あ")
        applier.setMarked("いう", cursor: 2) // 遅延される
        applier.clear()
        spinMainQueue()
        XCTAssertEqual(host.text, "あ")
        XCTAssertFalse(host.hasMarkedText)
    }

    // MARK: (b) 全文確定のみ

    func testFullCommitOnly() {
        let host = TextViewHost()
        let applier = makeApplier(host)
        applier.setMarked("今日は天気", cursor: 5)
        applier.commit("今日は天気")
        XCTAssertEqual(host.text, "今日は天気")
        XCTAssertFalse(host.hasMarkedText)
        XCTAssertTrue(applier.verifyTail(committed: "今日は天気"))
    }

    func testFullCommitReplacesMarkedWithDifferentText() {
        let host = TextViewHost(initialText: "A")
        let applier = makeApplier(host)
        applier.setMarked("きょう", cursor: 3)
        applier.commit("今日")
        XCTAssertEqual(host.text, "A今日")
        XCTAssertFalse(host.hasMarkedText)
    }

    func testCommitWithoutMarkedTextJustInserts() {
        let host = TextViewHost(initialText: "A")
        let applier = makeApplier(host)
        applier.commit("B")
        XCTAssertEqual(host.text, "AB")
    }

    // MARK: (c) キャレットが変換中の途中にある部分確定

    func testPartialCommitWithCaretInsideMarkedText() {
        let host = TextViewHost(initialText: "こんにちは。")
        let applier = makeApplier(host)
        applier.setMarked("今日はいい天気ですね", cursor: 3)
        XCTAssertEqual(host.caret, "こんにちは。今日は".utf16.count, "キャレットが変換中の途中にある")

        applier.commit("今日は")
        applier.setMarked("いいてんきですね", cursor: 8)
        XCTAssertEqual(host.text, "こんにちは。今日はいいてんきですね")
        XCTAssertEqual(host.markedString, "いいてんきですね")

        applier.commit("いい天気ですね")
        XCTAssertEqual(host.text, "こんにちは。今日はいい天気ですね")
        XCTAssertFalse(host.hasMarkedText)
    }

    func testFullCommitWithCaretInsideMarkedText() {
        let host = TextViewHost(initialText: "前")
        let applier = makeApplier(host)
        applier.setMarked("きょうは", cursor: 1)
        applier.commit("今日は")
        XCTAssertEqual(host.text, "前今日は")
        XCTAssertFalse(host.hasMarkedText)
    }

    // MARK: (d) 連続する setMarked の更新

    func testConsecutiveMarkedUpdatesReplaceTheMarkedRange() {
        let host = TextViewHost(initialText: "前")
        let applier = makeApplier(host)
        for (text, expected) in [("k", "前k"), ("き", "前き"), ("きょ", "前きょ"), ("きょう", "前きょう")] {
            applier.setMarked(text, cursor: text.utf16.count)
            XCTAssertEqual(host.text, expected)
            XCTAssertEqual(host.markedString, text)
        }
        // カーソル位置だけの更新
        applier.setMarked("きょう", cursor: 1)
        XCTAssertEqual(host.text, "前きょう")
        applier.clear()
        XCTAssertEqual(host.text, "前")
        XCTAssertFalse(host.hasMarkedText)
        applier.setMarked("あ", cursor: 1)
        XCTAssertEqual(host.text, "前あ")
    }

    func testKeepInHostLeavesTextWithoutUnderline() {
        let host = TextViewHost()
        let applier = makeApplier(host)
        applier.setMarked("かな", cursor: 2)
        applier.keepInHost()
        XCTAssertEqual(host.text, "かな")
        XCTAssertFalse(host.hasMarkedText)
        XCTAssertEqual(applier.shownMarked, "")
    }

    func testVerifyTailReportsMismatch() {
        let host = TextViewHost()
        let applier = makeApplier(host)
        applier.commit("今日は")
        host.textView.insertText("余分") // 想定外の本文の変化
        XCTAssertFalse(applier.verifyTail(committed: "今日は"))
    }
}
