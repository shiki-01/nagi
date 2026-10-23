# 実機（Windows / macOS）テストチェックリスト

このセッション（クラウドのLinuxサンドボックス）で実装した機能は、実データ（Mozc辞書・zenzモデル）も
実OSも無い環境で開発した。GitHub Actions は ubuntu/windows/macos の3OSでビルド・テストしているが、
`--features llama`（実LLM推論）は既定でCIに含めていないため、以下は実機でしか確認できない。

各項目の実装根拠・設計判断は `docs/decisions.md` の該当G番号を参照。

## 0. 準備

```sh
git clone https://github.com/shiki-01/nagi.git
cd nagi/core
cargo xtask fetch-dict          # Mozc OSS辞書を取得（SHA-256検証あり）
cargo xtask build-dict          # data/dict/nagi-dict.bin を生成
```

LLM（ビームサーチ・reconvertの品質確認等）も試す場合、追加で:

```sh
cargo xtask fetch-model         # zenz-v3.1-small を取得（SHA-256検証あり）
```

`--features llama` でビルドするには cmake と LLVM/clang（`LIBCLANG_PATH`）が必要（Windowsは
LLVM本体のインストールが必要な場合がある。macOSは `xcode-select --install` で足りるはず）。

## 1. ビルド・テストの基本確認（実機で初めて走らせる意味がある）

```sh
cargo fmt --all --check
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CIはubuntu/windows/macosの3OSで既に通っているはずだが、**実データ（`data/dict/nagi-dict.bin`）が
無い環境で走るCIでは、実データ依存のテストは自動でskipされている**（`dictionary_backed_conversion_end_to_end`
等、テスト内で `data_dir()` が `None` なら早期return）。実機で辞書を取得した状態で同じコマンドを
実行すると、これらのテストが初めて実際に走る。

LLMも含めて確認する場合:

```sh
cargo test -p nagi-infer -p nagi-server --features llama
```

## 2. ライブ変換（G-25・G-26・G-27）

`imserver` は標準入出力でJSON-RPCを受け付ける。手元で対話的に試す一番簡単な方法は、
別ターミナルで `imserver` を起動し、標準入力に手でJSON-RPC行を打つか、簡単なスクリプトで送る:

```sh
cargo run -p nagi-server --bin imserver -- --data-dir ../data --user-dir /tmp/nagi-test
```

（`--user-dir` を指定して実ユーザーの学習データ・設定を汚さないこと。LLMも試すなら
`cargo run --release -p nagi-server --features llama --bin imserver -- --data-dir ../data --user-dir /tmp/nagi-test`）

確認したいこと:

1. `initialize` → `process_key` でかな2文字以上を打った後、**何も送らずに**約228ms
   （既定の `live_conversion_delay_ms`）待つと、`update_preedit` 通知が自発的に出力されること。
   1文字だけでは出ないこと（既定 `live_conversion_min_key_length = 2`）。
2. ライブ変換の変換結果が、同じ読みをSpaceで変換した結果と比べて違和感が無いか（実データでの
   変換品質。辞書のみ/LLM併用の両方で確認できると良い）。
3. 高速に連続してキーを打ったとき（1文字ごとにデバウンススレッドが1本立つ設計）、CPU負荷・
   メモリが問題にならないか（タスクマネージャー/アクティビティモニタで `imserver` プロセスを見る）。
4. 60文字を超える長い読みを一度に打ったとき、ライブ変換の反映が遅く感じないか（`composing_chunk_len`
   でチャンク分割しているが、「編集されたチャンクのみ再変換」という最適化はまだ無いため、
   長文では毎回チャンク全体を再変換している）。

## 3. ビームサーチ（文全体N-best、G-30）— `--features llama` 推奨

`conversion.initial_view = "sentence"`（既定）で、同音異義語を含む文を打ってSpaceを1回押す:

1. 候補窓がその場で開き、複数の文全体の変換結果が並ぶこと（`conversion.initial_view = "segment"`
   に設定すると従来どおり候補窓が出ないことも確認できると良い）。
2. **1番目の候補が、LLM併用時の通常のSpace変換（1-best）と同じ表記になっていること**
   （辞書コスト順ではなくLLM再採点済みの結果を1番目に揃える設計にした。ここが辞書コスト順に
   戻っていたら品質の後退なので要報告）。
3. 2番目以降の候補（辞書コスト順、LLM再採点なし）の並びが不自然に感じられないか。
4. 候補を選んだ状態で矢印キー（MoveFocus）を押すと、その候補の文節分割に展開されて複数文節の
   通常編集に戻ること。

## 4. 確定取消 Ctrl+Backspace（G-28）

**フロント（macOS/Windows）が無いため、ホストアプリのテキストを実際に削除する動作はまだ無い。**
`imserver` に直接JSON-RPCを送って、エンジンの機構だけを確認する:

1. かな入力→Enterで確定
2. 直後に `Ctrl+Backspace`（`keysym` は BackSpace=0xff08、`modifiers.ctrl=true`）を送る
3. 応答に `delete_before`（削除すべきUTF-16長）が入り、`preedit` が確定前の状態に戻ること
4. 確定後に他のキー（何でも良い）を1回挟んでから `Ctrl+Backspace` を送ると、何も起きず
   素通し（`consumed: false`）になること

## 5. reconvert（G-29）

`reconvert` RPCに `{"session_id": "s", "text": "<確定済みの表記>"}` を送る:

1. 一般的な単語・文で、辞書の逆引きによる読みの推定がどの程度使えるか（辞書のみ、LLM補完なし）
2. 辞書に無い表記（固有名詞・当て字等）を渡したときにクラッシュしないこと（`reading` が
   正しくなくても編集を続けられればOK）
3. **`reconvert` を初めて呼んだ直後のメモリ増加量**（逆引き索引を辞書全体から構築する）。
   **2026-09-28にWindows実機で実測済み: 約270MB**（見積もりの「数十MB」は誤りだった。
   `docs/decisions.md` G-29 追記参照）。`minimal`/`light` プロファイルでは使用後に自動解放
   するよう対応済み（`Dictionary::release_reverse_index`）。macOSでも同様に、タスクマネージャー
   相当（アクティビティモニタ）で `imserver` のメモリを `reconvert` 前後・解放後で比較し、
   同じ傾向になるか確認してほしい。

## 6. アップデート署名検証（G-31）

`config/update-public-key.txt` の公開鍵は本番用（2026-10-22 に差し替え済み）。配布フローは
`macos-release.yml`（H-37）。リリース前に実機で試せるのは暗号ロジック自体の単体テスト:

```sh
cargo test -p nagi-server update_check
```

ed25519の実装（`ed25519-dalek`）はOS依存のCライブラリを使わない純Rustのはずなので、Win/Macでも
問題なく通るはずだが、実際に確認してほしい。

## 7. その他

- 標準IME（macOS標準／MS-IME／Mozc）との挙動照合（ゴールデンテストの期待値は設計書から書いたもので、
  標準IMEとの実際の照合はまだ行っていない）
- レイテンシベンチ（p95）: 設計書の目標値（キー入力→ひらがな反映 p95 < 16ms 等）を実機で計測できると良い
- `cargo xtask fetch-model` でモデルを取得した状態で `cargo run --release -p nagi-server --features llama --bin nagi-eval -- --data-dir ../data` を実行すると、精度・レイテンシの計測結果が出る（G-11の数値は評価環境（クラウドサンドボックス）でのもので、実機での再計測が無い）

## 8. Windows フロント（TSF、H-39〜H-43）— 未検証

`frontend-windows/`（`nagi-windows`）と `frontend-windows-tsf/`（`nagi-windows-tsf`）は Linux 上で fmt / clippy / test と
`x86_64-pc-windows-gnu` の `cargo check` / `clippy` が通っただけで、Windows 実機では一度も動かしていない。
以下は Windows 10 22H2 以降 / 11 (x64) の実機で確認する項目。確認したら結果と日付を書き込むこと。
検証の進め方・コマンド・記録方法は [`docs/windows-verification.md`](windows-verification.md)。

### 8.1 ビルド・単体テスト
- [ ] `cd core && cargo test -p nagi-windows -p nagi-windows-tsf` が Windows ホストで通る
- [ ] `cargo build --release -p nagi-windows-tsf` で DLL（cdylib）ができる（MSVC ターゲット）
- [ ] `nagi-windows` の `process.rs`（`ProcessTransport`）のテストが Windows で通る（Linux では `cat` / `true` を使う `cfg(unix)` テスト。Windows 側は未検証）
- [ ] 子プロセス起動時に `CREATE_NO_WINDOW` でコンソールウィンドウが出ない

### 8.2 登録・読み込み
- [ ] 管理者権限の `regsvr32 nagi_windows_tsf.dll` が成功し、設定の「言語と地域」に入力方式として出る
- [ ] `regsvr32 /u` で登録が消える（レジストリに残骸がない）
- [ ] ホストアプリ（メモ帳・ブラウザ・VS Code・Office）で DLL が読み込まれ、アンロード（`DllCanUnloadNow`）でホストが落ちない
- [ ] DLL と同じディレクトリの imserver が起動する（配置は `%ProgramFiles%\Nagi`、H-42）。データは `%APPDATA%\Nagi\data`

### 8.3 キー入力・composition
- [ ] ローマ字入力でひらがなが composition として表示され、Enter で確定、Esc で取り消せる
- [ ] Space 変換 → 文節移動（←/→）→ 確定が通る
- [ ] `OnTestKeyDown` だけが呼ばれて `OnKeyDown` が来ない場合の挙動（エンジン状態が先に進む問題）の発生頻度
- [ ] 入力 250ms のタイムアウト（実測なしの暫定値）が妥当か。重い変換でもキーを取りこぼさない
- [ ] エンジン未接続時の素通し（composition 中だけ食う）で入力が壊れない。imserver を強制終了しても自動再起動し、`restore_session` で preedit が復元される
- [ ] スリープ復帰後の再起動
- [x] ~~ライブ変換・LLM 結果の差し替えが「次のキー入力まで画面に出ない」~~ → H-41 で隠しウィンドウ + PostMessage により解消（実機の確認は 8.5b）
- [ ] キャレット位置の設定（`ShiftEnd`）。確定後にカーソルが文末に来る

### 8.4 キー割り当て
- [ ] 半角/全角（`VK_KANJI` / `VK_OEM_AUTO` / `VK_OEM_ENLW`）でモードが切り替わる
- [ ] 変換（`VK_CONVERT`）・無変換（`VK_NONCONVERT`）・カタカナひらがな（`VK_KANA`）の動作
- [ ] `VK_IME_ON` → HIRAGANA、`VK_IME_OFF` → EISU_TOGGLE の割り当てが OS 側の IME オン/オフと食い違わない
- [ ] `VK_OEM_BACKTAB` が実際にどのキーに当たるか（資料が乏しい。HIRAGANA_KATAKANA に割り当てた判断が妥当か）
- [ ] JIS / US 配列の両方。AltGr 配列で `@` などの記号が失われない（Ctrl+Alt 併用時に text を残す判断）
- [ ] 修飾キー単独の押下・離上（`flags_changed`）。押下を食ったキーの離上を食わない挙動

### 8.5 OS 非依存ロジック（H-40。Linux の単体テスト済み。実機の値・見た目で確認する）
- [ ] 候補ウィンドウの位置: キャレット行の下に出る。画面下端（タスクバーの上）では上に反転する。複数モニター・DPI 違い・縦に長い候補で作業領域からはみ出さない
- [ ] 候補ウィンドウの寸法: 文字幅の計測（`TextMeasure`）と行の高さ・余白が見た目で窮屈でない（DPI 倍率の掛け方）。ページ表示「2/5」の位置
- [ ] ページング: 候補が `page_size` を超えたとき、選択に追従してページが替わる。数字キーで選べる（キーの解釈はコア）
- [ ] 出所ラベル（学習・AI・辞書・もしかして等）の表示
- [ ] キャレット矩形が取れないアプリでマウス位置に出る（`anchor_rect`）
- [ ] `set_environment`: AC/電池の切替、バッテリー節約の切替で `get_status` のプロファイルが変わる。電池の無いデスクトップ PC で電池駆動と誤判定されない
- [ ] メモリ逼迫の推定閾値（利用可能 5%/10%・300MB/800MB）が妥当か（実測なし）。メモリを圧迫したときだけ light/minimal に落ちる
- [ ] サーマルの推定（周波数 / 上限の比 85%/60%/40%）。バッテリー節約中や省電力プランで誤って軽いプロファイルに落ちない（節約が有効または不明なら推定しない仕様）
- [ ] `set_context`: 入力開始時にキャレット直前のテキストが送られ、次語候補・文脈が効く。取得できないアプリで前回の文脈が維持される。パスワード欄（`InputScope` の `IS_PASSWORD` = 31 が `windows` クレートの定数と一致するか）で周辺テキストを読まず `secure_input: true`、学習・ログが止まる
- [ ] 表示属性: 通常 = 細い実線、注目文節・選択 = 太い実線、確定済み = 点線、スタイル変換 = 破線、保留ローマ字 = 点線。ホストアプリが属性を無視する場合の見え方（MS-IME 風に未変換 = 点線へ変える要否）
- [ ] 通知の文言（エンジン停止・minimal 移行・復元失敗・update_available）と 30 秒間引き、モード表示（あ / A）

### 8.5b OS 層（H-41。型検査のみ。実機で一度も動かしていない）
- [ ] 候補ウィンドウ: Space 変換で表示され、**フォーカスを奪わない**（入力中のアプリのキャレットが消えない・タイトルバーが非アクティブにならない）。選択行のハイライト、番号、出所ラベル、ページ「2/5」が崩れない。DPI 100/150/200% と、DPI の違う複数モニター（Per-Monitor 対応ホストと非対応ホストの両方）で大きさ・位置が合う。タスクバー付近で上に反転する
- [ ] キャレット矩形（`GetTextExt`）: メモ帳・ブラウザ・VS Code・Office・ターミナル・UWP で候補ウィンドウが composition の直下に出る。取れないアプリでマウス位置に出る
- [ ] 非同期通知の即時反映: ライブ変換・LLM 結果の差し替えが次のキーを待たずに画面に出る。差し替え直後のキー入力と順序が崩れない（`TF_ES_ASYNCDONTCARE` の編集セッションと同期セッションの順序）。通知が別ウィンドウ（別スレッド）の入力に混ざらない
- [ ] スリープ復帰: 復帰後にエンジンが再起動し、`restore_session` で preedit が復元される（メッセージ専用ウィンドウの `WM_POWERBROADCAST` が届くか、届かなくてもタイマーの検出が働くか）。二重に再起動しない
- [ ] 環境の報告: AC/電池・バッテリー節約の切り替えが約 30 秒以内（電源通知が届けば即時）に `get_status` のプロファイルへ反映される。`MhzLimit` を使うサーマル推定がアイドル時に誤って軽いプロファイルへ落とさない
- [ ] `set_context`: 文字入力の開始時だけ文書を読む（矢印・ショートカット・修飾キーで読まない）。パスワード欄（`InputScope` が `IS_PASSWORD`。定数 31 はコンパイル時に一致を確認済み）で周辺テキストを読まず `secure_input: true` を送る。アプリ ID に実行ファイル名（`chrome.exe` 等）が入る
- [ ] 表示属性: 通常 = 細い実線、注目文節 = 太い実線、確定済み・保留 = 点線、スタイル変換 = 破線がホストに反映される。確定後に下線が残らない。属性を無視するアプリでも入力は壊れない。`IEnumTfDisplayAttributeInfo` の終端（`S_FALSE`）で列挙するアプリが固まらない。アクティブ化されないプロセスで imserver が起動しない
- [ ] 入力モード: タスクトレイの表示が「あ / A」で切り替わる。英数キー・半角/全角キー・トレイのクリックの往復でモードが反転し続けない（`SetMode` の送りすぎ。入力中の文字列が意図せず確定されない）。英数 = IME 閉のとき、アプリ側が IME オフ扱いで困らない。Activate 直後・エンジン再起動後に OS とコアのモードが揃う
- [ ] 再変換: 確定済みテキストを選択して変換キー / かなキーで composition になる（コアの `reconvert_selection`）。Office など `ITfFnReconversion` を呼ぶアプリで動く。待ちきれないとき（初回の辞書の逆引き索引）の通知と取り消し。選択が空・200 文字超のとき何もしない
- [ ] 通知トースト: エンジン停止・minimal 移行・復元失敗・`update_available` が右下に約 4 秒出てフォーカスを奪わない。同文言は 30 秒間引かれる。背面のプロセスの通知が出ない
- [ ] アイコン: `regsvr32` で DLL と同じディレクトリに `nagi.ico` が作られ、設定の「言語と地域」とタスクトレイに「N」のアイコンが出る。`regsvr32 /u` で削除される。書き込めないディレクトリ（Program Files 以外の権限）での挙動

### 8.5c H-43 で追加した OS 層（型検査のみ。実機で一度も動かしていない）
- [ ] 言語バーのボタン: 従来の言語バー（設定の「言語バー」を有効にした環境、またはデスクトップ言語バー）に「あ / A」のボタンが出る。`ITfLangBarItemMgr` を `ITfThreadMgr` の `QueryInterface` で得られる（`LangBar::new` が失敗しない）。ホスト切り替え・Deactivate でボタンが残らない（`RemoveItem`）。トレイの「あ / A」と二重に出ないか
- [ ] ボタンの操作: 左クリックでひらがな ⇄ 英数が切り替わり、表示（アイコン・文字・ツールチップ）が更新される。右クリックでメニュー（ひらがな / 英数・現在にチェック・状態の行・「設定ファイルを開く…」）が出る（`BTN_BUTTON` で `InitMenu` が呼ばれるか）。メニューの選択でモードが変わる。入力中の文字列が意図せず確定されるのは `SetMode` の仕様どおりか
- [ ] ボタンの表示とトレイ・英数キー・半角/全角キーでの切り替えの同期（往復して反転し続けない）。エンジン停止中（接続前）のメニュー（「変換エンジンに接続していません」）と、開いたときの待ち（`get_status` の同期呼び出し、最大 250ms）が気にならないか
- [ ] `GetIcon` の `HICON` を言語バーが破棄するか（長時間・多数回のモード切り替えで GDI オブジェクトが増え続けないか。タスクマネージャーの GDI オブジェクト数）。DPI 150/200% でアイコンが滲まない
- [ ] 「設定ファイルを開く…」: `config.toml`（`get_status` の `config_path`）がメモ帳で開く。ファイルが無いときの作成の確認。`config_path` が無いとき項目が灰色
- [ ] `GetReconversion`: `ITfFnReconversion` を呼ぶアプリ（Office 等）で再変換の候補一覧が出て、選ぶと範囲が置き換わりキャレットが末尾に来る。複数文節の範囲で、選んだ候補以外の部分（残りの文節）が失われない。アプリが文書ロックを持つ最中に呼んだ場合に失敗して固まらない。呼んだ後にコアが変換中のまま残らない（直後の入力が通常どおり）。選択が空・200 文字超・入力中は失敗する。待ちきれないとき（初回の逆引き索引）の通知
- [ ] `THIRD_PARTY_LICENSES_WINDOWS.md`（104 crates）が実際の Windows のビルド（MSVC・`--features llama`）の依存と一致する（`cargo tree --target x86_64-pc-windows-msvc` を Windows 上で実行して比較。llama.cpp のビルド用の crate が漏れていないか）。zip の `THIRD_PARTY_LICENSES.md` が Windows 用になっている

### 8.6 設計の再検討事項
- ホストプロセスごとに imserver を起動する方式のメモリ・起動時間。名前付きパイプ + 常駐ブローカーへの変更要否（H-39）。**判断に必要な実測項目と設計案は decisions.md H-43 §4**（ホスト 1/5/10 個のメモリ合計、`initialize` の所要、パイプ往復の増分、`learning.tsv` の取りこぼし、AppContainer からの接続可否、再接続の所要）

### 8.7 配布・インストーラ・SmartScreen・更新（H-42。Linux では PowerShell の構文と `SHA256SUMS` の検証までしか確認していない）
- [ ] `windows-release.yml` を `-f publish=false` で実行して通る（windows-latest での MSVC ビルド、`LIBCLANG_PATH`、llama.cpp のビルド、DLL の CRT 静的リンク、`bundle-windows`、`sign-update`）。インストーラのスモークテスト（regsvr32 の登録と解除）の結果を Actions のログで確認する（失敗は警告扱い）
- [ ] 展開した zip で `powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1`: `SHA256SUMS` の検証、UAC の確認、`C:\Program Files\Nagi` への配置、`regsvr32` の登録、「アプリと機能」の項目、辞書・モデルの取得（`%APPDATA%\Nagi\data`、約 150MB）が通る。標準ユーザー（UAC で別アカウントの資格情報を入力）の場合に、入力方式が使うアカウントに登録されるか
- [ ] Program Files の DLL が UWP / 保護されたアプリ（設定アプリ・Edge の一部・Microsoft Store アプリ）でも読み込まれる（`ALL APPLICATION PACKAGES` の読み取り権限）
- [ ] 更新（上書きインストール）: 入力方式の使用中に再実行して `.old` への退避が通り、サインアウト / 再起動後に新しい版にそろう。`.old` が次回に片付く。動作中の imserver（旧版）が混在して問題を起こさない
- [ ] `uninstall.ps1`: 登録が消え（`regsvr32 /u`）、ファイルが消える。使用中のファイルが再起動後に消える。`-RemoveUserData` の確認（`yes`）と `-Force`。既定ではユーザーデータが残る。「アプリと機能」の項目が消える
- [ ] `irm https://nagi.shiki-01.com/install.ps1 | iex`（PowerShell 5.1 と 7）: latest.json の取得、SHA-256 検証、SmartScreen が出ないこと。`NAGI_SKIP_DATA=1`。prerelease の間は 404 になる（想定どおり）。公開前に `web.yml` の取り込み（`static/updates/windows/latest.json`）を確認する
- [ ] ブラウザで取得した zip の SmartScreen / Defender の警告の出方（Edge・Chrome）と、`docs/windows-install.md` の回避手順（zip のブロック解除、「詳細情報」→「実行」）で実際に進める。`.ps1` の MOTW 付きでも `-ExecutionPolicy Bypass` で実行できる
- [ ] Authenticode 署名（Secrets を登録した場合）: `signtool verify /pa` が通り、署名した DLL / exe が読み込まれる。タイムスタンプの取得
- [ ] 更新の通知: 旧版の imserver が `/updates/windows/latest.json` を取得し、新版があれば右下のトーストが出る（文言「PowerShell でインストールコマンドを…」）。`update_check.json` による 24 時間の間引き
- [ ] VC++ 再頒布可能パッケージが無い環境（クリーンな Windows）で、DLL（静的 CRT）は読み込まれ、imserver は警告のうえ起動しない / する（imserver は動的 CRT）
- [ ] ARM64 Windows（エミュレーション）で x64 の DLL がホストに読み込まれるか（現状は未対応の扱い。`install.ps1` は ARM64 を拒否する）
