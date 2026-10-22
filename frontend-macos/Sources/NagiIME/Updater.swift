#if os(macOS)
import AppKit
import NagiClient

/// アップデート（設計書 D 節）: latest.json の確認 → 配布物の取得 → SHA-256・ed25519 署名・コード署名
/// （Developer ID、同じ Team ID）の検証 → バンドルの差し替え → 再起動。
/// 起動時の自動確認はコア（`update.auto_check`）が通知し、ここは入力メニューからの確認と更新を担う
enum Updater {
    /// 確認先（core の `update_check::default_check_url()` の macOS 版と同じ）。`NAGI_UPDATE_URL` で上書きできる
    static var manifestURL: URL {
        if let s = ProcessInfo.processInfo.environment["NAGI_UPDATE_URL"], let url = URL(string: s) {
            return url
        }
        return URL(string: "https://nagi.shiki-01.com/updates/macos/latest.json")!
    }

    static var currentVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.0.0"
    }

    enum Failure: Error {
        case message(String)
        var text: String {
            switch self {
            case .message(let s): return s
            }
        }
    }

    /// 新しい版があれば返す
    static func check() async throws -> UpdateManifest? {
        var request = URLRequest(url: manifestURL, cachePolicy: .reloadIgnoringLocalCacheData)
        request.timeoutInterval = 10
        let (data, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw Failure.message("アップデート情報を取得できません")
        }
        let manifest = try JSONDecoder().decode(UpdateManifest.self, from: data)
        return UpdateVersion.isNewer(manifest.version, than: currentVersion) ? manifest : nil
    }

    /// 取得・検証して、実行中のバンドルを新しい版に差し替える（再起動は呼び出し側）
    static func install(_ manifest: UpdateManifest) async throws {
        let bundleURL = Bundle.main.bundleURL
        guard bundleURL.pathExtension == "app" else {
            throw Failure.message("アプリとして起動していないため更新できません")
        }
        guard let publicKey = Bundle.main.object(forInfoDictionaryKey: "NagiUpdatePublicKey") as? String
        else { throw Failure.message("このビルドには更新の検証用の公開鍵が入っていません") }
        guard let team = teamIdentifier(of: bundleURL) else {
            throw Failure.message(
                "このビルドは Developer ID で署名されていないため自動では更新できません。インストールスクリプトで入れ直してください")
        }
        guard let urlString = manifest.download_url, let url = URL(string: urlString),
            url.scheme == "https"
        else { throw Failure.message("アップデート情報の download_url が不正です") }

        let (data, response) = try await URLSession.shared.data(from: url)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw Failure.message("配布物をダウンロードできません")
        }
        do {
            try UpdateVerifier.verify(data, manifest: manifest, publicKeyHex: publicKey)
        } catch let e as UpdateVerificationError {
            throw Failure.message(e.message)
        }

        // 差し替えは同じボリューム上の一時ディレクトリから行う
        let work = try FileManager.default.url(
            for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: bundleURL,
            create: true)
        defer { try? FileManager.default.removeItem(at: work) }
        let zip = work.appendingPathComponent("Nagi.zip")
        try data.write(to: zip)
        let extracted = work.appendingPathComponent("extracted")
        guard run("/usr/bin/ditto", ["-x", "-k", zip.path, extracted.path]).status == 0 else {
            throw Failure.message("配布物を展開できません")
        }
        let newApp = extracted.appendingPathComponent("Nagi.app")
        guard FileManager.default.fileExists(atPath: newApp.path) else {
            throw Failure.message("配布物に Nagi.app が含まれていません")
        }
        // 同じ開発者（Team ID）の Developer ID 署名であること・公証済みであることを確かめる
        let requirement =
            "=anchor apple generic and certificate leaf[subject.OU] = \"\(team)\""
        guard
            run("/usr/bin/codesign", ["--verify", "--deep", "--strict", "-R", requirement, newApp.path])
                .status == 0
        else { throw Failure.message("新しい版のコード署名を検証できません") }
        guard run("/usr/sbin/spctl", ["--assess", "--type", "execute", newApp.path]).status == 0 else {
            throw Failure.message("新しい版が公証済みとして認められません")
        }
        _ = try FileManager.default.replaceItemAt(bundleURL, withItemAt: newApp)
    }

    /// バンドルのコード署名の Team ID（ad-hoc 署名・未署名なら nil）
    static func teamIdentifier(of url: URL) -> String? {
        let output = run("/usr/bin/codesign", ["-dv", "--verbose=2", url.path]).output
        for line in output.split(separator: "\n") where line.hasPrefix("TeamIdentifier=") {
            let team = String(line.dropFirst("TeamIdentifier=".count))
            return team == "not set" || team.isEmpty ? nil : team
        }
        return nil
    }

    /// 外部コマンドを実行し、終了コードと出力（標準出力 + 標準エラー）を返す
    @discardableResult
    private static func run(_ tool: String, _ arguments: [String]) -> (status: Int32, output: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: tool)
        process.arguments = arguments
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        do {
            try process.run()
        } catch {
            return (-1, "")
        }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, String(decoding: data, as: UTF8.self))
    }
}

extension NagiIMEContext {
    /// 入力メニューの「アップデートを確認…」
    @MainActor
    func checkForUpdates() async {
        let manifest: UpdateManifest?
        do {
            manifest = try await Updater.check()
        } catch {
            toast.show("アップデートを確認できませんでした")
            return
        }
        guard let manifest else {
            toast.show("最新版です（\(Updater.currentVersion)）")
            return
        }
        let alert = NSAlert()
        alert.messageText = "Nagi \(manifest.version) があります"
        alert.informativeText = "現在のバージョンは \(Updater.currentVersion) です。更新すると Nagi を再起動します。"
        alert.addButton(withTitle: "更新")
        alert.addButton(withTitle: "後で")
        if manifest.notes_url != nil { alert.addButton(withTitle: "リリースノート") }
        NSApp.activate(ignoringOtherApps: true)
        switch alert.runModal() {
        case .alertFirstButtonReturn:
            break
        case .alertThirdButtonReturn:
            if let s = manifest.notes_url, let url = URL(string: s) { NSWorkspace.shared.open(url) }
            return
        default:
            return
        }
        toast.show("Nagi \(manifest.version) をダウンロードしています")
        do {
            try await Updater.install(manifest)
        } catch let failure as Updater.Failure {
            toast.show(failure.text)
            return
        } catch {
            toast.show("更新できませんでした")
            return
        }
        await restartForUpdate()
    }

    /// 更新後の再起動: 入力中の文字列を確定し、エンジンを止めて（学習を保存）終了する。
    /// 入力メソッドは次に使われたときに OS が新しい版で起動し直す
    @MainActor
    func restartForUpdate() async {
        for controller in controllers.values { controller.value?.commitDisplayedTextLocally() }
        await supervisor.stop()
        exit(0)
    }
}
#endif
