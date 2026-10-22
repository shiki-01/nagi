import SwiftUI
import UIKit

// キーのアイコンの描画（キーボード拡張とホストアプリのプレビューで共有）。
// 拡張は通信も SVG の解釈もしない。取り込み時にホストアプリが保存した PNG（`KeyboardIconStore`）を読むだけ。

enum KeyboardIconImageCache {
    private static let cache = NSCache<NSString, UIImage>()

    /// 保存済みの Iconify アイコン（テンプレート画像。色は前景色に従う）。無ければ nil
    static func image(prefix: String, name: String) -> UIImage? {
        let key = KeyboardIconStore.fileName(prefix: prefix, name: name) as NSString
        if let cached = cache.object(forKey: key) { return cached }
        guard let url = KeyboardIconStore.fileURL(prefix: prefix, name: name),
              let data = try? Data(contentsOf: url),
              // ラスタライズは高さ 96px（@3x で 32pt）
              let base = UIImage(data: data, scale: 3) else { return nil }
        let image = base.withRenderingMode(.alwaysTemplate)
        cache.setObject(image, forKey: key)
        return image
    }
}

/// `icon` の描画。SF Symbols ならそのまま、Iconify なら保存済みの画像。
/// 取り込めていない Iconify アイコンは描画できない（`isAvailable` が false）。呼び出し側が label などにフォールバックする
struct KeyIconView: View {
    let raw: String
    let size: CGFloat

    static func isAvailable(_ raw: String) -> Bool {
        switch KeyboardIconRef.parse(raw) {
        case .symbol?: return true
        case .iconify(let prefix, let name)?: return KeyboardIconImageCache.image(prefix: prefix, name: name) != nil
        case nil: return false
        }
    }

    var body: some View {
        switch KeyboardIconRef.parse(raw) {
        case .symbol(let name)?:
            Image(systemName: name).font(.system(size: size))
        case .iconify(let prefix, let name)?:
            if let image = KeyboardIconImageCache.image(prefix: prefix, name: name) {
                Image(uiImage: image)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
                    .frame(width: size * 1.2, height: size * 1.2)
            }
        case nil:
            EmptyView()
        }
    }
}
