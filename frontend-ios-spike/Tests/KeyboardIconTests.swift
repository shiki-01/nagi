import XCTest

/// `icon` の書式（sf: / iconify: / 接頭辞なし）、レイアウトからの Iconify 使用箇所の抽出、
/// Iconify の `/collection` 応答の解釈。通信・変換（App/IconifyResolver.swift）はテスト対象外
final class KeyboardIconTests: XCTestCase {
    func testParseSymbolForms() {
        XCTAssertEqual(KeyboardIconRef.parse("shift.fill"), .symbol("shift.fill"))
        XCTAssertEqual(KeyboardIconRef.parse("sf:shift.fill"), .symbol("shift.fill"))
        // 接頭辞なしは従来どおり何でも SF Symbols 名扱い
        XCTAssertEqual(KeyboardIconRef.parse("あ"), .symbol("あ"))
    }

    func testParseIconify() {
        XCTAssertEqual(KeyboardIconRef.parse("iconify:mdi:home"), .iconify(prefix: "mdi", name: "home"))
        XCTAssertEqual(KeyboardIconRef.parse("iconify:material-symbols:arrow-back-ios-new"),
                       .iconify(prefix: "material-symbols", name: "arrow-back-ios-new"))
        XCTAssertEqual(KeyboardIconRef.parse("iconify:fa6-solid:0"), .iconify(prefix: "fa6-solid", name: "0"))
    }

    func testMalformedIconifyAndSf() {
        for bad in ["iconify:", "iconify:mdi", "iconify:mdi:", "iconify::home", "iconify:MDI:home", "iconify:mdi:Home",
                    "iconify:mdi:ho me", "iconify:mdi:home:x", "iconify:mdi:-home", "iconify:mdi:home-", "iconify:mdi/home",
                    "iconify:mdi:../x", "sf:", "sf:a b", "sf:あ"] {
            XCTAssertNotNil(KeyboardIconRef.formatError(bad), bad)
            XCTAssertNil(KeyboardIconRef.parse(bad), bad)
        }
    }

    func testValidationUsesFormat() {
        assertValid(LB.single(LB.text("a", "a", ["icon": "iconify:mdi:home"])))
        assertValid(LB.single(LB.text("a", "a", ["icon": "sf:star"])))
        assertValid(LB.single(LB.text("a", "a", ["icon": "star"])))
        assertInvalid(LB.single(LB.text("a", "a", ["icon": "iconify:mdi"])), contains: "icon")
        assertInvalid(LB.single(LB.text("a", "a", ["icon": "iconify:MDI:Home"])), contains: "icon")
        // 64 字の上限は維持
        let long = "iconify:mdi:" + String(repeating: "a", count: 53)
        assertInvalid(LB.single(LB.text("a", "a", ["icon": long])), contains: "icon")
    }

    func testValidationAppliesToLongPress() {
        let popup: JSON = ["kind": "text", "label": "x", "text": "y", "icon": "iconify:bad"]
        assertInvalid(LB.single(LB.text("a", "a", ["longPress": [popup]])), contains: "icon")
    }

    func testIconifyUsesCollectsKeysAndLongPress() throws {
        let popup: JSON = ["kind": "text", "label": "p", "text": "y", "icon": "iconify:tabler:star"]
        let layout = LB.layout([LB.row([
            LB.text("a", "a", ["icon": "iconify:mdi:home", "label": "家", "longPress": [popup]]),
            LB.text("b", "b", ["icon": "iconify:mdi:home"]),
            LB.text("c", "c", ["icon": "shift"]),
        ])])
        let data = try JSONSerialization.data(withJSONObject: layout)
        let def = try LayoutLoader.decode(data)
        let uses = def.iconifyUses()
        XCTAssertEqual(uses.map { $0.id }, ["mdi:home", "tabler:star", "mdi:home"])
        XCTAssertEqual(uses.map { $0.keyId }, ["a", "a", "b"])
        XCTAssertEqual(uses.map { $0.hasLabel }, [true, true, false])
    }

    func testStorageNamesAreFlat() {
        XCTAssertEqual(KeyboardIconStore.fileName(prefix: "mdi", name: "home"), "mdi--home.png")
    }

    func testParseCollectionInfoShape() {
        let json = """
        {"prefix":"mdi","total":7000,"info":{"name":"Material Design Icons","author":{"name":"Pictogrammers","url":"https://example.com/a"},
        "license":{"title":"Apache 2.0","spdx":"Apache-2.0","url":"https://example.com/l"}}}
        """
        let license = IconifyAPI.parseCollection(prefix: "mdi", data: Data(json.utf8))
        XCTAssertEqual(license?.name, "Material Design Icons")
        XCTAssertEqual(license?.licenseTitle, "Apache 2.0")
        XCTAssertEqual(license?.licenseSpdx, "Apache-2.0")
        XCTAssertEqual(license?.authorName, "Pictogrammers")
        XCTAssertEqual(license?.authorUrl, "https://example.com/a")
    }

    func testParseCollectionTopLevelShape() {
        let json = #"{"name":"Tabler","author":{"name":"Paweł"},"license":{"title":"MIT"}}"#
        let license = IconifyAPI.parseCollection(prefix: "tabler", data: Data(json.utf8))
        XCTAssertEqual(license?.name, "Tabler")
        XCTAssertEqual(license?.licenseTitle, "MIT")
        XCTAssertNil(license?.licenseUrl)
    }

    func testParseCollectionWithoutLicenseFails() {
        XCTAssertNil(IconifyAPI.parseCollection(prefix: "x", data: Data(#"{"info":{"name":"X"}}"#.utf8)))
        XCTAssertNil(IconifyAPI.parseCollection(prefix: "x", data: Data("not json".utf8)))
        XCTAssertNil(IconifyAPI.parseCollection(prefix: "x", data: Data(#"{"info":{"license":{"title":""}}}"#.utf8)))
    }

    func testAcceptableSVG() {
        XCTAssertTrue(IconifyAPI.isAcceptableSVG(Data(#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.utf8)))
        XCTAssertFalse(IconifyAPI.isAcceptableSVG(Data("404".utf8)))
        XCTAssertFalse(IconifyAPI.isAcceptableSVG(Data()))
        XCTAssertFalse(IconifyAPI.isAcceptableSVG(Data(repeating: 0x20, count: 300 * 1024)))
    }

    func testURLs() {
        XCTAssertEqual(IconifyAPI.svgURL(prefix: "mdi", name: "home")?.absoluteString,
                       "https://api.iconify.design/mdi/home.svg?height=96")
        XCTAssertEqual(IconifyAPI.collectionURL(prefix: "mdi")?.absoluteString,
                       "https://api.iconify.design/collection?prefix=mdi&info=true")
    }
}
