import SwiftUI

/// 製品版キーボードの画面: 候補バーと、レイアウト（JSON。`Shared/KeyboardLayout.swift`）から描くキー
struct NagiKeyboardView: View {
    @ObservedObject var model: KeyboardModel
    /// 検証用の画面（Probe）を開く
    var onShowProbes: () -> Void
    /// キーボード全体の大きさ（フリックの案内を view の枠内に収めるのに使う）
    @State private var rootSize = CGSize.zero

    var body: some View {
        VStack(spacing: 0) {
            CandidateBar(model: model, onShowProbes: onShowProbes)
            if model.ios.oneHanded == "off" {
                padArea
                    .padding(CGFloat(model.metrics.padding))
            } else {
                // 片手モード: キー部分を左または右に寄せる（候補バーは全幅のまま）
                GeometryReader { geo in
                    HStack(spacing: 0) {
                        if model.ios.oneHanded == "right" { OneHandedButtons(model: model) }
                        padArea
                            .padding(CGFloat(model.metrics.padding))
                            .frame(width: geo.size.width * CGFloat(model.ios.oneHandedWidth))
                        if model.ios.oneHanded == "left" { OneHandedButtons(model: model) }
                    }
                }
            }
        }
        // 背景は、レイアウトの `background` が無ければ塗らない。システムのキーボードの背景（フッターの地球儀・マイクの帯と
        // 同じ）がそのまま見えて、キーボード本体とフッターが一体に見える
        // 背景は画面の端（角丸・下のフッター側の余白）まで広げ、色を変えたときも角丸の部分まで同じ色にする
        .background(KeyboardBackgroundView(background: model.layoutDef.background).ignoresSafeArea())
        .background(
            GeometryReader { geo in
                Color.clear
                    .onAppear { rootSize = geo.size }
                    .onChange(of: geo.size) { size in rootSize = size }
            }
        )
        .coordinateSpace(name: NagiKeyboardSpace.name)
        .environment(\.nagiKeyboardSize, rootSize)
        // 設定 `keyboard.accent` / `keyboard.theme`。テーマが変わったときは子のビューも作り直して新しい色を見せる
        // （子のビューは入力が変わらないと再評価されないため）。外観の上書きは KeyboardViewController が行う
        .tint(themeAccent)
        .id("\(model.ios.theme)-\(model.ios.accent)")
    }

    /// キー（または展開した候補一覧）の領域
    private var padArea: some View {
        Group {
            if model.isClipboardHistoryOpen {
                ClipboardHistoryPage(
                    model: model, height: padHeight(model),
                    spacing: CGFloat(model.metrics.keySpacing), accent: themeAccent)
            } else if model.isExpanded {
                ExpandedCandidates(model: model)
            } else {
                LayoutPad(model: model, visual: model.visual)
            }
        }
    }
}

/// キーボード全体の座標空間の名前と大きさ（環境値）。フリックの案内の位置を view の枠内に収めるために使う
enum NagiKeyboardSpace {
    static let name = "nagiKeyboard"
}

private struct NagiKeyboardSizeKey: EnvironmentKey {
    static let defaultValue = CGSize.zero
}

extension EnvironmentValues {
    var nagiKeyboardSize: CGSize {
        get { self[NagiKeyboardSizeKey.self] }
        set { self[NagiKeyboardSizeKey.self] = newValue }
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

/// キー 1 行の高さ（設定の高さの倍率。`NagiKeyMetrics`）。キー間・行間・外周の余白は `KeyboardModel.metrics`
/// （設定 `keyboard.key_spacing_pt` を、レイアウトの `metrics` が上書きする）
private var keyHeight: CGFloat { CGFloat(NagiKeyMetrics.keyHeight) }
/// アクセントカラー（設定 `keyboard.accent`）
private var themeAccent: Color { Color(NagiTheme.accentUIColor(NagiKeyMetrics.current.accent)) }
/// キーの角丸（設定 `keyboard.key_corner_pt`）
private var keyCorner: CGFloat { CGFloat(NagiKeyMetrics.current.keyCornerPt) }

/// キー領域の高さ。絵文字は一覧を含め、キー 4 行分に揃える
private func padHeight(_ model: KeyboardModel) -> CGFloat {
    CGFloat(LayoutGeometry.padHeight(
        layout: model.layoutDef, keyHeight: NagiKeyMetrics.keyHeight, rowSpacing: model.metrics.rowSpacing
    ))
}

/// キー領域の高さ（行だけ）
private func rowsHeight(_ model: KeyboardModel) -> CGFloat {
    CGFloat(LayoutGeometry.rowsHeight(
        layout: model.layoutDef, keyHeight: NagiKeyMetrics.keyHeight, rowSpacing: model.metrics.rowSpacing
    ))
}

// MARK: - レイアウトの描画

/// レイアウトの行とキーを描く
private struct LayoutPad: View {
    @ObservedObject var model: KeyboardModel
    /// スペースのドラッグ移動中の起点（`spaceDrag` の状態。値が変わるのは移動の開始と終了だけ）
    @ObservedObject var visual: KeyboardVisualState
    /// 押下中のキー（"行:列"）。ポップアップが下の行に隠れないよう、その行を手前に出す
    @State private var pressedKey: String?

    var body: some View {
        VStack(spacing: CGFloat(model.metrics.rowSpacing)) {
            if model.layoutDef.mode == .emoji {
                EmojiGrid(
                    model: model,
                    height: padHeight(model) - rowsHeight(model) - CGFloat(model.metrics.rowSpacing)
                )
            }
            ForEach(Array(model.layoutDef.rows.enumerated()), id: \.offset) { rowIndex, row in
                RowView(
                    row: row, rowIndex: rowIndex, model: model, spaceDragOrigin: visual.spaceDragOrigin,
                    pressedKey: $pressedKey
                )
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
    let spaceDragOrigin: String?
    @Binding var pressedKey: String?

    var body: some View {
        GeometryReader { geo in
            HStack(spacing: keySpacing) {
                ForEach(Array(row.keys.enumerated()), id: \.offset) { index, key in
                    KeyCell(
                        key: key, rowIndex: rowIndex, model: model, spaceDragOrigin: spaceDragOrigin, height: rowHeight,
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

    private var keySpacing: CGFloat { CGFloat(model.metrics.keySpacing) }

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
    let rowIndex: Int
    @ObservedObject var model: KeyboardModel
    let spaceDragOrigin: String?
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
                face: face(KeyFaceContent(text: key.label ?? key.kana ?? key.center?.text, icon: key.icon)),
                chars: FlickChars(key: key),
                onPress: onPress,
                onGesture: { gesture, detail in model.flick(key, gesture, detail: detail) }
            )
        case .action where key.action == "backspace":
            RepeatKeyView(
                face: face(faceContent(key, model)),
                action: { model.backspace() },
                swipeWord: model.ios.backspaceSwipe ? { model.deleteWordBackward() } : nil
            )
        case .action where key.action == "space" && model.ios.spaceSwipe && (key.longPress ?? []).isEmpty:
            SpaceKeyView(
                face: face(faceContent(key, model)),
                step: model.ios.spaceSwipeStepPt,
                tap: { activate(key, model) },
                move: { model.spaceSwipeMove($0) },
                onSpaceDrag: { entered in
                    // スペースのドラッグ移動の開始 / 終了。この状態を使うスタイルがレイアウトに無ければ何も通知しない
                    let uses = model.lookCache.layoutUses(.spaceDrag, layout: model.layoutDef)
                    model.visual.setSpaceDragOrigin(entered && uses ? key.id : nil)
                }
            )
        default:
            if let popup = key.longPress, !popup.isEmpty {
                LongPressKeyView(
                    popup: popup,
                    face: face(faceContent(key, model)),
                    height: height,
                    originX: originX,
                    rowWidth: rowWidth,
                    onPress: onPress,
                    activateKey: { touch in activate(key, model, touch: touch) },
                    activateItem: { item in activate(item, model) }
                )
            } else {
                TapKeyView(
                    face: face(faceContent(key, model)),
                    action: { touch in activate(key, model, touch: touch) }
                )
            }
        }
    }
}

extension KeyCell {
    /// 描画の入力。Caps Lock 中の Shift キーは accent の色（現行の見た目）
    fileprivate func face(_ content: KeyFaceContent) -> KeyFaceInput {
        KeyFaceInput(
            layout: model.layoutDef, cache: model.lookCache, rowIndex: rowIndex, key: key, content: content,
            active: key.action == "shift" && model.shiftState == .caps,
            global: LookStateLogic.globalStates(
                shiftOnce: model.shiftState == .once, capsLock: model.shiftState == .caps,
                composing: model.hasPreedit, spaceDrag: spaceDragOrigin != nil
            ),
            origin: spaceDragOrigin
        )
    }
}

/// 状態（変換中か・シフトか）に応じて表示を決める
private func faceContent(_ key: LayoutKeyDef, _ model: KeyboardModel) -> KeyFaceContent {
    if key.kind == .action {
        switch key.action ?? "" {
        case "space":
            return KeyFaceContent(text: model.hasPreedit ? "変換" : (key.label ?? "空白"), icon: nil)
        case "enter":
            // 入力欄の改行キーの種類（検索・送信など）があればその表示にする
            return KeyFaceContent(text: model.hasPreedit ? "確定" : (model.returnKeyLabel ?? key.label ?? "改行"), icon: nil)
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

/// タップだけのキー
private struct TapKeyView: View {
    let face: KeyFaceInput
    /// 引数は、指が触れた位置のキー内の相対位置（曖昧入力の手がかり）
    let action: (TouchPoint?) -> Void

    @GestureState private var isPressed = false
    @StateObject private var hold = ReleaseHold()

    var body: some View {
        GeometryReader { geo in
            KeyFaceHost(
                input: face,
                local: LookStateLogic.localStates(pressed: isPressed, released: hold.active, longPress: false, flicking: false)
            )
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .updating($isPressed) { _, state, _ in state = true }
                        .onEnded { value in
                            if face.uses(.released) { hold.begin(ms: face.releasedHoldMs) }
                            // 指を大きく動かして離した場合は、押した扱いにしない
                            if abs(value.translation.width) < 20 && abs(value.translation.height) < 20 {
                                NagiKeyFeedback.keyDown()
                                action(touchPoint(value.startLocation, in: geo.size))
                            }
                        }
                )
        }
        // 再び押されたら `released` は即座に抜ける
        .onChange(of: isPressed) { pressed in
            if pressed { hold.cancel() }
        }
        .onDisappear { hold.cancel() }
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
    let face: KeyFaceInput
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
    @StateObject private var hold = ReleaseHold()

    var body: some View {
        GeometryReader { geo in
            KeyFaceHost(
                input: face,
                local: LookStateLogic.localStates(pressed: isDown, released: hold.active, longPress: popupShown, flicking: false)
            )
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
        .onDisappear {
            reset()
            hold.cancel()
        }
    }

    // MARK: ジェスチャ

    private func changed(_ value: DragGesture.Value, keyWidth: CGFloat) {
        lastLocation = value.location
        if !isDown {
            isDown = true
            hold.cancel()
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
        // 正常な終了（取り消しでない）。入力が実行されたかは問わない
        if face.uses(.released) { hold.begin(ms: face.releasedHoldMs) }
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
                    if let icon = item.icon, KeyIconView.isAvailable(icon) {
                        KeyIconView(raw: icon, size: 18)
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
    let face: KeyFaceInput
    let action: () -> Void
    /// 左スワイプで単語を削除する（設定 `touch.backspace_swipe`。nil なら無効）
    var swipeWord: (() -> Void)?

    @GestureState private var isPressed = false
    @State private var isDown = false
    /// 左スワイプの単語削除を済ませたか（離すまで連打・再実行をしない）
    @State private var swiped = false
    @State private var repeatTask: Task<Void, Never>?
    /// 連打が始まった（`longPress` の状態。この状態を使うスタイルがあるときだけ立てる）
    @State private var isLongPress = false
    @StateObject private var hold = ReleaseHold()

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
        KeyFaceHost(
            input: face,
            local: LookStateLogic.localStates(pressed: isPressed, released: hold.active, longPress: isLongPress, flicking: false)
        )
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        if !isDown {
                            isDown = true
                            hold.cancel()
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
                    .onEnded { _ in
                        stopRepeat()
                        if face.uses(.released) { hold.begin(ms: face.releasedHoldMs) }
                    }
            )
            // ジェスチャが `onEnded` なしで取り消されても `isPressed` は false に戻るので、そこで連打を止める
            .onChange(of: isPressed) { pressed in
                if !pressed { stopRepeat() }
            }
            .onDisappear {
                stopRepeat()
                hold.cancel()
            }
    }

    private func startRepeat() {
        repeatTask?.cancel()
        let step = action
        let delay = initialDelay
        let first = startInterval
        let floor = minInterval
        let limit = maxRepeats
        let tracksLongPress = face.uses(.longPress)
        repeatTask = Task { @MainActor in
            try? await Task.sleep(nanoseconds: delay)
            // 連打が始まる瞬間に `longPress` に入る
            if tracksLongPress && !Task.isCancelled { isLongPress = true }
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
        if isLongPress { isLongPress = false }
    }
}

/// スペースキー: タップはスペース（または変換）。押したまま左右に `step` 以上動かすとカーソル移動モードに入り、
/// ドラッグ距離に比例してカーソルを動かす。このモードで離したときはスペースを入力しない
private struct SpaceKeyView: View {
    let face: KeyFaceInput
    let step: Double
    let tap: () -> Void
    /// カーソルを動かす文字数（右が正）
    let move: (Int) -> Void
    /// カーソル移動モードの開始（true）/ 終了（false）。`spaceDrag` の状態
    let onSpaceDrag: (Bool) -> Void

    @GestureState private var isPressed = false
    @State private var tracker: SpaceSwipeTracker?
    @StateObject private var hold = ReleaseHold()

    var body: some View {
        KeyFaceHost(
            input: face,
            local: LookStateLogic.localStates(pressed: isPressed, released: hold.active, longPress: false, flicking: false)
        )
            .gesture(
                DragGesture(minimumDistance: 0)
                    .updating($isPressed) { _, state, _ in state = true }
                    .onChanged { value in
                        var current = tracker ?? SpaceSwipeTracker(step: step)
                        let wasActive = current.isActive
                        let delta = current.update(translationWidth: Double(value.translation.width))
                        tracker = current
                        if current.isActive && !wasActive {
                            NagiKeyFeedback.keyDown()
                            onSpaceDrag(true)
                        }
                        if delta != 0 { move(delta) }
                    }
                    .onEnded { value in
                        let active = tracker?.isActive ?? false
                        tracker = nil
                        onSpaceDrag(false)
                        if face.uses(.released) { hold.begin(ms: face.releasedHoldMs) }
                        // カーソル移動モードに入っていたらスペースは入れない。動いていない（単純タップ）ときだけスペース
                        if !active && abs(value.translation.width) < 20 && abs(value.translation.height) < 20 {
                            NagiKeyFeedback.keyDown()
                            tap()
                        }
                    }
            )
            // `onEnded` なしで取り消された場合は、カーソル移動の状態を捨てる
            .onChange(of: isPressed) { pressed in
                if pressed {
                    hold.cancel()
                } else {
                    DispatchQueue.main.async {
                        tracker = nil
                        onSpaceDrag(false)
                    }
                }
            }
            .onDisappear {
                tracker = nil
                onSpaceDrag(false)
                hold.cancel()
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
    let face: KeyFaceInput
    let chars: FlickChars
    let onPress: (Bool) -> Void
    /// 操作と、方向のあるフリックならその角度・距離（曖昧入力の手がかり）
    let onGesture: (KeyGesture, FlickDetail?) -> Void

    /// 押している間の移動量（押していなければ nil）
    @GestureState private var drag: CGSize? = nil
    @State private var reported = false
    @StateObject private var hold = ReleaseHold()
    @Environment(\.nagiKeyboardSize) private var rootSize

    var body: some View {
        GeometryReader { keyGeo in
            keyBody(keySize: keyGeo.size, keyFrame: keyGeo.frame(in: .named(NagiKeyboardSpace.name)))
        }
    }

    /// 案内を view の枠内に収める移動量（最下段の下方向のセルなど、枠の外へ出るものを内側へ寄せる）
    private func popupShift(keySize: CGSize, keyFrame: CGRect) -> CGSize {
        let bounds = CGRect(x: -keyFrame.minX, y: -keyFrame.minY, width: rootSize.width, height: rootSize.height)
        return PopupClamp.flickShift(
            keySize: keySize, spacing: FlickPopup.cellSpacing,
            left: chars.left != nil, up: chars.up != nil, right: chars.right != nil, down: chars.down != nil,
            bounds: bounds
        )
    }

    private func keyBody(keySize: CGSize, keyFrame: CGRect) -> some View {
        KeyFaceHost(
            input: face,
            local: LookStateLogic.localStates(
                pressed: drag != nil, released: hold.active, longPress: false,
                flicking: drag.map { FlickState.isFlicking(
                    dx: Double($0.width), dy: Double($0.height), threshold: NagiKeyMetrics.current.flickThresholdPoints,
                    keyWidth: Double(keySize.width), keyHeight: Double(keySize.height)
                ) } ?? false
            )
        )
        .overlay(
            GeometryReader { geo in
                if let drag = drag {
                    // 取り消しの範囲では選択なし（nil）にする
                    let selected: KeyGesture? = Self.isCancelled(drag, keySize: keySize) ? nil : Self.gesture(for: drag)
                    ZStack {
                        if chars.hasAny, NagiKeyMetrics.current.flickPopup {
                            FlickPopup(
                                chars: chars, selected: selected, size: geo.size,
                                shift: popupShift(keySize: keySize, keyFrame: keyFrame)
                            )
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
                        hold.cancel()
                        onPress(true)
                    }
                }
                .updating($drag) { value, state, _ in state = value.translation }
                .onEnded { value in
                    reported = false
                    onPress(false)
                    if face.uses(.released) { hold.begin(ms: face.releasedHoldMs) }
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
            hold.cancel()
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
    /// view の枠内に収めるための移動量（`PopupClamp.flickShift`）
    var shift = CGSize.zero
    /// セルとセルのあいだ（pt）
    static let cellSpacing: CGFloat = 2

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
                    x: size.width / 2 + dx * (size.width + Self.cellSpacing) + shift.width,
                    y: size.height / 2 + dy * (size.height + Self.cellSpacing) + shift.height
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
            } else if ToolbarLogic.shouldShow(
                hasPreedit: model.hasPreedit, candidatesEmpty: model.candidates.isEmpty, hasMessage: false
            ) {
                // 入力中でなく候補も無いときは、候補バーの場所にツールバー（レイアウトの `toolbar`、無ければ設定
                // `toolbar.items`）を出す。候補が来たら候補バーに切り替わる（バーの高さは変わらない）。
                // このとき候補は空なので、候補の一覧（ScrollView）は出さない
                ToolbarRow(model: model)
                if let warning = model.warning {
                    Text(warning)
                        .font(.system(size: 11))
                        .foregroundColor(.orange)
                        .lineLimit(2)
                }
            } else {
                if model.isConverting && model.preeditParts.count > 1 {
                    // 変換中で文節が複数あれば、注目文節を太字・アクセント色で区別する
                    // （iOS の marked text には属性を付けられないため、ここが注目文節の主な表示）
                    segmentsText(model.preeditParts)
                        .font(.system(size: 14))
                        .underline()
                        .lineLimit(1)
                }
                // 入力中の読みの文字列は出さない（ホストの下線つきの文字で見える）。候補だけを一列に並べる
                // 入力中の候補は読みの接頭辞の分まで 100 件を超えることがある（H-58）ので、遅延して並べる。
                // 候補が入れ替わったら先頭へ戻す（id を変えて作り直す）。
                // 曖昧入力の補正候補のうち最有力（コアの並びで最初のもの）は、一覧から外して右端に固定する
                let pinned = pinnedFuzzyIndex(hints: model.candidates.map { $0.hint },
                                              nextWord: model.isNextWordCandidates).map { model.candidates[$0] }
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(spacing: 6) {
                        ForEach(model.candidates.filter { $0.id != pinned?.id }) { item in
                            CandidateButton(item: item,
                                            selected: item.id == model.selectedCandidate && !model.isNextWordCandidates,
                                            nextWord: model.isNextWordCandidates) {
                                model.selectCandidate(item.id)
                            }
                        }
                    }
                }
                .id(candidateBarIdentity(model.candidates.map { $0.surface }))
                // 固定した候補の手前で一覧を背景へ溶かす（背景はシステムの色・レイアウトの画像のどちらもありうるので、
                // 色を塗らずに一覧の右端を透明にする）
                .mask {
                    HStack(spacing: 0) {
                        Rectangle()
                        if pinned != nil {
                            LinearGradient(colors: [.black, .clear], startPoint: .leading, endPoint: .trailing)
                                .frame(width: 24)
                        }
                    }
                }
                if let pinned = pinned {
                    PinnedFuzzyButton(item: pinned,
                                      selected: pinned.id == model.selectedCandidate) {
                        model.selectCandidate(pinned.id)
                    }
                }
                // 次語予測は展開パネルを使わない（候補数が少なく、選ぶとそのまま挿入される）。
                // 押せる範囲は矢印だけでなく、候補バーの高さいっぱい
                if !model.candidates.isEmpty && !model.isNextWordCandidates {
                    Button(action: { model.isExpanded.toggle() }) {
                        Image(systemName: model.isExpanded ? "chevron.up" : "chevron.down")
                            .font(.system(size: 15))
                            .foregroundColor(Color(UIColor.label))
                            .frame(width: 36)
                            .frame(maxHeight: .infinity)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(model.isExpanded ? "候補を閉じる" : "候補を広げる")
                }
            }
            // 検証用の画面は DEBUG / TestFlight のビルドで、開発者モード（設定 `developer.mode`）がオンのときだけ
            // （App Store 版では出さない）
            if DiagLog.isTestBuild && model.ios.developerMode {
                Button("検証", action: onShowProbes)
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
            }
        }
        .padding(.horizontal, 8)
        .frame(height: 44)
        // 塗らない: システムのキーボードの背景（またはレイアウトの `background`）がそのまま見えて、
        // 候補バー・キー・フッターが一体に見える
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

/// ツールバー（高さ 44pt 固定の 1 行）。キーはレイアウトの `toolbar.keys`、無ければ設定 `toolbar.items` から作ったキー列。
/// 描画は rows のキーと同じ `KeyFaceHost`（見た目の解決は行番号 -1）。幅は `width` の比で按分し、タップだけ使う。
/// 貼り付けは、クリップボードに文字があるときだけ出す
/// （表示の判定は `hasStrings` のみで、中身はタップしたときに初めて読む。フルアクセスが無い・許可されなければ隠れる）
private struct ToolbarRow: View {
    @ObservedObject var model: KeyboardModel

    var body: some View {
        let keys = ToolbarKeys.visibleKeys(
            layoutToolbar: model.layoutDef.toolbar, items: model.ios.toolbarItems, clipboardHasText: model.clipboardHasText
        )
        let spacing = CGFloat(model.metrics.keySpacing)
        GeometryReader { geo in
            HStack(spacing: spacing) {
                ForEach(Array(keys.enumerated()), id: \.offset) { _, key in
                    cell(key)
                        .frame(width: cellWidth(key, in: keys, total: geo.size.width, spacing: spacing), height: geo.size.height)
                }
            }
        }
        .frame(height: 44)
    }

    @ViewBuilder
    private func cell(_ key: LayoutKeyDef) -> some View {
        if key.kind == .spacer {
            Color.clear
        } else {
            TapKeyView(
                face: KeyFaceInput(
                    layout: model.layoutDef, cache: model.lookCache, rowIndex: LookResolver.toolbarRow, key: key,
                    content: ToolbarKeys.faceContent(for: key), active: false,
                    global: LookStateLogic.globalStates(
                        shiftOnce: model.shiftState == .once, capsLock: model.shiftState == .caps,
                        composing: model.hasPreedit, spaceDrag: false
                    ),
                    origin: nil
                ),
                action: { _ in model.activateToolbarKey(key) }
            )
            .accessibilityLabel(ToolbarKeys.accessibilityLabel(for: key) ?? "")
        }
    }

    private func cellWidth(_ key: LayoutKeyDef, in keys: [LayoutKeyDef], total: CGFloat, spacing: CGFloat) -> CGFloat {
        let weights = keys.reduce(0.0) { $0 + $1.width }
        guard weights > 0 else { return 0 }
        let available = total - spacing * CGFloat(max(keys.count - 1, 0))
        return max(0, available * CGFloat(key.width / weights))
    }
}

/// 候補バーの右端に固定する曖昧入力の補正候補（最有力の 1 件）。「💡: 青豚」の形で出す
private struct PinnedFuzzyButton: View {
    let item: CandidateItem
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 3) {
                Image(systemName: "lightbulb")
                    .font(.system(size: 14))
                    .foregroundColor(Color(UIColor.label))
                Text(verbatim: ": \(item.surface)")
                    .font(.system(size: 17))
                    .foregroundColor(Color(UIColor.label))
                    .lineLimit(1)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                RoundedRectangle(cornerRadius: 6)
                    .fill(selected ? themeAccent.opacity(0.25) : Color(UIColor.systemBackground))
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel(item.hint.map { "\($0)、\(item.surface)" } ?? item.surface)
    }
}

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
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 84), spacing: CGFloat(model.metrics.keySpacing))],
                spacing: CGFloat(model.metrics.rowSpacing)
            ) {
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
        .frame(height: padHeight(model))
    }
}
