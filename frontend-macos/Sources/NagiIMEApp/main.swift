// macOS の入力メソッドアプリのエントリポイント。Linux ではビルドだけ通る空の実行ファイルになる。
#if os(macOS)
import AppKit
import NagiIME

let options = HostOptions.parse(
    arguments: Array(CommandLine.arguments.dropFirst()),
    environment: ProcessInfo.processInfo.environment)

if options.register {
    // IMKServer・エンジンを起動せず、TIS への登録と有効化だけ行って終了する
    let report = InputSourceRegistrar.register(appURL: Bundle.main.bundleURL)
    print(report.text)
    exit(report.succeeded ? 0 : 1)
}

if options.diagnose {
    // IMKServer・エンジンを起動せず、入力ソースの属性を表示して終了する
    print(InputSourceRegistrar.diagnose())
    exit(0)
}

let app = NSApplication.shared
let host = NagiIMEHost(options: options)

if options.settings {
    // 開発時の確認用: IMKServer を起動せず、設定ウィンドウだけ開く（Cmd-Q で終了できるよう通常アプリ扱い）
    app.setActivationPolicy(.regular)
    do {
        try host.startSettingsOnly()
    } catch {
        NSLog("nagi: failed to start: \(error) (specify the engine with --engine or NAGI_ENGINE_PATH)")
        exit(1)
    }
    app.run()
    exit(0)
}

app.setActivationPolicy(.accessory)
do {
    try host.start()
} catch {
    NSLog("nagi: failed to start: \(error) (specify the engine with --engine or NAGI_ENGINE_PATH)")
    exit(1)
}
app.run()
#endif
