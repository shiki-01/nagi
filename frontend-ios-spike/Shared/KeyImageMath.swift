import Foundation

// 背景画像（`assets` / `fillImage`）の大きさ・矩形の計算（docs/keyboard-style-spec.md §6）。
// UIKit / SwiftUI に依存しない純粋関数（XCTest で直接検証する）。デコードとキャッシュは Keyboard/KeyImageCache.swift。

enum KeyImageMath {
    /// デコードする長辺の上限（px。§6.2）
    static let maxPixelCap = 1024
    /// デコード結果のキャッシュの上限（バイト。§6.2、§6.3）
    static let cacheCostLimit = 12 * 1024 * 1024
    /// 利用可能なメモリがこれ未満なら画像をデコードしない（MB。§6.3）
    static let lowMemoryThresholdMB = 40.0
    /// `tile` は 1pt = 3px（@3x として扱う。§6.3）
    static let tilePixelsPerPoint = 3.0
    /// これより小さい辺を持つ画像は `tile` にせず `fill` にする（px。§6.3）
    static let minTilePixels = 8
    /// `fill` / `fit` のデコードサイズをまとめる単位（px）。表示サイズの端数でキャッシュが増えないように切り上げる
    static let quantizeStep = 64

    /// メモリが逼迫しているか。`os_proc_available_memory()` が 0 を返す環境（シミュレータなど）は「不明」として逼迫とみなさない
    static func isLowMemory(availableMB: Double) -> Bool {
        availableMB > 0 && availableMB < lowMemoryThresholdMB
    }

    /// 実際の当て方。`tile` で画像が小さすぎるときは `fill`、知らない値も `fill`
    static func effectiveFit(_ fit: String, sourceWidth: Int, sourceHeight: Int) -> String {
        switch fit {
        case "fit":
            return "fit"
        case "tile":
            return min(sourceWidth, sourceHeight) < minTilePixels ? "fill" : "tile"
        default:
            return "fill"
        }
    }

    static func quantize(_ pixels: Int) -> Int {
        max(1, (pixels + quantizeStep - 1) / quantizeStep * quantizeStep)
    }

    /// デコードする長辺の最大ピクセル数（`kCGImageSourceThumbnailMaxPixelSize` に渡す）。
    /// 表示に必要な分（当て方ごとの拡大率 × 画面の倍率）を、`maxPixelCap` と元画像の大きさで頭打ちにする。
    /// `tile` は元の大きさ（1pt = 3px）で敷くので、画面の倍率に合わせた大きさ（量子化しない）
    static func maxPixel(fit: String, sourceWidth: Int, sourceHeight: Int, display: CGSize, screenScale: Double) -> Int {
        let longSide = max(sourceWidth, sourceHeight)
        guard sourceWidth > 0, sourceHeight > 0, display.width > 0, display.height > 0, screenScale > 0 else { return 1 }
        let kind = effectiveFit(fit, sourceWidth: sourceWidth, sourceHeight: sourceHeight)
        let needed: Double
        switch kind {
        case "tile":
            needed = Double(longSide) * screenScale / tilePixelsPerPoint
        case "fit":
            let ratio = min(Double(display.width) / Double(sourceWidth), Double(display.height) / Double(sourceHeight))
            needed = Double(longSide) * ratio * screenScale
        default:
            let ratio = max(Double(display.width) / Double(sourceWidth), Double(display.height) / Double(sourceHeight))
            needed = Double(longSide) * ratio * screenScale
        }
        var pixels = Int(min(needed, 1e6).rounded(.up))
        if kind != "tile" { pixels = quantize(pixels) }
        return max(1, min(pixels, maxPixelCap, longSide))
    }

    /// 長辺を `maxPixel` に収めて縮小したあとの大きさ（拡大はしない）
    static func downsampledSize(sourceWidth: Int, sourceHeight: Int, maxPixel: Int) -> (width: Int, height: Int) {
        let longSide = max(sourceWidth, sourceHeight)
        guard longSide > 0, maxPixel > 0 else { return (0, 0) }
        if longSide <= maxPixel { return (sourceWidth, sourceHeight) }
        let ratio = Double(maxPixel) / Double(longSide)
        return (max(1, Int((Double(sourceWidth) * ratio).rounded())), max(1, Int((Double(sourceHeight) * ratio).rounded())))
    }

    /// デコード結果のメモリ（バイト。RGBA 8bit）。`NSCache` の cost に使う
    static func cost(width: Int, height: Int) -> Int {
        max(0, width) * max(0, height) * 4
    }

    /// `fill`: 縦横比を保って全体を覆う矩形（コンテナの中心に合わせる。はみ出しは呼び出し側がクリップする）
    static func fillRect(image: CGSize, in container: CGSize) -> CGRect {
        scaledRect(image: image, in: container, cover: true)
    }

    /// `fit`: 縦横比を保って全体を収める矩形（コンテナの中心に合わせる。余白は背景色が見える）
    static func fitRect(image: CGSize, in container: CGSize) -> CGRect {
        scaledRect(image: image, in: container, cover: false)
    }

    private static func scaledRect(image: CGSize, in container: CGSize, cover: Bool) -> CGRect {
        guard image.width > 0, image.height > 0, container.width > 0, container.height > 0 else { return .zero }
        let widthRatio = container.width / image.width
        let heightRatio = container.height / image.height
        let scale = cover ? max(widthRatio, heightRatio) : min(widthRatio, heightRatio)
        let width = image.width * scale
        let height = image.height * scale
        return CGRect(x: (container.width - width) / 2, y: (container.height - height) / 2, width: width, height: height)
    }

    /// `tile` の 1 枚の大きさ（pt）。元の大きさ（px）を 1pt = 3px で割る
    static func tilePointSize(sourceWidth: Int, sourceHeight: Int) -> CGSize {
        CGSize(width: Double(sourceWidth) / tilePixelsPerPoint, height: Double(sourceHeight) / tilePixelsPerPoint)
    }

    /// デコード済み画像（長辺 `decodedLong` px）を `tilePointSize` の大きさで敷くための画像の倍率
    /// （`Image(decorative:scale:)` は px ÷ scale が pt になる）
    static func tileScale(decodedLong: Int, sourceLong: Int) -> Double {
        guard decodedLong > 0, sourceLong > 0 else { return 1 }
        return Double(decodedLong) * tilePixelsPerPoint / Double(sourceLong)
    }

    /// data URI（image/png か image/jpeg の base64）のバイト列。形式が違う・空・base64 が不正なら nil
    static func dataURIPayload(_ uri: String) -> Data? {
        for prefix in ["data:image/png;base64,", "data:image/jpeg;base64,"] where uri.hasPrefix(prefix) {
            let payload = String(uri.dropFirst(prefix.count))
            guard !payload.isEmpty, let data = Data(base64Encoded: payload), !data.isEmpty else { return nil }
            return data
        }
        return nil
    }
}
