import XCTest

// 見た目の解決の共有テストベクタ（config/keyboard/test-looks/*.json）。
// web/src/lib/keyboard/look.test.ts が同じファイルを読み、同じ結果になることを確かめる。形式は同フォルダの README.md。

private enum VectorValue: Decodable, Equatable {
    case number(Double)
    case string(String)

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let number = try? container.decode(Double.self) {
            self = .number(number)
        } else {
            self = .string(try container.decode(String.self))
        }
    }
}

private struct VectorContent: Decodable {
    let label: String?
    let icon: String?
    let hidesKeyIcon: Bool
}

private struct VectorCase: Decodable {
    let name: String
    let key: String
    let states: [String]?
    let origin: String?
    let scheme: String?
    let expect: [String: VectorValue]
    let expectHoldMs: Double?
    let expectContent: VectorContent?
}

private struct VectorTransition: Decodable {
    let name: String
    let from: Double?
    let to: Double?
    let reduceMotion: Bool
    let expect: Double
}

private struct VectorFile: Decodable {
    let layout: KeyboardLayoutDef
    let cases: [VectorCase]
    let transitions: [VectorTransition]?
}

final class KeyLookVectorTests: XCTestCase {
    func testVectorFilesArePresent() {
        let names = Set(TestSupport.testLookURLs().map { $0.lastPathComponent })
        for expected in ["resolve-defaults.json", "resolve-hierarchy.json", "resolve-invalid.json", "resolve-colors.json",
                         "resolve-states.json", "resolve-rules.json", "resolve-image.json"] {
            XCTAssertTrue(names.contains(expected), "\(expected) がバンドルにない")
        }
    }

    func testResolverMatchesEveryVector() throws {
        for url in TestSupport.testLookURLs() {
            try run(url)
        }
    }

    private func run(_ url: URL) throws {
        let file = url.lastPathComponent
        let vector = try JSONDecoder().decode(VectorFile.self, from: Data(contentsOf: url))
        XCTAssertFalse(vector.cases.isEmpty, file)
        // ベクタのレイアウト自体は取り込みエラーにならない（見た目の不正な値は無視されるだけ）
        XCTAssertNil(vector.layout.validationError(), file)
        for item in vector.cases {
            let label = "\(file): \(item.name)"
            guard let (rowIndex, key) = find(vector.layout, id: item.key) else {
                XCTFail("\(label): キー \(item.key) がレイアウトにない")
                continue
            }
            let names = item.states ?? []
            let states = Set(names.compactMap { LookStateName(rawValue: $0) })
            XCTAssertEqual(states.count, names.count, "\(label): 未知の状態名")
            let context = LookContext(
                states: states, origin: item.origin, scheme: LookScheme(rawValue: item.scheme ?? "light") ?? .light
            )
            let resolved = LookResolver.resolve(layout: vector.layout, rowIndex: rowIndex, key: key, context: context)
            XCTAssertEqual(values(resolved), item.expect, label)
            if let expected = item.expectHoldMs {
                XCTAssertEqual(LookResolver.releasedHoldMs(layout: vector.layout, key: key), expected, label)
            }
            if let expected = item.expectContent {
                let content = LookResolver.content(of: resolved)
                XCTAssertEqual(content.label, expected.label, label)
                XCTAssertEqual(content.icon, expected.icon, label)
                XCTAssertEqual(content.hidesKeyIcon, expected.hidesKeyIcon, label)
            }
        }
        for item in vector.transitions ?? [] {
            let from = animation(item.from)
            let to = animation(item.to)
            XCTAssertEqual(
                LookResolver.transitionMs(from: from, to: to, reduceMotion: item.reduceMotion), item.expect,
                "\(file): \(item.name)"
            )
        }
    }

    private func find(_ layout: KeyboardLayoutDef, id: String) -> (Int, LayoutKeyDef)? {
        for (index, row) in layout.rows.enumerated() {
            if let key = row.keys.first(where: { $0.id == id }) { return (index, key) }
        }
        return nil
    }

    private func animation(_ ms: Double?) -> ResolvedLook {
        guard let ms = ms else { return ResolvedLook() }
        return ResolvedLook(values: ["animationMs": .number(ms)])
    }

    /// 解決結果をベクタの `expect` と同じ表現にする（色は `#RRGGBBAA` か `@トークン`）
    private func values(_ look: ResolvedLook) -> [String: VectorValue] {
        var result: [String: VectorValue] = [:]
        for (name, value) in look.values {
            switch value {
            case .number(let number): result[name] = .number(number)
            case .string(let string): result[name] = .string(string)
            case .color(let color): result[name] = .string(hex(of: color))
            }
        }
        return result
    }

    private func hex(of color: LookColor) -> String {
        guard case .solid(let solid) = color else { return "(pair)" }
        switch solid {
        case .token(let name):
            return "@" + name
        case .rgba(let r, let g, let b, let a):
            return String(format: "#%02X%02X%02X%02X", UInt32(r), UInt32(g), UInt32(b), UInt32(a))
        }
    }
}
