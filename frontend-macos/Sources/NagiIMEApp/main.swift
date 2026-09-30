// macOS の入力メソッドアプリのエントリポイント。Linux ではビルドだけ通る空の実行ファイルになる。
#if os(macOS)
import AppKit
import NagiIME

let options = HostOptions.parse(
    arguments: Array(CommandLine.arguments.dropFirst()),
    environment: ProcessInfo.processInfo.environment)

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
