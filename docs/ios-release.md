# iOS リリース準備（TestFlight 外部テスト / App Store）

iOS 版 Nagi を TestFlight 外部テスト → App Store に出すための準備資料。2026-10-14 時点のコード・docs を読んで作成した。
事実はコード・docs で確認できたものだけを書き、確認できなかったものは「未確認」と明記する。
Apple 側の要件（審査ガイドライン、スクリーンショット寸法、年齢区分の質問票など）は改定されるため、**提出時に App Store Connect の画面と最新のガイドラインで再確認すること**（本書は Apple の文書を参照して書いたものではなく、記憶に基づく部分がある。該当箇所に「要確認」を付けた）。

関連: `docs/mobile-design.md` §5.2・§6、`docs/ios-device-checklist.md`、`docs/ios-settings.md` §2.6・§2.7、`web/README.md`（App Store Connect 申告との整合メモ）、`docs/decisions.md` H-15・H-18、`NOTICE`、`.github/workflows/ios-testflight.yml`。

---

## 0. 要修正（提出前に直すべき点。本書では修正していない）

コード・設定・文面の確認で見つかった問題。重い順。

### 0.1 PrivacyInfo の宣言漏れ（Required Reason API）

> **対応済み（ビルド確認待ち）: 両 PrivacyInfo に UserDefaults `CA92.1` を追加（`1C8F.1` は維持）。`AppleKeyboards` の読み取りは削除済み（§0.4 前の行参照）。他カテゴリの宣言の過不足は無し。ITMS-91053 メールでの最終確認は残る。**

宣言の現状（`frontend-ios-spike/App/PrivacyInfo.xcprivacy` と `frontend-ios-spike/Keyboard/PrivacyInfo.xcprivacy` は同一内容）:
`NSPrivacyTracking=false` / 収集データなし / UserDefaults `1C8F.1` / FileTimestamp `C617.1`。

`frontend-ios-spike/` の Swift を grep して実使用と突き合わせた結果:

| API カテゴリ | 実使用（file:line） | 宣言 | 判定 |
| --- | --- | --- | --- |
| UserDefaults（App Group `suiteName:`） | `Keyboard/KeyboardViewController.swift:29`、`Shared/KeyboardLayout.swift:534`、`Shared/DataPaths.swift:192,196`、`Shared/DiagLog.swift:20`、`App/SettingsView.swift:72`、`App/OnboardingView.swift:38` | `1C8F.1`（同じ App Group 内での共有） | 適合 |
| UserDefaults（**`.standard`**） | `Keyboard/KeyboardViewController.swift:355,366`（Probe の実行中マーカー `nagi.keyboard.runningProbe`）、`App/ContentView.swift:20`（`@AppStorage("nagi.onboarding.completed")`＝標準の UserDefaults） | `1C8F.1` のみ | **宣言漏れ**: アプリ自身だけが読み書きする値には `CA92.1` が必要。**ホストアプリ・キーボード拡張の両方**の PrivacyInfo に `CA92.1` を追加する（拡張側は `KeyboardViewController.swift:355,366` が根拠） |
| UserDefaults（システムの設定の読み取り） | `App/OnboardingView.swift:41`（**削除済み**。旧: `UserDefaults.standard.array(forKey: "AppleKeyboards")`。設定アプリの有効キーボード一覧という**非公開キー**） | 該当する理由なし | **グレー**: 自アプリの値ではないので `CA92.1` の趣旨（アプリ自身の情報）に当たらない。審査で指摘されるリスクがある。`docs/ios-device-checklist.md` §3 でも「iOS 16 以降でも効くか」が未確認の補助判定なので、**削除してマーカー（`keyboardLastSeen` / `fullAccessLastSeen`）だけで判定する**のが安全（ユーザー判断） |
| File timestamp | `Keyboard/KeyboardModel.swift:301-303`（`attributesOfItem` の `.modificationDate`。config.toml の更新検知）、`Shared/UserDictStore.swift:369`（`setAttributes([.modificationDate:])`）。`attributesOfItem` の `.size` は `Shared/DataPaths.swift:162`、`App/DataStatusView.swift:35`、`App/ContentView.swift:330,436` | `C617.1`（アプリ／App Group コンテナ内のファイル） | 適合（対象は App Group・アプリのコンテナ内のファイルのみ）。**ただし** `App/UserDictionaryView.swift:209` の `resourceValues(forKeys:[.fileSizeKey])` はファイル App から選んだ外部ファイルが対象で、サイズのみ（タイムスタンプ API ではない）ので宣言不要と判断した |
| システム起動時刻 / ディスク容量 / アクティブキーボード | grep（`systemUptime`・`mach_absolute_time`・`statfs`・`volumeAvailableCapacity`・`activeInputModes` など）で Swift 側のヒットなし | なし | 適合（Swift 側） |

静的ライブラリ（`NagiCore.xcframework` = `libnagi_ffi.a`、Rust コア + llama.cpp + ggml）側の使用は grep では確定できない。確認できたもの:

- llama.cpp `llama-mmap.cpp:204` が `fstat`（モデルの mmap。File timestamp カテゴリの API）。対象はコンテナ内のモデルなので `C617.1` に収まる
- Rust コア `core/server/src/setup.rs:203-216` が `std::fs::metadata(..).modified()`（config.toml・辞書の更新検知。内部は `stat`）。`C617.1` に収まる
- `std::time::Instant` / `SystemTime` を使用（`runtime.rs` `pipeline.rs` ほか）。ガイドの「システム起動時刻」カテゴリは `mach_absolute_time` 等の直接呼び出しが対象。Rust 1.89 の `Instant` がどのシステムコールを使うかは**未確認**
- 対処: CI でアーカイブしたバイナリに対し `nm -u` で `stat` `fstat` `statfs` `getattrlist` `mach_absolute_time` `clock_gettime` などの未解決シンボルを確認し、宣言と突合する。TestFlight にアップロードした際の Apple からの「ITMS-91053 Missing API declaration」メールが最終的な確認になる（TestFlight アップロード後に届く）

### 0.2 輸出コンプライアンス（`ITSAppUsesNonExemptEncryption: false`）の根拠が弱い

> **対応済み（コア側。選択肢 1 を採用）: `nagi-server` に cargo feature `update-check`（既定 ON。`ureq` と更新確認・配布物取得の HTTP 部分）を追加し、`nagi-ffi` は `default-features = false` で依存する。`cargo tree -p nagi-ffi -e normal`（`--target aarch64-apple-ios` 含む、`--features llama` あり・なし）に `ureq` / `rustls` / `ring` は含まれない。feature 無効時は `update.auto_check = true` でも何も起きない（更新確認スレッドを起動せず、通知もエラーも出さない）。更新確認が必要なら本体アプリ側で `URLSession`（OS の TLS）を使う。**
>
> **根拠メモ（バイナリに残る暗号関連クレート）**: `ed25519-dalek`（署名検証）と `sha2`（SHA-256）は、配布物・マニフェストの**認証とハッシュ**にだけ使う（データの暗号化・復号はしない）。Apple の輸出コンプライアンスの質問で「暗号化」に当たるかの最終判断は申告者（ユーザー）の責任で、免除区分の適用は提出時に要確認。TLS 実装が入らなくなったことで、`false` の根拠は「通信・データ暗号化の実装を持たない。残るのは認証・ハッシュ用途のみ」と説明できる。最終バイナリでの確認（`nm`/`strings` で `rustls`/`ring` が無いこと）は CI のビルド後に残る。

- `frontend-ios-spike/project.yml:61` のコメントは「独自の暗号化を使わない」だが、`nagi-ffi` → `nagi-server`（`core/server/Cargo.toml`: `ureq`（rustls + ring の TLS）・`ed25519-dalek`・`sha2`）に依存しているため、**XCFramework に TLS 実装（rustls/ring）が静的リンクされうる**（`core/Cargo.lock` に `rustls` `ring` `ureq`）。使われるのは `core/server/src/update_check.rs`（更新確認）で、iOS では `ProductSession.swift:126` が生成する `config.toml` に `auto_check = false` を書いて**実行はしない**が、コードがバイナリに入るかどうかは別
- Apple の質問は「OS の暗号機能のみ」なら免除、「OS 以外の標準暗号アルゴリズムを使う」場合は免除外（自己分類・年次報告が必要になりうる）。ed25519 署名検証・SHA-256 は認証・ハッシュ用途で、一般に免除の区分に入る可能性が高いが、TLS 実装の同梱は別問題。**詳細は要確認**
- 取れる選択肢（ユーザー判断。コアの変更を伴うので本書では触っていない）:
  1. iOS ビルドでは `nagi-server` の `ureq` を feature で外す（更新確認は本体アプリ側で `URLSession` を使う設計にすれば、OS の TLS のみになり `false` の根拠が成り立つ。`mobile-design.md` §6.3 の「更新確認は本体アプリ」とも一致）
  2. 現状のまま、バイナリ内の TLS を考慮して申告を `true` にし、免除区分と年次自己分類報告の要否を調べる
- 現状の `false` を維持するなら、少なくとも `nm`/`strings` で `rustls`/`ring` のシンボルが最終バイナリに残らないことを確認する
- なお `ITSAppUsesNonExemptEncryption` はホストアプリの Info.plist にだけある（`project.yml:61`）。キーボード拡張の Info にはない（App Store Connect の判定はアプリ単位なので問題にならない想定。要確認）

### 0.3 「フルアクセス無しでは漢字変換できない」の説明が、コードと一致しない

> **文面のみ対応済み（ビルド確認待ち）: `/privacy` `/support` `DataStatusView` `AboutView` `OnboardingView` の断定を「端末により異なる場合があり、辞書とモデルの利用に許可が必要になる場合がある」に弱めた。実機確認（4.4.1）は未了。トップページ `web/src/routes/+page.svelte:102` に同種の断定が残る。**

`/privacy` §4、`/support`、`DataStatusView.swift:165`、本書 §1.4 の「許可しない場合はひらがな・カタカナ変換のみ」という説明について:

- `Keyboard/KeyboardViewController.swift` は `hasFullAccess` を**マーカー書き込み（:32）にしか使わない**。変換機能をフルアクセスで切り替える分岐は無い
- CI は辞書（`nagi-dict.bin`、約 53MB）を `Resources/data` としてアプリ**と拡張の両方**に同梱し（`project.yml` の両ターゲットの `sources` に `Resources/data`、workflow の「辞書をアプリのリソースへ配置」）、`DataPaths.resolveDataDir` は App Group が無ければ**自分のバンドルの辞書**を使う（`DataPaths.swift:20-26`）
- つまり、フルアクセス無しでも拡張は自分のバンドル内の辞書で変換できる見込みがある（**実機で未確認**。`docs/ios-device-checklist.md` にこの項目はない）。モデル（App Group に取り込む）は、App Group のコンテナが拡張から見えるかどうか次第
- 審査ガイドライン 4.4.1 は「フルアクセスを要求しなくても機能すること」を求める（§1.5）。**実機で「フルアクセス OFF での漢字変換」「OFF での App Group の読み取り」を確認し**、(a) 文面を実態に合わせる、(b) 実態が「OFF だと変換できない」なら 4.4.1 のリスクとして扱う、のどちらかにする。判明するまで、`/privacy` `/support` の断定的な文面は変更しない（確認後に修正）

### 0.4 表示名・文面の不整合

> **対応済み（ビルド確認待ち）: `CFBundleDisplayName` を両ターゲット `Nagi` に変更、`ALTAppGroups` を `project.yml` から削除（`AppGroupResolver` は固定 ID にフォールバック）、`TARGETED_DEVICE_FAMILY: "1"`（iPhone のみ）を両ターゲットに設定。`bundleIdPrefix` は未変更。**

- `project.yml:57` `CFBundleDisplayName: Nagi Spike`、`project.yml:135` `Nagi Spike Keyboard`。ホーム画面・設定の「キーボード」一覧に出る名前が開発用のまま。`/support` は「一覧から「Nagi」を選びます」と書いているので不一致。提出前に `Nagi`（拡張は `Nagi` など、一覧で分かる名前）にする
- `project.yml:74,138` `ALTAppGroups`（SideStore 用の名残。H-15 で SideStore は使わない）。`AppGroupResolver.swift:57` が読んでいる。無害だが、提出前に削除してよいか確認する
- `project.yml:4` `bundleIdPrefix: com.shiki.inputmethod`（macOS 用の名残。ターゲットは `PRODUCT_BUNDLE_IDENTIFIER` で個別指定済みなので実害なし）
- ターゲットデバイスは明示されておらず（XcodeGen の既定で iPhone + iPad）、`UISupportedInterfaceOrientations` は iPad のマルチタスク向けの 4 方向になっている。iPad のスクリーンショット（13 インチ）が必要になる。iPhone 専用にするか判断が要る（§5）

### 0.5 ライセンス表示の不足（配布物に辞書を同梱する場合）

> **対応済み（ビルド確認待ち）: ルートの `NOTICE` と `THIRD_PARTY_LICENSES.md`（`cargo xtask gen-licenses` で生成。nagi-ffi の依存閉包 58 crates の名前・バージョン・SPDX・リポジトリ。依存を変えたら再生成してコミット）をホストアプリのリソースに同梱し、About > ライセンスからスクロール表示（`NagiBundledTextView`）。依存クレート・llama.cpp の一覧は生成済み（コア側）。**

- `App/AboutView.swift` の「辞書・モデルの出典」は**要約のみ**で、IPAdic の著作権表示（NAIST の Copyright 文）と免責条項、Mozc（BSD-3-Clause、Google の Copyright）の表示が無い。IPAdic は「著作権表示と免責条項を**すべての複製に含める**」ことが条件（`NOTICE`）。辞書を IPA に同梱して配布するなら、アプリ内から NOTICE の全文（少なくとも辞書・ローマ字表の部分）を読めるようにする
- Rust 依存クレート・llama.cpp（MIT）のライセンス表示も、バイナリ配布の義務を満たすために一覧が要る（現状 `NOTICE` はデータ資産のみ。`cargo about` 等で生成する案。要確認）

### 0.6 診断ログ

- **決定済み（H-22）**: TestFlight 期間中は入力内容を含めて既定 ON、App Store 版は既定 OFF・内容を含めない（パスワード欄は伏せる）。`CLAUDE.md` に期間限定の例外として明記。外部テスターへの開示は `/privacy` §7 と About のとおり。以下は判断前の状況メモ。
- `Shared/DiagLog.swift:30-36`: TestFlight（`sandboxReceipt`）では既定 ON、App Store 配布では OFF。ログは**入力内容を含む**（`docs/ios-device-checklist.md` §0 (a) が未決のまま）。`/privacy` §7 は「TestFlight 期間中の暫定」と開示している。外部テスターに配る前に、(a) の判断（入力内容を含めるか）と、`CLAUDE.md` の「ログに入力内容を残さない」との関係を決める。外部テスターは第三者なので、内部テストより重要

---

## 1. 提出前チェックリスト

`[ ]` は未完、`[x]` はコード・docs で確認済み。

### 1.1 ビルド・メタデータ

- [x] アプリアイコン: `App/Assets.xcassets/AppIcon.appiconset` に 1024x1024 の PNG が 3 枚（通常 `icon-light.png`・ダーク `icon-dark.png`・ティント `icon-tinted.png`）。`ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon`。ソースは `icon-src/*.svg`
- [ ] アイコンの確認: 透過・角丸が無いこと（`icon-light.png` は RGB で透過なし＝OK）。ダーク／ティントが iOS 18 以降の外観で崩れないことを実機で見る
- [ ] 表示名（§0.4）を `Nagi` にする
- [x] バージョン: 表示バージョン（`CFBundleShortVersionString`）は `ios-vX.Y.Z` タグ、ビルド番号（`CFBundleVersion`）は `github.run_number`（workflow コメント）。ホストアプリと拡張で同じ値（`project.yml` で両方 `$(MARKETING_VERSION)` / `$(CURRENT_PROJECT_VERSION)`）
- [ ] 初回の公開版の表示バージョンを決める（例: `ios-v1.0.0`）。TestFlight 外部テスト用は `0.x` でもよいが、App Store の最初の版は 1.0.0 から。手順: `git tag ios-v1.0.0 && git push origin ios-v1.0.0`
- [ ] 提出する版は **`llama=true` でビルド**したものか（既定は `false` で LLM なし）。`gh workflow run ios-testflight.yml --ref main -f llama=true`
- [x] 対応 OS: iOS 16.1 以上（`project.yml` `deploymentTarget`）
- [ ] App Group `group.com.shiki01.nagi` と App ID 2 つ（`com.shiki01.nagi` / `com.shiki01.nagi.keyboard`）が Developer ポータルに登録済みで、App Store Connect にアプリが作成済み（workflow 冒頭の前提）。アプリ名（§3）が使えるかは作成時に分かる
- [ ] PrivacyInfo の宣言漏れの修正（§0.1）

### 1.2 輸出コンプライアンス

- 現在の設定: ホストアプリの Info.plist に `ITSAppUsesNonExemptEncryption: false`（`project.yml:61`）。これを入れておくと、TestFlight にアップロードするたびの輸出コンプライアンスの質問が省かれる
- 根拠として言えること: Nagi 自身は暗号化機能を持たない。通信するのは辞書・モデルの取得と更新確認だけ（`/privacy` §5）で、現状の iOS 版は**アプリ内に通信コードが無い**（`URLSession` / `https://` を使う箇所は `AboutView.swift` のリンクのみ。Swift 側を grep）。SHA-256 検証・ed25519 署名検証は認証・ハッシュ用途
- 根拠として言えないこと: 「バイナリに暗号化の実装が一切含まれない」。**§0.2 のとおり `rustls/ring` がコア経由で入りうる**。ここが解消するまで、`false` の根拠は未確定
- **決定済み（H-22）**: `false` を維持する。TLS は `nagi-ffi` から除外済みで、残る暗号関連は認証・ハッシュ用途のみ。最終バイナリの確認は CI のビルド後に残る

### 1.3 プライバシー申告

- [x] App のプライバシー（栄養ラベル）: 「データを収集しない」（`web/README.md` の整合メモ。入力内容・学習データ・診断ログは端末内のみ。解析・広告 SDK なし。トラッキングなし）
- [ ] 将来、辞書・モデルのアプリ内ダウンロードを実装するとき（§4）、ダウンロード先（自前サーバー / Hugging Face）のアクセスログが「収集」に当たらないかを再確認する
- [x] `PrivacyInfo.xcprivacy`: `NSPrivacyTracking=false`、`NSPrivacyTrackingDomains` 空、`NSPrivacyCollectedDataTypes` 空。ホストアプリと拡張の 2 か所にある（`project.yml` は `App` / `Keyboard` ディレクトリ全体を `sources` にしているので、リソースとして取り込まれる想定。**アーカイブの .app / .appex の中に `PrivacyInfo.xcprivacy` が入っているかは未確認**）
- [ ] Required Reason API の宣言（§0.1）。アーカイブ後に `.app` と `.appex` の両方に `PrivacyInfo.xcprivacy` が入っていることを確認する
- [x] 入力内容を外部へ送らない（コード上、通信実装なし。ただし §0.2 のコア内の更新確認は iOS では無効化。`ProductSession.swift:126`）

### 1.4 フルアクセス（`RequestsOpenAccess: true`）

- 現状: `true`（H-18。`false` だと設定アプリにトグルが出ず、ホストのセットアップ検出が成立しなかったため）
- ユーザーに示す説明（`/privacy` §4、`/support`、ホストアプリのオンボーディング）:
  - 用途は「本体アプリが共有領域に保存した辞書・モデルを読む」ことと、拡張から共有領域への書き込み（セットアップ状況のマーカー、設定の反映）
  - キーボードは通信しない。入力内容を外部へ送らない
  - 許可しない場合はひらがな・カタカナ変換のみ ← **コードの裏付けが無い。§0.3**
- 審査メモにも同じ説明を書く（§2.3）
- 触覚フィードバックにフルアクセスが要るか（`ios-device-checklist.md` §3 の触覚の項目）は実機で未確認。要る場合は用途に追記する
- [ ] フルアクセス OFF の実機動作（キー入力・かな・漢字変換・候補・設定の反映・触覚）を確認し、結果を `ios-device-checklist.md` に書く

### 1.5 キーボード拡張の審査での注意（ガイドライン 4.4.1。要確認: 記憶に基づく要約）

4.4.1 の要旨は、キーボード拡張は (1) 文字入力の機能を提供すること、(2) 次のキーボードへ切り替える手段（地球儀キー）を持つこと、(3) **ネットワークアクセスやフルアクセス無しでも機能すること**、(4) ユーザーの活動の収集は、キーボード機能の向上の目的に限ること、(5) 拡張のアクセス権を超えた機能（拡張外での動作、広告など）を持たないこと、など。Nagi の確認状況:

| 論点 | 状況 |
| --- | --- |
| 地球儀キー | 実機で確認済み（`ios-device-checklist.md` §2「地球儀での入力ソース切替」） |
| 文字入力の機能 | 12 キー・フリック、英字、空白・削除・改行。確認済み（同 §2） |
| フルアクセス無しで機能すること | **未確認（§0.3）**。審査の最大のリスク |
| データ収集の開示 | 収集なし。`/privacy`、栄養ラベル、審査メモで明記 |
| 外付けキーボード | 拡張に入力が届かないため非対応（`/support` に記載済み）。審査で「物理キーボードで動かない」と指摘された場合は、仕様と説明する |
| 学習データ | 端末内のみ。パスワード欄・機密欄で停止（`isSecureTextEntry` ほか。`ios-settings.md` §2.6）。「学習データをすべて削除」あり |
| ホストアプリの機能 | 辞書・モデルの取り込み、設定、ユーザー辞書、レイアウト、入力テスト。キーボード追加の案内あり（`OnboardingView`）。**キーボードを使わないと価値がないアプリだが、ホストアプリが「ただのラッパー」ではなく設定・辞書管理・案内を持つ**ことは審査メモで示す |
| 診断ログ・Probes | About >「開発者向け」にある（`Probes.swift`）。開発用の機能が製品版に出ている状態。App Store 版で隠すかはユーザー判断（§5） |

### 1.6 年齢区分

- App Store Connect の年齢区分の質問票（2026 年の新区分 4+/9+/13+/16+/18+ の質問形式に変わっているはず。要確認）に回答する。コンテンツ（暴力・性的表現等）はなく、ユーザー生成コンテンツの共有・Web の自由閲覧・ギャンブルもない。**4+ の見込み**だが、質問票の回答結果を見て確定する
- `/privacy` §10 に「年齢区分の表記（App Store の年齢制限の回答との整合）」の【要確認】が残っている。回答後に埋める
- 子ども向けカテゴリには入れない（`/privacy` §10 の方針と一致）

### 1.7 URL

- サポート URL: `https://nagi.shiki-01.com/support`（`web/README.md`）
- プライバシーポリシー URL: `https://nagi.shiki-01.com/privacy`
- マーケティング URL（任意）: `https://nagi.shiki-01.com/`
- [ ] **サイトが公開されていること**。デプロイ先は Cloudflare Pages に決定（`web/README.md`）だが、プロジェクト作成と DNS は未設定。**URL が 404 だと審査に通らない**
- [ ] `/privacy` の【要確認】表示（施行日・提供者名・アクセスログの扱い・年齢区分）を埋めて消す（`web/src/routes/privacy/+page.svelte`、`web/README.md` 末尾）。提供者名は個人開発者の氏名または屋号（法務確認の TODO あり）
- [ ] `/support` の問い合わせ先は GitHub Issues のみ（アカウントが必要）。App Store の「連絡先」とは別に、**メールアドレスなど GitHub を使わない窓口が要るか**はユーザー判断（§5）
- [ ] `/support` の「現在は TestFlight で検証中の開発版」の文言は、App Store 公開時に直す

### 1.8 その他

- [ ] 「部分変換で残りが二重に出る」不具合（U-1。`ios-device-checklist.md` §1）が未解決。`/support` に既知の不具合として記載済み。公開前に直すか、機能として説明に出さない（§3）
- [x] 既定モデルの判断: small を基本に端末の予算で自動選択（H-22）。実機でホームの「推奨」表示と拡張の使用モデルが一致するかは未確認
- [ ] 実機での確認項目（`ios-device-checklist.md` §3）の消化。特にメモリ警告時の挙動、入力欄の切替、パスワード欄
- [ ] クラッシュ時の挙動: 拡張がメモリ超過で落ちたときに入力が失われないこと（同 §3 堅牢性）

---

## 2. TestFlight 外部テストの準備

内部テスト（App Store Connect のユーザー、最大 100 人）は審査なし。外部テスト（公開リンクまたはメール招待）は、**各ビルドの最初の外部配布時に Beta App Review** がある（要確認: 同じ版の後続ビルドは審査が省かれることが多い）。

### 2.1 事前条件

- [ ] §0.1（宣言漏れ）、§0.2（暗号化）、§0.4（表示名）を直す
- [ ] `/privacy` と `/support` を公開する（§1.7）。Beta App Review 情報にも URL を入れる欄がある
- [x] 診断ログの既定の判断（§0.6）: TestFlight 期間は入力内容を含めて既定 ON で許容（H-22）。外部テスターへの開示（`/privacy` §7、About、`/support`）が実装と整合していることだけ公開前に確認する

### 2.2 Beta App Review 情報（App Store Connect > TestFlight > テスト情報）

| 項目 | 記入案 |
| --- | --- |
| ベータ版 App の説明 | 日本語入力キーボード Nagi のテスト版です。端末内で動くかな漢字変換（辞書 + 任意でニューラルモデル）を試していただきます。入力内容は端末の外へ送信されません。 |
| フィードバック用メールアドレス | **（ユーザー記入）** |
| マーケティング URL / プライバシーポリシー URL | `https://nagi.shiki-01.com/` / `https://nagi.shiki-01.com/privacy` |
| 連絡先（名・姓・電話・メール） | **（ユーザー記入。審査担当者が連絡する先）** |
| サインイン情報 | 不要（アカウントなし）。「サインイン情報が必要」のチェックは外す |
| 新機能のテスト内容（テスターへの案内） | §2.4 の文面 |

### 2.3 レビュー用メモ（審査担当者向け。英語での記入を推奨。以下は案）

```
Nagi is a Japanese input method (custom keyboard extension). The app has no account, no ads, no analytics and no network access in this build.

HOW TO ADD THE KEYBOARD
1. Open Settings > General > Keyboard > Keyboards > Add New Keyboard... and choose "Nagi".
2. Open any text field (e.g., Notes), tap the globe key, and select "Nagi". It starts in the Japanese 12-key flick layout (tap and flick the keys to type kana; candidates appear in the bar above the keys).
3. The host app also has an "Input test" tab with a text field, so you can try the keyboard without leaving the app.

FULL ACCESS ("Allow Full Access")
The keyboard requests Full Access. It is used ONLY to read the dictionary/model files that the host app stores in the shared App Group container (group.com.shiki01.nagi), and to write small setup-status markers and settings there. The keyboard does not use the network and does not transmit anything the user types. No data is collected (App Privacy: Data Not Collected). [TO CONFIRM BEFORE SUBMISSION: the keyboard remains usable with Full Access off — kana input works; state here exactly what degrades.]

DICTIONARY AND MODEL
- The system dictionary is bundled with the app. Conversion works with the dictionary alone: type "きょうはいいてんきですね" and tap a candidate. No setup beyond adding the keyboard is needed. [CONFIRM: is the dictionary used without tapping "Copy dictionary from bundle" in the host app Home tab? If not, add that step here.]
- The optional neural conversion model (a ~20-70 MB .gguf file, CC BY-SA 4.0) is NOT bundled. Importing it is optional and is done from the host app (Home > "Import model"). Without a model, the keyboard runs in dictionary-only mode and is fully functional. You do not need to import a model to review the app.
- Licenses of the dictionary and model are shown in the host app: About > Dictionary and model credits.

PASSWORD FIELDS
In password fields, the keyboard does not learn, does not read surrounding text, and does not log.

EXTERNAL KEYBOARDS
Hardware keyboards are not supported by third-party keyboard extensions; use the on-screen keyboard.
```

（`[...]` の箇所は、§0.3 の実機確認とコードの確認が済んでから埋めて、削除する。確認できていないことを審査メモに断定で書かない。）

### 2.4 テスターへの案内（TestFlight の「テスト内容」。日本語）

```
Nagi（日本語キーボード）のテスト版です。

【キーボードの追加】
設定 > 一般 > キーボード > キーボード > 新しいキーボードを追加 > Nagi。文字入力できる画面で 🌐 を押して Nagi に切り替えます。アプリのホームにも手順があります。

【フルアクセス】
漢字変換に使う辞書・モデルを共有領域から読むために「フルアクセスを許可」をお願いしています。キーボードは通信せず、入力内容を外に送りません。[§0.3 の確認後に、許可しない場合の挙動を書く]

【見てほしいこと】
・12 キー・フリックの入力しやすさ、変換候補の精度
・キーボードが突然閉じる・切り替わる（メモリ不足）
・入力した文字が消えたり二重になったりする（既知: 候補の途中選択で残りが二重になることがあります）

【データ】
辞書は同梱されています。変換モデルは同梱されていません。使う場合は .gguf ファイルをホームの「モデルをインポート」で取り込みます（無くても辞書だけで動きます）。

【フィードバック】
スクリーンショットを撮ると TestFlight から送れます。診断ログ（入力テストタブ）には入力内容が含まれるため、共有前にご確認ください。
```

### 2.5 外部テストの進め方

1. `ios-testflight` を `llama=true` で実行（ビルドは App Store Connect の TestFlight に処理後に出る）
2. 輸出コンプライアンスの質問が出ないことを確認（`false` 設定時）
3. 外部グループを作成し、ビルドを追加 → Beta App Review に提出
4. 承認後、公開リンク（人数上限を設定可能）またはメールで招待
5. 外部テストでは**ビルドの有効期間は 90 日**（要確認）。期限切れ前に新しいビルドを上げる

---

## 3. App Store 掲載文の下書き（日本語のみ）

方針: **実装済みで、実機で確認済みか CI で動くことだけを書く**。未実装（スタイル変換、確定済みテキストの再変換、クリップボード履歴、アプリ内の辞書・モデル取得、ライブ変換の iOS 対応の有無）は書かない。部分変換は不具合（U-1）があるため書かない。

### 3.1 文字数制限（要確認）

| 項目 | 上限 |
| --- | --- |
| アプリ名 | 30 文字 |
| サブタイトル | 30 文字 |
| プロモーションテキスト | 170 文字 |
| 説明文 | 4,000 文字 |
| キーワード | 100 文字（カンマ区切り、空白なし） |

### 3.2 アプリ名・サブタイトル

- アプリ名: `Nagi 日本語キーボード`（13 文字。名前が他のアプリと重複していないかは App Store Connect で作成するときに分かる）
- サブタイトル: `端末内で完結する日本語入力`（13 文字）

### 3.3 プロモーションテキスト（審査なしで随時変更できる欄）

```
入力した文章を外に出さない、端末内で動く日本語キーボード。フリック入力と、辞書・学習による変換に対応しています。
```
（51 文字）

### 3.4 説明文

```
Nagi は、入力した文章を端末の外へ送らない日本語キーボードです。かな漢字変換・学習・辞書のすべてを iPhone の中で行います。通信は行わず、アカウントも不要です。

【特長】
・12 キーのフリック入力（ケータイ式のトグル入力も設定で選べます）
・ローマ字入力、英字、数字・記号、絵文字のキーボード
・入力中に候補バーを表示。候補をタップして変換できます
・選んだ候補を学習して、よく使う言葉が出やすくなります
・ユーザー辞書とスニペット（読みを入力すると本文を呼び出せます）。Mozc / Google 日本語入力などの辞書ファイルの取り込み・書き出しに対応
・キーボードの見た目や配列を JSON で作成して取り込めます（サイトのエディタで作れます）
・触覚フィードバック、片手モード、キーボードの高さやキーの大きさの調整
・パスワード欄などでは学習と文脈の取得を行いません
・学習データはアプリからいつでもすべて削除できます

【プライバシー】
・入力内容、学習データ、ユーザー辞書、設定は端末内にのみ保存されます
・解析ツールや広告は使っていません
・「フルアクセスを許可」は、本体アプリが保存した辞書・モデルを読み込むために使います。キーボードは通信せず、入力内容を外部へ送信しません

【ニューラル変換（任意）】
オープンなニューラル変換モデル（zenz）のファイルを本体アプリから取り込むと、文全体の変換の精度が上がります。モデルは別途ご用意いただくもので、アプリには含まれていません。モデルがなくても辞書だけで変換できます。

【使い方】
1. 設定 > 一般 > キーボード > キーボード > 新しいキーボードを追加 から「Nagi」を追加
2. 文字入力の画面で 🌐 を押して Nagi に切り替え
3. 本体アプリのホームに、手順と設定状況の確認があります

【ご注意】
・外付けキーボードでの入力には対応していません（iOS の仕様上、キーボード拡張には届かないため）
・変換精度の向上や機能追加を続けている開発中のアプリです。不具合は サイトのサポートページ から GitHub でお知らせください

辞書・モデルの出典とライセンスは、アプリの About で確認できます。
```

注意点:
- 「ニューラル変換（任意）」の節は、**モデルを一般ユーザーがどう入手するか**（§4）が決まるまで、書き方が変わる。現状は `.gguf` を自分で用意して取り込むため、一般向けの説明としては厳しい。アプリ内ダウンロードを実装する案（§4）を採るなら「アプリ内から取得できます」に直す（実装後に）
- 「ローマ字入力」は `qwerty-kana` レイアウトを設定で割り当てる方式（`SettingsView.swift:240`）。既定は 12 キー。書き方は「選べます」でよいが、実機での確認状況は `ios-device-checklist.md` §3
- 「トグル入力」「触覚」「片手モード」は実装済み・実機未確認の項目（同 §3）。**確認が済んだものだけ残す**
- 「Mozc / Google 日本語入力などの取り込み」は実装済みで実機未確認（同 §3 ユーザー辞書）

### 3.5 キーワード

```
日本語,キーボード,かな漢字変換,フリック,入力,IME,辞書,ローカル,オフライン,ニューラル,ローマ字,ユーザー辞書,プライバシー
```
（67 文字。アプリ名・サブタイトルに含まれる語は重複させない方が効率的なので、「日本語」「キーボード」はアプリ名に含めるなら外す余地がある。他社名・他社製品名（Google 日本語入力など）はキーワードに入れない）

### 3.6 スクリーンショット案

寸法は提出時に App Store Connect の要求に合わせる（要確認: iPhone 6.9 インチは 1320x2868 が基準で、小さい寸法は縮小で足りる場合がある。iPad に対応するなら 13 インチの画像も要る）。実機はシミュレータではなく、**実機で撮る**（キーボード拡張と LLM を含む挙動を見せるため）。

**撮影の前提**:
- 端末: iPhone（表示言語は日本語、ダークモードは別セット）。バッテリー表示・時刻はフル・9:41 に近づける（任意）
- ビルド: `llama=true`、辞書あり、可能ならモデル（small）を取り込み済み
- 入力先: メモアプリ（標準の見た目で信頼感が出る）。本体アプリのスクリーンショットはホーム・設定・レイアウトタブから
- 例文は個人情報を含まないものを使う

**構図（5 枚）**:

| # | 構図 | 撮影手順 | 見出し案 |
| --- | --- | --- | --- |
| 1 | キーボードで文章を変換中 | メモを開く → Nagi に切り替え → 「きょうはいいてんきですね」をフリックで入力 → 候補バーに変換候補が出た状態で撮る | 「端末の中で、日本語を変換。」 |
| 2 | 12 キーのフリック | フリックキーを押して 4 方向のポップアップが出た状態（`ios-device-checklist.md` §3 のフリックポップアップ） | 「フリックで素早く入力」 |
| 3 | レイアウトの選択 | 本体アプリの設定タブ、またはレイアウトタブ（プレビュー表示）で、12 キー・ローマ字・英字を見せる | 「配列も自分好みに」 |
| 4 | プライバシー | 本体アプリのホーム（セットアップ状況: キーボード追加 OK・フルアクセス）か、`/privacy` を画像化。入力内容を外部へ送らない旨 | 「入力内容は端末の外へ出ません」 |
| 5 | ユーザー辞書・スニペット | ホーム >「ユーザー辞書・スニペット」で単語を登録した画面 | 「よく使う言葉を登録」 |

注意:
- 撮影前に診断ログを OFF にして消去する（入力テストタブ）。画面に入力内容が映る場面で、ログ表示を写さない
- 候補の精度を誇張する構図（実際より良い例だけを並べる）は避ける。モデルの有無で結果が変わるため、モデルを入れた状態で撮ったなら説明文にも「モデルを取り込んだ場合」と書く
- 他社のアプリ名・ロゴが映らないようにする（メモは標準アプリなので可）
- `web/shots/` にあるのは Web サイトのスクリーンショットで、App Store 用ではない

### 3.7 その他の掲載項目

- カテゴリ: ユーティリティ（またはプロダクティビティ）。ユーザー判断
- 著作権表記: `© <年> <提供者名>`（`/privacy` の提供者名と同じにする。ユーザー記入）
- 「新機能」（バージョン情報）: 初回は不要
- App Store Connect の「App Review に関する情報」は §2.3 と同じ内容
- サポート URL・プライバシーポリシー URL: §1.7

---

## 4. 辞書・モデルの配布方法

現状（`docs/ios-settings.md` §2.7、`web/README.md`）: **取り込みのみ**。
- 辞書（`nagi-dict.bin`、約 53MB。`decisions.md`）: CI が IPA の `Resources/data` に同梱し（`ios-testflight.yml`）、ホストアプリの「辞書をバンドルからコピー」で App Group に置く。**同梱はホストアプリと拡張の両方**（`project.yml` の両ターゲットの `sources`）で、IPA に 2 コピー入る（Apple の App Thinning / 圧縮後のサイズは**未確認**）
- モデル: IPA に含めない。ユーザーが `.gguf` を用意して「モデルをインポート」で取り込む（xsmall 約 20MB、small 約 70MB。`decisions.md` H-9・H-17）
- アプリ内ダウンロード・更新確認は未実装。`/privacy` §5 は実装時の挙動として書いてある

### 4.1 比較

| 観点 | A. 取り込みのみ（現状） | B. アプリ内ダウンロード | C. On-Demand Resources（ODR） | D. IPA に同梱 |
| --- | --- | --- | --- | --- |
| 一般ユーザーが使えるか | **使えない**（`.gguf` を自分で用意する必要） | 使える | 使える | 使える |
| 初回インストールサイズ | 辞書のみ（現状約 53MB x 2 コピー） | 辞書を同梱するなら同じ。モデルは後から | 小さくできる | モデル分（20〜70MB 以上）が増える |
| 審査 | ホストアプリに「取り込み」しか無いと、審査員が機能を試すために `.gguf` を用意できず、**辞書だけで評価されて不利**になりうる。審査メモで「辞書だけで機能する」と説明（§2.3） | ガイドライン 4.2.3 / 2.5.2 の観点: 実行コードのダウンロードは禁止だが、データ（辞書・モデルの重み）は可。必要な追加リソースのサイズを開示し、ユーザーの許可を取る形が安全（要確認）。審査員の端末でダウンロード先に到達できる必要がある（ダウンロード先が落ちていると審査落ち） | Apple が配信するため審査員の環境でも確実。**ただし拡張から使えるか、App Group に移せるかは要確認**（ODR は本体アプリのバンドル側のリソースで、キーボード拡張からは見えない／パージされうると考えられる） | 審査は最も単純（全部入り） |
| 更新（再インストール不要） | 手動 | **可能**（`mobile-design.md` §6.3 の方針どおり。マニフェスト + SHA-256 + ed25519） | ODR の更新はアプリの更新に紐づく（データだけの随時更新は難しい） | アプリの更新が必要 |
| 実装コスト | 済み | 大: ダウンロード UI・再開・Wi-Fi 判定（`data.download_wifi_only`）・SHA-256・保存先・失敗時の旧データ維持・更新確認 | 中〜大: タグ設計、拡張への受け渡し | 小（CI の変更のみ） |
| プライバシー申告 | 変更なし | 通信が発生する。入力内容を含まない旨。配布元（Hugging Face 等）の第三者サーバーへの接続を `/privacy` §5 に記載済み。「データを収集しない」を維持できるかは再確認 | 変更なし | 変更なし |
| ライセンス（NOTICE） | 配布者は Nagi ではない（ユーザーが取得） | **配布元から直接取得すれば、Nagi がモデルを再配布しない**（`NOTICE` の方針「同梱せず別途取得」と一致）。自前サーバーに置くなら再配布になる | Apple のサーバーから配るので**再配布**に当たる | **再配布**に当たる |

### 4.2 ライセンスの観点（`NOTICE`）

- **zenz モデル（CC BY-SA 4.0）**: 再配布する場合は、表示（作者・出典・ライセンスのリンク・変更の有無）と同一条件での共有が必要。IPA（FairPlay で暗号化される）や ODR に含めて配布することが、CC BY-SA 4.0 の「受領者の権利行使を制限する技術的手段を課さない」条項と衝突しないか、法的な確認が要る（**未確認。結論を出していない**）。アプリ内の表示は `AboutView.swift` に要約がある
- **Mozc OSS 辞書（IPAdic + 沖縄辞書）**: IPAdic は著作権表示と免責条項を複製に含めることが条件。IPA に辞書を入れる現状は、すでに「配布」に当たる。**§0.5 のとおりアプリ内の表示が要約だけ**。BSD 3-Clause（ローマ字表）も、バイナリ配布時に著作権表示の再掲が要る
- **全体**: 同梱（D）はライセンスの義務が最も重い。取得（B）で配布元から直接ダウンロードさせる形（Nagi がファイルを再配布しない）は、`NOTICE` の「同梱せず別途取得」の方針に最も近く、CC BY-SA の share-alike の影響を製品コードに及ぼさない

### 4.3 推奨

1. **辞書**: 現状どおり IPA に同梱する（約 53MB。変換が辞書なしでは動かない＝審査員が試せる、`minimal` 経路が成立する）。ただし (a) **拡張側の同梱コピーが要るか**は検証して減らせるなら減らす（App Group のコピーのみにして IPA を小さくする。ただしフルアクセス OFF 時に拡張が自バンドルの辞書で動く性質（§0.3）を失う。**トレードオフはユーザー判断**）、(b) §0.5 のライセンス表示をアプリ内に追加する
2. **モデル**: **B（アプリ内ダウンロード）を目標**にする。配布元から直接取得して SHA-256 で検証し、サイズの確認と Wi-Fi 判定を付ける。ODR（C）は拡張から使えるか不明なので推奨しない（要検証）。同梱（D）は再配布のライセンス確認が済むまで採らない
3. 実装までのつなぎ（TestFlight 外部テスト・最初の App Store 版）: **A のまま**で、辞書だけで審査・試用できることを審査メモと説明文に明記する。説明文のニューラル変換の節（§3.4）は「任意・別途用意」と正直に書く
4. B の実装前に決めること: ダウンロード先（Hugging Face の `Miwa-Keita/zenz-v3.1-small-gguf` を直接参照するか、`shiki-01.com` に置くか。後者は再配布）、SHA-256 の埋め込み方、Hugging Face の URL 構造変更時の対処、更新確認（`update.auto_check`）の扱い、輸出コンプライアンス（§0.2。TLS を OS の `URLSession` で行えば OS の暗号のみ）

---

## 5. 未決・要確認（ユーザー判断）

提出に直接影響が大きい順。**2026-10-14 に判断済みの項目は「決定済み」と付けた**（経緯と理由は `docs/decisions.md` H-22）。

1. **フルアクセス OFF での動作の実機確認と、4.4.1 への対応**（§0.3）。OFF で漢字変換が動くなら `RequestsOpenAccess` を `false` に戻せるか（H-18 の問題: トグルが出ず、セットアップ検出が成立しない）。`true` のまま行くなら、説明文と審査メモをどう書くか
2. **決定済み: `ITSAppUsesNonExemptEncryption=false` を維持**（TLS は除外済み）。**輸出コンプライアンス**（§0.2）: iOS ビルドから `ureq`（rustls/ring）を外すか、`true` で申告するか。`ITSAppUsesNonExemptEncryption` の最終値
3. **決定済み: 修正する（対応済み）**。**PrivacyInfo の修正の可否**（§0.1）: `CA92.1` の追加（2 か所）、`AppleKeyboards` の読み取りの削除
4. **サイトの公開**（§1.7）: **ホスティングは Cloudflare Pages に決定**（ドメイン `nagi.shiki-01.com`、DNS は後で設定。手順は `web/README.md`）。**`/privacy` の【要確認】欄は空けておく指示のため未記入のまま**。残り: DNS・`/privacy` の【要確認】（提供者名、施行日、アクセスログ、年齢区分）。サポートの連絡手段（メールアドレス等）
5. **モデルの配布方針**（§4）: アプリ内ダウンロードを初回の App Store 版に間に合わせるか。間に合わせないなら「取り込みのみ」の版を公開するか（一般ユーザーには使いにくい）、辞書のみの版として公開するか
6. **決定済み: TestFlight 期間は入力内容を含めて既定 ON、App Store 版は既定 OFF・内容を含めない。About の「開発者向け」は App Store 版で隠す**。**診断ログ・Probes の扱い**（§0.6、§1.5）: 既定（TestFlight 外部テストで入力内容を含めるか）、About >「開発者向け」を App Store 版で隠すか
7. **決定済み: `Nagi`（設定済み）**。**表示名**（§0.4）: `Nagi Spike` → `Nagi`、拡張の表示名。アプリ名 `Nagi 日本語キーボード`（§3.2）が確保できるか
8. **決定済み: iPhone 専用（`TARGETED_DEVICE_FAMILY: "1"` 設定済み）**。**iPad 対応**（§0.4）: iPhone 専用にするか（`TARGETED_DEVICE_FAMILY` を指定する）。iPad 対応なら 13 インチのスクリーンショットとキーボードの iPad レイアウトの確認が要る（`ios-device-checklist.md` に iPad の項目はない）
9. **決定済み: small を基本に、端末の予算で自動選択**（`NagiModelRecommendation`。`ios-device-checklist.md` §0 (c)）。**既定モデル**: 審査・初回体験に影響する
10. **未解決の不具合 U-1**（部分変換で残りが二重に出る）: 公開前に直すか、機能として出さない
11. **ライセンス**（§0.5、§4.2）: アプリ内に NOTICE 全文と依存クレートのライセンス一覧を入れるか。CC BY-SA モデルを IPA／ODR で配布することの可否（法的確認）
12. **ed25519 の更新確認鍵**: `core/server/src/update_check.rs` の `UPDATE_PUBLIC_KEY_HEX` はテスト用プレースホルダーのまま（コード内コメントで明記）。更新確認を iOS で有効にする前に本物の鍵に差し替える
13. 年齢区分の最終回答、カテゴリ、著作権表記、連絡先（名・電話・メール）の記入
14. TestFlight 外部テスターの人数・公開リンクにするか招待制にするか
