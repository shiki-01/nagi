import Foundation
import CoreGraphics

/// 描画領域に配置したキー 1 つ分の矩形
struct KeyFrame {
    /// 行の添字
    let row: Int
    /// 行内のキーの添字（空白を含む）
    let index: Int
    let key: KeyDef
    /// キーのセル全体（キー間の隙間を含む。描画時に内側へ縮める）
    let rect: CGRect
}

enum LayoutGeometry {
    /// レイアウト定義を `size` の領域に敷き詰めたときの各キーの矩形。空白は含まない。
    /// 並び順は定義の順（行→列）で、`size` に依存しない
    static func frames(for layout: KeyboardLayout, in size: CGSize) -> [KeyFrame] {
        let totalHeight = layout.rows.reduce(0.0) { $0 + $1.relativeHeight }
        guard totalHeight > 0, size.width > 0, size.height > 0 else { return [] }
        var result: [KeyFrame] = []
        var y: CGFloat = 0
        for (r, row) in layout.rows.enumerated() {
            let height = size.height * CGFloat(row.relativeHeight / totalHeight)
            let totalWidth = row.keys.reduce(0.0) { $0 + $1.relativeWidth }
            if totalWidth > 0 {
                var x: CGFloat = 0
                for (i, key) in row.keys.enumerated() {
                    let width = size.width * CGFloat(key.relativeWidth / totalWidth)
                    if !key.isSpacer {
                        let rect = CGRect(x: x, y: y, width: width, height: height)
                        result.append(KeyFrame(row: r, index: i, key: key, rect: rect))
                    }
                    x += width
                }
            }
            y += height
        }
        return result
    }
}
