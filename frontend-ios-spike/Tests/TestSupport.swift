import Foundation
import XCTest

// テスト共通の補助。テスト対象（Shared/ など）は同じモジュールに直接コンパイルされるので import は不要。

final class TestBundleToken {}

typealias JSON = [String: Any]

enum TestSupport {
    static var bundle: Bundle { Bundle(for: TestBundleToken.self) }

    /// バンドルに同梱した `config/keyboard/presets/*.json`（フォルダ参照 `presets/`。無ければバンドル直下）
    static func presetURLs() -> [URL] {
        let candidates: [URL?] = [
            bundle.resourceURL?.appendingPathComponent("presets", isDirectory: true),
            bundle.resourceURL,
        ]
        for case let dir? in candidates {
            guard let names = try? FileManager.default.contentsOfDirectory(atPath: dir.path) else { continue }
            let urls = names.filter { $0.hasSuffix(".json") }.sorted().map { dir.appendingPathComponent($0) }
            if !urls.isEmpty { return urls }
        }
        return []
    }

    /// バンドルに同梱した `config/keyboard/test-looks/*.json`（見た目の解決の共有テストベクタ。フォルダ参照 `test-looks/`）
    static func testLookURLs() -> [URL] {
        guard let dir = bundle.resourceURL?.appendingPathComponent("test-looks", isDirectory: true),
              let names = try? FileManager.default.contentsOfDirectory(atPath: dir.path) else { return [] }
        return names.filter { $0.hasSuffix(".json") }.sorted().map { dir.appendingPathComponent($0) }
    }
}

/// レイアウト JSON を辞書で組み立てて検証にかける補助
enum LB {
    static func key(_ id: String, _ kind: String, _ extra: JSON = [:]) -> JSON {
        var k: JSON = ["id": id, "kind": kind]
        for (name, value) in extra { k[name] = value }
        return k
    }

    static func text(_ id: String = "a", _ text: String? = nil, _ extra: JSON = [:]) -> JSON {
        var e = extra
        e["text"] = text ?? id
        return key(id, "text", e)
    }

    static func row(_ keys: [JSON], height: Double? = nil) -> JSON {
        var r: JSON = ["keys": keys]
        if let height = height { r["height"] = height }
        return r
    }

    /// 既定は有効な最小のレイアウト。`overrides` で上書きし、`remove` のキーは取り除く
    static func layout(_ rows: [JSON], overrides: JSON = [:], remove: [String] = []) -> JSON {
        var l: JSON = ["schema": 2, "id": "test", "name": "テスト", "mode": "kana", "rows": rows]
        for (name, value) in overrides { l[name] = value }
        for name in remove { l.removeValue(forKey: name) }
        return l
    }

    /// キー 1 つだけのレイアウト（幅 1 のキーなら行の幅の合計は範囲内）
    static func single(_ key: JSON) -> JSON {
        layout([row([key])])
    }

    /// decode と検証をまとめて行い、問題があれば理由（decode 失敗は "decode: ..."）、無ければ nil
    static func validate(_ obj: JSON, targets: [String]? = nil) -> String? {
        guard let data = try? JSONSerialization.data(withJSONObject: obj) else { return "serialize failed" }
        do {
            let def = try JSONDecoder().decode(KeyboardLayoutDef.self, from: data)
            return def.validationError(targets: targets)
        } catch {
            return "decode: " + LayoutLoader.describe(error)
        }
    }
}

func assertValid(_ obj: JSON, targets: [String]? = nil, file: StaticString = #filePath, line: UInt = #line) {
    XCTAssertNil(LB.validate(obj, targets: targets), file: file, line: line)
}

/// 拒否されること。`contains` を指定したら、理由にその文字列が含まれること
func assertInvalid(_ obj: JSON, contains: String? = nil, targets: [String]? = nil,
                   file: StaticString = #filePath, line: UInt = #line) {
    guard let reason = LB.validate(obj, targets: targets) else {
        XCTFail("拒否されるはずが有効と判定された", file: file, line: line)
        return
    }
    if let needle = contains {
        XCTAssertTrue(reason.contains(needle), "理由に「\(needle)」が含まれない: \(reason)", file: file, line: line)
    }
}
