import UIKit
import XCTest

// 背景画像（docs/keyboard-style-spec.md §6）: デコードサイズ・矩形・tile の計算、デコードとキャッシュ、取り込み時の警告。

enum ImageFixtures {
    /// 単色の PNG の data URI（テスト内で生成する）
    static func pngDataURI(width: Int, height: Int) -> String {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = true
        let renderer = UIGraphicsImageRenderer(size: CGSize(width: width, height: height), format: format)
        let data = renderer.pngData { context in
            UIColor.red.setFill()
            context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        }
        return "data:image/png;base64," + data.base64EncodedString()
    }
}

final class KeyImageMathTests: XCTestCase {
    func testFillDecodeSizeFollowsTheCoverScaleAndIsQuantized() {
        // 1000x500 を 100x50pt に敷く → 0.1pt/px。@3x で 300px → 64 の倍数に切り上げて 320
        let px = KeyImageMath.maxPixel(fit: "fill", sourceWidth: 1000, sourceHeight: 500, display: CGSize(width: 100, height: 50), screenScale: 3)
        XCTAssertEqual(px, 320)
    }

    func testFillOfAMismatchedAspectNeedsMorePixelsThanFit() {
        let display = CGSize(width: 100, height: 200)
        let fit = KeyImageMath.maxPixel(fit: "fit", sourceWidth: 2000, sourceHeight: 1000, display: display, screenScale: 3)
        let fill = KeyImageMath.maxPixel(fit: "fill", sourceWidth: 2000, sourceHeight: 1000, display: display, screenScale: 3)
        XCTAssertEqual(fit, 320)
        // 2000 * 0.2 * 3 = 1200 → 上限 1024
        XCTAssertEqual(fill, 1024)
    }

    func testNeverExceedsTheSourceOrTheCap() {
        XCTAssertEqual(
            KeyImageMath.maxPixel(fit: "fill", sourceWidth: 100, sourceHeight: 100, display: CGSize(width: 300, height: 300), screenScale: 3),
            100
        )
        XCTAssertEqual(
            KeyImageMath.maxPixel(fit: "fill", sourceWidth: 4000, sourceHeight: 4000, display: CGSize(width: 400, height: 400), screenScale: 3),
            1024
        )
    }

    func testTileDecodesAtTheDeviceScaleOfTheOriginalSize() {
        // 1pt = 3px の元の大きさ。@3x ならそのまま、@2x なら 2/3
        XCTAssertEqual(
            KeyImageMath.maxPixel(fit: "tile", sourceWidth: 300, sourceHeight: 150, display: CGSize(width: 100, height: 50), screenScale: 3),
            300
        )
        XCTAssertEqual(
            KeyImageMath.maxPixel(fit: "tile", sourceWidth: 300, sourceHeight: 150, display: CGSize(width: 100, height: 50), screenScale: 2),
            200
        )
    }

    func testTinyImagesDoNotTile() {
        XCTAssertEqual(KeyImageMath.effectiveFit("tile", sourceWidth: 7, sourceHeight: 100), "fill")
        XCTAssertEqual(KeyImageMath.effectiveFit("tile", sourceWidth: 8, sourceHeight: 8), "tile")
        XCTAssertEqual(KeyImageMath.effectiveFit("fit", sourceWidth: 1, sourceHeight: 1), "fit")
        XCTAssertEqual(KeyImageMath.effectiveFit("stretch", sourceWidth: 100, sourceHeight: 100), "fill")
    }

    func testDegenerateInputsGiveTheMinimum() {
        XCTAssertEqual(KeyImageMath.maxPixel(fit: "fill", sourceWidth: 0, sourceHeight: 10, display: CGSize(width: 10, height: 10), screenScale: 3), 1)
        XCTAssertEqual(KeyImageMath.maxPixel(fit: "fill", sourceWidth: 10, sourceHeight: 10, display: .zero, screenScale: 3), 1)
        XCTAssertEqual(KeyImageMath.maxPixel(fit: "fill", sourceWidth: 10, sourceHeight: 10, display: CGSize(width: 10, height: 10), screenScale: 0), 1)
    }

    func testQuantize() {
        XCTAssertEqual(KeyImageMath.quantize(1), 64)
        XCTAssertEqual(KeyImageMath.quantize(64), 64)
        XCTAssertEqual(KeyImageMath.quantize(65), 128)
    }

    func testDownsampledSizeKeepsTheAspectAndNeverUpscales() {
        let reduced = KeyImageMath.downsampledSize(sourceWidth: 2000, sourceHeight: 1000, maxPixel: 1000)
        XCTAssertEqual(reduced.width, 1000)
        XCTAssertEqual(reduced.height, 500)
        let same = KeyImageMath.downsampledSize(sourceWidth: 200, sourceHeight: 100, maxPixel: 1000)
        XCTAssertEqual(same.width, 200)
        XCTAssertEqual(same.height, 100)
        let thin = KeyImageMath.downsampledSize(sourceWidth: 1000, sourceHeight: 1, maxPixel: 10)
        XCTAssertEqual(thin.width, 10)
        XCTAssertEqual(thin.height, 1)
    }

    func testCostAndTheCacheLimit() {
        XCTAssertEqual(KeyImageMath.cost(width: 1024, height: 1024), 4 * 1024 * 1024)
        XCTAssertEqual(KeyImageMath.cacheCostLimit, 12 * 1024 * 1024)
        // 上限の大きさの画像 4 枚（長辺 1024 の正方形）は 12MB を超える → 全部は載らない
        XCTAssertGreaterThan(4 * KeyImageMath.cost(width: 1024, height: 1024), KeyImageMath.cacheCostLimit)
    }

    func testFillRectCoversAndCenters() {
        let rect = KeyImageMath.fillRect(image: CGSize(width: 200, height: 100), in: CGSize(width: 100, height: 100))
        XCTAssertEqual(rect, CGRect(x: -50, y: 0, width: 200, height: 100))
        let tall = KeyImageMath.fillRect(image: CGSize(width: 100, height: 200), in: CGSize(width: 100, height: 100))
        XCTAssertEqual(tall, CGRect(x: 0, y: -50, width: 100, height: 200))
    }

    func testFitRectContainsAndCenters() {
        let rect = KeyImageMath.fitRect(image: CGSize(width: 200, height: 100), in: CGSize(width: 100, height: 100))
        XCTAssertEqual(rect, CGRect(x: 0, y: 25, width: 100, height: 50))
    }

    func testRectOfEmptySizesIsZero() {
        XCTAssertEqual(KeyImageMath.fillRect(image: .zero, in: CGSize(width: 10, height: 10)), .zero)
        XCTAssertEqual(KeyImageMath.fitRect(image: CGSize(width: 10, height: 10), in: .zero), .zero)
    }

    func testTilePointSizeAndScale() {
        let size = KeyImageMath.tilePointSize(sourceWidth: 300, sourceHeight: 150)
        XCTAssertEqual(size.width, 100)
        XCTAssertEqual(size.height, 50)
        // @3x（デコード 300px）なら倍率 3、@2x（200px）なら 2。どちらも 300px の画像が 100pt になる
        XCTAssertEqual(KeyImageMath.tileScale(decodedLong: 300, sourceLong: 300), 3)
        XCTAssertEqual(KeyImageMath.tileScale(decodedLong: 200, sourceLong: 300), 2)
        XCTAssertEqual(KeyImageMath.tileScale(decodedLong: 0, sourceLong: 300), 1)
    }

    func testLowMemoryThreshold() {
        XCTAssertTrue(KeyImageMath.isLowMemory(availableMB: 39.9))
        XCTAssertFalse(KeyImageMath.isLowMemory(availableMB: 40))
        // 0 は「不明」（シミュレータなど）
        XCTAssertFalse(KeyImageMath.isLowMemory(availableMB: 0))
    }

    func testDataURIPayload() {
        XCTAssertEqual(KeyImageMath.dataURIPayload("data:image/png;base64,AAEC"), Data([0, 1, 2]))
        XCTAssertEqual(KeyImageMath.dataURIPayload("data:image/jpeg;base64,AAEC"), Data([0, 1, 2]))
        XCTAssertNil(KeyImageMath.dataURIPayload("data:image/gif;base64,AAEC"))
        XCTAssertNil(KeyImageMath.dataURIPayload("data:image/png;base64,"))
        XCTAssertNil(KeyImageMath.dataURIPayload("data:image/png;base64,***"))
        XCTAssertNil(KeyImageMath.dataURIPayload("hello"))
    }
}

final class KeyImageSourceTests: XCTestCase {
    func testReadsTheSizeWithoutDecoding() throws {
        let source = try XCTUnwrap(KeyImageSource.read(uri: ImageFixtures.pngDataURI(width: 200, height: 100)))
        XCTAssertEqual(source.width, 200)
        XCTAssertEqual(source.height, 100)
    }

    func testThumbnailIsAtMostMaxPixel() throws {
        let source = try XCTUnwrap(KeyImageSource.read(uri: ImageFixtures.pngDataURI(width: 200, height: 100)))
        let image = try XCTUnwrap(source.thumbnail(maxPixel: 50))
        XCTAssertLessThanOrEqual(max(image.width, image.height), 50)
        XCTAssertEqual(Double(image.width) / Double(image.height), 2, accuracy: 0.1)
    }

    func testBrokenImageIsNil() {
        XCTAssertNil(KeyImageSource.read(uri: "data:image/png;base64,AAECAwQF"))
        XCTAssertNil(KeyImageSource.read(uri: "data:image/png;base64,"))
    }
}

final class KeyImageCacheTests: XCTestCase {
    private let display = CGSize(width: 100, height: 50)

    func testDecodesDownsampledAndReusesTheCachedEntry() async throws {
        let cache = KeyImageCache()
        cache.setAssets(["a": ImageFixtures.pngDataURI(width: 200, height: 100)])
        // 200x100 を 100x50pt（@1x）に敷く → 0.5pt/px → 100px → 128 に切り上げ
        let first = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        let entry = try XCTUnwrap(first)
        XCTAssertLessThanOrEqual(max(entry.image.width, entry.image.height), 128)
        XCTAssertEqual(entry.sourceWidth, 200)
        XCTAssertEqual(entry.sourceHeight, 100)
        let second = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        XCTAssertTrue(second === entry)
    }

    func testCacheLimitIsTwelveMegabytes() {
        XCTAssertEqual(KeyImageCache().cache.totalCostLimit, 12 * 1024 * 1024)
    }

    func testUnknownOrBrokenAssetsGiveNil() async {
        let cache = KeyImageCache()
        cache.setAssets(["bad": "data:image/png;base64,AAECAwQF"])
        let missing = await cache.entry(assetId: "none-such", fit: "fill", display: display, screenScale: 1)
        let broken = await cache.entry(assetId: "bad", fit: "fill", display: display, screenScale: 1)
        XCTAssertNil(missing)
        XCTAssertNil(broken)
    }

    func testSuspendStopsDecodingUntilResumed() async {
        let cache = KeyImageCache()
        cache.setAssets(["a": ImageFixtures.pngDataURI(width: 64, height: 64)])
        cache.suspend()
        XCTAssertTrue(cache.suspended)
        let suspended = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        XCTAssertNil(suspended)
        cache.resume()
        XCTAssertFalse(cache.suspended)
        // suspend は assets を捨てない
        let resumed = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        XCTAssertNotNil(resumed)
    }

    func testReplacingAssetsDropsTheOldImage() async throws {
        let cache = KeyImageCache()
        cache.setAssets(["a": ImageFixtures.pngDataURI(width: 200, height: 100)])
        let old = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        cache.setAssets(["a": ImageFixtures.pngDataURI(width: 100, height: 200)])
        let replaced = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        let new = try XCTUnwrap(replaced)
        XCTAssertFalse(new === old)
        XCTAssertEqual(new.sourceWidth, 100)
        XCTAssertEqual(new.sourceHeight, 200)
    }

    func testSameAssetsKeepTheCache() async throws {
        let cache = KeyImageCache()
        let uri = ImageFixtures.pngDataURI(width: 200, height: 100)
        cache.setAssets(["a": uri])
        let first = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        cache.setAssets(["a": uri])
        let second = await cache.entry(assetId: "a", fit: "fill", display: display, screenScale: 1)
        XCTAssertTrue(first === second)
    }

    func testCacheKeyIncludesThePixelSize() {
        XCTAssertEqual(KeyImageCache.cacheKey(assetId: "wood", maxPixel: 320), "wood@320")
    }
}

final class LayoutAssetCheckTests: XCTestCase {
    private func layout(look: String, assets: [String: String]) throws -> KeyboardLayoutDef {
        let assetJSON = assets.map { "\"\($0.key)\":\"\($0.value)\"" }.joined(separator: ",")
        let json = """
        {"schema":2,"id":"x","name":"n","mode":"kana",\(look)"assets":{\(assetJSON)},
         "rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]}]}
        """
        return try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(json.utf8))
    }

    func testReferencesCollectEveryPlace() throws {
        let json = ##"""
        {"schema":2,"id":"x","name":"n","mode":"kana",
         "look":{"fillImage":"l"},"roles":{"function":{"fillImage":"r"}},"background":{"fillImage":"b"},
         "rules":[{"when":"pressed","look":{"fillImage":"u"}}],
         "rows":[{"look":{"fillImage":"w"},"keys":[
          {"id":"a","kind":"text","text":"a","look":{"fillImage":"k"},"states":{"longPress":{"fillImage":"s"},"shift":{"fillImage":"none"}}}]}]}
        """##
        let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: Data(json.utf8))
        XCTAssertEqual(LayoutAssetCheck.references(def), ["l", "r", "b", "u", "w", "k", "s"])
    }

    func testNoWarningsWhenEverythingMatches() throws {
        let def = try layout(look: ##""look":{"fillImage":"wood"},"##, assets: ["wood": ImageFixtures.pngDataURI(width: 64, height: 64)])
        XCTAssertEqual(LayoutAssetCheck.warnings(def), [])
        XCTAssertEqual(LayoutAssetCheck.note([]), "")
    }

    func testMissingAndUnusedAssetsWarn() throws {
        let def = try layout(look: ##""look":{"fillImage":"ghost"},"##, assets: ["wood": ImageFixtures.pngDataURI(width: 64, height: 64)])
        let warnings = LayoutAssetCheck.warnings(def)
        XCTAssertEqual(warnings.count, 2)
        XCTAssertTrue(warnings[0].contains("ghost"))
        XCTAssertTrue(warnings[1].contains("wood") && warnings[1].contains("使われていません"))
    }

    func testUnreadableImageWarns() throws {
        let def = try layout(look: ##""look":{"fillImage":"bad"},"##, assets: ["bad": "data:image/png;base64,AAECAwQF"])
        let warnings = LayoutAssetCheck.warnings(def)
        XCTAssertEqual(warnings.count, 1)
        XCTAssertTrue(warnings[0].contains("読み込めません"))
    }

    func testOversizedImageWarns() throws {
        let def = try layout(look: ##""look":{"fillImage":"big"},"##, assets: ["big": ImageFixtures.pngDataURI(width: 1100, height: 20)])
        let warnings = LayoutAssetCheck.warnings(def)
        XCTAssertEqual(warnings.count, 1)
        XCTAssertTrue(warnings[0].contains("1024"))
    }

    func testNoteListsTheWarnings() {
        let note = LayoutAssetCheck.note(["a", "b"])
        XCTAssertTrue(note.contains("・a"))
        XCTAssertTrue(note.contains("・b"))
    }
}
