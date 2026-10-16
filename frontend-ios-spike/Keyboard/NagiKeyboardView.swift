import SwiftUI

/// 製品版キーボードの画面: 候補バーと、レイアウト（JSON。`Shared/KeyboardLayout.swift`）から描くキー
struct NagiKeyboardView: View {
    @ObservedObject var model: KeyboardModel
    /// 検証用の画面（Probe）を開く
    var onShowProbes: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            CandidateBar(model: model, onShowProbes: onShowProbes)
            if model.ios.oneHanded == "off" {
                padArea
                    .padding(6)
            } else {
                // 片手モード: キー部分を左または右に寄せる（候補バーは全幅のまま）
                GeometryReader { geo in
                    HStack(spacing: 0) {
                        if model.ios.oneHanded == "right" { OneHandedButtons(model: model) }
                        padArea
                            .padding(6)
                            .frame(width: geo.size.width * CGFloat(model.ios.oneHandedWidth))
                        if model.ios.oneHanded == "left" { OneHandedButtons(model: model) }
                    }
                }
            }
        }
        .background(Color(UIColor.systemGray6))
        // 設定 `keyboard.accent` / `keyboard.theme`。テーマが変わったときは子のビューも作り直して新しい色を見せる
        // （子のビューは入力が変わらないと再評価されないため）。外観の上書きは KeyboardViewController が行う
        .tint(themeAccent)
        .id("\(model.ios.theme)-\(model.ios.accent)")
    }

    /// キー（または展開した候補一覧）の領域
    private var padArea: some View {
        Group {
            if model.isExpanded {
                ExpandedCandidates(model: model)
            } else {
                LayoutPad(model: model)
            }
        }
    }
}

/// 片手モードで空いた側に出すボタン: 左右の切替と、片手モードの解除。
/// 設定 `keyboard.one_handed` を更新する（ホストアプリの設定画面と同じ値）
private struct OneHandedButtons: View {
    @ObservedObject var model: KeyboardModel

    var body: some View {
        VStack(spacing: 10) {
            Spacer(minLength: 0)
            sideButton(text: "⇄") {
                model.setOneHanded(model.ios.oneHanded == "left" ? "right" : "left")
            }
            sideButton(text: "↔") { model.setOneHanded("off") }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity)
    }

    private func sideButton(text: String, action: @escaping () -> Void) -> some View {
        Button(action: {
            NagiKeyFeedback.keyDown()
            action()
        }) {
            Text(text)
                .font(.system(size: 18))
                .foregroundColor(Color(UIColor.label))
                .frame(width: 30, height: 36)
                .background(RoundedRectangle(cornerRadius: keyCorner).fill(Color(UIColor.systemGray4)))
        }
        .buttonStyle(.plain)
    }
}

/// キーの間隔と高さ（設定 `keyboard.key_spacing_pt` / 高さの倍率。`NagiKeyMetrics`）
private var keySpacing: CGFloat { CGFloat(NagiKeyMetrics.keySpacing) }
private var keyHeight: CGFloat { CGFloat(NagiKeyMetrics.keyHeight) }
/// アクセントカラー（設定 `keyboard.accent`）
private var themeAccent: Color { Color(NagiTheme.accentUIColor(NagiKeyMetrics.current.accent)) }
/// キーの角丸（設定 `keyboard.key_corner_pt`）
private var keyCorner: CGFloat { CGFloat(NagiKeyMetrics.current.keyCornerPt) }

/// キー領域の高さ。絵文字は一覧を含め、キー 4 行分に揃える
private func padHeight(_ def: KeyboardLayoutDef) -> CGFloat {
    let base = keyHeight * 4 + keySpacing * 3
    if def.mode == .emoji { return base }
    let rows = def.rows.reduce(CGFloat(0)) { $0 + keyHeight * CGFloat($1.height) }
    return rows + keySpacing * CGFloat(max(def.rows.count - 1, 0))
}

/// キー領域の高さ（行だけ）
private func rowsHeight(_ def: KeyboardLayoutDef) -> CGFloat {
    let rows = def.rows.reduce(CGFloat(0)) { $0 + keyHeight * CGFloat($1.height) }
    return rows + keySpacing * CGFloat(max(def.rows.count - 1, 0))
}

// MARK: - レイアウトの描画

/// レイアウトの行とキーを描く
private struct LayoutPad: View {
    @ObservedObject var model: KeyboardModel
    /// 押下中のキー（"行:列"）。ポップアップが下の行に隠れないよう、その行を手前に出す
    @State private var pressedKey: String?

    var body: some View {
        VStack(spacing: keySpacing) {
            if model.layoutDef.mode == .emoji {
                EmojiGrid(
                    model: model,
                    height: padHeight(model.layoutDef) - rowsHeight(model.layoutDef) - keySpacing
                )
            }
            ForEach(Array(model.layoutDef.rows.enumerated()), id: \.offset) { rowIndex, row in
                RowView(row: row, rowIndex: rowIndex, model: model, pressedKey: $pressedKey)
                    .zIndex((pressedKey ?? "").hasPrefix("\(rowIndex):") ? 1 : 0)
            }
        }
        // キー領域の大きさをコアへ伝える（曖昧入力のキー近接。表示後・レイアウト切替・向き/高さの変更で送り直す。
        // 内容が同じなら `KeyboardModel` が送らない）
        .background(
            GeometryReader { geo in
                Color.clear
                    .onAppear { model.updateLayoutGeometry(size: geo.size) }
                    .onChange(of: geo.size) { size in model.updateLayoutGeometry(size: size) }
                    .onChange(of: model.layoutDef.id) { _ in model.updateLayoutGeometry(size: geo.size) }
            }
        )
    }
}

/// 1 行。幅の比（`width`）で按分する
private struct RowView: View {
    let row: LayoutRow
    let rowIndex: Int
    @ObservedObject var model: KeyboardModel
    @Binding var pressedKey: String?

    var body: some View {
        GeometryReader { geo in
            HStack(spacing: keySpacing) {
                ForEach(Array(row.keys.enumerated()), id: \.offset) { index, key in
                    KeyCell(
                        key: key, model: model, height: rowHeight,
                        originX: originX(index, total: geo.size.width), rowWidth: geo.size.width
                    ) { pressed in
                        pressedKey = pressed ? "\(rowIndex):\(index)" : nil
                    }
                    .frame(width: cellWidth(key, total: geo.size.width), height: rowHeight)
                    // 押下中のキーを兄弟キーより手前に描く（右隣のキーの下にポップアップが隠れないように）
                    .zIndex(pressedKey == "\(rowIndex):\(index)" ? 10 : 0)
                }
            }
        }
        .frame(height: rowHeight)
    }

    private var rowHeight: CGFloat {
        keyHeight * CGFloat(row.height)
    }

    /// 行内で index 番目のキーの左端の位置（長押しのポップアップを行からはみ出さないようにするため）
    private func originX(_ index: Int, total: CGFloat) -> CGFloat {
        var x: CGFloat = 0
        for previous in 0..<index {
            x += cellWidth(row.keys[previous], total: total) + keySpacing
        }
        return x
    }

    private func cellWidth(_ key: LayoutKeyDef, total: CGFloat) -> CGFloat {
        let weights = row.keys.reduce(0.0) { $0 + $1.width }
        guard weights > 0 else { return 0 }
        let available = total - keySpacing * CGFloat(max(row.keys.count - 1, 0))
        return max(0, available * CGFloat(key.width / weights))
    }
}

/// キー 1 つ。種類ごとに描き分ける。`longPress` を持つキー（flick 以外）は長押しでポップアップを出す
private struct KeyCell: View {
    let key: LayoutKeyDef
    @ObservedObject var model: KeyboardModel
    let height: CGFloat
    let originX: CGFloat
    let rowWidth: CGFloat
    let onPress: (Bool) -> Void

    var body: some View {
        switch key.kind {
        case .spacer:
            Color.clear
        case .flick:
            FlickKeyView(
                key: key,
                content: KeyFaceContent(text: key.label ?? key.kana ?? key.center?.text, icon: key.icon),
                chars: FlickChars(key: key),
                onPress: onPress,
                onGesture: { gesture, detail in model.flick(key, gesture, detail: detail) }
            )
        case .action where key.action == "backspace":
            RepeatKeyView(
                content: faceContent(key, model),
                style: key.style,
                action: { model.backspace() },
                swipeWord: model.ios.backspaceSwipe ? { model.deleteWordBackward() } : nil
            )
        case .action where key.action == "space" && model.ios.spaceSwipe && (key.longPress ?? []).isEmpty:
            SpaceKeyView(
                content: faceContent(key, model),
                style: key.style,
                fontSize: fontSize(key),
                step: model.ios.spaceSwipeStepPt,
                tap: { activate(key, model) },
                move: { model.spaceSwipeMove($0) }
            )
        default:
            if let popup = key.longPress, !popup.isEmpty {
                LongPressKeyView(
                    popup: popup,
                    content: faceContent(key, model),
                    style: key.style,
                    active: key.action == "shift" && model.shiftState == .caps,
                    fontSize: fontSize(key),
                    height: height,
                    originX: originX,
                    rowWidth: rowWidth,
                    onPress: onPress,
                    activateKey: { touch in activate(key, model, touch: touch) },
                    activateItem: { item in activate(item, model) }
                )
            } else {
                TapKeyView(
                    content: faceContent(key, model),
                    style: key.style,
                    active: key.action == "shift" && model.shiftState == .caps,
                    fontSize: fontSize(key),
                    action: { touch in activate(key, model, touch: touch) }
                )
            }
        }
    }
}

/// キーの表示（文字か SF Symbols）
private struct KeyFaceContent {
    var text: String?
    var icon: String?
}

private func fontSize(_ key: LayoutKeyDef) -> CGFloat {
    let count = (key.label ?? key.text ?? key.cycle?.first ?? "").count
    if count > 2 { return 16 }
    return (key.kind == .text) ? 20 : 17
}

/// 状態（変換中か・シフトか）に応じて表示を決める
private func faceContent(_ key: LayoutKeyDef, _ model: KeyboardModel) -> KeyFaceContent {
    if key.kind == .action {
        switch key.action ?? "" {
        case "space":
            return KeyFaceContent(text: model.hasPreedit ? "変換" : (key.label ?? "空白"), icon: nil)
        case "enter":
            return KeyFaceContent(text: model.hasPreedit ? "確定" : (key.label ?? "改行"), icon: nil)
        case "shift":
            // ラベルが無ければ状態で記号を変える（小文字 / 次の 1 文字だけ大文字 / Caps Lock）
            if let label = key.label { return KeyFaceContent(text: label, icon: nil) }
            let icon: String
            switch model.shiftState {
            case .off: icon = "shift"
            case .once: icon = "shift.fill"
            case .caps: icon = "capslock.fill"
            }
            return KeyFaceContent(text: nil, icon: icon)
        default:
            break
        }
    }
    if key.kind == .text, model.layoutDef.mode != .kana, model.shiftState != .off {
        return KeyFaceContent(text: key.shift ?? (key.label ?? key.text ?? "").uppercased(), icon: nil)
    }
    return KeyFaceContent(text: key.label ?? key.text ?? key.cycle?.first, icon: key.icon)
}

/// タップしたときの動作
private func activate(_ key: LayoutKeyDef, _ model: KeyboardModel, touch: TouchPoint? = nil) {
    switch key.kind {
    case .text:
        model.inputText(key.text ?? key.label ?? "", shift: key.shift, touch: touch)
    case .action:
        if let action = key.action { model.perform(action: action) }
    case .layout:
        if let target = key.target { model.switchLayout(target: target) }
    case .shortcut:
        model.shortcut(key.steps ?? [])
    case .toggle:
        model.toggle(key)
    case .flick, .spacer:
        break
    }
}

/// キーの見た目
private struct KeyFace: View {
    let content: KeyFaceContent
    let style: KeyStyle
    let active: Bool
    let pressed: Bool
    let fontSize: CGFloat

    var body: some View {
        label
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(RoundedRectangle(cornerRadius: keyCorner).fill(fill.opacity(pressed ? 0.6 : 1)))
            .contentShape(Rectangle())
    }

    @ViewBuilder
    private var label: some View {
        if let icon = content.icon {
            Image(systemName: icon)
                .font(.system(size: 17))
                .foregroundColor(foreground)
        } else {
            Text(content.text ?? "")
                .font(.system(size: fontSize))
                .foregroundColor(foreground)
                .lineLimit(1)
                .minimumScaleFactor(0.6)
        }
    }

    private var isAccent: Bool { active || style == .accent }

    private var fill: Color {
        if isAccent { return themeAccent }
        switch style {
        case .function: return Color(UIColor.systemGray4)
        default: return Color(UIColor.systemBackground)
        }
    }

    private var foreground: Color {
        isAccent ? Color.white : Color(UIColor.label)
    }
}

/// タップだけのキー
private struct TapKeyView: View {
    let content: KeyFaceContent
    let style: KeyStyle
    let active: Bool
    let fontSize: CGFloat
    /// 引数は、指が触れた位置のキー内の相対位置（曖昧入力の手がかり）
    let action: (TouchPoint?) -> Void

    @GestureState private var isPressed = false

    var body: some View {
        GeometryReader { geo in
            KeyFace(content: content, style: style, active: active, pressed: isPressed, fontSize: fontSize)
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .updating($isPressed) { _, state, _ in state = true }
                        .onEnded { value in
                            // 指を大きく動かして離した場合は、押した扱いにしない
                            if abs(value.translation.width) < 20 && abs(value.translation.height) < 20 {
                                NagiKeyFeedback.keyDown()
                                action(touchPoint(value.startLocation, in: geo.size))
                            }
                        }
                )
        }
    }
}

/// キー内のローカル座標を、キーの矩形に対する相対位置（0..1）にする
private func touchPoint(_ location: CGPoint, in size: CGSize) -> TouchPoint? {
    guard let rel = TouchGeometry.relative(
        x: Double(location.x), y: Double(location.y), width: Double(size.width), height: Double(size.height)
    ) else { return nil }
    return TouchPoint(x: rel.x, y: rel.y)
}

/// 長押しでポップアップを出すキー。`flick.long_press_ms` 押し続けると、キーの上に候補キーを並べる。
/// 指を滑らせて選び、離すと実行する（text / action / shortcut / toggle / layout）。
/// 長押しになる前に離したときは通常のタップ。ポップアップが出たあと、指がキーの下端より下へキーの高さの 0.3 倍以上、
/// またはポップアップの左右の外側へ項目 1 つ分を超えて離れた状態で離すと取り消し
/// （そのときポップアップは半透明になり「取り消し」と表示する）
private struct LongPressKeyView: View {
    let popup: [LayoutKeyDef]
    let content: KeyFaceContent
    let style: KeyStyle
    let active: Bool
    let fontSize: CGFloat
    let height: CGFloat
    let originX: CGFloat
    let rowWidth: CGFloat
    let onPress: (Bool) -> Void
    let activateKey: (TouchPoint?) -> Void
    let activateItem: (LayoutKeyDef) -> Void

    /// 指が触れている間 true。ジェスチャが `onEnded` なしで取り消された（システムのジェスチャに奪われた等）とき、
    /// 自動で false に戻るので、それを検出して状態をリセットする
    @GestureState private var touching = false
    @State private var isDown = false
    @State private var popupShown = false
    @State private var selected: Int?
    @State private var lastLocation = CGPoint.zero
    @State private var timer: Task<Void, Never>?

    var body: some View {
        GeometryReader { geo in
            KeyFace(content: content, style: style, active: active, pressed: isDown, fontSize: fontSize)
                .overlay(alignment: .topLeading) {
                    if popupShown {
                        popupView(keyWidth: geo.size.width)
                    }
                }
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .updating($touching) { _, state, _ in state = true }
                        .onChanged { value in changed(value, keyWidth: geo.size.width) }
                        .onEnded { value in ended(value, keySize: geo.size) }
                )
        }
        .onChange(of: touching) { nowTouching in
            guard !nowTouching else { return }
            // 正常に終わった場合は `onEnded` が状態を戻す。そちらが先に処理されるよう 1 周遅らせ、
            // それでもまだ押下中の状態が残っていれば、取り消されたとみなしてリセットする
            DispatchQueue.main.async { if isDown { reset() } }
        }
        .onDisappear { reset() }
    }

    // MARK: ジェスチャ

    private func changed(_ value: DragGesture.Value, keyWidth: CGFloat) {
        lastLocation = value.location
        if !isDown {
            isDown = true
            onPress(true)
            startTimer(keyWidth: keyWidth)
        }
        if popupShown {
            selected = index(at: value.location, keyWidth: keyWidth)
        } else if abs(value.translation.width) >= 20 || abs(value.translation.height) >= 20 {
            // 長押しになる前に大きく動いた: ポップアップは出さない
            timer?.cancel()
            timer = nil
        }
    }

    private func startTimer(keyWidth: CGFloat) {
        timer?.cancel()
        let delay = UInt64(NagiKeyMetrics.current.flickLongPressMs) * 1_000_000
        timer = Task { @MainActor in
            try? await Task.sleep(nanoseconds: delay)
            guard !Task.isCancelled, isDown else { return }
            popupShown = true
            selected = index(at: lastLocation, keyWidth: keyWidth)
            NagiKeyFeedback.keyDown()
        }
    }

    private func ended(_ value: DragGesture.Value, keySize: CGSize) {
        let wasPopup = popupShown
        let chosen = selected
        reset()
        if wasPopup {
            if let chosen = chosen, popup.indices.contains(chosen) {
                NagiKeyFeedback.keyDown()
                activateItem(popup[chosen])
            }
        } else if abs(value.translation.width) < 20 && abs(value.translation.height) < 20 {
            NagiKeyFeedback.keyDown()
            activateKey(touchPoint(value.startLocation, in: keySize))
        }
    }

    private func reset() {
        timer?.cancel()
        timer = nil
        if isDown { onPress(false) }
        isDown = false
        popupShown = false
        selected = nil
    }

    // MARK: ポップアップ

    private func itemWidth(keyWidth: CGFloat) -> CGFloat {
        min(max(keyWidth, 34), rowWidth / CGFloat(max(popup.count, 1)))
    }

    /// ポップアップの左端（キー内の座標）。行からはみ出さないよう寄せる
    private func popupLeft(keyWidth: CGFloat) -> CGFloat {
        let total = itemWidth(keyWidth: keyWidth) * CGFloat(popup.count)
        var left = keyWidth / 2 - total / 2
        if originX + left < 0 { left = -originX }
        if originX + left + total > rowWidth { left = rowWidth - total - originX }
        return left
    }

    /// 指の位置から選択中の項目を決める。キーの下端より下へキーの高さの 0.3 倍以上、またはポップアップの左右の外側へ
    /// 項目 1 つ分を超えて離れていれば nil（取り消し。`LongPressCancel`）。左右に少し外れたときは端の項目
    private func index(at location: CGPoint, keyWidth: CGFloat) -> Int? {
        let width = itemWidth(keyWidth: keyWidth)
        guard width > 0 else { return nil }
        let left = popupLeft(keyWidth: keyWidth)
        if LongPressCancel.isCancelled(
            x: Double(location.x), y: Double(location.y), keyHeight: Double(height),
            popupLeft: Double(left), popupWidth: Double(width * CGFloat(popup.count)), itemWidth: Double(width)
        ) { return nil }
        let raw = Int(((location.x - left) / width).rounded(.down))
        return min(max(raw, 0), popup.count - 1)
    }

    /// ポップアップが出ていて、指が取り消しの位置にある
    private var cancelled: Bool { popupShown && selected == nil }

    private func popupView(keyWidth: CGFloat) -> some View {
        let width = itemWidth(keyWidth: keyWidth)
        return HStack(spacing: 0) {
            ForEach(Array(popup.enumerated()), id: \.offset) { offset, item in
                let isSelected = (selected == offset) && !cancelled
                Group {
                    if let icon = item.icon {
                        Image(systemName: icon).font(.system(size: 18))
                    } else {
                        Text(item.label ?? item.text ?? "").font(.system(size: 20)).lineLimit(1).minimumScaleFactor(0.6)
                    }
                }
                .foregroundColor(isSelected ? Color.white : Color(UIColor.label))
                .frame(width: width, height: height)
                .background(
                    RoundedRectangle(cornerRadius: keyCorner)
                        .fill(isSelected ? themeAccent : Color(UIColor.secondarySystemBackground))
                )
            }
        }
        .background(RoundedRectangle(cornerRadius: keyCorner).fill(Color(UIColor.secondarySystemBackground)))
        .overlay(RoundedRectangle(cornerRadius: keyCorner).stroke(Color(UIColor.separator), lineWidth: 0.5))
        .fixedSize()
        .opacity(cancelled ? 0.4 : 1)
        .overlay(alignment: .top) {
            if cancelled {
                Text("取り消し")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundColor(.white)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Capsule().fill(Color.red))
                    .offset(y: -8)
                    .fixedSize()
            }
        }
        .offset(x: popupLeft(keyWidth: keyWidth), y: -(height + 6))
        .allowsHitTesting(false)
    }
}

/// ⌫ のキー: 押した瞬間に 1 文字消し、押し続けると連打（時間とともに加速）、離すと止まる。
/// 1 回ごとに `action`（入力中ならコアの backspace、そうでなければホストの 1 文字削除）を呼ぶ
private struct RepeatKeyView: View {
    let content: KeyFaceContent
    let style: KeyStyle
    let action: () -> Void
    /// 左スワイプで単語を削除する（設定 `touch.backspace_swipe`。nil なら無効）
    var swipeWord: (() -> Void)?

    @GestureState private var isPressed = false
    @State private var isDown = false
    /// 左スワイプの単語削除を済ませたか（離すまで連打・再実行をしない）
    @State private var swiped = false
    @State private var repeatTask: Task<Void, Never>?

    /// 連打を始めるまでの待ち（ナノ秒。設定 `touch.key_repeat_delay_ms`）
    private var initialDelay: UInt64 { UInt64(NagiKeyMetrics.current.keyRepeatDelayMs) * 1_000_000 }
    /// 最初の間隔（設定 `touch.key_repeat_ms`）と、加速の下限（最初の間隔の約 3 割、25ms 以上）
    private var startInterval: UInt64 { UInt64(NagiKeyMetrics.current.keyRepeatMs) * 1_000_000 }
    private var minInterval: UInt64 { max(25_000_000, startInterval * 32 / 100) }
    /// 暴走防止（離したイベントが届かなかった場合に、無限に消し続けない）
    private let maxRepeats = 150
    /// 単語削除とみなす左への移動量（pt）
    private static let swipeDistance: CGFloat = 40

    var body: some View {
        KeyFace(content: content, style: style, active: false, pressed: isPressed, fontSize: 17)
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        if !isDown {
                            isDown = true
                            NagiKeyFeedback.keyDown()
                            action()
                            startRepeat()
                            return
                        }
                        // 押した瞬間の 1 文字削除と連打はそのまま。左へ大きく動かしたら連打を止めて単語を消す
                        // （`stopRepeat` は押下状態を戻すので使わず、連打のタスクだけ止める）
                        if let swipeWord = swipeWord, !swiped,
                           value.translation.width < -Self.swipeDistance,
                           abs(value.translation.height) < abs(value.translation.width) {
                            swiped = true
                            repeatTask?.cancel()
                            repeatTask = nil
                            NagiKeyFeedback.keyDown()
                            swipeWord()
                        }
                    }
                    .updating($isPressed) { _, state, _ in state = true }
                    .onEnded { _ in stopRepeat() }
            )
            // ジェスチャが `onEnded` なしで取り消されても `isPressed` は false に戻るので、そこで連打を止める
            .onChange(of: isPressed) { pressed in
                if !pressed { stopRepeat() }
            }
            .onDisappear { stopRepeat() }
    }

    private func startRepeat() {
        repeatTask?.cancel()
        let step = action
        let delay = initialDelay
        let first = startInterval
        let floor = minInterval
        let limit = maxRepeats
        repeatTask = Task { @MainActor in
            try? await Task.sleep(nanoseconds: delay)
            var interval = first
            var count = 0
            while !Task.isCancelled && count < limit {
                step()
                count += 1
                if count % 4 == 0 && interval > floor {
                    interval = max(floor, interval * 85 / 100)
                }
                try? await Task.sleep(nanoseconds: interval)
            }
        }
    }

    private func stopRepeat() {
        repeatTask?.cancel()
        repeatTask = nil
        isDown = false
        swiped = false
    }
}

/// スペースキー: タップはスペース（または変換）。押したまま左右に `step` 以上動かすとカーソル移動モードに入り、
/// ドラッグ距離に比例してカーソルを動かす。このモードで離したときはスペースを入力しない
private struct SpaceKeyView: View {
    let content: KeyFaceContent
    let style: KeyStyle
    let fontSize: CGFloat
    let step: Double
    let tap: () -> Void
    /// カーソルを動かす文字数（右が正）
    let move: (Int) -> Void

    @GestureState private var isPressed = false
    @State private var tracker: SpaceSwipeTracker?

    var body: some View {
        KeyFace(content: content, style: style, active: false, pressed: isPressed, fontSize: fontSize)
            .gesture(
                DragGesture(minimumDistance: 0)
                    .updating($isPressed) { _, state, _ in state = true }
                    .onChanged { value in
                        var current = tracker ?? SpaceSwipeTracker(step: step)
                        let wasActive = current.isActive
                        let delta = current.update(translationWidth: Double(value.translation.width))
                        tracker = current
                        if current.isActive && !wasActive { NagiKeyFeedback.keyDown() }
                        if delta != 0 { move(delta) }
                    }
                    .onEnded { value in
                        let active = tracker?.isActive ?? false
                        tracker = nil
                        // カーソル移動モードに入っていたらスペースは入れない。動いていない（単純タップ）ときだけスペース
                        if !active && abs(value.translation.width) < 20 && abs(value.translation.height) < 20 {
                            NagiKeyFeedback.keyDown()
                            tap()
                        }
                    }
            )
            // `onEnded` なしで取り消された場合は、カーソル移動の状態を捨てる
            .onChange(of: isPressed) { pressed in
                if !pressed { DispatchQueue.main.async { tracker = nil } }
            }
    }
}

/// フリックキーの 5 つの文字（中央・左・上・右・下）。ポップアップの表示用
private struct FlickChars {
    var center: String?
    var left: String?
    var up: String?
    var right: String?
    var down: String?

    var hasAny: Bool {
        center != nil || left != nil || up != nil || right != nil || down != nil
    }

    /// レイアウトが文字を直接指定していればそれを、`kana` だけなら表示用の表（`KanaKeyTable`）を使う
    init(key: LayoutKeyDef) {
        if key.center != nil || key.left != nil || key.up != nil || key.right != nil || key.down != nil {
            center = key.center?.text
            left = key.left?.text
            up = key.up?.text
            right = key.right?.text
            down = key.down?.text
        } else if let kana = key.kana, let entry = KanaKeyTable.entry(forName: kana), entry.chars.count == 5 {
            center = entry.chars[0]
            left = entry.chars[1]
            up = entry.chars[2]
            right = entry.chars[3]
            down = entry.chars[4]
        }
    }
}

/// フリックできるキー。タップは `.tap`、一定以上動かして離すと動かした向きのフリック。
/// 押している間は、上下左右の文字をポップアップで案内する。
/// キーの幅・高さの 2 倍を超えて動かすと取り消し（離しても入力しない。`FlickCancel`）。
/// 取り消しは表示を出さずに静かに行い、その範囲にいる間はポップアップのどのセルも選択色にしない
private struct FlickKeyView: View {
    let key: LayoutKeyDef
    let content: KeyFaceContent
    let chars: FlickChars
    let onPress: (Bool) -> Void
    /// 操作と、方向のあるフリックならその角度・距離（曖昧入力の手がかり）
    let onGesture: (KeyGesture, FlickDetail?) -> Void

    /// 押している間の移動量（押していなければ nil）
    @GestureState private var drag: CGSize? = nil
    @State private var reported = false

    var body: some View {
        GeometryReader { keyGeo in
            keyBody(keySize: keyGeo.size)
        }
    }

    private func keyBody(keySize: CGSize) -> some View {
        KeyFace(
            content: content,
            style: key.style,
            active: false,
            pressed: drag != nil,
            fontSize: (content.text ?? "").count > 2 ? 16 : 22
        )
        .overlay(
            GeometryReader { geo in
                if let drag = drag {
                    // 取り消しの範囲では選択なし（nil）にする
                    let selected: KeyGesture? = Self.isCancelled(drag, keySize: keySize) ? nil : Self.gesture(for: drag)
                    ZStack {
                        if chars.hasAny, NagiKeyMetrics.current.flickPopup {
                            FlickPopup(chars: chars, selected: selected, size: geo.size)
                        }
                    }
                    .frame(width: geo.size.width, height: geo.size.height)
                }
            }
            .allowsHitTesting(false)
        )
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in
                    if !reported {
                        reported = true
                        onPress(true)
                    }
                }
                .updating($drag) { value, state, _ in state = value.translation }
                .onEnded { value in
                    reported = false
                    onPress(false)
                    // 取り消しの範囲で離したときは、入力もフィードバックもしない
                    if Self.isCancelled(value.translation, keySize: keySize) { return }
                    NagiKeyFeedback.keyDown()
                    let gesture = Self.gesture(for: value.translation)
                    var detail: FlickDetail?
                    if gesture != .tap,
                       let flick = TouchGeometry.flick(
                           dx: Double(value.translation.width), dy: Double(value.translation.height),
                           keyWidth: Double(keySize.width)
                       ) {
                        detail = FlickDetail(angle_deg: flick.angleDeg, distance: flick.distance)
                    }
                    onGesture(gesture, detail)
                }
        )
        // `onEnded` なしで取り消された場合（`drag` が nil に戻る）は、押下中の報告を取り消す。
        // 正常に終わった場合は `onEnded` が先に `reported` を戻すので、1 周遅らせて判定する
        .onChange(of: drag != nil) { nowDown in
            guard !nowDown else { return }
            DispatchQueue.main.async {
                if reported {
                    reported = false
                    onPress(false)
                }
            }
        }
        .onDisappear {
            if reported {
                reported = false
                onPress(false)
            }
        }
    }

    /// 移動量が取り消しの範囲か（キーの幅・高さの `FlickCancel.factor` 倍を超えた）
    static func isCancelled(_ translation: CGSize, keySize: CGSize) -> Bool {
        FlickCancel.isCancelled(
            dx: Double(translation.width), dy: Double(translation.height),
            keyWidth: Double(keySize.width), keyHeight: Double(keySize.height)
        )
    }

    /// 移動量から操作を決める（小さな移動はタップ）
    static func gesture(for translation: CGSize) -> KeyGesture {
        let threshold = CGFloat(NagiKeyMetrics.current.flickThresholdPoints)
        if abs(translation.width) < threshold && abs(translation.height) < threshold {
            return .tap
        }
        if abs(translation.width) > abs(translation.height) {
            return translation.width < 0 ? .flickLeft : .flickRight
        }
        return translation.height < 0 ? .flickUp : .flickDown
    }
}

/// フリックの押下ポップアップ: 押したキーの位置に中央の文字、上下左右に各方向の文字を出す
private struct FlickPopup: View {
    let chars: FlickChars
    /// 選択色にするセル。nil はどれも選ばない（フリックの取り消しの範囲）
    let selected: KeyGesture?
    let size: CGSize

    var body: some View {
        ZStack {
            cell(chars.center, .tap, 0, 0)
            cell(chars.left, .flickLeft, -1, 0)
            cell(chars.up, .flickUp, 0, -1)
            cell(chars.right, .flickRight, 1, 0)
            cell(chars.down, .flickDown, 0, 1)
        }
        .frame(width: size.width, height: size.height)
    }

    @ViewBuilder
    private func cell(_ text: String?, _ gesture: KeyGesture, _ dx: CGFloat, _ dy: CGFloat) -> some View {
        if let text = text {
            let isSelected = (selected == gesture)
            Text(text)
                .font(.system(size: 22))
                .foregroundColor(isSelected ? Color.white : Color(UIColor.label))
                .frame(width: size.width, height: size.height)
                .background(
                    RoundedRectangle(cornerRadius: keyCorner)
                        .fill(isSelected ? themeAccent : Color(UIColor.secondarySystemBackground))
                )
                .overlay(RoundedRectangle(cornerRadius: keyCorner).stroke(Color(UIColor.separator), lineWidth: 0.5))
                .position(
                    x: size.width / 2 + dx * (size.width + 2),
                    y: size.height / 2 + dy * (size.height + 2)
                )
        }
    }
}

/// 絵文字の一覧（静的。スクロールできる）
private struct EmojiGrid: View {
    @ObservedObject var model: KeyboardModel
    let height: CGFloat

    var body: some View {
        ScrollView(.vertical, showsIndicators: true) {
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 2), count: 8), spacing: 2) {
                ForEach(Array(EmojiData.all.enumerated()), id: \.offset) { _, emoji in
                    Button(action: { model.inputText(emoji, shift: nil) }) {
                        Text(emoji)
                            .font(.system(size: 26))
                            .frame(maxWidth: .infinity, minHeight: 40)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .frame(height: height)
    }
}

/// 候補バー: 変換中の文字列と候補。メッセージがあれば代わりに出す
private struct CandidateBar: View {
    @ObservedObject var model: KeyboardModel
    var onShowProbes: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            if let message = model.message {
                Text(message)
                    .font(.system(size: 12))
                    .foregroundColor(.red)
                    .lineLimit(2)
                Spacer(minLength: 0)
            } else {
                // 左端の「貼り付け」: クリップボードに文字列があり、入力中でないときだけ（設定 `candbar.paste_button`）。
                // 表示の判定は `hasStrings` のみ。クリップボードの中身はタップしたときに初めて読む
                if model.clipboardHasText && !model.hasPreedit {
                    Button(action: {
                        NagiKeyFeedback.keyDown()
                        model.pasteFromClipboard()
                    }) {
                        HStack(spacing: 3) {
                            Image(systemName: "doc.on.clipboard").font(.system(size: 13))
                            Text("貼り付け").font(.system(size: 13)).lineLimit(1)
                        }
                        .foregroundColor(Color(UIColor.label))
                        .padding(.horizontal, 8)
                        .frame(height: 32)
                        .background(RoundedRectangle(cornerRadius: 6).fill(Color(UIColor.systemBackground)))
                    }
                    .buttonStyle(.plain)
                }
                if let warning = model.warning, !model.hasPreedit, model.candidates.isEmpty {
                    Text(warning)
                        .font(.system(size: 11))
                        .foregroundColor(.orange)
                        .lineLimit(2)
                }
                if model.isConverting && model.preeditParts.count > 1 {
                    // 変換中で文節が複数あれば、注目文節を太字・アクセント色で区別する
                    // （iOS の marked text には属性を付けられないため、ここが注目文節の主な表示）
                    segmentsText(model.preeditParts)
                        .font(.system(size: 14))
                        .underline()
                        .lineLimit(1)
                } else if model.hasPreedit {
                    Text(model.preedit)
                        .font(.system(size: 14))
                        .foregroundColor(.secondary)
                        .underline()
                        .lineLimit(1)
                }
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(model.candidates) { item in
                            CandidateButton(item: item,
                                            selected: item.id == model.selectedCandidate && !model.isNextWordCandidates,
                                            nextWord: model.isNextWordCandidates) {
                                model.selectCandidate(item.id)
                            }
                        }
                    }
                }
                // 次語予測は展開パネルを使わない（候補数が少なく、選ぶとそのまま挿入される）
                if !model.candidates.isEmpty && !model.isNextWordCandidates {
                    Button(action: { model.isExpanded.toggle() }) {
                        Text(model.isExpanded ? "︿" : "﹀")
                            .font(.system(size: 16))
                            .foregroundColor(Color(UIColor.label))
                            .frame(width: 32, height: 32)
                    }
                }
            }
            // 検証用の画面は DEBUG / TestFlight のビルドだけ（App Store 版では出さない）
            if DiagLog.isTestBuild {
                Button("検証", action: onShowProbes)
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
            }
        }
        .padding(.horizontal, 8)
        .frame(height: 44)
        .background(Color(UIColor.systemGray5))
    }

    /// 文節ごとの Text を連結する（注目文節は太字・アクセント色、ほかは secondary）
    private func segmentsText(_ parts: [KeyboardModel.PreeditPart]) -> Text {
        parts.reduce(Text(verbatim: "")) { result, part in
            let piece: Text = part.focused
                ? Text(verbatim: part.text).bold().foregroundColor(themeAccent)
                : Text(verbatim: part.text).foregroundColor(.secondary)
            return result + piece
        }
    }
}

/// 候補 1 つのボタン
private struct CandidateButton: View {
    let item: CandidateItem
    let selected: Bool
    /// 確定直後の次語予測（背景を薄くして、入力中の候補と区別する）
    var nextWord = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 0) {
                // 曖昧入力の補正候補: 「もしかして: かんじ」（選ぶと補正後の読みで確定する）
                if let hint = item.hint {
                    Text(hint)
                        .font(.system(size: 9))
                        .foregroundColor(.orange)
                        .lineLimit(1)
                }
                Text(item.surface)
                    .font(.system(size: 17))
                    .foregroundColor(Color(UIColor.label))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, item.hint == nil ? 6 : 2)
            .background(
                RoundedRectangle(cornerRadius: 6)
                    .fill(selected ? themeAccent.opacity(0.25)
                          : Color(UIColor.systemBackground).opacity(nextWord ? 0.55 : 1))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 6)
                    .stroke(Color.orange.opacity(item.hint == nil ? 0 : 0.5), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}

/// 広げた候補の一覧（キーの代わりに出す）。縦にスクロールできる
private struct ExpandedCandidates: View {
    @ObservedObject var model: KeyboardModel

    var body: some View {
        ScrollView(.vertical, showsIndicators: true) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 84), spacing: keySpacing)], spacing: keySpacing) {
                ForEach(model.candidates) { item in
                    Button(action: { model.selectCandidate(item.id) }) {
                        VStack(spacing: 0) {
                            if let hint = item.hint {
                                Text(hint)
                                    .font(.system(size: 9))
                                    .foregroundColor(.orange)
                                    .lineLimit(1)
                                    .minimumScaleFactor(0.7)
                            }
                            Text(item.surface)
                                .font(.system(size: 17))
                                .lineLimit(1)
                                .foregroundColor(Color(UIColor.label))
                        }
                        .frame(maxWidth: .infinity, minHeight: keyHeight)
                            .background(
                                RoundedRectangle(cornerRadius: 6)
                                    .fill(item.id == model.selectedCandidate
                                          ? themeAccent.opacity(0.25)
                                          : Color(UIColor.systemBackground))
                            )
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        // 現在のレイアウトのキー領域の高さに合わせる
        .frame(height: padHeight(model.layoutDef))
    }
}
