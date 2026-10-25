import Foundation

// 取り込み時に出す画像（`assets`）まわりの警告（docs/keyboard-style-spec.md §5.5、§6）。
// 資源の上限（個数・サイズ・形式）は取り込みエラー（`LayoutValidation.assetsError`）。ここは「取り込みはできるが、
// 画像が使われない / 縮小される」ことを知らせる警告だけを作る（見た目の値の不正は無視する流儀に合わせ、エラーにはしない）。

enum LayoutAssetCheck {
    /// レイアウトが `fillImage` で参照している画像 id（`"none"` と id として不正なものは除く）
    static func references(_ layout: KeyboardLayoutDef) -> Set<String> {
        var result = Set<String>()
        func add(_ look: KeyLook?) {
            guard let look = look else { return }
            let clean = LookSanitizer.sanitize(look, allowsContent: true)
            if let id = clean.string("fillImage"), id != "none" { result.insert(id) }
        }
        add(layout.look)
        if let roles = layout.roles {
            for look in roles.byRole.values { add(look) }
        }
        add(layout.background?.look)
        for rule in layout.rules { add(rule.look) }
        func addKey(_ key: LayoutKeyDef) {
            add(key.look)
            if let states = key.states {
                for state in states.byState.values { add(state.look) }
            }
        }
        for row in layout.rows {
            add(row.look)
            for key in row.keys { addKey(key) }
        }
        if let toolbar = layout.toolbar {
            add(toolbar.look)
            for key in toolbar.keys { addKey(key) }
        }
        return result
    }

    /// 日本語の警告（無ければ空）
    static func warnings(_ layout: KeyboardLayoutDef) -> [String] {
        var result: [String] = []
        let used = references(layout)
        for id in used.sorted() where layout.assets[id] == nil {
            result.append("画像「\(id)」が assets にありません（その画像は使わず、色だけで表示します）")
        }
        for id in layout.assets.keys.sorted() {
            guard let uri = layout.assets[id] else { continue }
            if !used.contains(id) {
                result.append("assets の画像「\(id)」は、どの fillImage からも使われていません")
            }
            guard let source = KeyImageSource.read(uri: uri) else {
                result.append("assets の画像「\(id)」を読み込めません（その画像は使わず、色だけで表示します）")
                continue
            }
            if max(source.width, source.height) > KeyImageMath.maxPixelCap {
                result.append(
                    "assets の画像「\(id)」は長辺が \(KeyImageMath.maxPixelCap)px を超えています"
                        + "（\(source.width)×\(source.height)。縮小して表示します）"
                )
            }
        }
        return result
    }

    /// 取り込み結果に添える案内（無ければ空文字）
    static func note(_ warnings: [String]) -> String {
        guard !warnings.isEmpty else { return "" }
        return "\n\n画像について:\n" + warnings.map { "・\($0)" }.joined(separator: "\n")
    }
}
