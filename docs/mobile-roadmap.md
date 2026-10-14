# モバイル対応ロードマップ

v0.1 draft（2026-09-30）。`docs/mobile-design.md`（設計）と `docs/decisions.md` H 節（決定）を前提に、着手順とデスクトップ側の修正をまとめる。

## 0. 現在の状況（2026-10-14）

**実際の着手順は、計画（Android 先行）と異なり、iOS を先に進めている**（Apple Developer Program に加入し、TestFlight で実機検証を回せるようになったため。H-15）。そのため M2（フリック・学習）の一部を iOS で前倒しで実装している。

| 項目 | 状況 |
| --- | --- |
| D-1 辞書の mmap 化 | **完了**（形式は変えず、要素ごとの読み出しに。拡張で +61MB → +0.2MB。H-7） |
| D-2 `insert_reading` / `perform_action` | **完了**（12 キーのトグル・フリック・濁点循環をコアで解決。H-11） |
| D-3 クロスビルド CI | 一部。iOS は `ios-testflight` で毎回ビルドする。Android（`aarch64-linux-android`）と `aarch64-apple-ios` の `cargo check` は `ci.yml` に無く、**未実施** |
| D-4 `initialize` の capability | コア側は実装済み（2026-10-14、H-16）: `initialize.params.capabilities`（`input_method` / `environment`）と `set_environment.memory_budget_mb` でメモリ上限・電源・熱・メモリ逼迫を受け取りプロファイル判定に反映。LLM だけの解放・再読み込み（`unload_model` / `reload_model`、メモリ逼迫 critical で自動解放）も実装済み。iOS 側の追従は済み（`NagiSession.swift`・`NagiEnvironment.swift`・`KeyboardModel.swift` で capabilities・`set_environment`・`get_key_table` を使う。実機確認待ち。複製表 `KanaKeyTable.swift` はフォールバックとして残る） |
| D-5 複数プロセスの同時アクセス | 一部（ホストが学習ファイルを直接消す経路は、コアと同じ `learning.lock` の flock を取るようにした。コアの flush が他プロセスの更新を取り込まず後勝ちで上書きする点、ロックファイルの削除の競合は未対応でコア側の課題。実機未確認） |
| D-6 予測変換の要件 | 未着手（入力中の候補・部分変換で一部を代替。H-14） |
| M0 実機での技術検証（iOS） | **ほぼ完了**（拡張の上限 約 177MB、xsmall・small とも拡張内で動作、応答時間は同期待ち 30ms + 非同期の差し替えで目標内、C ABI の非同期通知を確認。H-6〜H-10）。SideStore 検証は不要。Android は未着手 |
| M1 Android MVP | 未着手 |
| M2 フリック・学習・範囲操作 | フリックは iOS で実装済み。学習・ユーザー辞書はコアに有り（モバイルでの共有・同時アクセスは D-5 とともに未着手）。範囲操作の UI は未着手 |
| M3 iOS | **進行中**: TestFlight で実機を使える段階。実装済み（実機未確認の項目あり。`docs/ios-device-checklist.md`）: かな 12 キー・フリック・変換・候補・部分変換・英字、データ駆動レイアウト（`docs/ios-keyboard-layout.md`。toggle・長押し・shortcut・片手モード）、設定タブ（シンプル/詳細。`docs/ios-settings.md`）、レイアウト管理（取り込み・割当・`nagi://layout`）、初回案内・データ・About、診断ログ。サイト（`web/`）にレイアウト編集 GUI。残り: 部分変換の不具合 U-1 の再確認、実機確認、本物のアイコン・プライバシーポリシー・Beta App Review、堅牢性（メモリ警告・電源の実機確認、他機種）、設定のコア側新規項目（`docs/ios-settings.md` §4 段階4）、見た目の調整。タブは 5 つに整理済み（ホーム / 入力テスト / 設定 / レイアウト / About。実機確認待ち） |
| M4 スタイル変換等 | 未着手 |

## 1. 着手順の方針

- 設計の制約は厳しい側（iOS）に合わせる。ただし実装・検証は Android を先行する（H-2 の既存決定と整合）。
  - iOS 拡張は子プロセス不可・メモリ上限が数十 MB 級・SideStore 配布で（当時の前提。H-15 で TestFlight 配布に変更）、実装の試行回数が稼ぎにくい。Android で FFI・タッチ入力・辞書共有の仕組みを先に確立する。
  - 「厳しい側に合わせる」は実装順ではなくコア設計の基準として適用する。辞書は mmap 前提、LLM は `minimal` で常用に耐える、依存は iOS/Android 両方でビルド可能、を Android 実装中も守る。
- iOS の致命的リスク（拡張のメモリ上限。SideStore は H-15 で不要になった）は M0 で実測し、Android 実装に入る前に判定する。iOS が成立しない場合の設計変更を早く知るため。

## 2. デスクトップ側の先行修正（D 系）

モバイル着手前にデスクトップ開発と並行して行う。いずれもデスクトップ単体でも価値がある変更に限る。

| ID | 内容 | 完了条件 | 根拠 |
| --- | --- | --- | --- |
| D-1 | 辞書形式を mmap 可能にする（アラインメント・オフセット表現。読み込みの mmap 化は形式変更後） | `nagi-dict.bin` を `std::fs::read` せず参照でき、既存の精度評価（`nagi-eval --no-llm`）が変わらない | progress #6、H-3 |
| D-2 | `insert_reading` / `perform_action` をプロトコルに追加（フリック等のかな直接挿入。Action 駆動のまま） | `core/protocol` に追加、`gen-types` / `check-types` 通過、状態機械のゴールデンテスト追加 | progress #7、H-3/H-4 |
| D-3 | モバイル向けクロスビルドの CI 化。`cargo check --target aarch64-apple-ios` / `aarch64-linux-android` を engine / romaji / dict / protocol / profile / infer（llama 無し）に対して実行。`sysinfo` の可否を確認し、不可なら trait で注入に変更 | CI で上記ターゲットの check が通る | mobile-design §3-6 |
| D-4 | `initialize` の capability で、メモリ上限・電源状態・熱状態・入力方式（物理キー/タッチ）をホストから受け取れるようにし、プロファイル判定をこれに従わせる（OS 問い合わせをコア内に持たない） | デスクトップでも同経路で動作。`profile` が `sysinfo` に直接依存しない | mobile-design §3-2 |
| D-5 | 学習ファイルとユーザー辞書の複数プロセス同時アクセス対策（キーボード拡張と本体アプリが同じ App Group / アプリ領域を触る）。排他制御と原子的書き込みの再点検 | 2 プロセス同時 flush のテストが通る | mobile-design §2 |
| D-6 | 予測変換の仕様を確定する（G-44 は現状仕様で確定済み。モバイルは予測候補バーが主要 UI のため、候補供給と件数・順位の要件を追記） | 設計書に要件を追記 | decisions G-44 |

D-1・D-2・D-3 は先行して実施する。D-4〜D-6 は M0 の結果を見て具体化する。

## 3. モバイルのマイルストーン

| M | 内容 | DoD |
| --- | --- | --- |
| M0 | 技術検証（実機） | 下記の実測結果が `docs/decisions.md` に記録されている |
| M1 | FFI 層 + Android MVP | `core/ffi/`（UniFFI）で `handle_line` を公開し、AAR を生成。InputMethodService でフリック/QWERTY 入力・候補バー・確定が動き、`minimal` プロファイルで日常入力に使える |
| M2 | Android 拡張 | 学習・ユーザー辞書・本体アプリでのデータ更新・更新確認（ed25519 検証）。LLM 変換（可能な端末のみ）とプロファイル自動切替 |
| M3 | iOS | XCFramework。Custom Keyboard Extension + 本体アプリ（App Group 共有）。M0 のメモリ実測に基づき `minimal` から開始し、実測余裕に応じて昇格 |
| M4 | スタイル変換等の拡張 | デスクトップ Phase 4 の機能をモバイルに展開 |

### M0 の実測項目（優先順）

1. iOS キーボード拡張で、mmap した辞書 + Viterbi が動く際の常駐メモリと、拡張が強制終了される閾値
2. iOS 拡張で LLM（zenz-v3.1-small の GGUF）が載るか。載らない場合は本体アプリ側での推論やオンデバイス外の代替は採らず、`minimal` 固定とする
3. ~~SideStore 配布で App Group と Custom Keyboard が実際に動作するか~~ → TestFlight 配布に変更（H-15）。App Group と拡張は TestFlight で動作を確認済み
4. UniFFI 経由の 1 キーあたりのレイテンシ（JSON 文字列渡しと型渡しの比較）。目標は既存の p95 < 16ms（ひらがな反映）を維持
5. Android で `android:process` 分離した IME サービスのメモリと、コールドスタート時間
6. azooKey のキーボード UI・拡張構成の調査（参考実装）

## 4. 依存関係と順序

```
D-1 mmap ─┐
D-2 insert_reading ─┼─▶ M0（実測）─▶ M1（Android MVP）─▶ M2 ─▶ M3（iOS）─▶ M4
D-3 クロスビルドCI ─┘        │
                            └─ iOS 不成立時は M3 の方式を再検討
```

- デスクトップの Phase 2 以降（ライブ変換・学習・範囲操作等）と D 系は並行可能。ただしモバイルの M1 着手はデスクトップ Phase 1〜2 の完了後とする（mobile-design §7 に従う）。
- D-1・D-2 は状態機械・辞書の破壊的変更を含むため、デスクトップ側の機能追加が増える前に済ませる。

## 5. 未決事項

- iOS 拡張で LLM が載らなかった場合の品質許容ライン（`minimal` の精度目標）
- タッチ UI（フリック配列・QWERTY・候補バー）の実装をどこまでネイティブに持ち、コアに何を置くか（ロジックはコア、描画はフロントの原則は維持）
- M1 の UI 実装言語（Kotlin + Compose を想定）
