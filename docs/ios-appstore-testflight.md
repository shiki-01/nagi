# iOS: Nagi / NagiSpike 統合と App Store Connect・TestFlight 配布手順

状態: 手順書（草案）。調査時点の `frontend-ios-spike/` の構成に基づく。`[要確認]` は実装・実機で未検証の項目。

## 0. 前提と方針変更

- 現状の iOS 実装は検証用スパイク `frontend-ios-spike/` のみ。ホスト `NagiSpike`、キーボード拡張 `NagiSpikeKeyboard`、App Group `group.com.shiki.nagi.spike`（`frontend-ios-spike/project.yml`）。
- Rust コアとの接続は C ABI（`core/ffi/` → `nagi_ffi.h` → `Shared/NagiCore.swift`）。XCFramework は `cargo xtask` の `build_ios_xcframework`（`aarch64-apple-ios` / simulator）で生成する。
- 配布方針は現状 SideStore（`docs/mobile-design.md` §6.1）で、Apple Developer Program 非加入が前提（`docs/decisions.md:181`）。
- App Store / TestFlight には Apple Developer Program（年額 US$99）への加入が必須。次の文書を更新する（§6 の手順 9）。
  - `CLAUDE.md`（配布の記述）
  - `docs/decisions.md`（新規決定として追記）
  - `docs/mobile-design.md` §6.1
- 先に決めること: SideStore 配布を併用するか廃止するか。
- 先に決めること: iOS の品質基準。メモリ実測（`docs/progress.md:36-38`、M0 最優先）が未完了で、実測まで基準は minimal（`decisions.md:714`）。LLM 搭載の可否は TestFlight 配布の前提ではない。

## 1. Apple Developer Program 登録

1. Apple ID を用意し、二要素認証を有効化する。
2. https://developer.apple.com/programs/enroll/ から登録する（個人または組織。組織は D-U-N-S 番号が必要）。
3. 年会費を支払う。承認まで数日かかる場合がある。
4. 承認後、App Store Connect（https://appstoreconnect.apple.com）に同じ Apple ID でサインインできることを確認する。
5. 契約（Paid Applications Agreement は無料アプリなら不要。ただしアプリ内課金や有料配布をする場合は必要）。

## 2. Nagi と NagiSpike の統合

方針: `NagiSpike` を製品版 `Nagi` に昇格させ、プロジェクトを 1 つにする。

### 2.1 識別子（提案。確定は別途）

| 項目 | 現状（スパイク） | 統合後（提案） |
| --- | --- | --- |
| ホストアプリ bundle id | `com.shiki.inputmethod.NagiSpike` | `com.shiki.nagi.ios` |
| キーボード拡張 bundle id | `com.shiki.inputmethod.NagiSpike.Keyboard` | `com.shiki.nagi.ios.keyboard`（ホストの子である必要がある） |
| App Group | `group.com.shiki.nagi.spike` | `group.com.shiki.nagi` |

- macOS IME の `com.shiki.inputmethod.Nagi` とは別の ID にする。iOS 側に `.inputmethod.` は不要。
- App Group は Apple Developer のポータルで登録が必要。変更するとスパイクで蓄積した共有データは引き継がれない。
- bundle id は一度 App Store Connect に登録すると変更できない。確定してから登録する。

### 2.2 リポジトリ作業

1. `frontend-ios-spike/` を `frontend-ios/` に改名する（`git mv`）。
2. `project.yml` のターゲット名・bundle id・App Group・表示名を §2.1 に合わせて変更する。entitlements と Info.plist の App Group 参照も更新する。
3. スパイク固有のコード（計測用 `Probes.swift` など）は、デバッグビルド専用ターゲットまたは `#if DEBUG` に分離する。製品ビルドに含めない。
4. `.github/workflows/ios-spike.yml` を `ios.yml` に改名し、パスとターゲット名を更新する。
5. `ci.yml` に `aarch64-apple-ios` の `cargo check` を追加する（`mobile-roadmap.md:20` D-3）。
6. `Info.plist` / `project.yml` に製品向けの項目を設定する。
   - バージョン（`MARKETING_VERSION`）とビルド番号（`CURRENT_PROJECT_VERSION`）。ビルド番号は TestFlight アップロードごとに増やす。
   - `ITSAppUsesNonExemptEncryption` = `false`（独自の暗号化を使わない場合。輸出コンプライアンスの質問を省略できる）。 `[要確認]` 実装が標準暗号のみであること。
   - アプリアイコン（1024x1024）と起動画面。
   - キーボード拡張の `RequestsOpenAccess`（フルアクセス）。必要性を確認し、不要なら `false` にする。
   - `PrivacyInfo.xcprivacy`（Privacy Manifest）。ホストとキーボード拡張の両方に置く。 `[要確認]`
7. 辞書・モデルの同梱方法を決める。スパイクは CI で辞書を IPA に同梱している。アプリサイズとキーボード拡張のメモリ上限に影響するため、実測（M0）の結果で確定する。
8. ホストアプリは、キーボード追加手順の案内と設定画面を最低限備える。キーボード拡張だけのアプリはリジェクトされやすい。

### 2.3 統合後の確認

- `cargo xtask` の XCFramework 生成 → `xcodegen generate` → シミュレータで実行できる。
- 実機でキーボードを追加し、変換・確定が動く。
- スパイクの記録（計測結果など）が必要なら、統合前に `docs/` へ移す。

## 3. App Store Connect の準備

1. Certificates, Identifiers & Profiles で次を登録する。
   - App ID（ホスト、キーボード拡張）
   - App Group（`group.com.shiki.nagi`）と、両 App ID への割り当て
2. App Store Connect → Apps → 新規 App を作成する。
   - プラットフォーム: iOS、名前、主要言語（日本語）、bundle id（§2.1）、SKU（任意の一意な文字列）。
   - アプリ名は他のアプリと重複できない。 `[要確認]` "Nagi" が使用可能か。
3. App Information にプライバシーポリシー URL を設定する（`shiki-01.com` 上に用意する）。外部テスターへの TestFlight 配布で必須。
4. App Privacy（プライバシー栄養ラベル）に回答する。方針は「全処理ローカル完結、学習データは端末内」。フルアクセスを要求する場合は、その理由を説明できるようにする。
5. CI 用に App Store Connect API キー（ユーザーとアクセス → 統合 → キー、権限は App Manager 以上）を作成する。Issuer ID・Key ID・`.p8` を GitHub Secrets に保存する。リポジトリには入れない。

## 4. 署名とビルド

手動で一度通してから CI 化する。

1. Xcode でチームを設定し、Automatic Signing で実機ビルドが通ることを確認する。
2. `Product → Archive`（または `xcodebuild archive`）を実行する。事前に XCFramework 生成と辞書の配置を済ませる。
3. Archive を App Store Connect にアップロードする（Xcode Organizer の Distribute App、または `xcodebuild -exportArchive` + Transporter / `xcrun altool`）。
4. 処理完了（数分〜数十分）後、App Store Connect の TestFlight タブにビルドが表示される。
5. CI 化する場合は、`macos` ランナーで次を行う。
   - API キーで署名（`xcodebuild -allowProvisioningUpdates -authenticationKeyPath ...`）
   - アップロード
   - fastlane の利用は任意

## 5. TestFlight 配布

| 種別 | 対象 | Beta App Review |
| --- | --- | --- |
| 内部テスト | App Store Connect のユーザー（最大 100 人） | 不要 |
| 外部テスト | 招待メールまたは公開リンクのテスター（最大 10,000 人） | 初回ビルドに必要 |

1. 内部テスト: ユーザーとアクセスでテスターを追加 → TestFlight → 内部グループを作成 → ビルドを割り当てる。
2. テスターは TestFlight アプリから `Nagi` をインストールし、設定 → 一般 → キーボードで追加する。
3. 外部テスト: グループ作成 → ビルド割り当て → 「テスト情報」（概要、連絡先、フィードバック先）を入力 → Beta App Review に提出する。
4. ビルドは 90 日で期限切れになる。定期的に更新する。

## 6. 実施順序（チェックリスト）

1. [ ] 配布方針の決定（SideStore の扱い、iOS の品質基準）
2. [ ] Apple Developer Program 加入（§1）
3. [ ] 識別子の確定（§2.1）
4. [ ] リポジトリの統合作業（§2.2）
5. [ ] シミュレータ・実機での動作確認（§2.3）
6. [ ] プライバシーポリシーの用意（§3.3）
7. [ ] App Store Connect 登録（§3）
8. [ ] 手動で Archive → アップロード → 内部テスト（§4, §5.1）
9. [ ] `CLAUDE.md`・`docs/decisions.md`・`docs/mobile-design.md` の更新
10. [ ] CI 化（§4.5）、外部テスト（§5.3）

## 7. 未確認事項・リスク

- iOS キーボード拡張のメモリ上限は未実測（`docs/progress.md:36-38`）。実測で上限を超える場合、辞書方式（mmap 化）やモデル搭載を見直す。
- App Review でキーボード拡張が指摘される論点（フルアクセスの用途、ホストアプリの機能）。 `[要確認]`
- `decisions.md:707・719` は UniFFI 確定、実装と `mobile-design.md:61` は C ABI。製品版でどちらを採るか未整理。統合作業と並行して決定を記録する。
