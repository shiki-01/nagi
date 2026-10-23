#if os(macOS)
import AppKit
import NagiClient

/// 辞書・モデル・語彙の取得（同梱の `imserver --install-data` を実行する）。取得・検証・組み立てはコアが行い、
/// ここは起動・進捗の表示・エンジンの再起動だけを担う。データは配布元から利用者の手元へ直接取得する（再配布しない）
extension NagiIMEContext {
    /// 英語の語彙・カタカナ語の原語データが置かれているか（無くても変換は動くが、該当の機能は働かない）
    var hasEnglishLexicon: Bool {
        guard let dataDir else { return true }
        return FileManager.default.fileExists(atPath: dataDir + "/english/words.fst")
    }

    var hasLoanwordIndex: Bool {
        guard let dataDir else { return true }
        return FileManager.default.fileExists(atPath: dataDir + "/loanword/loanword.fst")
    }

    /// 辞書が置かれているか（無ければ読みがそのまま入力される）
    var hasDictionary: Bool {
        guard let dataDir else { return true }
        return FileManager.default.fileExists(atPath: dataDir + "/dict/nagi-dict.bin")
    }

    /// 辞書が無ければ、起動後の最初の入力時に一度だけ取得を勧める
    @MainActor
    func offerDataInstallIfNeeded() {
        guard !dataInstallOffered, !isInstallingData, !hasDictionary else { return }
        dataInstallOffered = true
        let alert = NSAlert()
        alert.messageText = "辞書と変換モデルをダウンロードしますか？"
        alert.informativeText =
            "Nagi で漢字に変換するには、辞書（Mozc）と変換モデル（zenz）が必要です。配布元から約 150MB を取得して、この Mac の中で組み立てます。後から入力メニューの「辞書とモデルをダウンロード…」でも入れられます。"
        alert.addButton(withTitle: "ダウンロード")
        alert.addButton(withTitle: "後で")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        Task { await installData() }
    }

    /// 取得を実行する（実行中なら何もしない）。終わったらエンジンを再起動して読み込ませる
    @MainActor
    func installData() async {
        guard !isInstallingData else {
            toast.show("辞書とモデルをダウンロード中です")
            return
        }
        guard let engineURL, let dataDir else {
            toast.show("変換エンジンの場所が分からないため、辞書とモデルを入れられません")
            return
        }
        isInstallingData = true
        defer { isInstallingData = false }
        toast.show("辞書とモデルのダウンロードを始めます")

        let process = Process()
        process.executableURL = engineURL
        process.arguments = ["--install-data", "--data-dir", dataDir, "--model", "small"]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            toast.show("辞書とモデルを入れられませんでした（変換エンジンを起動できません）")
            return
        }

        var quarter = -1
        var finished = false
        var changed = false
        var warningMessage: String?
        do {
            for try await line in pipe.fileHandleForReading.bytes.lines {
                guard let progress = DataInstallProgress.parse(line) else { continue }
                if case .done(let c) = progress {
                    finished = true
                    changed = c
                }
                var message = progress.message(lastModelQuarter: &quarter)
                if case .warning = progress {
                    // 最後の「入れました」で上書きされないよう、完了時にまとめて知らせる
                    warningMessage = message
                    continue
                }
                if case .done = progress, let warningMessage { message = warningMessage }
                if let message { toast.show(message) }
            }
        } catch {
            // 読み取りの失敗は終了コードで判断する
        }
        process.waitUntilExit()
        guard finished, process.terminationStatus == 0 else {
            if !finished { toast.show("辞書とモデルを入れられませんでした") }
            return
        }
        // 取得済みで何も変わっていなければ、読み込み直す必要はない
        if changed { await supervisor.restartNow() }
    }
}
#endif
