import UIKit

/// キーボード上部のバー。候補（横スクロール）・未確定文字列・セグメント列と、診断ボタンを表示する。
/// - 変換中（候補表示中）: 候補を並べ、タップで選択する
/// - 変換中（候補が非表示）: セグメント列を並べ、注目文節を強調する
/// - 未変換: 読みを 1 つのチップで表示し、タップでそのまま確定する
/// - 変換中は右端に 文節移動（◀ ▶）と区切りの伸縮（縮 伸）のボタンを出す
final class CandidateBarView: UIView {
    var onSelectCandidate: ((Int) -> Void)?
    var onInfoTap: (() -> Void)?
    var onInfoLongPress: (() -> Void)?
    /// -1: 左、+1: 右
    var onMoveFocus: ((Int) -> Void)?
    /// -1: 左へ縮める、+1: 右へ伸ばす
    var onResize: ((Int) -> Void)?

    private let infoButton = UIButton(type: .system)
    private let scrollView = UIScrollView()
    private let messageLabel = UILabel()
    private var chips: [UIButton] = []
    private var controlButtons: [UIButton] = []
    private var showControls = false
    private var selectedChip: Int?

    private let infoWidth: CGFloat = 36
    private let controlWidth: CGFloat = 36
    private let chipFont = UIFont.systemFont(ofSize: 18)

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor.systemGray5

        infoButton.setTitle("ⓘ", for: .normal)
        infoButton.addTarget(self, action: #selector(infoTapped), for: .touchUpInside)
        let longPress = UILongPressGestureRecognizer(target: self, action: #selector(infoLongPressed(_:)))
        infoButton.addGestureRecognizer(longPress)
        addSubview(infoButton)

        scrollView.showsHorizontalScrollIndicator = false
        addSubview(scrollView)

        messageLabel.font = UIFont.systemFont(ofSize: 14)
        messageLabel.textColor = UIColor.secondaryLabel
        messageLabel.isHidden = true
        addSubview(messageLabel)

        let titles = ["◀", "▶", "縮", "伸"]
        for (i, title) in titles.enumerated() {
            let button = UIButton(type: .system)
            button.setTitle(title, for: .normal)
            button.tag = i
            button.addTarget(self, action: #selector(controlTapped(_:)), for: .touchUpInside)
            button.isHidden = true
            addSubview(button)
            controlButtons.append(button)
        }
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) は使いません")
    }

    // MARK: - 状態の反映

    func apply(_ state: BarState) {
        for chip in chips { chip.removeFromSuperview() }
        chips = []
        selectedChip = nil

        if !state.candidates.isEmpty {
            for (i, candidate) in state.candidates.enumerated() {
                let title = candidate.corrected ? "↺ " + candidate.surface : candidate.surface
                addChip(title: title, tag: i, highlighted: i == state.selectedCandidate, tappable: true)
                if i == state.selectedCandidate { selectedChip = i }
            }
        } else if state.isConverting {
            for (i, segment) in state.segments.enumerated() {
                addChip(title: segment.surface, tag: i, highlighted: segment.focused, tappable: false)
                if segment.focused { selectedChip = i }
            }
        } else if !state.preedit.isEmpty {
            addChip(title: state.preedit, tag: 0, highlighted: false, tappable: true)
        }

        if let message = state.message {
            messageLabel.text = message
            messageLabel.isHidden = false
        } else {
            messageLabel.isHidden = true
        }
        showControls = state.isConverting
        for button in controlButtons { button.isHidden = !showControls }
        setNeedsLayout()
    }

    private func addChip(title: String, tag: Int, highlighted: Bool, tappable: Bool) {
        let chip = UIButton(type: .system)
        chip.setTitle(title, for: .normal)
        chip.titleLabel?.font = chipFont
        chip.tag = tag
        chip.isEnabled = tappable
        chip.layer.cornerRadius = 6
        if highlighted {
            chip.backgroundColor = UIColor.systemBlue
            chip.setTitleColor(UIColor.white, for: .normal)
            chip.setTitleColor(UIColor.white, for: .disabled)
        } else {
            chip.backgroundColor = UIColor.systemBackground
            chip.setTitleColor(UIColor.label, for: .normal)
            chip.setTitleColor(UIColor.label, for: .disabled)
        }
        chip.addTarget(self, action: #selector(chipTapped(_:)), for: .touchUpInside)
        scrollView.addSubview(chip)
        chips.append(chip)
    }

    // MARK: - レイアウト

    override func layoutSubviews() {
        super.layoutSubviews()
        let controlsTotal = showControls ? controlWidth * CGFloat(controlButtons.count) : 0
        infoButton.frame = CGRect(x: 0, y: 0, width: infoWidth, height: bounds.height)
        let scrollWidth = max(0, bounds.width - infoWidth - controlsTotal)
        scrollView.frame = CGRect(x: infoWidth, y: 0, width: scrollWidth, height: bounds.height)
        messageLabel.frame = CGRect(x: infoWidth + 4, y: 0, width: scrollWidth, height: bounds.height)
        for (i, button) in controlButtons.enumerated() {
            button.frame = CGRect(
                x: bounds.width - controlsTotal + CGFloat(i) * controlWidth,
                y: 0,
                width: controlWidth,
                height: bounds.height
            )
        }

        var x: CGFloat = 4
        let chipHeight = max(0, bounds.height - 8)
        for chip in chips {
            let title = chip.title(for: .normal) ?? ""
            let textWidth = (title as NSString).size(withAttributes: [NSAttributedString.Key.font: chipFont]).width
            let width = ceil(textWidth) + 24
            chip.frame = CGRect(x: x, y: 4, width: width, height: chipHeight)
            x += width + 4
        }
        scrollView.contentSize = CGSize(width: x, height: bounds.height)
        if let selected = selectedChip, selected < chips.count {
            scrollView.scrollRectToVisible(chips[selected].frame, animated: false)
        }
    }

    // MARK: - アクション

    @objc private func chipTapped(_ sender: UIButton) {
        onSelectCandidate?(sender.tag)
    }

    @objc private func infoTapped() {
        onInfoTap?()
    }

    @objc private func infoLongPressed(_ gesture: UILongPressGestureRecognizer) {
        if gesture.state == .began { onInfoLongPress?() }
    }

    @objc private func controlTapped(_ sender: UIButton) {
        switch sender.tag {
        case 0: onMoveFocus?(-1)
        case 1: onMoveFocus?(1)
        case 2: onResize?(-1)
        default: onResize?(1)
        }
    }
}
