import SwiftUI
import UIKit

/// 候補バーに出す候補 1 つ
struct CandidateItem: Identifiable {
    let id: Int
    let surface: String
    /// 入力中（変換前）の候補で、先頭の何文字分の読みを置き換えるか。nil は読み全体
    let covers: Int?
    /// 曖昧入力（タイプミス補正）の候補の「もしかして: かんじ」表示。補正候補でなければ nil
    var hint: String? = nil
}

/// 英字のシフトの状態
enum ShiftState {
    /// 小文字
    case off
    /// 次の 1 文字だけ大文字
    case once
    /// Caps Lock（解除するまで大文字）
    case caps
}

/// キーボードの状態と操作。ロジック（文字の決定・変換・確定）はコアが持ち、ここはキー操作をコアへ送り、
/// 返ってきた描画差分（確定・プリエディット・候補）をホストと画面に反映するだけにする
final class KeyboardModel: ObservableObject {
    /// プロセスで 1 つのモデル（とコア）。同じプロセス内で `UIInputViewController` が作り直されても、
    /// コア（LLM 込み）を新しく作らずにこれを引き継ぐ（旧インスタンスと二重に持たない）。メインスレッドからのみ読み書きする
    nonisolated(unsafe) static var processShared: KeyboardModel?

    /// 変換中の文字列（候補バーの左に出す）
    @Published private(set) var preedit = ""
    /// 変換中の文節 1 つ（候補バーで注目文節を区別して出す）
    struct PreeditPart: Identifiable {
        let id: Int
        let text: String
        let focused: Bool
    }
    /// 変換中（注目文節あり）の文節列。変換中でなければ空
    @Published private(set) var preeditParts: [PreeditPart] = []
    /// 候補
    @Published private(set) var candidates: [CandidateItem] = []
    /// 候補が確定直後の次語予測か（候補バーで薄く区別し、展開パネルは開かない）
    @Published private(set) var isNextWordCandidates = false
    /// 候補の一覧を広げているか（候補が無くなれば自動で閉じる）
    @Published var isExpanded = false
    /// 選択中の候補（なければ -1）
    @Published private(set) var selectedCandidate = -1
    /// 表示中のレイアウト（JSON から読んだもの。`Shared/KeyboardLayout.swift`）
    @Published private(set) var layoutDef: KeyboardLayoutDef {
        didSet {
            // 見た目の解決結果はレイアウトごと。入れ替わったら捨て、スペースのドラッグ移動の状態も解除する
            lookCache.reset()
            visual.setSpaceDragOrigin(nil)
        }
    }
    /// 見た目の解決結果のメモ化（`Shared/KeyLookState.swift`）
    let lookCache = LookCache()
    /// キーボード全体で共有する見た目の状態（スペースのドラッグ移動の起点など）
    let visual = KeyboardVisualState()
    /// キー間・行間・外周の余白。設定 `keyboard.key_spacing_pt` を、レイアウトの `metrics` が上書きする
    var metrics: ResolvedMetrics {
        ResolvedMetrics.make(layoutDef.metrics, settingSpacing: NagiKeyMetrics.keySpacing)
    }
    /// 日本語入力の流れか（かなから数字・記号へ切り替えて来た間は true。数字・記号をコアに通すかの判定に使う）
    private var japaneseContext = true
    /// 英字のシフト（1 回で次の 1 文字、2 回連続で Caps Lock）
    @Published private(set) var shiftState: ShiftState = .off
    /// 画面に出すメッセージ（辞書が無いなど）
    @Published private(set) var message: String?
    /// 動作はするが画面に出しておきたい注意（設定の保存先を作れないなど）
    @Published private(set) var warning: String?
    /// iOS 固有の設定（`ios-settings.json`。focusIn のたびに読み直す）。
    /// 描画が参照する寸法は `NagiKeyMetrics.current` にも同じ値を入れる
    @Published private(set) var ios = NagiIOSSettings()

    /// クリップボードに文字列があるか（候補バーの「貼り付け」ボタンの表示判定）。
    /// 判定は `hasStrings` だけで行い、中身は読まない（読むとペーストの許可確認が出るため、タップ時のみ読む）
    @Published private(set) var clipboardHasText = false
    /// 改行キーに出す文字（入力欄の `returnKeyType` から。既定の改行なら nil でレイアウトのラベルのまま）
    @Published private(set) var returnKeyLabel: String?
    private var pasteboardObserver: NSObjectProtocol?

    weak var host: KeyboardHost? {
        didSet { applier.host = host }
    }
    /// ホストの変換中の文字列と確定の手順（`HostCompositionApplier`）。表示中の変換中の文字列もここが持つ
    private let applier = HostCompositionApplier()

    /// コアのセッション。メモリ警告やコアの異常で作り直す・手放すことがあるので var。nil のときはキーを直接ホストへ入れる
    private var session: NagiSession?
    private let store = SharedStore.shared
    /// OS の電源・熱・メモリ逼迫の状態をコアへ渡す
    private let environment = NagiEnvironmentReporter()
    /// コアを作り直すときに LLM を読み込まない（メモリ警告でコアが LLM を解放しなかった場合の最後の手段。
    /// このプロセスの間は辞書のみで動く）
    private var dictionaryOnly = false
    /// メモリ警告でコアが LLM を解放した（逼迫が解けて再読み込みされるまで、辞書のみ表示の注意を出す）
    private var llmReleasedByMemory = false
    /// 起動時に空きメモリが足りず、LLM を読み込まずに辞書のみで始めた（空きができたらコアを作り直して読み込む。H-26）
    private var llmDeferred = false
    /// キーボードを閉じる（`viewWillDisappear`）ときに `unload_model` で LLM を手放した（再表示で `reload_model`）
    private var llmSuspended = false
    /// キーボードが表示中か（`focusIn` で true、`suspendLLM` で false）
    private var isVisible = false
    /// LLM の復帰を試みる予約が済んでいるか
    private var llmRestoreScheduled = false
    /// `reload_model` を最後に送った時刻（読み込み中に重ねて送らない）
    private var lastReloadRequest: Date?
    /// 辞書のみの状態になった時刻（表示中のみ。簡易モードの注意を出すまでの猶予の起点）
    private var dictionaryOnlySince: Date?
    private var noticeCheckScheduled = false
    /// 生成時の注意（設定の保存先を作れないなど）。メモリの注意は別に重ねて出す
    private var baseWarning: String?
    /// コアが続けて応答しなかった回数（応答すれば 0 に戻る）
    private var consecutiveFailures = 0
    /// コアを作り直した回数（上限を超えたら直接入力に倒す）
    private var coreRestarts = 0
    /// 同じキーの連続タップでトグル入力にするか（false ならフリック入力で、タップは常に先頭の文字）。
    /// 設定（`input.toggle_input`）で決まる
    private var toggleInput: Bool
    /// コアの `config.toml`。focusIn で更新時刻が変わっていたら `reload_config` を送る
    private let configURL: URL?
    private var lastConfigStamp: Date?
    /// 学習データの全削除の合図（ホストアプリが更新する）を最後に処理した値
    private var lastLearningClearSignal: String?
    /// 直前に反映した、かな・英字モードのレイアウトの割り当て（変わったら表示中のレイアウトを切り替える）
    private var appliedAssignment: (kana: String, alpha: String)
    private var lastRevision: UInt32 = 0
    /// 直近の変換中の文字列の文節数と注目文節（候補を選んだあと、その文節までを確定するかの判定に使う）
    private var segmentCount = 0
    private var focusIndex: Int?
    /// キー領域の直近の大きさ（view が報告する）と、コアへ最後に送ったレイアウト幾何（曖昧入力用。同じ内容は送り直さない）
    private var layoutGeometrySize: CGSize?
    private var lastSentGeometry: LayoutGeometry.Snapshot?
    /// 自分のホスト操作（挿入・削除・マーク）の直近の時刻。`textDidChange` が自分の操作の結果か外部の変更かの判定に使う
    private var lastOwnHostOperation = Date.distantPast
    /// 自分の操作の結果とみなす猶予（秒）
    private let ownOperationWindow: TimeInterval = 0.5
    /// ホストに表示している変換中の文字列とカーソル位置
    private var shownMarked: String { applier.shownMarked }
    private var shownCursor: Int { applier.shownCursor }
    /// 読み込み済みレイアウトの控え（id → 定義）。フォーカスが戻るたびに破棄して読み直す
    private var layoutCache: [String: KeyboardLayoutDef] = [:]
    /// 表示中のレイアウトが入力欄の種類による自動選択か（指定のない欄に移ったら、かなへ戻す）。
    /// ユーザーが自分で切り替えたら false にし、その選択を変えない
    private var autoSelectedLayout = false
    /// 直近に反映した入力欄の種類（keyboardType・returnKeyType の rawValue）。入力欄が替わっても
    /// documentIdentifier の変化や viewWillAppear が来ないことがあるため、textDidChange でも見比べて変われば反映し直す
    private var lastAppliedTraits: (keyboard: Int, returnKey: Int)?
    /// シフトを直前に押した時刻（2 回連続の判定）
    private var lastShiftTap: Date?
    /// 2 回連続とみなす間隔（秒）
    private let doubleTapInterval: TimeInterval = 0.5

    /// 直前のトグルキー（`toggle`）の押下。同じキーを時間内に続けて押すと、直前の文字を次の候補に置き換える。
    /// 他のキー操作・レイアウト切替・フォーカス変化で破棄する（= 確定）
    private struct ToggleState {
        let keyId: String
        let layoutId: String
        var index: Int
        /// 直前に入れた文字列（置き換えで消す文字数の元）
        var last: String
        /// 英字で大文字にして入れたか（置き換えでも同じ大文字・小文字にする）
        var upper: Bool
        var at: Date
    }
    private var toggleState: ToggleState?

    // 文脈（set_context）
    private var contextScheduled = false
    /// 最後にコアへ送った文脈（同じ内容を送り直さない）。nil は未送信
    private var lastSentContext: String?
    /// コアに渡す文脈の最大文字数
    private let contextLength = 20

    /// - toggleInput: 設定ファイルが読めないときの既定（通常は設定 `input.toggle_input` で上書きされる）
    /// - llmDeferred: 空きメモリが足りず LLM を読み込まずに作ったセッションか（`ProductSession.shouldDeferLLM`）
    init(outcome: ProductSession.Outcome, toggleInput: Bool = false, llmDeferred: Bool = false) {
        self.session = outcome.session
        self.llmDeferred = llmDeferred && outcome.session != nil
        self.message = outcome.message
        self.warning = outcome.warning
        self.baseWarning = outcome.warning
        self.toggleInput = toggleInput
        self.configURL = outcome.configURL
        self.lastConfigStamp = Self.modificationDate(outcome.configURL)
        self.lastLearningClearSignal = NagiLearningClearSignal.current()
        self.appliedAssignment = (kana: LayoutAssignment.id(for: .kana), alpha: LayoutAssignment.id(for: .alpha))
        self.layoutDef = LayoutLoader.loadAssigned(mode: .kana)
        attachSession()
        environment.onChange = { [weak self] in self?.reportEnvironment(reason: "os change") }
        environment.start()
        reloadIOSSettings()
        pasteboardObserver = NotificationCenter.default.addObserver(
            forName: UIPasteboard.changedNotification, object: nil, queue: .main
        ) { [weak self] _ in self?.refreshClipboardState() }
    }

    deinit {
        if let observer = pasteboardObserver { NotificationCenter.default.removeObserver(observer) }
    }

    /// 現在のセッションに通知の受け取りを付け、実行環境を報告する（生成時・作り直し時）。
    /// 12 キーの文字表は、まだ取得を試みていなければここで 1 回だけ取得する
    private func attachSession() {
        session?.onPreeditUpdate = { [weak self] update in self?.applyPreeditUpdate(update) }
        session?.onCandidatesUpdate = { [weak self] update in self?.applyCandidatesUpdate(update) }
        session?.startNotifications()
        reportEnvironment(reason: "attach")
        // 新しいセッションは幾何を知らないので、送り直す
        lastSentGeometry = nil
        sendLayoutGeometry()
        if let session = session, !KanaKeyTable.fetchAttempted {
            let table = session.getKeyTable()
            KanaKeyTable.install(table)
            DiagLog.log("get_key_table ok=\(table != nil)")
        }
    }

    /// 旧セッションの通知を止めて、コールバックを外す（手放したあとに古い通知が届いて状態を壊さないように）
    private func detach(_ old: NagiSession) {
        old.stopNotifications()
        old.onPreeditUpdate = nil
        old.onCandidatesUpdate = nil
    }

    /// 電源・熱・メモリ逼迫の状態をコアへ渡す（focusIn、OS の変化通知、メモリ警告、セッション生成時）
    private func reportEnvironment(reason: String) {
        guard let session = session else { return }
        let params = environment.snapshot()
        session.setEnvironment(params)
        DiagLog.log("set_environment (\(reason)) \(NagiEnvironmentReporter.describe(params))")
        // 逼迫が解けたら、コアの自動再読み込み（約 30 秒後）を待たず、空きメモリを確かめて読み込む
        if params.memory_pressure == .normal { scheduleLLMRestore(after: 0.5) }
    }

    // MARK: - 堅牢性（メモリ警告・コアの異常）

    private static let memoryNotice = "メモリ不足のため、変換を簡易モード（辞書のみ）に切り替えました"

    /// 辞書のみで動いている（LLM を読み込んでいない・手放している）か
    private var isDictionaryOnly: Bool { llmReleasedByMemory || llmDeferred || llmSuspended }

    /// 簡易モードの注意は、辞書のみの状態が `dictionaryOnlyNoticeDelay` 秒を超えて続いたときだけ出す
    /// （別アプリへの切替直後などの一時的な辞書のみでは出さない。閉じている間は数えない。H-26）
    private func updateWarning() {
        if isDictionaryOnly && isVisible {
            if dictionaryOnlySince == nil { dictionaryOnlySince = Date() }
        } else {
            dictionaryOnlySince = nil
        }
        let show = NagiModelRecommendation.shouldShowDictionaryOnlyNotice(since: dictionaryOnlySince, now: Date())
        warning = show ? Self.memoryNotice : baseWarning
        if dictionaryOnlySince != nil, !show, !noticeCheckScheduled {
            noticeCheckScheduled = true
            DispatchQueue.main.asyncAfter(deadline: .now() + NagiModelRecommendation.dictionaryOnlyNoticeDelay + 0.1) { [weak self] in
                guard let self = self else { return }
                self.noticeCheckScheduled = false
                self.updateWarning()
            }
        }
    }

    /// OS のメモリ警告（`didReceiveMemoryWarning`）。拡張はメモリ上限を超えると強制終了されるので、
    /// `set_environment` の `memory_pressure: critical` でコアに LLM を解放させ（入力中の状態は保たれる。H-16）、
    /// 画面側のキャッシュを捨てる。逼迫が解けると（一定時間後に normal を送る）コアが LLM を読み直す。
    /// 解放を `get_status` で確認できなかったときだけ、最後の手段として辞書のみのコアに作り直す
    func handleMemoryWarning() {
        let stats = MemoryProbe.getStats()
        DiagLog.log("memory warning footprint=\(Int(stats.physFootprintMB))MB available=\(Int(stats.availableMemoryMB))MB dictOnly=\(dictionaryOnly)")
        // 画面側のキャッシュ（読み込み済みレイアウト・展開中の候補一覧）を捨てる。
        // 入力中の候補はコアの状態と対応しているので消さない
        layoutCache.removeAll()
        isExpanded = false
        guard let session = session else { return }
        // LLM を使っていないなら解放するものがない（状態が取れなければ使っているとみなす）
        let usedLLM = session.getStatus()?.llm ?? true
        environment.noteMemoryWarning()
        reportEnvironment(reason: "memory warning")
        guard usedLLM else { return }
        if let status = session.getStatus(), !status.llm {
            llmReleasedByMemory = true
            updateWarning()
            DiagLog.log("llm released by core (get_status llm=false)")
        } else {
            DiagLog.log("llm still active after memory_pressure=critical: fallback to restart core")
            scheduleRestartCore(noLLM: true, reason: "memory warning (llm not released)")
        }
    }

    /// 辞書のみの状態から LLM を復帰させる必要があるか（永続的な辞書のみ `dictionaryOnly` は対象外）
    private var needsLLMRestore: Bool { !dictionaryOnly && isDictionaryOnly && isVisible && session != nil }

    /// LLM の復帰を試みる予約（空きメモリが足りるまで 2 秒おきに繰り返す）
    private func scheduleLLMRestore(after delay: TimeInterval = 2) {
        guard !llmRestoreScheduled, needsLLMRestore else { return }
        llmRestoreScheduled = true
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self = self else { return }
            self.llmRestoreScheduled = false
            self.tryRestoreLLM()
        }
    }

    /// LLM を復帰させる。読み込み済みなら注意を消す。メモリ逼迫が続いている間・空きメモリが足りない間は待つ。
    /// コアがモデルを持っている（解放しただけ）なら `reload_model`、起動時に読み込まなかった（モデルを持たない）なら
    /// コアを作り直す（入力中の文字列があれば確定してしまうので、入力が終わるまで待つ）
    private func tryRestoreLLM() {
        guard needsLLMRestore, let session = session else { return }
        defer { scheduleLLMRestore() }
        if !llmDeferred, let status = session.getStatus(), status.llm {
            llmReleasedByMemory = false
            llmSuspended = false
            updateWarning()
            DiagLog.log("llm restored")
            return
        }
        guard environment.snapshot().memory_pressure == .normal else { return }
        guard !ProductSession.shouldDeferLLM(store: store) else { return }
        if llmDeferred {
            guard !hasPreedit else { return }
            DiagLog.log("llm deferred: memory available, restart core")
            scheduleRestartCore(noLLM: false, reason: "llm deferred (memory available)")
        } else if lastReloadRequest.map({ Date().timeIntervalSince($0) > 15 }) ?? true {
            lastReloadRequest = Date()
            DiagLog.log("reload_model ok=\(session.reloadModel()) available=\(Int(MemoryProbe.getAvailableMemoryMB()))MB")
        }
    }

    /// キーボードを閉じるとき（`viewWillDisappear`）: LLM を速やかに手放す。別アプリへ切り替えた先のキーボードが
    /// LLM を読み込む間、旧インスタンスがメモリを持ち続けて一時的に倍になるのを避ける（H-26）。再表示（`focusIn`）で読み直す
    func suspendLLM() {
        isVisible = false
        updateWarning()
        guard let session = session, !dictionaryOnly, !llmDeferred else { return }
        llmSuspended = true
        DiagLog.log("unload_model (keyboard hidden) ok=\(session.unloadModel())")
    }

    /// 作り直しの予約が済んでいるか
    private var restartScheduled = false

    /// コアの作り直しを、呼び出し元（`send` や `handleMemoryWarning`）が戻ったあとに行う。
    /// 呼び出し元は旧セッションをローカル変数で持っているため、その場で作り直すと旧コア（LLM 入り）が
    /// 解放されないまま新しいコアを作ってしまい、メモリの上限を超えやすい
    private func scheduleRestartCore(noLLM: Bool, reason: String) {
        guard !restartScheduled else { return }
        restartScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self = self else { return }
            self.restartScheduled = false
            self.restartCore(noLLM: noLLM, reason: reason)
        }
    }

    /// コアを作り直す（呼び出し元が旧セッションを持っていないこと。持ちうる場合は `scheduleRestartCore`）。入力中の文字列はホストの入力欄に残した上で（失わない）、古いセッションを手放してから
    /// 新しく作る（二重に持ってメモリを増やさない）。作り直せなければ session は nil になり、
    /// キーは直接ホストへ入る
    private func restartCore(noLLM: Bool, reason: String) {
        DiagLog.log("restart core noLLM=\(noLLM) reason=\(reason)")
        if let old = session {
            // 応答するなら、focusOut で入力中の文字列を確定する（学習の書き出しもここで行われる）
            if let result = old.focusOut() { apply(result, origin: "response(focus_out before restart)") }
            detach(old)
        }
        keepPreeditInHost()
        session = nil
        if noLLM { dictionaryOnly = true }
        // 空きメモリが足りなければ辞書のみで始め、空きができてから読み込む（旧コアは上で手放し済み）
        let deferNow = !dictionaryOnly && ProductSession.shouldDeferLLM(store: store)
        let outcome = ProductSession.make(store: store, noLLM: dictionaryOnly || deferNow)
        session = outcome.session
        message = outcome.message
        baseWarning = outcome.warning
        // 新しいコアの revision は 0 から数え直す（旧コアの値を持ち越すと、新しい通知を古いと誤って捨てる）
        lastRevision = 0
        llmReleasedByMemory = dictionaryOnly && outcome.session != nil
        llmSuspended = false
        lastReloadRequest = nil
        llmDeferred = deferNow && outcome.session != nil
        updateWarning()
        consecutiveFailures = 0
        lastSentContext = nil
        attachSession()
        if session != nil { scheduleContextUpdate(delay: 0.1) }
    }

    /// 同じプロセスで新しいキーボードのインスタンス（VC）がこのモデルを引き継ぐ。
    /// 旧ホストに変換中の文字列を下線を外して残し（失わない）、コアの入力中の状態を片付ける。
    /// 文脈・幾何は新しいホストで送り直す。`host` の差し替えは呼び出し側が行う
    func handOverToNewHost() {
        DiagLog.log("model handed over to new keyboard instance hadPreedit=\(hasPreedit)")
        toggleState = nil
        keepPreeditInHost()
        if let session = session, let result = session.focusOut() {
            // 確定した文字列は旧ホストに入れない（上で残した）。revision だけ進める
            lastRevision = max(lastRevision, result.revision)
        }
        lastSentContext = nil
        lastSentGeometry = nil
        layoutGeometrySize = nil
    }

    /// 変換中の文字列をホストの入力欄に残す（下線を外して確定扱いにする）。表示も空にする
    private func keepPreeditInHost() {
        applier.keepInHost()
        preedit = ""
        preeditParts = []
        isConverting = false
        candidates = []
        isNextWordCandidates = false
        selectedCandidate = -1
        isExpanded = false
    }

    /// コアが応答しなかった。続けて失敗したらコアを作り直す（2 回まで）。それでも直らなければ直接入力に倒す
    private func noteCoreFailure(_ label: String) {
        consecutiveFailures += 1
        guard consecutiveFailures >= 3 else { return }
        consecutiveFailures = 0
        if coreRestarts < 2 {
            coreRestarts += 1
            scheduleRestartCore(noLLM: dictionaryOnly, reason: "no response x3 (\(label))")
        } else {
            DiagLog.log("core unavailable: direct input mode")
            if let old = session { detach(old) }
            keepPreeditInHost()
            session = nil
            message = "変換エンジンが応答しません。直接入力に切り替えました"
        }
    }

    private static func modificationDate(_ url: URL?) -> Date? {
        guard let url = url else { return nil }
        return (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
    }

    /// `ios-settings.json` を読み直して反映する（不正な値は既定に丸められている）
    func reloadIOSSettings() {
        let settings = NagiIOSSettingsStore.load()
        // 描画が参照する値を先に更新してから、@Published で再描画を起こす
        NagiKeyMetrics.current = settings
        toggleInput = settings.toggleInput
        applier.commitMethod = HostCompositionApplier.CommitMethod(rawValue: settings.commitMethod) ?? .insert
        if settings != ios { ios = settings }
        refreshClipboardState()
    }

    /// 「貼り付け」ボタンを出すか更新する。`UIPasteboard.hasStrings` は中身を読まず、貼り付け許可の確認も出さない
    func refreshClipboardState() {
        let has = ios.pasteButton && UIPasteboard.general.hasStrings
        if has != clipboardHasText { clipboardHasText = has }
    }

    /// 「貼り付け」ボタン: タップされたときだけクリップボードの文字列を読んで入れる。
    /// フルアクセスが無い・許可されなかった場合は nil になるので、何も入れずにボタンを隠す
    func pasteFromClipboard() {
        toggleState = nil
        // 入力中は出さないボタンだが、押す直前に入力が始まっていた場合に備えて確定してから入れる
        commitPreedit()
        if let text = UIPasteboard.general.string, !text.isEmpty {
            hostInsert(text)
        } else {
            DiagLog.log("paste: clipboard empty or denied")
        }
        refreshClipboardState()
    }

    var isAvailable: Bool { session != nil }

    /// 入力中（変換前のかな、または変換中）か
    var hasPreedit: Bool { !preedit.isEmpty }
    /// 変換中（注目文節がある）か。←/→ は、変換前はカーソル、変換中は注目文節の移動になる
    @Published private(set) var isConverting = false

    // MARK: - キー操作（レイアウトのキー）

    /// `text` キー: 文字を送る。かなモードはコアのローマ字変換に、それ以外はホストへそのまま入れる
    /// - touch: QWERTY の押下位置（キーの矩形内の相対位置）。曖昧入力の手がかりで、かなモードのローマ字入力にだけ使う
    func inputText(_ text: String, shift: String?, touch: TouchPoint? = nil) {
        toggleState = nil
        if layoutDef.mode == .kana {
            insertReading(text, touch: touch)
            return
        }
        if composesNumbers {
            insertReading(text)
            return
        }
        let output: String
        switch shiftState {
        case .off: output = text
        case .once, .caps: output = shift ?? text.uppercased()
        }
        if composesEnglish {
            insertReading(output)
        } else {
            hostInsert(output)
        }
        if shiftState == .once { shiftState = .off }
    }

    /// 数字・記号のレイアウトの文字を、入力中の文字列（コアの composing）に入れるか。
    /// 設定 `input.compose_numbers` がオン、パスワード欄でなく、日本語入力の流れ（かなから切り替えた・入力中）のときだけ
    private var composesNumbers: Bool {
        NumberCompose.shouldCompose(
            mode: layoutDef.mode, setting: ios.composeNumbers, isSecure: host?.hostIsSecure() ?? false,
            japaneseContext: japaneseContext, hasPreedit: hasPreedit
        )
    }

    /// 英字のレイアウトの文字を、入力中の文字列（コアの composing）に続けるか。
    /// 設定 `input.keep_preedit_on_english` がオンで、入力中の文字列があり、パスワード欄でないときだけ
    private var composesEnglish: Bool {
        EnglishCompose.shouldCompose(
            mode: layoutDef.mode, keepSetting: ios.keepPreeditOnEnglish, isSecure: host?.hostIsSecure() ?? false,
            hasPreedit: hasPreedit
        )
    }

    /// かなモードの文字入力: かな直接 / ASCII（ローマ字）はコアが解決する。コアが無い・消費しないときはそのまま入れる
    private func insertReading(_ text: String, touch: TouchPoint? = nil) {
        // 座標は英字 1 文字の押下にだけ意味がある。パスワード欄などでは送らない（ログにも出さない）
        let sentTouch = isSingleLetter(text) && !(host?.hostIsSecure() ?? false) ? touch : nil
        if !send("text \(logText(text))", { $0.insert(.text(text), touch: sentTouch) }) {
            hostInsert(text)
        }
    }

    private func isSingleLetter(_ text: String) -> Bool {
        let scalars = text.unicodeScalars
        guard scalars.count == 1, let scalar = scalars.first else { return false }
        return (0x41...0x5A).contains(scalar.value) || (0x61...0x7A).contains(scalar.value)
    }

    /// `toggle` キー（ガラケー式）: 押すたびに `cycle` の次の文字に切り替わる。
    /// 時間内（設定 `input.toggle_timeout_ms`、無ければ 1000ms）に同じキーをもう一度押すと、直前の文字を消して次を入れる。
    /// かなモードは、直前の文字をコアの backspace で消してから `insert_reading(text)` を送る
    /// （`ReadingInput.key12` のトグルはコアが 12 キーの行の表で解決するので、任意の cycle には使えない）。
    /// かな以外はホストの deleteBackward + insertText
    func toggle(_ key: LayoutKeyDef) {
        toggle(keyId: key.id, cycle: key.cycle ?? [])
    }

    /// トグル入力の本体（`toggle` キーと、`kana` 指定の無い flick キーのトグル入力で共有する）。
    /// `keyId` が直前と同じで時間内なら、直前の文字を `cycle` の次の文字に置き換える
    private func toggle(keyId: String, cycle: [String]) {
        guard !cycle.isEmpty else { return }
        let now = Date()
        var index = 0
        var replaceCount = 0
        var upper = false
        var continued = false
        if let state = toggleState, state.keyId == keyId, state.layoutId == layoutDef.id,
           ToggleCycle.isContinuation(
               cycleCount: cycle.count, elapsedMs: now.timeIntervalSince(state.at) * 1000, timeoutMs: ios.toggleTimeoutMs
           ) {
            index = ToggleCycle.nextIndex(previous: state.index, cycleCount: cycle.count)
            replaceCount = state.last.count
            upper = state.upper
            continued = true
        } else if layoutDef.mode != .kana {
            upper = shiftState != .off
            if shiftState == .once { shiftState = .off }
        }
        DiagLog.log("toggle key=\(keyId) index=\(index) continued=\(continued) hadState=\(toggleState != nil) timeout=\(ios.toggleTimeoutMs)")
        let base = cycle[index]
        let text = (layoutDef.mode != .kana && upper) ? base.uppercased() : base
        if layoutDef.mode == .kana || composesNumbers || composesEnglish {
            for _ in 0..<replaceCount {
                if !send("backspace(toggle)", { $0.perform(.backspace) }) { hostDeleteBackward(1) }
            }
            insertReading(text)
        } else {
            hostDeleteBackward(replaceCount)
            hostInsert(text)
        }
        toggleState = ToggleState(keyId: keyId, layoutId: layoutDef.id, index: index, last: text, upper: upper, at: now)
    }

    /// 入力中の文字列を確定する（なければ何もしない）
    private func commitPreedit() {
        if hasPreedit, let session = session, let result = session.perform(.commit) {
            apply(result, origin: "response(commit)")
        }
    }

    /// 片手モードの左右切替（キーボード上のボタン）。`side` は `left` / `right` / `off`。
    /// 書くのは本来ホストアプリだが、キーボード上のボタンなので拡張から App Group の設定ファイルへ書く
    func setOneHanded(_ side: String) {
        var latest = NagiIOSSettingsStore.load()
        latest.set(NagiIOSSettings.Key.keyboardOneHanded, .string(side))
        do {
            try NagiIOSSettingsStore.save(latest)
        } catch {
            DiagLog.log("setOneHanded save failed: \(error.localizedDescription)")
        }
        NagiKeyMetrics.current = latest
        ios = latest
    }

    /// `flick` キーの操作。`kana` 指定ならコアに任せ、文字を直接指定したキーはその文字を送る。
    /// 文字を直接指定したキーも、トグル入力（設定 `input.toggle_input`）ならタップで
    /// center → left → up → right → down の順に回す（2 文字以上あるときだけ。1 文字なら center を入れる）
    /// - detail: フリックの角度・距離（曖昧入力の手がかり。`kana` 指定のキーにだけ使う）
    func flick(_ key: LayoutKeyDef, _ gesture: KeyGesture, detail: FlickDetail? = nil) {
        if let kana = key.kana {
            toggleState = nil
            guard let entry = KanaKeyTable.entry(forName: kana) else { return }
            self.key(entry.key, gesture, flick: detail)
            return
        }
        if gesture == .tap && toggleInput {
            let cycle = ToggleCycle.flickCycle(
                center: key.center, left: key.left, up: key.up, right: key.right, down: key.down
            )
            if cycle.count > 1 {
                // toggleState は共通関数が続きかどうかの判定に使うので、ここでは破棄しない
                toggle(keyId: key.id, cycle: cycle)
                return
            }
        }
        toggleState = nil
        let char: FlickChar?
        switch gesture {
        case .tap, .toggle: char = key.center
        case .flickLeft: char = key.left
        case .flickUp: char = key.up
        case .flickRight: char = key.right
        case .flickDown: char = key.down
        }
        guard let text = char?.text else { return }
        inputText(text, shift: nil)
    }

    /// `action` として実行できる名前（`perform(action:)` と同じ）
    private static let knownActions: Set<String> = [
        "space", "enter", "backspace", "shift", "capslock", "convert", "moveLeft", "moveRight",
        "dakuten", "smallKana", "modeSwitch", "globe", "dismiss",
    ]

    /// `shortcut` キー: 入力中の文字列を確定してから、手順を順に実行する。
    /// `text` はコアに通さずそのままホストへ入れる（定型文の挿入）。
    /// 未知の action や、action も text も無い手順があれば、そこで中断する（それまでの手順は実行済み）
    func shortcut(_ steps: [ShortcutStep]) {
        toggleState = nil
        commitPreedit()
        for (index, step) in steps.enumerated() {
            if let action = step.action {
                guard Self.knownActions.contains(action) else {
                    DiagLog.log("shortcut aborted at step \(index + 1): unknown action \(action)")
                    return
                }
                perform(action: action)
            } else if let text = step.text, !text.isEmpty {
                hostInsert(text)
            } else {
                DiagLog.log("shortcut aborted at step \(index + 1): empty step")
                return
            }
        }
    }

    /// `action` キー。未知の名前は無視する（将来の拡張で壊れないように）
    func perform(action: String) {
        toggleState = nil
        switch action {
        case "space": space()
        case "enter": enter()
        case "backspace": backspace()
        case "shift": tapShift()
        case "capslock": toggleCapsLock()
        case "convert": send("start_convert") { $0.perform(.startConvert) }
        case "moveLeft": arrow(.left)
        case "moveRight": arrow(.right)
        case "dakuten", "smallKana": variant()
        case "modeSwitch": switchMode()
        case "globe": nextKeyboard()
        case "dismiss": host?.hostDismissKeyboard()
        default:
            DiagLog.log("unknown action \(action)")
        }
    }

    /// ツールバーの操作（`ToolbarAction`）。ツールバーは入力中でないときだけ出るが、押す直前に入力が始まっていた場合に
    /// 備えて、文字を入れる操作は入力中の文字列を確定してから行う。既存のキー操作・機能をそのまま呼ぶ
    func perform(toolbar action: ToolbarAction) {
        toggleState = nil
        switch action {
        case .dismiss: perform(action: "dismiss")
        case .paste: pasteFromClipboard()
        case .cursorLeft: arrow(.left)
        case .cursorRight: arrow(.right)
        case .newline: perform(action: "enter")
        case .space:
            commitPreedit()
            hostInsert(" ")
        case .deleteWord: deleteWordBackward()
        case .emoji: switchLayout(target: "emoji")
        case .nextKeyboard: nextKeyboard()
        }
    }

    // MARK: - キー操作（コアへ）

    func key(_ key: KanaKey, _ gesture: KeyGesture, flick: FlickDetail? = nil) {
        toggleState = nil
        // フリック入力では、タップは常に先頭の文字。トグル入力では連続タップを置き換えにする
        let effective: KeyGesture = (gesture == .tap && toggleInput) ? .toggle : gesture
        // フリックの角度は方向のあるフリックにだけ付ける。パスワード欄などでは送らない
        let sentFlick = (effective == .tap || effective == .toggle || (host?.hostIsSecure() ?? false)) ? nil : flick
        let consumed = send("key12 \(key.rawValue) \(effective.rawValue)") {
            $0.insert(.key12(ReadingInputKey12Inner(key: key, gesture: effective)), flick: sentFlick)
        }
        // コアが使えないときは、押されたキーの文字をそのままホストへ入れる（入力を失わない）
        if !consumed, let char = KanaKeyTable.char(for: key, gesture: effective) {
            hostInsert(char)
        }
    }

    func variant() {
        toggleState = nil
        send("variant") { $0.insert(.variant) }
    }

    func space() {
        toggleState = nil
        if !send("space", { $0.perform(.space) }) { hostInsert(" ") }
    }

    func enter() {
        toggleState = nil
        if !send("enter", { $0.perform(.enter) }) { hostInsert("\n") }
    }

    /// ⌫: 入力中ならコアの backspace、未入力ならホストの 1 文字削除（長押しの連打でも 1 回ごとにこれを呼ぶ）
    func backspace() {
        toggleState = nil
        if !send("backspace", { $0.perform(.backspace) }) { hostDeleteBackward(1) }
    }

    /// ←/→: 変換中は注目文節、変換前の入力中はカーソル、未入力ならホストの入力欄のカーソルを動かす
    func arrow(_ direction: TouchDirection) {
        toggleState = nil
        if isConverting {
            send("move_focus \(direction.rawValue)") { $0.perform(.moveFocus(direction)) }
        } else if hasPreedit {
            send("move_cursor \(direction.rawValue)") { $0.perform(.moveCursor(direction)) }
        } else {
            DiagLog.log("host adjustCursor \(direction == .left ? -1 : 1)")
            host?.hostAdjustCursor(by: direction == .left ? -1 : 1)
        }
    }

    /// スペースキーのスワイプによるカーソル移動。`steps` は右が正。
    /// 変換中は注目文節、入力中はプリエディット内のカーソルを、`arrow` と同じ Action で 1 つずつ動かす。
    /// 未入力ならホストの入力欄のカーソルをまとめて動かす（`adjustTextPosition`）
    func spaceSwipeMove(_ steps: Int) {
        guard steps != 0 else { return }
        toggleState = nil
        if hasPreedit {
            let direction: TouchDirection = steps < 0 ? .left : .right
            for _ in 0..<min(abs(steps), 20) { arrow(direction) }
        } else {
            DiagLog.log("host adjustCursor \(steps)")
            host?.hostAdjustCursor(by: steps)
        }
    }

    /// ⌫の左スワイプ: カーソル手前の単語を消す。入力中（プリエディットあり）は何もしない
    /// （押した瞬間の 1 文字削除はコアの backspace で済んでいる）。文脈が取れない入力欄でも何もしない
    func deleteWordBackward() {
        toggleState = nil
        guard !hasPreedit, let context = host?.hostContextBeforeInput() else { return }
        let count = WordBoundary.deleteCount(before: context)
        guard count > 0 else { return }
        hostDeleteBackward(count)
    }

    // MARK: - レイアウトの切替

    /// かな ⇄ 英字。かな以外（英字・数字・記号・絵文字）からは、かなに戻る
    func switchMode() {
        switchLayout(target: layoutDef.mode == .kana ? "alpha" : "kana")
    }

    /// ユーザーの操作（レイアウトキー・モード切替）による切替。以後は入力欄の種類による自動選択で戻さない
    func switchLayout(target: String) {
        autoSelectedLayout = false
        changeLayout(target: target)
    }

    /// 別のレイアウトへ。`target` は `kana` / `alpha`（各モードに割り当てたレイアウト）またはレイアウトの id。
    /// 入力中の文字列は確定してから切り替える（設定 `input.keep_preedit_on_symbols` / `input.keep_preedit_on_english` で
    /// 保つ場合を除く。保てば切替先でも同じ入力中の文字列に続く）。切り替えたら true（自動選択・割り当ての反映からも使う）
    @discardableResult
    private func changeLayout(target: String) -> Bool {
        let next: KeyboardLayoutDef?
        switch target {
        case "kana": next = cachedLayout(assigned: .kana)
        case "alpha": next = cachedLayout(assigned: .alpha)
        default: next = cachedLayout(id: target)
        }
        guard let next = next else {
            DiagLog.log("layout target not found: \(target)")
            return false
        }
        toggleState = nil
        let keep = LayoutSwitchKeep.shouldKeep(
            from: layoutDef.mode, to: next.mode, hasPreedit: hasPreedit, isSecure: host?.hostIsSecure() ?? false,
            keepOnSymbols: ios.keepPreeditOnSymbols, keepOnEnglish: ios.keepPreeditOnEnglish,
            composeNumbers: ios.composeNumbers
        )
        if hasPreedit, !keep, let session = session, let result = session.perform(.commit) {
            apply(result, origin: "response(commit before layout switch)")
        }
        japaneseContext = NumberCompose.nextContext(
            previousMode: layoutDef.mode, previousContext: japaneseContext, nextMode: next.mode
        )
        layoutDef = next
        shiftState = .off
        lastShiftTap = nil
        isExpanded = false
        DiagLog.log("layout -> \(next.id) mode=\(next.mode.rawValue)")
        return true
    }

    /// 入力欄の種類（`keyboardType` / `returnKeyType`）を表示に反映する（focusIn・入力欄の切替のたび）。
    /// レイアウトの指定がある欄（メール・URL・数字など）ではそのレイアウトへ自動で切り替え、指定のない欄に移ったら
    /// 自動で切り替えていた場合だけかなへ戻す（ユーザーが選んだレイアウトは変えない）。
    /// 設定 `keyboard.layout_by_field` がオフなら、レイアウトは自動で選ばず、改行キーもレイアウトのラベルのままにする
    /// （オフにする前に自動で切り替えていた場合は、かなへ戻して印を消す）
    private func applyInputTraits() {
        let type = host?.hostKeyboardType()
        lastAppliedTraits = (type?.rawValue ?? -1, host?.hostReturnKeyType()?.rawValue ?? -1)
        let typeName = type.map { String($0.rawValue) } ?? "-"
        guard ios.layoutByField else {
            returnKeyLabel = nil
            if autoSelectedLayout {
                autoSelectedLayout = false
                if layoutDef.mode != .kana { changeLayout(target: "kana") }
                DiagLog.log("input traits keyboardType=\(typeName) -> kana (restore, layout_by_field=off)")
            } else {
                DiagLog.log("input traits keyboardType=\(typeName) -> none (layout_by_field=off)")
            }
            return
        }
        returnKeyLabel = host?.hostReturnKeyType().flatMap { InputTraitsMapping.returnLabel(for: $0) }
        if let preferred = type.flatMap({ InputTraitsMapping.preferredLayout(for: $0) }) {
            // 既にそのモードのレイアウトなら何もしない（ユーザーが選んだ同じモードの別レイアウトも含めて変えない）
            guard layoutDef.mode != preferred.mode else {
                DiagLog.log("input traits keyboardType=\(typeName) -> \(preferred.target) (already)")
                return
            }
            if changeLayout(target: preferred.target) {
                autoSelectedLayout = true
                // 入力欄の種類による自動選択は日本語入力の流れではない（電話番号などの数字欄で変換候補を出さない）
                japaneseContext = false
            }
            DiagLog.log("input traits keyboardType=\(typeName) -> \(preferred.target)")
        } else if autoSelectedLayout {
            autoSelectedLayout = false
            if layoutDef.mode != .kana { changeLayout(target: "kana") }
            DiagLog.log("input traits keyboardType=\(typeName) -> kana (restore)")
        } else {
            DiagLog.log("input traits keyboardType=\(typeName) -> none (keep \(layoutDef.id))")
        }
    }

    private func cachedLayout(id: String) -> KeyboardLayoutDef? {
        if let cached = layoutCache[id] { return cached }
        guard let def = LayoutLoader.load(id: id) else { return nil }
        layoutCache[id] = def
        return def
    }

    private func cachedLayout(assigned mode: LayoutMode) -> KeyboardLayoutDef {
        let id = LayoutAssignment.id(for: mode)
        if let cached = layoutCache[id] { return cached }
        let def = LayoutLoader.loadAssigned(mode: mode)
        layoutCache[id] = def
        return def
    }

    /// シフトキー: 1 回で次の 1 文字のみ大文字、0.5 秒以内の 2 回連続で Caps Lock、Caps Lock 中の 1 回で解除
    func tapShift() {
        // シフトは英字などの大文字化のためのもの。かなのレイアウトでは意味を持たないので無視する
        guard layoutDef.mode != .kana else { return }
        let now = Date()
        switch shiftState {
        case .off:
            shiftState = .once
            lastShiftTap = now
        case .once:
            if let last = lastShiftTap, now.timeIntervalSince(last) < doubleTapInterval {
                shiftState = .caps
            } else {
                shiftState = .off
            }
            lastShiftTap = nil
        case .caps:
            shiftState = .off
            lastShiftTap = nil
        }
    }

    func toggleCapsLock() {
        guard layoutDef.mode != .kana else { return }
        shiftState = (shiftState == .caps) ? .off : .caps
        lastShiftTap = nil
    }

    func nextKeyboard() {
        host?.hostAdvanceToNextInputMode()
    }

    /// 候補を選ぶ。変換中は選んだあと、後ろに文節が残っていればその文節までを確定して残りの変換を続け、最後の文節（または文節が 1 つ）なら確定する。
    /// 入力中（変換前）は選んだ候補が置き換える読みの部分をコアが確定する（部分変換。残りは入力中のまま続く）
    func selectCandidate(_ index: Int) {
        guard let session = session else { return }
        toggleState = nil
        let converting = isConverting
        // 選ぶ前の注目文節と文節数で判定する（選んだ結果で文節が変わっても、選んだ文節の後ろが残っているかで決める）
        let focusBefore = focusIndex
        let countBefore = segmentCount
        isExpanded = false
        DiagLog.log("select_candidate index=\(index) converting=\(converting) focus=\(focusBefore.map { String($0) } ?? "-")/\(countBefore)")
        if let result = session.selectCandidate(index) { apply(result, origin: "response(select_candidate)") }
        guard converting else { return }
        if let focus = focusBefore, focus < countBefore - 1 {
            // 選んだ文節までを確定し、残りは変換中のまま続ける（iOS 標準の日本語キーボードと同じ。H-34）
            send("commit_focused") { $0.perform(.commitFocused) }
        } else if let result = session.perform(.commit) {
            apply(result, origin: "response(commit)")
        }
    }

    func focusIn() {
        toggleState = nil
        isVisible = true
        // 直接入力に倒れている（コアが作れていない・応答しなかった）ときは、フォーカスのたびに作り直しを試みる
        if session == nil {
            restartCore(noLLM: dictionaryOnly, reason: "focusIn: no session")
        }
        reloadIOSSettings()
        applyCoreSettingsChanges()
        reportEnvironment(reason: "focusIn")
        // レイアウトのファイルが変わっていても追随する（ホストアプリでの割り当て・取り込み）
        layoutCache.removeAll()
        if let fresh = cachedLayout(id: layoutDef.id) { layoutDef = fresh }
        applyLayoutAssignment()
        lastSentContext = nil
        DiagLog.log("focusIn layout=\(layoutDef.id)")
        sendLayoutGeometry()
        if let result = session?.focusIn() { apply(result, origin: "response(focus_in)") }
        scheduleContextUpdate(delay: 0)
        // 閉じている間に手放した LLM、または起動時に待たせた LLM を、空きメモリを確かめて読み込む
        updateWarning()
        scheduleLLMRestore(after: 0.3)
        // 入力欄の種類によるレイアウトの自動選択（割り当ての反映 `applyLayoutAssignment` より後）
        applyInputTraits()
    }

    /// ホストアプリでの変更をコアに反映する。`config.toml` の更新時刻が変わっていれば `reload_config`
    /// （コアも要求のたびに更新を見て自動で読み直すが、focusIn の時点で確実に反映する）。
    /// 学習データの全削除の合図が変わっていれば `clear_learning`（拡張のメモリ上の学習データが、
    /// 削除済みのファイルへ書き戻されて復活しないようにする）
    private func applyCoreSettingsChanges() {
        guard let session = session else { return }
        if NagiLearningClearSignal.current() != lastLearningClearSignal {
            lastLearningClearSignal = NagiLearningClearSignal.current()
            _ = session.clearLearning()
            DiagLog.log("clear_learning (signal from host app)")
        }
        let stamp = Self.modificationDate(configURL)
        if stamp != lastConfigStamp {
            lastConfigStamp = stamp
            if let result = session.reloadConfig() {
                DiagLog.log("reload_config ok=\(result.ok) errors=\(result.errors.count)")
            }
        }
    }

    /// かな・英字の割り当てが変わっていたら、表示中のレイアウトをその新しい割り当てに切り替える
    private func applyLayoutAssignment() {
        let kana = LayoutAssignment.id(for: .kana)
        let alpha = LayoutAssignment.id(for: .alpha)
        let kanaChanged = kana != appliedAssignment.kana
        let alphaChanged = alpha != appliedAssignment.alpha
        guard kanaChanged || alphaChanged else { return }
        appliedAssignment = (kana: kana, alpha: alpha)
        // 割り当ての反映はユーザーの選択ではないので、自動選択の印は残す
        if kanaChanged && layoutDef.mode == .kana {
            changeLayout(target: "kana")
        } else if alphaChanged && layoutDef.mode == .alpha {
            changeLayout(target: "alpha")
        }
    }

    /// フォーカスを失うとき: 変換中の文字列を確定する
    func focusOut() {
        toggleState = nil
        DiagLog.log("focusOut")
        // 操作の途中で離れたとき、状態が残らないようにする
        visual.setSpaceDragOrigin(nil)
        if let result = session?.focusOut() { apply(result, origin: "response(focus_out)") }
    }

    // MARK: - レイアウト幾何（set_layout_geometry）

    /// キー領域の大きさが分かった・変わった（表示後・レイアウト切替・向き・高さの変更）。
    /// 描画中のレイアウトのキー矩形をコアへ渡す。前回と同じ内容なら送らない
    func updateLayoutGeometry(size: CGSize) {
        layoutGeometrySize = size
        sendLayoutGeometry()
    }

    private func sendLayoutGeometry() {
        guard let session = session, let size = layoutGeometrySize else { return }
        let spacing = metrics
        guard let snapshot = LayoutGeometry.snapshot(
            layout: layoutDef, size: size, keySpacing: spacing.keySpacing, rowSpacing: spacing.rowSpacing
        ),
              snapshot != lastSentGeometry else { return }
        lastSentGeometry = snapshot
        let ok = session.setLayoutGeometry(snapshot)
        // キーの座標は入力内容ではないが、ログには数だけ残す
        DiagLog.log("set_layout_geometry layout=\(snapshot.layoutId) keys=\(snapshot.keys.count) ok=\(ok)")
    }

    // MARK: - 文脈（set_context）

    /// ホストの入力欄が変わった（外部からの変更・カーソル移動・自分の操作の結果）
    func hostTextChanged() {
        // 入力欄の種類が変わっていれば（同じ VC のまま別の欄へ移った場合など）、レイアウトと改行キーを反映し直す
        let traits = (host?.hostKeyboardType()?.rawValue ?? -1, host?.hostReturnKeyType()?.rawValue ?? -1)
        if let last = lastAppliedTraits, last.keyboard != traits.0 || last.returnKey != traits.1 {
            applyInputTraits()
        }
        // 自分の操作の結果ではない変更（カーソル移動・他アプリによる書き換え）が起きたら、
        // 直前のトグル入力の「置き換え」は続けない
        // （ホストからの通知は遅れて届くことがある。本文の末尾が直前に入れた文字のままなら、続きとみなして残す）
        // ホストの文脈は非同期で古いこと・取れないことがあるので、nil のときは捨てない
        // （末尾が直前の文字と一致しないことを確認できたときだけ捨てる）
        if let state = toggleState, Date().timeIntervalSince(lastOwnHostOperation) > ownOperationWindow,
           let context = host?.hostContextBeforeInput(), !context.hasSuffix(state.last) {
            DiagLog.log("toggleState dropped by external text change tail=\(logText(String(context.suffix(4)))) last=\(logText(state.last))")
            toggleState = nil
        }
        scheduleContextUpdate(delay: 0.05)
        refreshClipboardState()
    }

    /// 入力欄そのものが切り替わった（`textDocumentProxy.documentIdentifier` が変わった）。
    /// 入力中の文字は前の欄に入っている（確定扱い）ので、新しい欄には何も反映せず、コアの状態だけを初期化し、
    /// 新しい欄の secure 判定を含む文脈をコアへ必ず送り直す
    func hostDocumentChanged() {
        DiagLog.log("document changed")
        toggleState = nil
        if let session = session {
            if let result = session.focusOut() { lastRevision = max(lastRevision, result.revision) }
            if let result = session.focusIn() { lastRevision = max(lastRevision, result.revision) }
        }
        applier.reset()
        preedit = ""
        preeditParts = []
        isConverting = false
        candidates = []
        isNextWordCandidates = false
        selectedCandidate = -1
        isExpanded = false
        lastSentContext = nil
        scheduleContextUpdate(delay: 0)
        // 新しい欄の種類に合わせてレイアウトと改行キーの表示を選び直す
        applyInputTraits()
    }

    /// 文脈の送信を予約する。連続した変更は 1 回にまとめる
    private func scheduleContextUpdate(delay: TimeInterval) {
        guard session != nil, !contextScheduled else { return }
        contextScheduled = true
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self = self else { return }
            self.contextScheduled = false
            self.sendContext()
        }
    }

    /// カーソル手前の末尾（最大 `contextLength` 文字）をコアに渡す。変換中の入力は含めない
    /// （ホストの文脈には変換中の文字列も入るため、入力中は送らず、確定・消去のあとで送る）。
    /// パスワード欄などでは文脈を送らず、secure であることだけ伝える
    private func sendContext() {
        guard let session = session, let host = host else { return }
        guard shownMarked.isEmpty, !hasPreedit else { return }
        let secure = host.hostIsSecure()
        let tail: String? = secure ? nil : String((host.hostContextBeforeInput() ?? "").suffix(contextLength))
        let signature = secure ? "\u{0}secure" : (tail ?? "")
        guard signature != lastSentContext else { return }
        lastSentContext = signature
        session.setContext(before: tail, secure: secure)
        DiagLog.log("set_context secure=\(secure) tail=\(DiagLog.quote(tail ?? "", redact: secure))")
    }

    // MARK: - コアとのやり取り

    /// コアに要求を送り、結果を反映する。コアが操作を消費したら true（セッションが無ければ false）
    @discardableResult
    private func send(_ label: String, _ call: (NagiSession) -> ProcessKeyResult?) -> Bool {
        guard let session = session else {
            DiagLog.log("send \(label) -> no session")
            return false
        }
        let shown = loggedLabel(label)
        guard let result = call(session) else {
            if session.lastRequestWasRPCError {
                // コアは生きていて要求を拒んだ（JSON-RPC のエラー応答）。無応答ではないので失敗には数えない
                DiagLog.log("send \(shown) -> rpc error")
            } else {
                DiagLog.log("send \(shown) -> no result")
                noteCoreFailure(shown)
            }
            return false
        }
        consecutiveFailures = 0
        DiagLog.log("send \(shown)")
        apply(result, origin: "response")
        return result.consumed
    }

    private func apply(_ result: ProcessKeyResult, origin: String) {
        lastOwnHostOperation = Date()
        if DiagLog.isEnabled {
            var parts: [String] = ["\(origin) consumed=\(result.consumed) rev=\(result.revision)"]
            if let commit = result.commit { parts.append("commit=\(logText(commit))") }
            if let n = result.delete_before { parts.append("delete_before=\(n)") }
            if let preedit = result.preedit {
                parts.append("preedit=\(logText(preedit.text)) cursor=\(preedit.cursor) focus=\(preedit.focus != nil)")
            }
            if let candidates = result.candidates { parts.append("candidates=\(candidates.items.count)") }
            DiagLog.log(parts.joined(separator: " "))
        }
        lastRevision = max(lastRevision, result.revision)
        if let n = result.delete_before, n > 0 {
            hostDeleteBackward(Int(n))
        }
        if let commit = result.commit, !commit.isEmpty {
            // 部分確定・全文確定とも同じ決定的な手順（範囲を確定扱い → 表示中の文字数だけ消す → 挿入）。
            // ホスト側の変換中の範囲の追跡に頼る置換は、実機で範囲がずれて二重になる。
            // 残りの変換中は、直後の showPreedit が新しい変換中として入れる
            lastOwnHostOperation = Date()
            applier.commit(commit)
            // 確定した文字列を文脈に含めるため、ホストが反映したあとに送る
            scheduleContextUpdate(delay: 0.15)
        }
        if let preedit = result.preedit { showPreedit(preedit) }
        if let candidates = result.candidates { showCandidates(candidates) }
        DiagLog.log("shownMarked=\(logText(shownMarked)) cursor=\(shownCursor)")
    }

    private func applyPreeditUpdate(_ update: UpdatePreeditParams) {
        // 古い通知は捨てる
        guard update.revision >= lastRevision else {
            DiagLog.log("notify update_preedit dropped rev=\(update.revision) < \(lastRevision)")
            return
        }
        DiagLog.log("notify update_preedit rev=\(update.revision) text=\(logText(update.preedit.text))")
        lastRevision = update.revision
        showPreedit(update.preedit)
        DiagLog.log("shownMarked=\(logText(shownMarked)) cursor=\(shownCursor)")
    }

    private func applyCandidatesUpdate(_ update: UpdateCandidatesParams) {
        guard update.revision >= lastRevision else {
            DiagLog.log("notify update_candidates dropped rev=\(update.revision) < \(lastRevision)")
            return
        }
        DiagLog.log("notify update_candidates rev=\(update.revision) candidates=\(update.candidates.items.count)")
        lastRevision = update.revision
        showCandidates(update.candidates)
    }

    private func showPreedit(_ view: PreeditView) {
        // 注目文節の表示（候補バー）。ホストの変換中の表示とは別に、毎回更新する
        if let focus = view.focus {
            let focused = Int(focus)
            preeditParts = view.segments.enumerated().map { index, segment in
                PreeditPart(id: index, text: segment.surface, focused: index == focused)
            }
        } else {
            preeditParts = []
        }
        segmentCount = view.segments.count
        focusIndex = view.focus.map { Int($0) }
        preedit = view.text
        isConverting = view.focus != nil
        let cursor = HostCompositionApplier.markedCursor(
            coreCursor: Int(view.cursor), text: view.text, converting: view.focus != nil
        )
        guard view.text != shownMarked || cursor != shownCursor else { return }
        lastOwnHostOperation = Date()
        if view.text.isEmpty {
            applier.clear()
            scheduleContextUpdate(delay: 0.15)
        } else {
            applier.setMarked(view.text, cursor: cursor)
        }
    }

    private func showCandidates(_ view: CandidatesView) {
        let kind = classifyCandidates(view, hasPreedit: hasPreedit)
        if kind == .hidden {
            // visible = false（次語候補が入力の開始などで消える）も、候補なしと同じに片付ける
            candidates = []
        } else {
            candidates = view.items.enumerated().map { index, item in
                CandidateItem(
                    id: index, surface: item.surface, covers: item.covers.map { Int($0) },
                    hint: fuzzyHint(sources: item.sources, reading: item.reading)
                )
            }
        }
        isNextWordCandidates = (kind == .nextWord)
        selectedCandidate = candidates.isEmpty ? -1 : Int(view.selected)
        // 次語候補は選ぶまで確定しない候補ではないので、展開パネルは開かない
        if candidates.isEmpty || isNextWordCandidates { isExpanded = false }
    }

    // MARK: - ホストへの操作（ログ付き）

    private func hostInsert(_ text: String) {
        applier.flushPending()
        // applier を通らない操作なので、古い照合を走らせないよう applier に知らせる
        applier.noteExternalOperation()
        lastOwnHostOperation = Date()
        DiagLog.log("host insertText \(logText(text))")
        host?.hostInsertText(text)
    }

    private func hostDeleteBackward(_ count: Int) {
        applier.flushPending()
        applier.noteExternalOperation()
        lastOwnHostOperation = Date()
        DiagLog.log("host deleteBackward x\(count)")
        for _ in 0..<count { host?.hostDeleteBackward() }
    }

    /// ログに出す操作名。パスワード欄などでは、押したキーが分かる（かな 12 キーの行・方向など）ので伏せる
    private func loggedLabel(_ label: String) -> String {
        guard DiagLog.isEnabled, host?.hostIsSecure() ?? false else { return label }
        return "<secure>"
    }

    /// ログ用の文字列。パスワード欄などでは内容を伏せる
    private func logText(_ text: String) -> String {
        DiagLog.quote(text, redact: host?.hostIsSecure() ?? false)
    }
}
