#if os(macOS)
import AppKit

/// 候補一覧を描く軽量ビュー（IMKCandidates は使わない。設計書 §5.2）
final class CandidateListView: NSView {
    var page: CandidatePage? {
        didSet { recomputeLayout() }
    }
    var fontSize: CGFloat = 15 {
        didSet { recomputeLayout() }
    }

    private(set) var preferredSize = NSSize(width: 0, height: 0)

    private let rowHeight: CGFloat = 24
    private let padding: CGFloat = 8
    private let gap: CGFloat = 12
    private var numberWidth: CGFloat = 0
    private var surfaceWidth: CGFloat = 0
    private var labelWidth: CGFloat = 0

    override var isFlipped: Bool { true }

    private var surfaceFont: NSFont { NSFont.systemFont(ofSize: fontSize) }
    private var labelFont: NSFont { NSFont.systemFont(ofSize: max(fontSize - 4, 9)) }
    private var numberFont: NSFont {
        NSFont.monospacedDigitSystemFont(ofSize: max(fontSize - 2, 9), weight: .regular)
    }
    private var footerHeight: CGFloat { (page?.pageCount ?? 0) > 1 ? 18 : 0 }

    private func width(of text: String, font: NSFont) -> CGFloat {
        (text as NSString).size(withAttributes: [.font: font]).width
    }

    private func recomputeLayout() {
        guard let page else {
            preferredSize = .zero
            return
        }
        numberWidth = ceil(width(of: "0", font: numberFont)) + 6
        surfaceWidth = ceil(page.rows.map { width(of: $0.surface, font: surfaceFont) }.max() ?? 0)
        labelWidth = ceil(page.rows.map { width(of: $0.label, font: labelFont) }.max() ?? 0)
        var w = padding + numberWidth + surfaceWidth + padding
        if labelWidth > 0 { w += gap + labelWidth }
        preferredSize = NSSize(
            width: max(w, 80), height: CGFloat(page.rows.count) * rowHeight + footerHeight + 4)
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let page else { return }
        NSColor.windowBackgroundColor.setFill()
        NSBezierPath.fill(bounds)
        NSColor.separatorColor.setStroke()
        NSBezierPath(rect: bounds.insetBy(dx: 0.5, dy: 0.5)).stroke()

        var y: CGFloat = 2
        for row in page.rows {
            let rowRect = NSRect(x: 2, y: y, width: bounds.width - 4, height: rowHeight)
            let textColor: NSColor
            let subColor: NSColor
            if row.isSelected {
                NSColor.selectedContentBackgroundColor.setFill()
                NSBezierPath.fill(rowRect)
                textColor = .alternateSelectedControlTextColor
                subColor = .alternateSelectedControlTextColor
            } else {
                textColor = .labelColor
                subColor = .secondaryLabelColor
            }
            let textY = { (font: NSFont) -> CGFloat in y + (self.rowHeight - font.boundingRectForFont.height) / 2 }
            ("\(row.number)" as NSString).draw(
                at: NSPoint(x: padding, y: textY(numberFont)),
                withAttributes: [.font: numberFont, .foregroundColor: subColor])
            (row.surface as NSString).draw(
                at: NSPoint(x: padding + numberWidth, y: textY(surfaceFont)),
                withAttributes: [.font: surfaceFont, .foregroundColor: textColor])
            if labelWidth > 0 {
                (row.label as NSString).draw(
                    at: NSPoint(
                        x: padding + numberWidth + surfaceWidth + gap, y: textY(labelFont)),
                    withAttributes: [.font: labelFont, .foregroundColor: subColor])
            }
            y += rowHeight
        }
        if footerHeight > 0 {
            let text = "\(page.pageIndex + 1)/\(page.pageCount)" as NSString
            let attrs: [NSAttributedString.Key: Any] = [
                .font: labelFont, .foregroundColor: NSColor.secondaryLabelColor,
            ]
            let w = text.size(withAttributes: attrs).width
            text.draw(at: NSPoint(x: bounds.width - w - padding, y: y + 1), withAttributes: attrs)
        }
    }
}

/// 非アクティブな候補ウィンドウ。フォーカスを奪わず、カーソル位置の近くに出す
final class CandidatePanel: NSPanel {
    private let listView = CandidateListView(frame: .zero)

    init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 100, height: 100),
            styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        isFloatingPanel = true
        level = .popUpMenu
        hidesOnDeactivate = false
        hasShadow = true
        isOpaque = false
        backgroundColor = .clear
        ignoresMouseEvents = true  // v1 はマウス操作なし（設計書 §2.4）
        contentView = listView
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    var fontSize: CGFloat {
        get { listView.fontSize }
        set { listView.fontSize = newValue }
    }

    /// `caret` はスクリーン座標のカーソル行矩形。画面下端に収まらなければ上側に出す
    func show(page: CandidatePage, near caret: NSRect) {
        listView.page = page
        let size = listView.preferredSize
        listView.frame = NSRect(origin: .zero, size: size)
        setContentSize(size)

        var anchor = caret
        if caret == .zero {
            // カーソル位置が取れないクライアントではマウス位置で代用する
            let p = NSEvent.mouseLocation
            anchor = NSRect(x: p.x, y: p.y, width: 0, height: 0)
        }
        var origin = NSPoint(x: anchor.minX, y: anchor.minY - size.height - 2)
        let screen =
            NSScreen.screens.first(where: { $0.frame.contains(anchor.origin) }) ?? NSScreen.main
        if let visible = screen?.visibleFrame {
            if origin.y < visible.minY { origin.y = anchor.maxY + 2 }
            origin.x = max(visible.minX, min(origin.x, visible.maxX - size.width))
        }
        setFrameOrigin(origin)
        orderFrontRegardless()
    }

    func hide() {
        orderOut(nil)
    }
}
#endif
