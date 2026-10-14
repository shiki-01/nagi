import SwiftUI

/// 製品版キーボードの画面: 候補バーと、かな（12 キー・フリック）または英字（QWERTY）のキー
struct NagiKeyboardView: View {
    @ObservedObject var model: KeyboardModel
    /// 検証用の画面（Probe）を開く
    var onShowProbes: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            CandidateBar(model: model, onShowProbes: onShowProbes)
            Group {
                switch model.layout {
                case .kana:
                    KanaPad(model: model)
                case .qwerty:
                    QwertyPad(model: model)
                }
            }
            .padding(6)
        }
        .background(Color(UIColor.systemGray6))
    }
}

private let keySpacing: CGFloat = 6
private let keyHeight: CGFloat = 46

/// かなのキー: 12 キー（フリック入力）と機能キー
private struct KanaPad: View {
    @ObservedObject var model: KeyboardModel

    var body: some View {
        HStack(alignment: .top, spacing: keySpacing) {
            // 左の列
            VStack(spacing: keySpacing) {
                FunctionKey(label: "←", height: keyHeight) { model.arrow(.left) }
                FunctionKey(label: "→", height: keyHeight) { model.arrow(.right) }
                FunctionKey(label: "A", height: keyHeight) { model.switchLayout() }
                FunctionKey(label: "🌐", height: keyHeight) { model.nextKeyboard() }
            }
            .frame(width: 54)
            // かなキー（3 列 × 4 行）
            VStack(spacing: keySpacing) {
                ForEach(0..<rows.count, id: \.self) { rowIndex in
                    HStack(spacing: keySpacing) {
                        ForEach(rows[rowIndex]) { spec in
                            FlickKey(label: spec.label, hint: spec.hint, height: keyHeight) { gesture in
                                spec.action(model, gesture)
                            }
                        }
                    }
                }
            }
            // 右の列
            VStack(spacing: keySpacing) {
                FunctionKey(label: "⌫", height: keyHeight) { model.backspace() }
                FunctionKey(label: model.hasPreedit ? "変換" : "空白", height: keyHeight) { model.space() }
                FunctionKey(label: model.hasPreedit ? "確定" : "改行", height: keyHeight * 2 + keySpacing) { model.enter() }
            }
            .frame(width: 64)
        }
    }

    private var rows: [[KeySpec]] {
        [
            [.kana("あ", .a), .kana("か", .ka), .kana("さ", .sa)],
            [.kana("た", .ta), .kana("な", .na), .kana("は", .ha)],
            [.kana("ま", .ma), .kana("や", .ya), .kana("ら", .ra)],
            [.variant, .kana("わ", .wa), .kana("、。？！", .punct)],
        ]
    }
}

/// 英字のキー（QWERTY）。変換は通さず、押した文字をそのままホストに入れる
private struct QwertyPad: View {
    @ObservedObject var model: KeyboardModel

    private let letterRows = [
        ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
        ["a", "s", "d", "f", "g", "h", "j", "k", "l"],
        ["z", "x", "c", "v", "b", "n", "m"],
    ]

    var body: some View {
        VStack(spacing: keySpacing) {
            letterRow(0)
            letterRow(1).padding(.horizontal, 14)
            HStack(spacing: keySpacing) {
                FunctionKey(label: model.isShifted ? "⇪" : "⇧", height: keyHeight) { model.toggleShift() }
                    .frame(width: 44)
                letterRow(2)
                FunctionKey(label: "⌫", height: keyHeight) { model.backspace() }
                    .frame(width: 44)
            }
            HStack(spacing: keySpacing) {
                FunctionKey(label: "あ", height: keyHeight) { model.switchLayout() }
                    .frame(width: 54)
                FunctionKey(label: "🌐", height: keyHeight) { model.nextKeyboard() }
                    .frame(width: 54)
                FunctionKey(label: ",", height: keyHeight) { model.typeLetter(",") }
                    .frame(width: 40)
                FunctionKey(label: "空白", height: keyHeight) { model.space() }
                FunctionKey(label: ".", height: keyHeight) { model.typeLetter(".") }
                    .frame(width: 40)
                FunctionKey(label: "改行", height: keyHeight) { model.enter() }
                    .frame(width: 64)
            }
        }
    }

    private func letterRow(_ index: Int) -> some View {
        HStack(spacing: keySpacing) {
            ForEach(letterRows[index], id: \.self) { letter in
                FunctionKey(label: model.isShifted ? letter.uppercased() : letter, height: keyHeight) {
                    model.typeLetter(letter)
                }
            }
        }
    }
}

/// かなキー 1 つの定義
private struct KeySpec: Identifiable {
    let id: String
    let label: String
    let hint: String?
    let action: (KeyboardModel, KeyGesture) -> Void

    static func kana(_ label: String, _ key: KanaKey) -> KeySpec {
        KeySpec(id: label, label: label, hint: nil) { model, gesture in model.key(key, gesture) }
    }

    static var variant: KeySpec {
        KeySpec(id: "variant", label: "゛゜小", hint: nil) { model, gesture in
            // フリックでも、タップと同じ濁点・小書きの循環にする
            _ = gesture
            model.variant()
        }
    }
}

/// フリックできるキー。タップは `.tap`、一定以上動かして離すと動かした向きのフリック
private struct FlickKey: View {
    let label: String
    let hint: String?
    let height: CGFloat
    let onGesture: (KeyGesture) -> Void

    @GestureState private var isPressed = false

    var body: some View {
        Text(label)
            .font(.system(size: label.count > 2 ? 16 : 22))
            .foregroundColor(Color(UIColor.label))
            .frame(maxWidth: .infinity, minHeight: height, maxHeight: height)
            .background(
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color(UIColor.systemBackground).opacity(isPressed ? 0.6 : 1))
            )
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .updating($isPressed) { _, state, _ in state = true }
                    .onEnded { value in onGesture(Self.gesture(for: value.translation)) }
            )
    }

    /// 移動量から操作を決める（小さな移動はタップ）
    static func gesture(for translation: CGSize) -> KeyGesture {
        let threshold: CGFloat = 14
        if abs(translation.width) < threshold && abs(translation.height) < threshold {
            return .tap
        }
        if abs(translation.width) > abs(translation.height) {
            return translation.width < 0 ? .flickLeft : .flickRight
        }
        return translation.height < 0 ? .flickUp : .flickDown
    }
}

/// 機能キー（タップのみ）
private struct FunctionKey: View {
    let label: String
    let height: CGFloat
    let action: () -> Void

    @GestureState private var isPressed = false

    var body: some View {
        Text(label)
            .font(.system(size: 17))
            .foregroundColor(Color(UIColor.label))
            .frame(maxWidth: .infinity, minHeight: height, maxHeight: height)
            .background(
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color(UIColor.systemGray4).opacity(isPressed ? 0.6 : 1))
            )
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .updating($isPressed) { _, state, _ in state = true }
                    .onEnded { value in
                        // 指を大きく動かして離した場合は、押した扱いにしない
                        if abs(value.translation.width) < 20 && abs(value.translation.height) < 20 {
                            action()
                        }
                    }
            )
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
                if let warning = model.warning, !model.hasPreedit, model.candidates.isEmpty {
                    Text(warning)
                        .font(.system(size: 11))
                        .foregroundColor(.orange)
                        .lineLimit(2)
                }
                if model.hasPreedit {
                    Text(model.preedit)
                        .font(.system(size: 14))
                        .foregroundColor(.secondary)
                        .underline()
                        .lineLimit(1)
                }
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(Array(model.candidates.enumerated()), id: \.offset) { index, surface in
                            Button(action: { model.selectCandidate(index) }) {
                                Text(surface)
                                    .font(.system(size: 17))
                                    .foregroundColor(Color(UIColor.label))
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 6)
                                    .background(
                                        RoundedRectangle(cornerRadius: 6)
                                            .fill(index == model.selectedCandidate
                                                  ? Color.accentColor.opacity(0.25)
                                                  : Color(UIColor.systemBackground))
                                    )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            Button("検証", action: onShowProbes)
                .font(.system(size: 11))
                .foregroundColor(.secondary)
        }
        .padding(.horizontal, 8)
        .frame(height: 44)
        .background(Color(UIColor.systemGray5))
    }
}
