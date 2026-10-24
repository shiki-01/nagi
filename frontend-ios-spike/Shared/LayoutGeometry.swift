import Foundation

/// 曖昧入力（キー配置に応じたタイプミス補正）のために、描画中のレイアウトのキー矩形を求める純粋関数群。
/// コアの `set_layout_geometry`（docs/decisions.md H-25）へ渡す。
///
/// 矩形の計算は `NagiKeyboardView.swift` の描画（`RowView`）と同じ規則: 行の高さは高さの比（`row.height`）、
/// キーの幅は幅の比（`key.width`）で、間隔（キー間 `keySpacing`・行間 `rowSpacing`）を除いた残りを按分する。座標は領域の左上が原点
enum LayoutGeometry {
    /// キー 1 つの矩形（キーボード領域のローカル座標）
    struct KeyRect: Equatable {
        let id: String
        /// QWERTY の英字 1 文字（小文字）
        let label: String
        let x: Double
        let y: Double
        let w: Double
        let h: Double
    }

    /// 送る内容。前回送ったものと等しければ送り直さない
    struct Snapshot: Equatable {
        let layoutId: String
        let mode: String
        let keys: [KeyRect]
        let width: Double
        let height: Double
    }

    /// コアが QWERTY として扱う最小のキー数（コア側も 10 個未満のレイアウトは無視する）
    static let minLetterKeys = 10

    /// 英字 1 文字のキー（`text` キーで、文字が ASCII の英字 1 文字）の矩形。それ以外のキーは含めない。
    /// - size: 行が並ぶ領域（キー領域）の大きさ
    /// - keySpacing: キー同士の間隔、rowSpacing: 行同士の間隔（既定はどちらも設定 `keyboard.key_spacing_pt`。
    ///   レイアウトの `metrics` があればそれを優先する）
    static func rects(layout: KeyboardLayoutDef, size: CGSize, keySpacing: Double, rowSpacing: Double) -> [KeyRect] {
        let width = Double(size.width)
        let height = Double(size.height)
        guard width > 0, height > 0, !layout.rows.isEmpty else { return [] }
        let heightWeights = layout.rows.reduce(0.0) { $0 + $1.height }
        guard heightWeights > 0 else { return [] }
        let availableHeight = max(0, height - rowSpacing * Double(layout.rows.count - 1))

        var result: [KeyRect] = []
        var y = 0.0
        for row in layout.rows {
            let rowHeight = availableHeight * row.height / heightWeights
            let weights = row.keys.reduce(0.0) { $0 + $1.width }
            if weights > 0 {
                let availableWidth = max(0, width - keySpacing * Double(max(row.keys.count - 1, 0)))
                var x = 0.0
                for key in row.keys {
                    let keyWidth = availableWidth * key.width / weights
                    if let label = letterLabel(key) {
                        result.append(KeyRect(id: key.id, label: label, x: x, y: y, w: keyWidth, h: rowHeight))
                    }
                    x += keyWidth + keySpacing
                }
            }
            y += rowHeight + rowSpacing
        }
        return result
    }

    /// コアへ送る内容。英字キーが `minLetterKeys` 個に満たない（12 キーのかな・数字・記号など）か、
    /// 絵文字・領域が空のときは nil（送らない）
    static func snapshot(layout: KeyboardLayoutDef, size: CGSize, keySpacing: Double, rowSpacing: Double) -> Snapshot? {
        guard layout.mode != .emoji else { return nil }
        let keys = rects(layout: layout, size: size, keySpacing: keySpacing, rowSpacing: rowSpacing)
        guard keys.count >= minLetterKeys else { return nil }
        return Snapshot(
            layoutId: layout.id, mode: "qwerty", keys: keys,
            width: Double(size.width), height: Double(size.height)
        )
    }

    /// 行だけの高さ。行の高さは高さの比（`row.height`）× キー 1 行の高さ、行同士の間隔は `rowSpacing`
    static func rowsHeight(layout: KeyboardLayoutDef, keyHeight: Double, rowSpacing: Double) -> Double {
        let rows = layout.rows.reduce(0.0) { $0 + keyHeight * $1.height }
        return rows + rowSpacing * Double(max(layout.rows.count - 1, 0))
    }

    /// キー領域の高さ。絵文字は一覧を含め、キー 4 行分に揃える
    static func padHeight(layout: KeyboardLayoutDef, keyHeight: Double, rowSpacing: Double) -> Double {
        if layout.mode == .emoji { return keyHeight * 4 + rowSpacing * 3 }
        return rowsHeight(layout: layout, keyHeight: keyHeight, rowSpacing: rowSpacing)
    }

    /// `text` キーの文字が英字 1 文字ならその小文字。シフト時の表示（大文字）には依存しない
    static func letterLabel(_ key: LayoutKeyDef) -> String? {
        guard key.kind == .text, let text = key.text ?? key.label else { return nil }
        let scalars = Array(text.unicodeScalars)
        guard scalars.count == 1, let scalar = scalars.first else { return nil }
        switch scalar.value {
        case 0x61...0x7A: return String(scalar)
        case 0x41...0x5A: return String(scalar).lowercased()
        default: return nil
        }
    }
}

/// キー内のタッチ位置・フリックの角度を求める純粋関数（曖昧入力に渡す。値はキーの矩形内の相対位置）
enum TouchGeometry {
    /// キー内のローカル座標を、キーの矩形に対する相対位置（0..1）にする。範囲外は端に丸める。
    /// キーの大きさが 0 以下なら nil
    static func relative(x: Double, y: Double, width: Double, height: Double) -> (x: Double, y: Double)? {
        guard width > 0, height > 0 else { return nil }
        return (min(max(x / width, 0), 1), min(max(y / height, 0), 1))
    }

    /// 移動量からフリックの角度（度。右 0、上 90、左 180、下 270。反時計回り）と距離（キー幅を 1 とする）を求める。
    /// 画面座標は下向きが正なので、縦は反転する。距離が 0 のとき（タップ）は nil
    static func flick(dx: Double, dy: Double, keyWidth: Double) -> (angleDeg: Double, distance: Double)? {
        guard keyWidth > 0 else { return nil }
        let distance = (dx * dx + dy * dy).squareRoot()
        guard distance > 0 else { return nil }
        var angle = atan2(-dy, dx) * 180 / Double.pi
        if angle < 0 { angle += 360 }
        return (angle, distance / keyWidth)
    }
}
