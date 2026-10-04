import UIKit

enum ShiftState {
    case off
    case once
    case locked
}

protocol KeyboardLayoutViewDelegate: AnyObject {
    func layoutView(_ view: KeyboardLayoutView, didProduce input: KeyboardInput)
}

/// 1 つのキーの見た目
final class KeyView: UIView {
    let label = UILabel()
    private let style: String

    var isPressed = false {
        didSet { updateColors() }
    }

    init(style: String) {
        self.style = style
        super.init(frame: .zero)
        layer.cornerRadius = 6
        isUserInteractionEnabled = false
        label.textAlignment = .center
        label.adjustsFontSizeToFitWidth = true
        label.minimumScaleFactor = 0.5
        label.font = UIFont.systemFont(ofSize: style == "normal" ? 22 : 16)
        addSubview(label)
        updateColors()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) は使いません")
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        label.frame = bounds.insetBy(dx: 2, dy: 2)
    }

    private func updateColors() {
        if isPressed {
            backgroundColor = UIColor.systemGray3
            label.textColor = UIColor.label
            return
        }
        switch style {
        case "function":
            backgroundColor = UIColor.systemGray4
            label.textColor = UIColor.label
        case "accent":
            backgroundColor = UIColor.systemBlue
            label.textColor = UIColor.white
        default:
            backgroundColor = UIColor.systemBackground
            label.textColor = UIColor.label
        }
    }
}

/// レイアウト定義（JSON）からキーを描画し、タッチを入力イベントに変える。
/// - タップ: 指を離した位置のキー（フリック・かなキー・連打キーは押し始めのキー）
/// - フリック: 押し始めのキーの方向ゾーンのうち、離した位置に最も近いもの
/// - 曖昧入力の代替（近傍のキー・ゾーンと正規化距離）は `TouchGeometry` / `FlickGeometry` で計算する
final class KeyboardLayoutView: UIView {
    weak var delegate: KeyboardLayoutViewDelegate?

    private(set) var layout: KeyboardLayout?

    var shiftState: ShiftState = .off {
        didSet { refreshLabels() }
    }

    private var frames: [KeyFrame] = []
    private var keyViews: [KeyView] = []
    private var tracks: [ObjectIdentifier: TouchTrack] = [:]
    private let keyInset: CGFloat = 3

    /// 1 本の指の追跡状態
    private final class TouchTrack {
        let frameIndex: Int?
        var current: CGPoint
        let start: CGPoint
        var didLongPress = false
        var didRepeat = false
        var longPressTimer: Timer?
        var repeatTimer: Timer?

        init(frameIndex: Int?, point: CGPoint) {
            self.frameIndex = frameIndex
            self.current = point
            self.start = point
        }

        func cancelTimers() {
            longPressTimer?.invalidate()
            longPressTimer = nil
            repeatTimer?.invalidate()
            repeatTimer = nil
        }
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        backgroundColor = UIColor.systemGray6
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) は使いません")
    }

    // MARK: - レイアウトの設定

    func setLayout(_ newLayout: KeyboardLayout) {
        cancelAllTracks()
        layout = newLayout
        keyViews.forEach { $0.removeFromSuperview() }
        keyViews = []
        // キーの数と順序は大きさに依存しないので、仮の大きさで生成する
        let prototype = LayoutGeometry.frames(for: newLayout, in: CGSize(width: 100, height: 100))
        for frame in prototype {
            let view = KeyView(style: frame.key.style ?? "normal")
            addSubview(view)
            keyViews.append(view)
        }
        frames = []
        refreshLabels()
        setNeedsLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard let layout = layout else { return }
        frames = LayoutGeometry.frames(for: layout, in: bounds.size)
        for (i, frame) in frames.enumerated() where i < keyViews.count {
            keyViews[i].frame = frame.rect.insetBy(dx: keyInset, dy: keyInset)
        }
    }

    private func refreshLabels() {
        guard let layout = layout else { return }
        let prototype = LayoutGeometry.frames(for: layout, in: CGSize(width: 100, height: 100))
        for (i, frame) in prototype.enumerated() where i < keyViews.count {
            keyViews[i].label.text = displayText(for: frame.key)
        }
    }

    private func displayText(for key: KeyDef) -> String {
        if case .special(.shift) = key.tapAction {
            switch shiftState {
            case .off: return key.displayLabel
            case .once: return "⬆︎"
            case .locked: return "⇪"
            }
        }
        if case .char = key.tapAction, shiftState != .off, key.label == nil {
            return key.displayLabel.uppercased()
        }
        return key.displayLabel
    }

    private func applyShift(_ text: String) -> String {
        return shiftState == .off ? text : text.uppercased()
    }

    // MARK: - タッチ

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let point = touch.location(in: self)
            let index = hitIndex(at: point)
            let track = TouchTrack(frameIndex: index, point: point)
            let id = ObjectIdentifier(touch)
            tracks[id] = track
            if let i = index, i < keyViews.count {
                keyViews[i].isPressed = true
                scheduleTimers(track: track, index: i, id: id)
            }
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let track = tracks[ObjectIdentifier(touch)] else { continue }
            let point = touch.location(in: self)
            track.current = point
            guard let i = track.frameIndex, i < frames.count, i < keyViews.count else { continue }
            let key = frames[i].key
            // 長押しのキーは、大きく動いたら長押しとみなさない
            let dx = point.x - track.start.x
            let dy = point.y - track.start.y
            if key.longPress != nil, (dx * dx + dy * dy).squareRoot() > 15 {
                track.longPressTimer?.invalidate()
                track.longPressTimer = nil
            }
            // フリックのプレビュー（確定する方向の文字をキーに表示）
            if key.flick != nil {
                if let d = FlickGeometry.decide(start: frames[i], startPoint: track.start, end: point, allFrames: frames),
                   d.direction != .center {
                    keyViews[i].label.text = d.text
                } else {
                    keyViews[i].label.text = displayText(for: key)
                }
            }
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let id = ObjectIdentifier(touch)
            guard let track = tracks.removeValue(forKey: id) else { continue }
            track.cancelTimers()
            if let i = track.frameIndex, i < keyViews.count, i < frames.count {
                keyViews[i].isPressed = false
                keyViews[i].label.text = displayText(for: frames[i].key)
            }
            if track.didLongPress || track.didRepeat { continue }
            if let i = track.frameIndex {
                emitTap(startIndex: i, startPoint: track.start, at: touch.location(in: self))
            }
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let track = tracks.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            track.cancelTimers()
            if let i = track.frameIndex, i < keyViews.count, i < frames.count {
                keyViews[i].isPressed = false
                keyViews[i].label.text = displayText(for: frames[i].key)
            }
        }
    }

    private func cancelAllTracks() {
        for (_, track) in tracks { track.cancelTimers() }
        tracks.removeAll()
        for view in keyViews { view.isPressed = false }
    }

    private func hitIndex(at point: CGPoint) -> Int? {
        guard let hit = TouchGeometry.hitTest(point: point, frames: frames) else { return nil }
        return frames.firstIndex(where: { $0.row == hit.row && $0.index == hit.index })
    }

    // MARK: - タイマー（長押し・連打）

    private func scheduleTimers(track: TouchTrack, index: Int, id: ObjectIdentifier) {
        guard index < frames.count else { return }
        let key = frames[index].key
        if key.repeats == true {
            track.longPressTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: false) { [weak self] _ in
                self?.beginRepeat(id: id)
            }
        } else if key.longPress != nil {
            track.longPressTimer = Timer.scheduledTimer(withTimeInterval: 0.45, repeats: false) { [weak self] _ in
                self?.fireLongPress(id: id)
            }
        }
    }

    private func beginRepeat(id: ObjectIdentifier) {
        guard let track = tracks[id], let i = track.frameIndex, i < frames.count else { return }
        track.didRepeat = true
        emitTap(startIndex: i, startPoint: track.start, at: track.current)
        track.repeatTimer = Timer.scheduledTimer(withTimeInterval: 0.08, repeats: true) { [weak self] _ in
            guard let self = self, let tr = self.tracks[id], let idx = tr.frameIndex, idx < self.frames.count else {
                return
            }
            self.emitTap(startIndex: idx, startPoint: tr.start, at: tr.current)
        }
    }

    private func fireLongPress(id: ObjectIdentifier) {
        guard let track = tracks[id], let i = track.frameIndex, i < frames.count,
              let action = frames[i].key.longPress else { return }
        track.didLongPress = true
        if let input = simpleInput(for: action.resolved) {
            delegate?.layoutView(self, didProduce: input)
        }
    }

    // MARK: - 入力イベントの生成

    /// 位置に依存しない動作（長押し等）を入力イベントにする
    private func simpleInput(for action: ResolvedAction) -> KeyboardInput? {
        switch action {
        case .char(let text): return .char(text: applyShift(text), alternatives: [])
        case .kana(let text): return .kana(reading: text, alternatives: [])
        case .special(let key): return .special(key)
        case .switchLayout(let id): return .switchLayout(id)
        case .engineAction(let name): return .engineAction(name)
        case .unsupported: return nil
        }
    }

    /// タップ・フリックの確定。`point` は指を離した位置
    private func emitTap(startIndex: Int, startPoint: CGPoint, at point: CGPoint) {
        guard startIndex < frames.count else { return }
        let start = frames[startIndex]
        let key = start.key

        // フリックまたはかなキー: 離した位置に最も近いゾーンを確定し、近傍のゾーンを代替にする
        if key.flick != nil || key.isKana {
            guard let decision = FlickGeometry.decide(
                start: start, startPoint: startPoint, end: point, allFrames: frames
            ) else { return }
            if key.isKana {
                delegate?.layoutView(
                    self,
                    didProduce: .kana(reading: decision.text, alternatives: decision.alternatives)
                )
            } else {
                var alternatives: [AltCandidate] = []
                for alt in decision.alternatives {
                    alternatives.append(AltCandidate(text: applyShift(alt.text), distance: alt.distance))
                }
                delegate?.layoutView(
                    self,
                    didProduce: .char(text: applyShift(decision.text), alternatives: alternatives)
                )
            }
            return
        }

        // 連打キー以外は、指を離した位置のキーを採る（隣のキーへ滑らせて直せる）
        let target: KeyFrame
        if key.repeats == true {
            target = start
        } else {
            target = TouchGeometry.hitTest(point: point, frames: frames) ?? start
        }

        if case .char(let text) = target.key.tapAction {
            let chosen = applyShift(text)
            let targets = TouchGeometry.charTargets(frames: frames, transform: { self.applyShift($0) })
            let alternatives = TouchGeometry.alternatives(
                touch: point, targets: targets, excluding: chosen, includeChosen: true
            )
            delegate?.layoutView(self, didProduce: .char(text: chosen, alternatives: alternatives))
            return
        }
        if let input = simpleInput(for: target.key.tapAction) {
            delegate?.layoutView(self, didProduce: input)
        }
    }
}
