# 実機・CI で確認が残っている項目（一覧）

`docs/` の各文書で「要実機確認」「未確認」「未検証」「CI で確認」とされている項目を、プラットフォーム別にまとめたもの（2026-10-27 時点。「確認済み」と書かれた項目は除いた）。手順・期待値は参照先の文書を見ること。確認したら、参照先の文書（と決定番号の節）に結果を書き、ここからは消す。

内訳（重複は 1 件にまとめた）: iOS 49 / macOS 7 / Windows 35 / CI 6 / その他 18。この日に実装した「きょう」「いま」（H-72）は末尾の「追加分」に書いた。

## iOS

| No. | 何を確かめるか | 参照先 | 決定 |
|---|---|---|---|
| 1 | 指定なし同梱レイアウトの見た目・押下・長押し・フリック・曖昧入力が変わらないか（ライト/ダーク・アクセント・片手・候補展開） | ios-device-checklist.md §3 | H-65, H-66 |
| 2 | look（角丸・影・書体・色）の反映と、テーマ切替への追従 | ios-device-checklist.md §3 | H-66 |
| 3 | inset/offset/scale を指定しても当たり判定が変わらないか | ios-device-checklist.md §3 | H-66 |
| 4 | metrics・background の反映と、曖昧入力の矩形がずれないか | ios-device-checklist.md §3 | H-66 |
| 5 | spaceDrag（移動ラベル・他キーが薄くなる・離すと戻る・スペースは入らない） | ios-device-checklist.md §3 | H-66 |
| 6 | 押下縮小・longPress の発光・150ms で戻る・⌫ 連打開始で longPress | ios-device-checklist.md §3 | H-66 |
| 7 | システムにジェスチャを奪われたとき、見た目と released が残らないか | ios-device-checklist.md §3 | H-66 |
| 8 | shift/capsLock/composing の rules と「視差効果を減らす」で補間が止まるか | ios-device-checklist.md §3 | H-66 |
| 9 | 押下ごとに他キーが再描画されないか（Instruments）。既存レイアウトの入力反映が遅くならないか | ios-device-checklist.md §3、keyboard-style-spec.md §11 | H-66 |
| 10 | 状態の icon に iconify を指定したレイアウトの解決・保存 | ios-device-checklist.md §3 | H-66, H-36 |
| 11 | 背景画像 fill/fit/tile、デコード前のチラつき、画像なし・壊れた画像で色だけ出るか | ios-device-checklist.md §3 | H-66 |
| 12 | 上限超過の拒否文言と、参照先なし・未使用・長辺超過の警告表示 | ios-device-checklist.md §3 | H-66 |
| 13 | 4 枚・768KB のレイアウトで拡張のメモリ増加が 12MB 以内か。非表示で解放されるか | ios-device-checklist.md §3、keyboard-style-spec.md §9 | H-66 |
| 14 | 既存レイアウトの表示（role 改名・Decodable 化の影響）。手元のユーザーレイアウトは書き出し → 移行 → 取り込み直しが必要 | decisions.md H-65、keyboard-style-spec.md §12 | H-65 |
| 15 | Web エディタで書き出した JSON を iOS に取り込んで同じ見た目になるか（JPEG・各ブラウザ） | decisions.md H-67、keyboard-style-spec.md §9 | H-67 |
| 16 | toolbar 無しの見た目変化、toolbar 指定時の look/pressed、12 個並べたときの窮屈さ、候補バー切替時の高さ | decisions.md H-68 | H-68 |
| 17 | クリップボード履歴: 貼り付け許可の出方、前面化時の確認、contextMenu、カードのタップ・スクロール、履歴から戻った後のキー反応 | decisions.md H-69 | H-69 |
| 18 | 開発者画面の導線、オン/オフの即時反映、キーボードの「検証」ボタンの出入り | decisions.md H-64 | H-64 |
| 19 | `convert.symbol_position` の各値での候補の並び（LLM あり・実辞書） | decisions.md H-63 | H-63 |
| 20 | 候補が無いときのツールバーの見た目・幅（5 個以上）、次語予測での入れ替え、絵文字レイアウトへの切替 | decisions.md H-62 | H-62 |
| 21 | 最下段フリック案内の位置、ライト/ダーク・テーマ上書き時の色 | decisions.md H-61 | H-61 |
| 22 | 記号候補の見え方・件数、普通の語で記号が 2 番目に出る煩わしさ、LLM あり実バックエンドでの順序 | decisions.md H-60 | H-60 |
| 23 | 語彙・原語データ導入後に候補バーへ `computer` が出るか（導入後にキーボードを開き直す） | decisions.md H-59 | H-59 |
| 24 | 100 件前後の候補のスクロール・メモリ、入れ替え時の先頭化、接頭辞候補の下線、LLM ありの総所要時間 | decisions.md H-58 | H-58 |
| 25 | 切替時確定の設定の見た目、英字レイアウトでの Shift/BS/Space/Enter、トグル置換、語彙導入後の英語候補 | decisions.md H-57 | H-57 |
| 26 | `1+2` などの入力中に候補バーへ電卓・全角・記号の候補が出るか | decisions.md H-55 | H-55 |
| 27 | 数字・記号キーの入力中文字列化（下線、候補、Space/Enter/BS、= で電卓、未確定でのレイアウト切替、トグル置換、全角記号候補、電話番号欄の直接入力） | decisions.md H-53 | H-53 |
| 28 | 速打ちで前回のクリック音・触覚が終わるまで次が鳴らない（改善要望。未着手） | decisions.md H-44、ios-device-checklist.md §1 | H-44, H-30 |
| 29 | 旧 VC の deinit と、旧プロセスの終了タイミング | decisions.md H-27 | H-27, H-33 |
| 30 | 旧インスタンスのメモリが新インスタンスの `os_proc_available_memory` に反映されるか。ended while visible の扱い | decisions.md H-26 | H-26 |
| 31 | small の差し替えの見た目と、他機種での揺れ | decisions.md H-22 | H-22 |
| 32 | small の応答時間（iPhone の CPU・p50）の再確認 | decisions.md H-17 | H-17, H-9 |
| 33 | CPU のみ（Metal 無効）、長時間・多数回のメモリ推移、候補 UI を載せた余裕、他機種 | decisions.md H-6 | H-6 |
| 34 | 曖昧入力の矩形の座標系とタッチ位置 | progress.md 曖昧入力 M2 節、decisions.md H-25 | H-25 |
| 35 | 英語の綴りの「もしかして」の探索時間とメモリ（語彙は `--install-data` またはアプリから導入） | decisions.md H-70 | H-70 |
| 36 | `convert.half_width_alnum = half` の実際の半角化（全角英数を打てる入力で） | decisions.md H-71 | H-71 |
| 37 | 原語データのアプリ内取得、拡張での反映、メモリ | decisions.md H-52 | H-52 |
| 38 | `set_context` が `focus_in` より先に送られているか | decisions.md H-49 | H-49 |
| 39 | 「簡易モード（辞書のみ）」の注意の出る条件（10 秒以上）と文言 | progress.md B-2 の 5 | H-26 |
| 40 | 入力テストタブの説明文の再確認 | progress.md B-2 の 10 | H-34 |
| 41 | メモリ閾値（150/1024/2048MB）と LLM の再読み込み（自然に起きたときの診断ログ待ち） | ios-device-checklist.md §5.2 | H-16, H-26 |
| 42 | フルアクセス OFF/ON で触覚が出るか（互換モードも比較） | ios-device-checklist.md §5.3 | H-18 |
| 43 | 他機種の拡張メモリ上限・推奨・応答 | ios-device-checklist.md §5.4 | H-17, H-22(c), H-8 |
| 44 | 既定の `sync_budget_ms`（現行 30ms）の決定 | ios-device-checklist.md §5.5 | H-10 |
| 45 | フルアクセス OFF で漢字変換の候補が出るか | ios-device-checklist.md §5.6(a) | H-18 |
| 46 | TestFlight のアーカイブに PrivacyInfo.xcprivacy が 2 つとも入るか（Mac と Xcode が要る） | ios-device-checklist.md §5.6(b) | — |
| 47 | IPA の圧縮後サイズ（Mac と Xcode が要る） | ios-device-checklist.md §5.6(c) | — |
| 48 | 審査ガイドライン 4.4.1（フルアクセス無しでも動く旨）の文面と (a) の結果の整合（ホーム・About・オンボーディング・サイト） | ios-device-checklist.md §5.6(d) | — |
| 49 | 学習データの同時書き込み（偶然にしか起きない） | ios-device-checklist.md §5.7 | H-44 |

## macOS

| No. | 何を確かめるか | 参照先 | 決定 |
|---|---|---|---|
| 1 | IMK の `setValue(_:forTag:client:)` の呼ばれ方、英数/かなキーの先取り、`attributedSubstring` の対応アプリ（ブラウザ・ターミナル・Electron）、`setMarkedText` の replacementRange、メモリ逼迫イベントの頻度 | decisions.md H-37 | H-37 |
| 2 | 配布: hardened runtime で LLM（Metal）と imserver が起動するか、公証が通るか、IMK の終了後の再起動（Secrets 未登録で未実行） | decisions.md H-37(5) | H-37 |
| 3 | アプリから imserver を子プロセスで実行したときの進捗表示、取得中の入力、hardened runtime 下のネットワーク接続 | decisions.md H-38 | H-38 |
| 4 | Space 変換の候補拡充による Space→候補表示 p95（目標 150ms）への影響、サジェストの ↓/Tab/↑ と選択中 Enter | decisions.md H-45 | H-45 |
| 5 | 確定文字の複製が再現するか（低頻度。使いながら確認） | decisions.md H-46 | H-46 |
| 6 | Fn の言語切替アイコン（白い四角・透明）が直ったか。白いままなら CFBundleIconFile を外す | decisions.md H-47 | H-47 |
| 7 | `activateServer` の `adoptSelectedMode` が、切替結果をアプリごとの記憶として残すか | decisions.md H-49 | H-49 |

## Windows

| No. | 何を確かめるか | 参照先 | 決定 |
|---|---|---|---|
| 1 | DLL の登録（regsvr32）、入力方式としての認識、キー入力、composition の表示と確定、DllCanUnloadNow、imserver の起動と stdout、ShiftEnd のキャレット位置 | decisions.md H-39、real-hardware-checklist.md §8.1〜8.3、windows-verification.md 段階 A・D・E-1 | H-39, H-41 |
| 2 | VK_OEM_BACKTAB の実キー割り当て。VK_IME_ON/OFF と OS の IME オン/オフの食い違い。半角/全角・変換・無変換・カナ | decisions.md H-39、real-hardware-checklist.md §8.4、windows-verification.md E-6 | H-39 |
| 3 | OnTestKeyDown だけ呼ばれ OnKeyDown が来ない場面の頻度。入力タイムアウト 250ms の妥当性 | real-hardware-checklist.md §8.3、windows-verification.md E-9 | H-39 |
| 4 | IS_PASSWORD（値 31）の欄で周辺テキストを読まず、学習・ログが止まるか | decisions.md H-40、real-hardware-checklist.md §8.5・§8.5b、windows-verification.md E-10 | H-40, H-41 |
| 5 | 候補ウィンドウ: フォーカスを奪わない、キャレット直下、DPI・複数モニター、画面下端で上に反転、番号・出所ラベル・ページ表示、マウス位置への代替 | decisions.md H-41、real-hardware-checklist.md §8.5・§8.5b、windows-verification.md E-2 | H-40, H-41 |
| 6 | 表示属性（通常/注目/確定/スタイル変換/保留）の反映、属性を無視するアプリで入力が壊れないか | real-hardware-checklist.md §8.5・§8.5b、windows-verification.md E-3 | H-40, H-41 |
| 7 | 非同期通知（ライブ変換・LLM 結果）が次のキーを待たずに出るか。直後のキー入力と編集セッションの順序 | decisions.md H-41、real-hardware-checklist.md §8.5b、windows-verification.md E-7 | H-41 |
| 8 | 入力モード: トレイ・英数キー・半角/全角・クリックの往復で反転し続けないか。英数 = IME 閉でアプリが困らないか。Activate 直後と再起動後の同期 | decisions.md H-41、real-hardware-checklist.md §8.5b、windows-verification.md E-5 | H-41 |
| 9 | 環境・文脈: AC/電池・バッテリー節約の反映（約 30 秒）、サーマル/メモリ推定の閾値、`set_context` の送信タイミング、アプリ ID | real-hardware-checklist.md §8.5・§8.5b、windows-verification.md E-10 | H-40, H-41 |
| 10 | スリープ復帰: WM_POWERBROADCAST が届くか、届かなければ 30 秒タイマーで復帰するか、二重に再起動しないか | decisions.md H-41、real-hardware-checklist.md §8.3・§8.5b、windows-verification.md E-8 | H-41 |
| 11 | エンジンのクラッシュ復帰（指数バックオフ、`restore_session` で preedit 復元、composition 中の強制終了） | real-hardware-checklist.md §8.3、windows-verification.md E-8 | H-39 |
| 12 | 再変換（ITfFnReconversion）を呼ぶアプリ（Office 等）での候補一覧、範囲の置換、キャレット位置、初回の逆引き索引の待ち通知 | decisions.md H-41、windows-verification.md E-4 | H-41, H-43 |
| 13 | GetReconversion: 文書ロック中の呼び出しで固まらないか、失敗後にコアが変換中のまま残らないか | decisions.md H-43、real-hardware-checklist.md §8.5c | H-43 |
| 14 | 言語バーの「あ / A」ボタン: 表示、左クリック切替、右クリックメニュー、トレイとの二重表示、Deactivate で残らないか | decisions.md H-43、real-hardware-checklist.md §8.5c、windows-verification.md E-5 | H-43 |
| 15 | GetIcon の HICON の寿命（GDI オブジェクトの増加） | decisions.md H-43、real-hardware-checklist.md §8.5c、windows-verification.md E-5 | H-43 |
| 16 | トレイに「あ / A」が出るか（出なければ dwStyle の調整）、tsf.log の内容、複数プロセスからの追記で行が壊れないか | decisions.md H-54、real-hardware-checklist.md §8.5d、windows-verification.md E-5 | H-54 |
| 17 | imserver 起動失敗（5 回失敗 → 辞書のみ）の原因が stderr の panic か、修正後に消えるか、`imserver-<pid>.log` の作成・追記・掃除、終了コード | decisions.md H-56、real-hardware-checklist.md §8.5e、windows-verification.md「imserver の起動失敗の切り分け」 | H-56 |
| 18 | nagi.ico が DLL と同じフォルダに作られ言語設定とトレイに出るか、`regsvr32 /u` で消えるか、書けないフォルダでの挙動 | real-hardware-checklist.md §8.5b、windows-verification.md D-1 | H-41 |
| 19 | 設定ファイルを開く、通知トースト（右下約 4 秒・フォーカスを奪わない・30 秒間引き・背面プロセスの通知） | real-hardware-checklist.md §8.5b・§8.5c、windows-verification.md E-8・E-10 | H-41, H-43 |
| 20 | ホスト 1/5/10 個での imserver のメモリ・起動時間・learning.tsv の取りこぼし（設計判断のための実測） | real-hardware-checklist.md §8.6、windows-verification.md §5 の 13 | H-39, H-43 |
| 21 | THIRD_PARTY_LICENSES_WINDOWS.md が実ビルドの依存と一致するか（cargo tree で比較） | real-hardware-checklist.md §8.5c | H-43 |
| 22 | install.ps1: UAC・SHA256SUMS 検証・配置・regsvr32・「アプリと機能」への表示・辞書とモデル（約 150MB）の取得 | windows-verification.md C-1、real-hardware-checklist.md §8.7 | H-42 |
| 23 | 入力方式が使うアカウントに登録されるか（別アカウントの UAC では昇格側に掛かる可能性） | decisions.md H-42、windows-verification.md C-1 | H-42 |
| 24 | Program Files の DLL が UWP・保護されたアプリで読み込まれるか（ALL APPLICATION PACKAGES の ACL） | decisions.md H-42、windows-verification.md D-2、real-hardware-checklist.md §8.7 | H-42 |
| 25 | `irm \| iex`（PowerShell 5.1 と 7）: latest.json の取得、SHA-256、SmartScreen が出ない、一時フォルダの削除、不一致で停止 | decisions.md H-42、windows-verification.md C-2、real-hardware-checklist.md §8.7 | H-42 |
| 26 | ブラウザで取得した zip の SmartScreen / Defender の文言と、windows-install.md の回避手順 | windows-verification.md C-3、real-hardware-checklist.md §8.7 | H-42 |
| 27 | 上書き更新: .old への退避、再起動後の切り替え、旧版 imserver の混在、次回の .old の片付け | decisions.md H-42、windows-verification.md F-1、real-hardware-checklist.md §8.7 | H-42 |
| 28 | 更新通知: 旧版 imserver のトースト、update_check.json の 24 時間の間引き、`auto_check=false` で出ない | windows-verification.md F-1、real-hardware-checklist.md §8.7 | H-42 |
| 29 | アンインストール: regsvr32 /u、ファイル削除、.old の再起動後削除、残骸なし、`-RemoveUserData`、`-Force`、既定でユーザーデータが残る | decisions.md H-42、windows-verification.md F-2、real-hardware-checklist.md §8.7 | H-42 |
| 30 | VC++ 再頒布可能パッケージの無い環境での DLL と imserver | windows-verification.md C-1・F-3、real-hardware-checklist.md §8.7 | H-42 |
| 31 | ARM64 Windows で x64 の DLL が読み込まれるか | windows-verification.md F-3、real-hardware-checklist.md §8.7 | H-42 |
| 32 | Authenticode 署名（Secrets 登録時のみ）: signtool verify、署名済みの読み込み、タイムスタンプ | windows-verification.md F-3、real-hardware-checklist.md §8.7 | H-42 |
| 33 | DLL の CRT 静的リンク（dumpbin で vcruntime140.dll への依存が無いか） | windows-verification.md 段階 A・付録 | — |
| 34 | 実機で確認できたら release を `-f prerelease=false` にする（それまで `irm \| iex` は 404） | decisions.md H-42、progress.md Windows 節 | H-42 |
| 35 | imserver をブローカー（名前付きパイプ）へ集約するかの判断に必要な実測と試作 | decisions.md H-43 §4、real-hardware-checklist.md §8.6 | H-43, H-39 |

## CI（Actions の実行が必要なもの）

| No. | 何を確かめるか | 参照先 | 決定 |
|---|---|---|---|
| 1 | ci.yml の core（3 OS の build/test）・windows-tsf・windows-scripts が通るか（ci.yml は手動実行のみ） | progress.md CI 節・Windows 節、decisions.md H-40, H-42 | H-40, H-42 |
| 2 | Swift のビルド・テスト（ios-test / ios-testflight）。ローカルでコンパイルしていない Swift の変更はここで初めて検証される | decisions.md H-50, H-52, H-53, H-57, H-58, H-65, H-66, H-68, H-69, H-72、keyboard-style-spec.md 進捗 | — |
| 3 | 曖昧入力 M2（Swift）のビルド・テスト | progress.md 曖昧入力 M2 節、decisions.md H-25 | H-25 |
| 4 | macos-release.yml（Developer ID 署名・公証・sign-update・リリース作成）の手動実行（Secrets 未登録） | decisions.md H-37(5)、progress.md「macOS のリリース手順」 | H-37 |
| 5 | windows-release.yml の手動実行（publish=false）: MSVC ビルド、llama ビルド、bundle-windows、sign-update、Secrets の検査 | decisions.md H-42、real-hardware-checklist.md §8.7、windows-verification.md 段階 B | H-42 |
| 6 | インストーラのスモークテスト（continue-on-error のためログを開いて確認） | windows-verification.md 段階 B、decisions.md H-42 | H-42 |

## その他（コア・複数 OS 共通）

| No. | 何を確かめるか | 参照先 | 決定 |
|---|---|---|---|
| 1 | ライブ変換の基本動作（2 文字以上で約 228ms 後に update_preedit、1 文字では届かない） | progress.md「実機で確認してほしいこと」1、real-hardware-checklist.md §2 | G-25, G-26 |
| 2 | ライブ変換の変換品質（Space 変換との違和感） | progress.md、real-hardware-checklist.md §2 | G-25 |
| 3 | 高速タイピング時のデバウンススレッドの負荷 | progress.md、real-hardware-checklist.md §2 | G-26 |
| 4 | 60 文字超の読みのライブ変換の遅さ | progress.md、real-hardware-checklist.md §2 | G-27 |
| 5 | Ctrl+Backspace の `delete_before` をフロントで使う（未実装） | progress.md、real-hardware-checklist.md §4 | G-28 |
| 6 | reconvert の読み推定の精度と未知語の頻度 | progress.md、real-hardware-checklist.md §5 | G-29 |
| 7 | 文全体 N-best: 1 件目の LLM 再採点、2 件目以降の並び、候補選択後の矢印での文節展開 | progress.md、real-hardware-checklist.md §3 | G-30, G-34 |
| 8 | reconvert 初回のメモリ増加が macOS でも同傾向か（Windows は約 270MB で実測済み） | real-hardware-checklist.md §5 | G-29 |
| 9 | ed25519 の単体テスト（`cargo test -p nagi-server update_check`）が Windows と macOS で通るか | real-hardware-checklist.md §6 | G-31 |
| 10 | 標準 IME（macOS 標準・MS-IME・Mozc）との挙動照合、レイテンシ（p95）と nagi-eval の実機計測 | progress.md、real-hardware-checklist.md §7 | — |
| 11 | 実データ（辞書）を取得した状態での実データ依存テストと `--features llama` のテスト | real-hardware-checklist.md §1 | — |
| 12 | macOS 標準のライブ変換との詳細比較（Shift+Enter 等） | decisions.md G-39 | G-39 |
| 13 | Mozc・macOS 標準 IME との `nn` の挙動照合 | decisions.md G-8, G-40, G-48 | G-8, G-40, G-48 |
| 14 | 変換モデルの最終決定（ターゲット相当の再計測、left_context を使った評価） | decisions.md G-36 | G-36 |
| 15 | 候補拡充の見え方、LLM 込みの遅延、形式候補を学習から除外するかの判断 | decisions.md H-48 | H-48 |
| 16 | 他 IME の書き出しの実物（MS-IME・ATOK の品詞名・文字コード、macOS の plist）での取り込み | decisions.md H-50 | H-50 |
| 17 | 英語の自動判定の使い勝手（macOS・iOS・Windows） | decisions.md H-51 | H-51 |
| 18 | plan-next-features.md の各項目の「実機確認は残る」（記号・電卓、素通し・モード、辞書の取り込み、綴りの「もしかして」） | plan-next-features.md | H-49, H-50, H-55, H-60, H-70 |

## 文書どうしの食い違い（未解決）

- Windows の IS_PASSWORD（値 31）: windows-verification.md の段階 A・D・E では未チェックだが、real-hardware-checklist.md §8.5b は「定数 31 はコンパイル時に一致を確認済み」としている（実機でパスワード欄の挙動を確かめるのは残る）
- keyboard-style-spec.md §11 の決定記録の草案の見出しが「H-??」のまま（decisions.md の H-65〜H-67 に記録済みかの照合が要る）

## 追加分（2026-10-27）

| No. | 何を確かめるか | 参照先 | 決定 |
|---|---|---|---|
| 1 | 「きょう」「いま」などの日付・時刻の候補が、端末の時刻・タイムゾーンどおりに出るか（iOS・macOS・Windows） | decisions.md H-72 | H-72 |
| 2 | タイムゾーンを変えたとき（設定アプリで変更・旅行先）に、候補の日時が追従するか（`NSSystemTimeZoneDidChange`・Windows の再送） | decisions.md H-72 | H-72 |
| 3 | Windows の `GetDynamicTimeZoneInformation` で求めたずれが正しいか（夏時間のある地域を含む。型検査のみ） | decisions.md H-72 | H-72 |
