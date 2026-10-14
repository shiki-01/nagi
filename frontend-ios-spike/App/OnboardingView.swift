import SwiftUI

// 初回案内（オンボーディング）。キーボードの追加 → フルアクセス → 辞書・モデル → 入力テストの順に案内する。
// 初回起動で 1 度だけ自動表示し（UserDefaults `nagi.onboarding.completed`）、ホームタブからいつでも再表示できる。

/// キーボード拡張が App Group の UserDefaults に書くマーカーを読む（ホスト側は読むだけ）。
///
/// 書き込み側（Keyboard/ の `viewWillAppear` など）の仕様:
/// - `keyboardLastSeen`: 拡張が表示されたときの時刻（`Date().timeIntervalSince1970`、Double）
/// - `fullAccessLastSeen`: `hasFullAccess == true` を確認した時刻（同上）。フルアクセスなしのときは書かない
/// 両方を同じ表示のタイミングで書くので、フルアクセスがオンなら 2 つの時刻はほぼ一致する。
/// 後からオフにされた場合は `fullAccessLastSeen` が古いままになるため、`keyboardLastSeen` との差で判定する。
enum NagiSetupMarker {
    static let keyboardLastSeenKey = "keyboardLastSeen"
    static let fullAccessLastSeenKey = "fullAccessLastSeen"
    /// 2 つの時刻が同じ表示で書かれたとみなす許容差（秒）
    private static let sameVisitTolerance: TimeInterval = 5

    struct Snapshot {
        /// 拡張が表示されたことのある時刻（nil = 未検出）
        var keyboardLastSeen: Date?
        /// フルアクセスありで表示された最後の時刻
        var fullAccessLastSeen: Date?
        /// 設定アプリの有効キーボード一覧（`AppleKeyboards`）に Nagi がある。非公開キーのため補助的な判定
        var listedInSystemKeyboards: Bool

        var keyboardDetected: Bool { keyboardLastSeen != nil || listedInSystemKeyboards }

        /// 直近の表示でフルアクセスがオンだったか。マーカーが無ければ nil（不明）
        var fullAccessOn: Bool? {
            guard let keyboard = keyboardLastSeen else { return nil }
            guard let full = fullAccessLastSeen else { return false }
            return abs(keyboard.timeIntervalSince(full)) < NagiSetupMarker.sameVisitTolerance
        }
    }

    static func read() -> Snapshot {
        let defaults = UserDefaults(suiteName: SharedStore.shared.appGroupId)
        let keyboard = (defaults?.object(forKey: keyboardLastSeenKey) as? Double).map { Date(timeIntervalSince1970: $0) }
        let full = (defaults?.object(forKey: fullAccessLastSeenKey) as? Double).map { Date(timeIntervalSince1970: $0) }
        let listed = (UserDefaults.standard.array(forKey: "AppleKeyboards") as? [String] ?? [])
            .contains { $0.hasPrefix("com.shiki01.nagi.keyboard") }
        return Snapshot(keyboardLastSeen: keyboard, fullAccessLastSeen: full, listedInSystemKeyboards: listed)
    }
}

/// 設定アプリのこのアプリのページを開く（キーボードの追加・フルアクセスは 設定 > 一般 > キーボード から）
enum NagiOpenSettings {
    static func open() {
        guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
        UIApplication.shared.open(url)
    }
}

/// ホームタブの「セットアップ」セクション。状態の要約と、案内の再表示ボタン
struct NagiSetupStatusSection: View {
    let onShowGuide: () -> Void

    @State private var marker = NagiSetupMarker.read()
    @State private var data = NagiDataSnapshot.load()

    var body: some View {
        Section("セットアップ") {
            NagiSetupStatusRow(title: "キーボードの追加", state: keyboardState)
            NagiSetupStatusRow(title: "フルアクセス", state: fullAccessState)
            NagiSetupStatusRow(title: "辞書", state: data.hasDict ? .ok : .ng)
            NagiSetupStatusRow(title: "モデル", state: data.hasModel ? .ok : .ng)

            Button("セットアップの案内を表示") { onShowGuide() }
                .buttonStyle(.borderless)
            Button("設定アプリを開く") { NagiOpenSettings.open() }
                .buttonStyle(.borderless)
        }
        .onAppear { refresh() }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.didBecomeActiveNotification)) { _ in
            refresh()
        }
    }

    private var keyboardState: NagiSetupStatusRow.Status {
        marker.keyboardDetected ? .ok : .unknown
    }

    private var fullAccessState: NagiSetupStatusRow.Status {
        switch marker.fullAccessOn {
        case .some(true): return .ok
        case .some(false): return .ng
        case .none: return .unknown
        }
    }

    private func refresh() {
        marker = NagiSetupMarker.read()
        data = NagiDataSnapshot.load()
    }
}

struct NagiSetupStatusRow: View {
    enum Status {
        case ok
        case ng
        /// 検出できていない（未実施か、検出手段がない）
        case unknown
    }

    let title: String
    let state: Status

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            switch state {
            case .ok:
                Label("OK", systemImage: "checkmark.circle.fill").foregroundColor(.green)
            case .ng:
                Label("未設定", systemImage: "xmark.circle.fill").foregroundColor(.red)
            case .unknown:
                Label("未検出", systemImage: "questionmark.circle").foregroundColor(.secondary)
            }
        }
        .font(.callout)
    }
}

/// 初回案内の本体（シートで表示する）
struct NagiOnboardingView: View {
    /// 閉じる（初回表示済みとして記録するのは呼び出し側）
    let onClose: () -> Void
    /// 手順 3: データタブへ移動
    let onOpenData: () -> Void
    /// 手順 4: 入力テストタブへ移動
    let onOpenInputTest: () -> Void

    @State private var marker = NagiSetupMarker.read()
    @State private var data = NagiDataSnapshot.load()

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text("Nagi は端末の中だけで動く日本語キーボードです。使い始めるまでの手順です。")
                        .font(.callout)
                }

                Section("手順 1  キーボードを追加する") {
                    Text("設定 > 一般 > キーボード > キーボード > 新しいキーボードを追加 > Nagi")
                        .font(.callout)
                    NagiSetupStatusRow(title: "追加の確認", state: marker.keyboardDetected ? .ok : .unknown)
                    Button("設定アプリを開く") { NagiOpenSettings.open() }
                        .buttonStyle(.borderless)
                    Text("追加したら、どこかの入力欄で 🌐 キーを押して Nagi に切り替えると、ここの確認が「OK」になります。")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }

                Section("手順 2  フルアクセスを許可する") {
                    Text("追加した Nagi を選び、「フルアクセスを許可」をオンにします。")
                        .font(.callout)
                    NagiSetupStatusRow(title: "フルアクセス", state: fullAccessState)
                    Button("設定アプリを開く") { NagiOpenSettings.open() }
                        .buttonStyle(.borderless)
                    VStack(alignment: .leading, spacing: 6) {
                        Text("なぜ必要か").font(.caption).fontWeight(.semibold)
                        Text("・辞書とモデルの共有領域（このアプリが保存した場所）をキーボードが読むためです。")
                        Text("・入力した内容は端末の外に送信しません。ネットワーク通信もしません。")
                        Text("・許可しなくても、ひらがな・カタカナ変換は使えます（漢字変換には許可が必要です）。")
                    }
                    .font(.caption)
                    .foregroundColor(.secondary)
                }

                Section("手順 3  辞書とモデルを確認する") {
                    NagiSetupStatusRow(title: "辞書", state: data.hasDict ? .ok : .ng)
                    NagiSetupStatusRow(title: "モデル", state: data.hasModel ? .ok : .ng)
                    Text("足りないものは、データ画面から取り込み方法を確認できます。")
                        .font(.caption)
                        .foregroundColor(.secondary)
                    Button("データ画面を開く") {
                        onOpenData()
                        onClose()
                    }
                    .buttonStyle(.borderless)
                }

                Section("手順 4  入力テストで試す") {
                    Text("入力テスト画面でキーボードを呼び出し、変換できるか試します。")
                        .font(.callout)
                    Button("入力テストを開く") {
                        onOpenInputTest()
                        onClose()
                    }
                    .buttonStyle(.borderless)
                }

                Section {
                    Button("閉じる") { onClose() }
                        .frame(maxWidth: .infinity)
                    Text("この案内はホームタブの「セットアップの案内を表示」からいつでも開けます。")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
            }
            .navigationTitle("はじめに")
            .navigationBarTitleDisplayMode(.inline)
            .onAppear { refresh() }
            .onReceive(NotificationCenter.default.publisher(for: UIApplication.didBecomeActiveNotification)) { _ in
                refresh()
            }
        }
    }

    private var fullAccessState: NagiSetupStatusRow.Status {
        switch marker.fullAccessOn {
        case .some(true): return .ok
        case .some(false): return .ng
        case .none: return .unknown
        }
    }

    private func refresh() {
        marker = NagiSetupMarker.read()
        data = NagiDataSnapshot.load()
    }
}
