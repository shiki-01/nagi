import Foundation
import CoreGraphics

// 曖昧入力（fuzzy-input-design.md §4.1・§4.2）のフロント側。
// タッチ位置から各キー（フリックの方向ゾーン）の中心までの距離をキー幅で正規化し、
// 近傍（distance <= 1.0、最大 3 件）を代替としてコアへ送る。計算はレイアウト定義の矩形だけから行い、
// 特定の配列を決め打ちしない。すべて純粋関数。

/// 距離計算の対象（キー、またはフリックのゾーン）
struct TouchTarget: Equatable {
    /// このターゲットを選んだときの出力文字
    let text: String
    /// 中心座標
    let center: CGPoint
    /// 距離の正規化に使うキー幅
    let width: CGFloat
}

enum TouchGeometry {
    /// 送る代替の距離の上限（コアの `fuzzy.max_distance` 既定と同じ）
    static let defaultMaxDistance: Double = 1.0
    /// 送る代替の最大件数
    static let defaultMaxCount: Int = 3

    /// タッチ位置からターゲット中心までの距離をキー幅で正規化した値。幅が 0 以下なら無限大
    static func normalizedDistance(from point: CGPoint, to target: TouchTarget) -> Double {
        guard target.width > 0 else { return Double.infinity }
        let dx = Double(point.x - target.center.x)
        let dy = Double(point.y - target.center.y)
        return (dx * dx + dy * dy).squareRoot() / Double(target.width)
    }

    /// 近傍の代替を、近い順に返す。`chosenText`（実際に採用した文字）は除く。
    /// 同じ文字が複数あれば近い方を採る。距離が同じなら文字の昇順。
    /// `includeChosen` なら、`chosenText` 自身の距離（同じ文字のターゲットのうち最も近いもの）を末尾に加える
    /// （`maxCount` の外。コアは打鍵自身の距離を基準にした相対コストで換算する）。ターゲットに無ければ加えない
    static func alternatives(
        touch: CGPoint,
        targets: [TouchTarget],
        excluding chosenText: String,
        includeChosen: Bool = false,
        maxDistance: Double = TouchGeometry.defaultMaxDistance,
        maxCount: Int = TouchGeometry.defaultMaxCount
    ) -> [AltCandidate] {
        var best: [String: Double] = [:]
        for target in targets where target.text != chosenText {
            let d = normalizedDistance(from: touch, to: target)
            guard d.isFinite, d <= maxDistance else { continue }
            if let old = best[target.text], old <= d { continue }
            best[target.text] = d
        }
        var list: [AltCandidate] = []
        for (text, d) in best {
            list.append(AltCandidate(text: text, distance: d))
        }
        list.sort { a, b in
            if a.distance != b.distance { return a.distance < b.distance }
            return a.text < b.text
        }
        var result = Array(list.prefix(max(0, maxCount)))
        if includeChosen {
            var chosenDistance = Double.infinity
            for target in targets where target.text == chosenText {
                chosenDistance = min(chosenDistance, normalizedDistance(from: touch, to: target))
            }
            if chosenDistance.isFinite {
                result.append(AltCandidate(text: chosenText, distance: chosenDistance))
            }
        }
        return result
    }

    /// 点から矩形までの距離（内側なら 0）
    static func distance(from point: CGPoint, to rect: CGRect) -> CGFloat {
        let dx = max(rect.minX - point.x, 0, point.x - rect.maxX)
        let dy = max(rect.minY - point.y, 0, point.y - rect.maxY)
        return (dx * dx + dy * dy).squareRoot()
    }

    /// タッチ位置にあるキー。どの矩形にも入らなければ最も近いキー。キーが無ければ nil
    static func hitTest(point: CGPoint, frames: [KeyFrame]) -> KeyFrame? {
        if let hit = frames.first(where: { $0.rect.contains(point) }) { return hit }
        var best: KeyFrame?
        var bestDistance = CGFloat.greatestFiniteMagnitude
        for frame in frames {
            let d = distance(from: point, to: frame.rect)
            if d < bestDistance {
                bestDistance = d
                best = frame
            }
        }
        return best
    }

    /// ローマ字キー（`char`）をすべてターゲットにしたもの。`transform` は出力文字の変換（Shift の大文字化など）
    static func charTargets(frames: [KeyFrame], transform: (String) -> String) -> [TouchTarget] {
        var targets: [TouchTarget] = []
        for frame in frames {
            guard case .char(let text) = frame.key.tapAction else { continue }
            targets.append(TouchTarget(
                text: transform(text),
                center: CGPoint(x: frame.rect.midX, y: frame.rect.midY),
                width: frame.rect.width
            ))
        }
        return targets
    }
}

/// フリックのゾーン（キー中央と上下左右）
struct FlickZone: Equatable {
    let direction: FlickDirection
    let text: String
}

/// フリックの判定結果
struct FlickDecision: Equatable {
    let direction: FlickDirection
    /// 確定した方向の出力文字
    let text: String
    /// 他のゾーン・隣接キーのゾーンのうち近いもの（ひらがな・長音のみ。最大 `maxCount` 件）と、
    /// 確定したゾーン自身（離した位置からそのゾーン中心までの距離。末尾）。
    /// 確定したゾーンを含めると、コアは打鍵自身の距離を基準にした相対コストで換算する
    let alternatives: [AltCandidate]
}

enum FlickGeometry {
    /// 方向ゾーンの中心を、キー中心からどれだけ離すか（キーの短辺に対する比）。
    /// 実機での較正対象（F-5）。ここ 1 箇所だけで決める
    static let spreadRatio: CGFloat = 0.8

    static func spread(for rect: CGRect) -> CGFloat {
        return min(rect.width, rect.height) * spreadRatio
    }

    static func zoneCenter(of rect: CGRect, direction: FlickDirection) -> CGPoint {
        let cx = rect.midX
        let cy = rect.midY
        let s = spread(for: rect)
        switch direction {
        case .center: return CGPoint(x: cx, y: cy)
        case .left: return CGPoint(x: cx - s, y: cy)
        case .right: return CGPoint(x: cx + s, y: cy)
        case .up: return CGPoint(x: cx, y: cy - s)
        case .down: return CGPoint(x: cx, y: cy + s)
        }
    }

    /// キーが持つゾーン。中央（`tap` の文字）と、定義された方向
    static func zones(of key: KeyDef) -> [FlickZone] {
        var result: [FlickZone] = []
        if let center = key.outputText, !center.isEmpty {
            result.append(FlickZone(direction: .center, text: center))
        }
        let order: [FlickDirection] = [.left, .up, .right, .down]
        for direction in order {
            if let text = key.flick?[direction.rawValue], !text.isEmpty {
                result.append(FlickZone(direction: direction, text: text))
            }
        }
        return result
    }

    static func target(for rect: CGRect, zone: FlickZone) -> TouchTarget {
        return TouchTarget(
            text: zone.text,
            center: zoneCenter(of: rect, direction: zone.direction),
            width: rect.width
        )
    }

    /// 開始キー `start` を `startPoint` で押し、指が `end` で離れたときの判定。
    /// 確定するのは開始キーのゾーンのうち `end` に最も近いもの。代替は次の 2 種類のうち近傍
    /// （正規化距離 <= `maxDistance`）のもの。
    /// - 開始キーの他のゾーン（方向の取り違え）: `end` からゾーン中心までの距離
    /// - 同じ種類（`tap.type`）の隣接キーの中央ゾーン（押し始めのキーの取り違え）: `startPoint` からキー中心までの距離
    ///   （隣接キーの方向ゾーンは、開始キーの領域に重なるため含めない）
    ///
    /// 代替の文字は、かなキー（`tap.type` が `kana`）ならひらがな・長音 1 文字のみ（コアの打鍵ログが受け付ける形。
    /// 「(」「、」などは枠を消費しないよう除いてから上位 `maxCount` 件を取る）、それ以外のキーなら ASCII の英小文字 1 文字。
    /// 確定した文字自身が同じ条件を満たすときだけ、その距離を末尾に加える。満たさない（記号キーなど）ときは、
    /// 隣接キーの代替も含めて代替を一切送らない
    static func decide(
        start: KeyFrame,
        startPoint: CGPoint,
        end: CGPoint,
        allFrames: [KeyFrame],
        maxDistance: Double = TouchGeometry.defaultMaxDistance,
        maxCount: Int = TouchGeometry.defaultMaxCount
    ) -> FlickDecision? {
        let ownZones = zones(of: start.key)
        guard var chosen = ownZones.first else { return nil }
        var chosenDistance = Double.infinity
        for zone in ownZones {
            let d = TouchGeometry.normalizedDistance(from: end, to: target(for: start.rect, zone: zone))
            if d < chosenDistance {
                chosenDistance = d
                chosen = zone
            }
        }
        let kanaKey = start.key.isKana
        func isSendable(_ text: String) -> Bool {
            return kanaKey ? KanaText.isLoggableKana(text) : isLowercaseLetter(text)
        }
        var best: [String: Double] = [:]
        func consider(_ text: String, _ d: Double) {
            guard text != chosen.text, isSendable(text), d.isFinite, d <= maxDistance else { return }
            if let old = best[text], old <= d { return }
            best[text] = d
        }
        for zone in ownZones {
            consider(zone.text, TouchGeometry.normalizedDistance(from: end, to: target(for: start.rect, zone: zone)))
        }
        for frame in allFrames where frame.key.tap?.type == start.key.tap?.type {
            if frame.row == start.row && frame.index == start.index { continue }
            guard let center = frame.key.outputText, !center.isEmpty else { continue }
            let zone = FlickZone(direction: .center, text: center)
            consider(center, TouchGeometry.normalizedDistance(from: startPoint, to: target(for: frame.rect, zone: zone)))
        }
        var list: [AltCandidate] = []
        for (text, d) in best {
            list.append(AltCandidate(text: text, distance: d))
        }
        list.sort { a, b in
            if a.distance != b.distance { return a.distance < b.distance }
            return a.text < b.text
        }
        // 確定した文字自体が代替の条件を満たさない（記号キーなど）なら、代替は送らない
        var alternatives: [AltCandidate] = []
        if isSendable(chosen.text) {
            alternatives = Array(list.prefix(max(0, maxCount)))
            if chosenDistance.isFinite {
                alternatives.append(AltCandidate(text: chosen.text, distance: chosenDistance))
            }
        }
        return FlickDecision(
            direction: chosen.direction,
            text: chosen.text,
            alternatives: alternatives
        )
    }

    /// ASCII の英小文字 1 文字か（かな以外のキーの代替）
    private static func isLowercaseLetter(_ text: String) -> Bool {
        let scalars = text.unicodeScalars
        guard scalars.count == 1, let scalar = scalars.first else { return false }
        return scalar.value >= 0x61 && scalar.value <= 0x7a
    }
}
