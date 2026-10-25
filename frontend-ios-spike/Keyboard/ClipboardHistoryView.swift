import SwiftUI

/// クリップボード履歴のページ（キー領域を置き換えて出す。`ExpandedCandidates` と同じ位置・高さ）。
/// 上部に「戻る」「履歴」「全削除（2 度押し）」、本文は 2 列のカードのギャラリー（縦スクロール）。
/// 見た目はシステムの配色（レイアウトの `look` には接続しない）。内容はログに出さない。
struct ClipboardHistoryPage: View {
    @ObservedObject var model: KeyboardModel
    /// キー領域の高さ
    let height: CGFloat
    /// カード間の余白（キー間の余白に合わせる）
    let spacing: CGFloat
    let accent: Color

    /// 「全削除」を 1 度押した状態（もう一度押すと削除。数秒で戻る）
    @State private var armedClear = false

    private let headerHeight: CGFloat = 36

    var body: some View {
        VStack(spacing: 4) {
            header
            content
        }
        .frame(height: height)
    }

    // MARK: ヘッダ

    private var header: some View {
        HStack(spacing: 8) {
            Button(action: { model.closeClipboardHistory() }) {
                Label("戻る", systemImage: "chevron.left")
                    .font(.system(size: 15))
                    .foregroundColor(accent)
                    .padding(.horizontal, 8)
                    .frame(height: headerHeight)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("戻る")

            Spacer(minLength: 0)
            Text("履歴")
                .font(.system(size: 15, weight: .semibold))
                .foregroundColor(Color(UIColor.label))
            Spacer(minLength: 0)

            // 履歴があるときだけ。1 度目で確認表示に変わり、もう一度押すと削除する（誤操作を避ける）
            if model.clipboardPageState == .items {
                Button(action: clearTapped) {
                    Text(armedClear ? "もう一度で全削除" : "全削除")
                        .font(.system(size: 14))
                        .foregroundColor(.red)
                        .padding(.horizontal, 8)
                        .frame(height: headerHeight)
                        .background(
                            RoundedRectangle(cornerRadius: 6)
                                .fill(Color.red.opacity(armedClear ? 0.15 : 0))
                        )
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(armedClear ? "もう一度押すと履歴をすべて削除" : "履歴をすべて削除")
            }
        }
        .padding(.horizontal, 4)
        .frame(height: headerHeight)
        // 数秒で確認表示を戻す（id が変わる = 状態が変わるたびに作り直し）
        .task(id: armedClear) {
            guard armedClear else { return }
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            armedClear = false
        }
    }

    private func clearTapped() {
        if armedClear {
            armedClear = false
            model.clearClipboardHistory()
        } else {
            armedClear = true
        }
    }

    // MARK: 本文

    @ViewBuilder
    private var content: some View {
        if let message = model.clipboardPageState.message {
            Text(message)
                .font(.system(size: 14))
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 16)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            gallery
        }
    }

    private var gallery: some View {
        let columns = [GridItem(.flexible(), spacing: spacing), GridItem(.flexible(), spacing: spacing)]
        let previewChars = model.ios.clipboardHistory.previewChars
        // 相対時刻はページを開いたときの時刻で出す（開いている間に細かく更新しない）
        let now = Date()
        return ScrollView(.vertical, showsIndicators: true) {
            LazyVGrid(columns: columns, spacing: spacing) {
                ForEach(model.clipboardItems) { item in
                    ClipboardCard(item: item, previewChars: previewChars, now: now, accent: accent)
                        .onTapGesture { model.insertClipboardItem(item) }
                        .contextMenu {
                            Button(item.pinned ? "ピン留めを外す" : "ピン留め") { model.toggleClipboardPin(item) }
                            Button("削除", role: .destructive) { model.deleteClipboardItem(item) }
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityAddTraits(.isButton)
                }
            }
            .padding(.bottom, 4)
        }
    }
}

/// 履歴のカード 1 枚: 本文（最大 4 行・設定の文字数で省略）、下に種類のアイコンと相対時刻、ピン留めのマーク
private struct ClipboardCard: View {
    let item: ClipboardItem
    let previewChars: Int
    let now: Date
    let accent: Color

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(ClipboardHistoryLogic.preview(item.text, limit: previewChars))
                .font(.system(size: 14))
                .foregroundColor(Color(UIColor.label))
                .lineLimit(4)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            Spacer(minLength: 0)
            HStack(spacing: 4) {
                Image(systemName: item.kind.systemImage)
                Text(ClipboardHistoryLogic.relativeTime(from: item.copiedAt, now: now))
                Spacer(minLength: 0)
                if item.pinned {
                    Image(systemName: "pin.fill").foregroundColor(accent)
                }
            }
            .font(.system(size: 11))
            .foregroundColor(.secondary)
        }
        .padding(8)
        .frame(maxWidth: .infinity, minHeight: 76, alignment: .topLeading)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color(UIColor.secondarySystemBackground)))
        // 塗りだけに頼らず、タップの範囲を明示する（カード全体が確実に反応するように）
        .contentShape(RoundedRectangle(cornerRadius: 8))
    }
}
