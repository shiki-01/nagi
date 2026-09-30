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

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let host = NagiIMEHost(options: options)
do {
    try host.start()
} catch {
    NSLog("nagi: failed to start: \(error) (specify the engine with --engine or NAGI_ENGINE_PATH)")
    exit(1)
}
app.run()
#endif
