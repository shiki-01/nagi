// IMKit 依存の層（IMKInputController・候補ウィンドウ等）は今後ここに実装する。
// Linux ビルドを壊さないよう、中身は macOS 限定にする。
#if os(macOS)
import InputMethodKit
import NagiClient

/// プレースホルダ。実装は後続タスク
enum NagiIMEPlaceholder {}
#endif
