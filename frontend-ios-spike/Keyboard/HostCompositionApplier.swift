import Foundation

/// ホストアプリのテキスト入力欄への操作（`UIInputViewController` が実装する）
protocol KeyboardHost: AnyObject {
    func hostInsertText(_ text: String)
    func hostDeleteBackward()
    /// 変換中の文字列（下線付き）を表示する。`cursor` はその中のカーソル位置（UTF-16）
    func hostSetMarkedText(_ text: String, cursor: Int)
    /// 変換中の表示を消す（確定した文字列は別に `hostInsertText` で入れる）
    func hostClearMarkedText()
    /// 変換中の範囲を確定扱いにする（文字は消さない）
    func hostUnmarkText()
    /// カーソル移動（文字数単位）
    func hostAdjustCursor(by offset: Int)
    func hostAdvanceToNextInputMode()
    /// キーボードを閉じる
    func hostDismissKeyboard()
    /// カーソル手前のテキスト（`documentContextBeforeInput`。変換中の文字列を含みうる）
    func hostContextBeforeInput() -> String?
    /// パスワード欄など、学習・文脈の送信・ログを止めるべき入力欄か
    func hostIsSecure() -> Bool
}

/// 変換中の文字列（マーク）と確定の手順をホストへ適用する。
///
/// `textDocumentProxy.setMarkedText` は非同期の XPC で、直前の `unmarkText` / `deleteBackward` / `insertText`
/// の直後に作った変換中の範囲をホスト側が追跡できず、次の `setMarkedText` による置換が効かないことが実機で起きた
/// （部分確定のあとの全文確定で、変換中のはずの文字が平文で残って二重になる）。
/// そのため確定は常に「範囲を確定扱いにする → 表示中の文字数だけ deleteBackward → insertText」の決定的な手順にし、
/// 置換（`setMarkedText(commit)` → `unmarkText`）には頼らない。
final class HostCompositionApplier {
    weak var host: KeyboardHost?

    /// ホストに表示している（べき）変換中の文字列とカーソル位置（UTF-16）。
    /// 確定直後の再開を遅らせている間は、ホストへの反映が済んでいなくてもこの値が最新
    private(set) var shownMarked = ""
    private(set) var shownCursor = 0

    /// 確定の直後に変換中を再開する `setMarkedText` を、メインキューを 1 周遅らせてから送るか
    private let deferResumeAfterCommit: Bool
    /// 確定後に本文の末尾を確かめるまでの時間（秒）。nil なら確かめない
    private let verifyDelay: TimeInterval?

    /// 反映待ちの `setMarkedText`
    private var pending: (text: String, cursor: Int)?
    /// 確定の直後（同じメインキューの 1 周の間）か
    private var justCommitted = false
    /// 操作の通し番号。遅延した検証が、その後の操作で無効になったかの判定に使う
    private var operationCount = 0

    init(host: KeyboardHost? = nil, deferResumeAfterCommit: Bool = true, verifyDelay: TimeInterval? = 0.3) {
        self.host = host
        self.deferResumeAfterCommit = deferResumeAfterCommit
        self.verifyDelay = verifyDelay
    }

    private var isSecure: Bool { host?.hostIsSecure() ?? false }

    private func quote(_ text: String) -> String {
        DiagLog.quote(text, redact: isSecure)
    }

    // MARK: - 変換中の表示

    /// ホストへ渡す `setMarkedText` のキャレット位置（UTF-16）。
    /// 変換中（注目文節あり）はコアの cursor に関わらず常に末尾（確定時にキャレットが手前に残らないように）。
    /// 入力中は、コアの cursor を編集カーソルとして使う
    static func markedCursor(coreCursor: Int, text: String, converting: Bool) -> Int {
        let end = text.utf16.count
        return converting ? end : min(max(coreCursor, 0), end)
    }

    /// 変換中の文字列を表示・更新する
    func setMarked(_ text: String, cursor: Int) {
        operationCount += 1
        if deferResumeAfterCommit && justCommitted {
            // 確定（unmark / delete / insert）の直後の最初の setMarked は、ホストが範囲を追跡できず不安定になりうる。
            // メインキューを 1 周遅らせて送る（その間に別の操作が来たら flush で先に反映する）
            DiagLog.log("host setMarked deferred \(quote(text)) cursor=\(cursor)")
            pending = (text, cursor)
            DispatchQueue.main.async { [weak self] in self?.flushPending() }
        } else {
            pending = nil
            DiagLog.log("host setMarked \(quote(text)) cursor=\(cursor)")
            host?.hostSetMarkedText(text, cursor: cursor)
        }
        shownMarked = text
        shownCursor = cursor
    }

    /// 変換中の表示を消す（変換中の文字列が空になった）
    func clear() {
        operationCount += 1
        if pending != nil {
            // まだホストに送っていないので、捨てるだけでよい
            DiagLog.log("host clearMarked (pending dropped)")
            pending = nil
        } else {
            DiagLog.log("host clearMarked")
            host?.hostClearMarkedText()
        }
        shownMarked = ""
        shownCursor = 0
    }

    /// 変換中の文字列を、下線を外して本文に残す（確定扱い。文字は消さない）
    func keepInHost() {
        flushPending()
        guard !shownMarked.isEmpty else { return }
        operationCount += 1
        DiagLog.log("keep preedit in host len=\(shownMarked.count)")
        host?.hostUnmarkText()
        shownMarked = ""
        shownCursor = 0
    }

    /// 入力欄そのものが切り替わった。前の欄に入っている文字は確定扱いなので、ホストには何もしない
    func reset() {
        operationCount += 1
        if pending != nil { DiagLog.log("host pending dropped (document changed)") }
        pending = nil
        shownMarked = ""
        shownCursor = 0
    }

    // MARK: - 確定

    /// `text` を確定する。変換中の文字列があれば、それを消して `text` に置き換える（部分確定・全文確定とも同じ手順）。
    /// 呼び出し側は、残りの変換中があれば確定の後に `setMarked` で入れる
    func commit(_ text: String) {
        guard !text.isEmpty else { return }
        flushPending()
        operationCount += 1
        if shownMarked.isEmpty {
            DiagLog.log("host insertText \(quote(text))")
            host?.hostInsertText(text)
        } else {
            let end = shownMarked.utf16.count
            DiagLog.log("host commit \(quote(text)) delete=\(shownMarked.count) cursor=\(shownCursor)/\(end) shownMarked=\(quote(shownMarked))")
            // キャレットが変換中の文字列の途中（先頭など）にあると、後ろへ消すと手前の本文を削ってしまう。
            // adjustTextPosition は実機で非同期に効くため頼らず、同じ文字列の setMarkedText（置換範囲が同じ）で
            // キャレットを末尾へ置いてから確定する
            if shownCursor != end {
                host?.hostSetMarkedText(shownMarked, cursor: end)
            }
            // 範囲を確定扱いにして平文にする
            host?.hostUnmarkText()
            for _ in 0..<shownMarked.count { host?.hostDeleteBackward() }
            host?.hostInsertText(text)
        }
        shownMarked = ""
        shownCursor = 0
        justCommitted = true
        DispatchQueue.main.async { [weak self] in self?.justCommitted = false }
        scheduleVerify(committed: text)
    }

    /// 反映待ちの `setMarked` があれば今すぐホストへ送る
    func flushPending() {
        guard let p = pending else { return }
        pending = nil
        DiagLog.log("host setMarked (flushed) \(quote(p.text)) cursor=\(p.cursor)")
        host?.hostSetMarkedText(p.text, cursor: p.cursor)
    }

    // MARK: - 本文の整合の確認

    private func scheduleVerify(committed: String) {
        guard let delay = verifyDelay else { return }
        let count = operationCount
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self = self, self.operationCount == count else { return }
            _ = self.verifyTail(committed: committed)
        }
    }

    /// 確定の直後に、ホストの本文の末尾が期待どおり（確定した文字列 + キャレットまでの変換中の文字列）かを確かめる。
    /// ずれていれば `host MISMATCH` を記録して false を返す。確かめられない（文脈が取れない・反映待ちがある）ときは true
    @discardableResult
    func verifyTail(committed: String) -> Bool {
        guard pending == nil, let host = host, let context = host.hostContextBeforeInput() else { return true }
        let units = Array(shownMarked.utf16)
        let upTo = min(max(shownCursor, 0), units.count)
        let expected = committed + String(decoding: units[0..<upTo], as: UTF16.self)
        if context.hasSuffix(expected) { return true }
        let actual = String(context.suffix(max(expected.count, 1) + 4))
        DiagLog.log("host MISMATCH expected tail=\(quote(expected)) actual tail=\(quote(actual)) shownMarked=\(quote(shownMarked))")
        return false
    }
}
