import Foundation
import ImageIO

// 画像（data URI）のヘッダ読み取り。デコードはしない（大きさだけ読む）。Keyboard/KeyImageCache.swift と
// 取り込み時の検査（Shared/LayoutAssetCheck.swift）が使う。

struct KeyImageSource {
    let data: Data
    let width: Int
    let height: Int

    /// data URI から元データと画素数を読む。デコードできない画像（壊れている・未対応）は nil
    static func read(uri: String) -> KeyImageSource? {
        guard let data = KeyImageMath.dataURIPayload(uri),
              let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width > 0, height > 0 else { return nil }
        return KeyImageSource(data: data, width: width, height: height)
    }

    /// 長辺を `maxPixel` に収めて縮小デコードする（`CGImageSourceCreateThumbnailAtIndex`。元の向きを反映）
    func thumbnail(maxPixel: Int) -> CGImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: max(1, maxPixel),
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
    }
}
