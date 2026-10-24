import Combine
import Foundation

/// キーボード全体で共有する見た目の状態（docs/keyboard-style-spec.md §4.1、§7.3）。
/// 局所の状態（pressed など）は各キー View が持つので、ここにあるのは複数のキーが参照するものだけ
final class KeyboardVisualState: ObservableObject {
    /// スペースのドラッグ移動中なら、起点のキーの id（`spaceDrag` の状態）。値が変わるときだけ更新する
    /// （ドラッグ中の毎フレームは更新しない）
    @Published private(set) var spaceDragOrigin: String?

    func setSpaceDragOrigin(_ id: String?) {
        if spaceDragOrigin != id { spaceDragOrigin = id }
    }
}

/// `released`（指を離した直後）の保持。`begin` で `ms` の間だけ `active` になり、`cancel`（再び押された・View が消えた）で即座に抜ける。
/// 正常な終了（`onEnded`）でだけ `begin` する。システムにジェスチャを奪われた取り消しでは呼ばない。
/// `released` を使うスタイルが無いキーは `begin` を呼ばないので、公開値は変わらず再描画も起きない
final class ReleaseHold: ObservableObject {
    @Published private(set) var active = false
    private var task: Task<Void, Never>?

    func begin(ms: Double) {
        task?.cancel()
        task = nil
        guard ms > 0 else {
            if active { active = false }
            return
        }
        active = true
        let nanos = UInt64(ms * 1_000_000)
        task = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: nanos)
            guard !Task.isCancelled else { return }
            self?.active = false
        }
    }

    func cancel() {
        task?.cancel()
        task = nil
        if active { active = false }
    }
}
