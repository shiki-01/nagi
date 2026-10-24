import Combine
import CoreGraphics
import Foundation

// 背景画像のデコードとキャッシュ（docs/keyboard-style-spec.md §6.3）。
// レイアウト読み込み時にはデコードしない。描画に必要になった時点で、バックグラウンドで縮小デコードし（ImageIO）、
// 結果は `NSCache`（上限 12MB）に置く。デコード中は呼び出し側が色だけで描き、完了したら再描画する。
// メモリ警告・キーボードの非表示では `suspend()` で捨てて、次に表示されるまで画像を使わない。
// 状態はメインスレッドからだけ読み書きする（`entry` は async だが、内部でメインスレッドへ移して処理する）。

/// デコード結果（`NSCache` の値は参照型）
final class KeyImageEntry {
    let image: CGImage
    /// 元画像の画素数（`tile` の敷き詰めの大きさ・矩形の計算に使う）
    let sourceWidth: Int
    let sourceHeight: Int

    init(image: CGImage, sourceWidth: Int, sourceHeight: Int) {
        self.image = image
        self.sourceWidth = sourceWidth
        self.sourceHeight = sourceHeight
    }

    var cost: Int { KeyImageMath.cost(width: image.width, height: image.height) }
}

final class KeyImageCache: ObservableObject {
    static let shared = KeyImageCache()

    /// 画像を使わない状態（メモリ警告・キーボードの非表示のあと、次に表示されるまで）
    @Published private(set) var suspended = false

    let cache = NSCache<NSString, KeyImageEntry>()
    private var assets: [String: String] = [:]
    private var sources: [String: KeyImageSource] = [:]
    private var inflight: [String: [CheckedContinuation<KeyImageEntry?, Never>]] = [:]
    /// レイアウトの `assets` が入れ替わるたびに増やす（古いデコード結果を捨てるため）
    private var generation = 0

    init(costLimit: Int = KeyImageMath.cacheCostLimit) {
        cache.totalCostLimit = costLimit
    }

    static func cacheKey(assetId: String, maxPixel: Int) -> String {
        "\(assetId)@\(maxPixel)"
    }

    /// 表示中のレイアウトの `assets`（id → data URI）。変わったときだけキャッシュを捨てる
    func setAssets(_ new: [String: String]) {
        guard new != assets else { return }
        assets = new
        sources.removeAll()
        cache.removeAllObjects()
        generation += 1
    }

    /// 画像を捨て、`resume()` まで画像を使わない（色だけで描く）
    func suspend() {
        cache.removeAllObjects()
        sources.removeAll()
        generation += 1
        if !suspended { suspended = true }
    }

    func resume() {
        if suspended { suspended = false }
    }

    /// 画像を縮小デコードして返す（キャッシュにあればそれ）。使えないとき（停止中・画像が無い・壊れている）は nil
    func entry(assetId: String, fit: String, display: CGSize, screenScale: Double) async -> KeyImageEntry? {
        await withCheckedContinuation { (continuation: CheckedContinuation<KeyImageEntry?, Never>) in
            DispatchQueue.main.async {
                self.begin(assetId: assetId, fit: fit, display: display, screenScale: screenScale, continuation: continuation)
            }
        }
    }

    private func begin(
        assetId: String, fit: String, display: CGSize, screenScale: Double,
        continuation: CheckedContinuation<KeyImageEntry?, Never>
    ) {
        guard !suspended, let uri = assets[assetId] else {
            continuation.resume(returning: nil)
            return
        }
        let source: KeyImageSource
        if let known = sources[assetId] {
            source = known
        } else if let read = KeyImageSource.read(uri: uri) {
            // ヘッダの読み取りだけ（デコードはしない）。base64 の戻しは最大 256KB
            sources[assetId] = read
            source = read
        } else {
            continuation.resume(returning: nil)
            return
        }
        let maxPixel = KeyImageMath.maxPixel(
            fit: fit, sourceWidth: source.width, sourceHeight: source.height, display: display, screenScale: screenScale
        )
        let key = Self.cacheKey(assetId: assetId, maxPixel: maxPixel)
        if let hit = cache.object(forKey: key as NSString) {
            continuation.resume(returning: hit)
            return
        }
        if inflight[key] != nil {
            inflight[key]?.append(continuation)
            return
        }
        inflight[key] = [continuation]
        let startedGeneration = generation
        DispatchQueue.global(qos: .userInitiated).async {
            let image = source.thumbnail(maxPixel: maxPixel)
            DispatchQueue.main.async {
                self.finish(key: key, image: image, source: source, generation: startedGeneration)
            }
        }
    }

    private func finish(key: String, image: CGImage?, source: KeyImageSource, generation startedGeneration: Int) {
        let waiters = inflight.removeValue(forKey: key) ?? []
        var result: KeyImageEntry?
        if let image = image, startedGeneration == generation, !suspended {
            let entry = KeyImageEntry(image: image, sourceWidth: source.width, sourceHeight: source.height)
            cache.setObject(entry, forKey: key as NSString, cost: entry.cost)
            result = entry
        }
        for waiter in waiters { waiter.resume(returning: result) }
    }
}
