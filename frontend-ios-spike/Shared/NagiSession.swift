import Foundation

/// コアの JSON-RPC を型付き（`Protocol.swift`）で扱うセッション。
///
/// 呼び出しの直列化は `NagiEngine` が持つキューが行う。ここの `request` と通知のコールバックは、
/// 呼び出した（またはハンドラが渡される）スレッドで実行されるので、UI から使うときはメインキューで呼ぶこと
final class NagiSession {
    let engine: NagiEngine
    let sessionId: String
    private(set) var profile = ""
    /// 直近の `request` が JSON-RPC のエラー応答（`error`）で終わったか。コアは生きているが要求を拒んだ場合で、
    /// 応答が無い（コアの異常）こととは区別する
    private(set) var lastRequestWasRPCError = false

    /// 非同期通知（LLM が同期の待ち時間を超えた分の完了、ライブ変換の結果）
    var onPreeditUpdate: ((UpdatePreeditParams) -> Void)?
    var onCandidatesUpdate: ((UpdateCandidatesParams) -> Void)?

    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()
    private var nextId = 1

    private struct Request<P: Encodable>: Encodable {
        let jsonrpc = "2.0"
        let id: Int
        let method: String
        let params: P
    }

    /// 応答を返さない通知（`id` なし）
    private struct NotificationMessage<P: Encodable>: Encodable {
        let jsonrpc = "2.0"
        let method: String
        let params: P
    }

    private struct Response<R: Decodable>: Decodable {
        let result: R?
    }

    /// JSON-RPC の `error` オブジェクト（中身は使わない。あるかどうかだけ見る）
    private struct RPCErrorBody: Decodable {}

    private struct Header: Decodable {
        let id: Int?
        let method: String?
        let error: RPCErrorBody?
    }

    private struct ParamsBox<P: Decodable>: Decodable {
        let params: P
    }

    /// `initialize` まで済ませる。コアが応答しなければ nil
    init?(engine: NagiEngine, sessionId: String = "keyboard") {
        self.engine = engine
        self.sessionId = sessionId
        let params = InitializeParams(
            protocol_version: 1,
            client_name: "nagi-ios",
            client_version: "0.1",
            os: "ios",
            // タッチ入力であることと、その時点の電源・熱・メモリ上限を申告する（コアはこれで最初のプロファイルを決める）
            capabilities: ClientCapabilities(
                input_method: .touch,
                environment: NagiEnvironmentReporter.systemSnapshot(memoryPressure: .normal)
            )
        )
        guard let result: InitializeResult = request("initialize", params) else { return nil }
        profile = result.profile
    }

    /// 要求を送り、応答の `result` を返す。同じ呼び出しに続いて返った通知は、コールバックに渡す
    func request<P: Encodable, R: Decodable>(_ method: String, _ params: P) -> R? {
        let id = nextId
        nextId += 1
        lastRequestWasRPCError = false
        guard let data = try? encoder.encode(Request(id: id, method: method, params: params)),
              let line = String(data: data, encoding: .utf8) else { return nil }
        var result: R?
        for response in engine.send(line: line) {
            guard let bytes = response.data(using: .utf8),
                  let header = try? decoder.decode(Header.self, from: bytes) else { continue }
            if header.id == id {
                if header.error != nil { lastRequestWasRPCError = true }
                result = (try? decoder.decode(Response<R>.self, from: bytes))?.result
            } else if let notification = header.method {
                dispatchNotification(notification, bytes)
            }
        }
        return result
    }

    /// 応答のない通知をコアへ送る。同じ呼び出しに続いて返った通知は、コールバックに渡す
    func notify<P: Encodable>(_ method: String, _ params: P) {
        guard let data = try? encoder.encode(NotificationMessage(method: method, params: params)),
              let line = String(data: data, encoding: .utf8) else { return }
        for response in engine.send(line: line) {
            guard let bytes = response.data(using: .utf8),
                  let header = try? decoder.decode(Header.self, from: bytes),
                  let name = header.method else { continue }
            dispatchNotification(name, bytes)
        }
    }

    private func dispatchNotification(_ method: String, _ bytes: Data) {
        switch method {
        case "update_preedit":
            if let box = try? decoder.decode(ParamsBox<UpdatePreeditParams>.self, from: bytes) {
                onPreeditUpdate?(box.params)
            }
        case "update_candidates":
            if let box = try? decoder.decode(ParamsBox<UpdateCandidatesParams>.self, from: bytes) {
                onCandidatesUpdate?(box.params)
            }
        default:
            break
        }
    }

    /// 次のリクエストを待たずに届く通知（非同期の LLM 結果など）の受け取りを始める。
    /// コールバックはメインキューで呼ばれる
    func startNotifications() {
        engine.startNotifications { [weak self] lines in
            for line in lines {
                guard let self = self, let bytes = line.data(using: .utf8),
                      let header = try? self.decoder.decode(Header.self, from: bytes),
                      let method = header.method else { continue }
                self.dispatchNotification(method, bytes)
            }
        }
    }

    func stopNotifications() {
        engine.stopNotifications()
    }

    // MARK: - メソッド

    /// 入力を送る。`touch`（QWERTY の 1 文字の押下位置。キーの矩形内の相対位置 0..1）と
    /// `flick`（12 キーのフリックの角度・距離）は曖昧入力（タイプミス補正）の手がかりで、どちらも任意。
    /// 両方 nil なら従来の `InsertReadingParams` と同じ内容を送る。座標はログに出さない
    func insert(_ input: ReadingInput, touch: TouchPoint? = nil, flick: FlickDetail? = nil) -> ProcessKeyResult? {
        if touch == nil && flick == nil {
            return request("insert_reading", InsertReadingParams(session_id: sessionId, input: input))
        }
        return request(
            "insert_reading",
            InsertReadingTouchParams(session_id: sessionId, input: input, touch: touch, flick: flick)
        )
    }

    /// 描画中のレイアウトのキー矩形を渡す（曖昧入力が、どのキーが隣かをここから導く）。
    /// 英字キーが 10 個未満のレイアウトはコアが無視する。成功すれば true
    @discardableResult
    func setLayoutGeometry(_ snapshot: LayoutGeometry.Snapshot) -> Bool {
        let keys = snapshot.keys.map {
            LayoutKey(id: $0.id, label: $0.label, x: $0.x, y: $0.y, w: $0.w, h: $0.h)
        }
        let params = SetLayoutGeometryParams(
            session_id: nil, layout_id: snapshot.layoutId, mode: snapshot.mode,
            keys: keys, width: snapshot.width, height: snapshot.height
        )
        let result: EmptyResult? = request("set_layout_geometry", params)
        return result != nil
    }

    func perform(_ action: TouchAction) -> ProcessKeyResult? {
        request("perform_action", PerformActionParams(session_id: sessionId, action: action))
    }

    func selectCandidate(_ index: Int) -> ProcessKeyResult? {
        request("select_candidate", SelectCandidateParams(session_id: sessionId, index: UInt32(index)))
    }

    /// 直前のテキスト（カーソル手前）を LLM の文脈として渡す。`before` が nil なら文脈を渡さない。
    /// `secure` のときコアは文脈を消し、学習・ログを止める
    func setContext(before: String?, secure: Bool) {
        let surrounding = before.map { SurroundingText(text: $0, cursor: UInt32($0.utf16.count)) }
        notify("set_context", SetContextParams(
            session_id: sessionId,
            surrounding_text: surrounding,
            app_id: nil,
            secure_input: secure
        ))
    }

    func focusIn() -> ProcessKeyResult? {
        request("focus_in", SessionParams(session_id: sessionId))
    }

    func focusOut() -> ProcessKeyResult? {
        request("focus_out", SessionParams(session_id: sessionId))
    }

    // MARK: - 環境・状態

    /// 実行環境（低電力モード・サーマル・メモリ逼迫）をコアへ報告する。コアはこれに応じてプロファイルを切り替える
    func setEnvironment(_ params: SetEnvironmentParams) {
        notify("set_environment", params)
    }

    /// 現在のプロファイル・LLM の使用有無など
    func getStatus() -> StatusResult? {
        request("get_status", EmptyResult())
    }

    /// 変換モデルだけを解放して辞書のみで動く状態にする（`reload_model` まで戻らない）。成功すれば true
    @discardableResult
    func unloadModel() -> Bool {
        let result: EmptyResult? = request("unload_model", EmptyResult())
        return result != nil
    }

    /// 解放した変換モデルを読み込み直す（読み込みは裏で行われ、完了までは辞書のみ）。成功すれば true
    @discardableResult
    func reloadModel() -> Bool {
        let result: EmptyResult? = request("reload_model", EmptyResult())
        return result != nil
    }

    /// 12 キーの文字表と濁点・小書きの循環（表示専用。入力される文字の決定はコアが行う）
    func getKeyTable() -> KeyTableResult? {
        request("get_key_table", EmptyResult())
    }

    // MARK: - 設定・学習

    /// 設定項目の記述子と現在値（`config.toml` を読む）
    func getSettings() -> SettingsSnapshot? {
        request("get_settings", EmptyResult())
    }

    /// 設定を書き込む（`config.toml` へ原子的に。`value` が nil の変更は明示指定を消して既定に戻す）
    func updateSettings(_ changes: [SettingChange]) -> UpdateSettingsResult? {
        request("update_settings", UpdateSettingsParams(changes: changes))
    }

    /// `config.toml` とユーザー辞書を読み直す（壊れていれば旧設定のまま）
    func reloadConfig() -> ReloadConfigResult? {
        request("reload_config", EmptyResult())
    }

    /// 学習データをすべて削除する
    func clearLearning() -> EmptyResult? {
        request("clear_learning", SessionParams(session_id: sessionId))
    }
}
