# モバイル対応ロードマップ

v0.1 draft（2026-09-30）。`docs/mobile-design.md`（設計）と `docs/decisions.md` H 節（決定）を前提に、着手順とデスクトップ側の修正をまとめる。

## 1. 着手順の方針

- 設計の制約は厳しい側（iOS）に合わせる。ただし実装・検証は Android を先行する（H-2 の既存決定と整合）。
  - iOS 拡張は子プロセス不可・メモリ上限が数十 MB 級・SideStore 配布で、実装の試行回数が稼ぎにくい。Android で FFI・タッチ入力・辞書共有の仕組みを先に確立する。
  - 「厳しい側に合わせる」は実装順ではなくコア設計の基準として適用する。辞書は mmap 前提、LLM は `minimal` で常用に耐える、依存は iOS/Android 両方でビルド可能、を Android 実装中も守る。
- iOS の致命的リスク（拡張のメモリ上限、SideStore の App Group 動作）は M0 で実測し、Android 実装に入る前に判定する。iOS が成立しない場合の設計変更を早く知るため。

## 2. デスクトップ側の先行修正（D 系）

モバイル着手前にデスクトップ開発と並行して行う。いずれもデスクトップ単体でも価値がある変更に限る。

| ID | 内容 | 完了条件 | 根拠 |
| --- | --- | --- | --- |
| D-1 | 辞書形式を mmap 可能にする（アラインメント・オフセット表現。読み込みの mmap 化は形式変更後） | `nagi-dict.bin` を `std::fs::read` せず参照でき、既存の精度評価（`nagi-eval --no-llm`）が変わらない | progress #6、H-3 |
| D-2 | `insert_reading` / `perform_action` をプロトコルに追加（フリック等のかな直接挿入。Action 駆動のまま） | `core/protocol` に追加、`gen-types` / `check-types` 通過、状態機械のゴールデンテスト追加 | progress #7、H-3/H-4 |
| D-3 | モバイル向けクロスビルドの CI 化。`cargo check --target aarch64-apple-ios` / `aarch64-linux-android` を engine / romaji / dict / protocol / profile / infer（llama 無し）に対して実行。`sysinfo` の可否を確認し、不可なら trait で注入に変更 | CI で上記ターゲットの check が通る | mobile-design §3-6 |
| D-7 | 曖昧入力（タッチ誤差・タイポ許容）のコア実装。段階 F-0〜F-1（`nagi-fuzzy` 新設・タイポ仮説・`convert_hypotheses`・`CandidateSource::Corrected`）。F-2 以降（タッチ・フリック・学習）は D-2 のプロトコル追加と同時に進める | 合成ノイズ評価で補正あり top-5 が改善し、ノイズ無しの精度は劣化 0 | `fuzzy-input-design.md` |
| D-4 | `initialize` の capability で、メモリ上限・電源状態・熱状態・入力方式（物理キー/タッチ）をホストから受け取れるようにし、プロファイル判定をこれに従わせる（OS 問い合わせをコア内に持たない） | デスクトップでも同経路で動作。`profile` が `sysinfo` に直接依存しない | mobile-design §3-2 |
| D-5 | 学習ファイルとユーザー辞書の複数プロセス同時アクセス対策（キーボード拡張と本体アプリが同じ App Group / アプリ領域を触る）。排他制御と原子的書き込みの再点検 | 2 プロセス同時 flush のテストが通る | mobile-design §2 |
| D-6 | 予測変換の仕様を確定する（G-44 は現状仕様で確定済み。モバイルは予測候補バーが主要 UI のため、候補供給と件数・順位の要件を追記） | 設計書に要件を追記 | decisions G-44 |

D-1・D-2・D-3 は先行して実施する。D-4〜D-6 は M0 の結果を見て具体化する。D-7 は D-2 と同時にプロトコルを決める（`alternatives` フィールド。破壊的変更を 2 回に分けないため）。

**D-2 の状況（2026-10-03）**: 実装済み。`insert_reading` / `perform_action`、共通型 `InputAlternative { text, distance }`、`process_key` の任意 `alternatives`、`ServerCapabilities.touch_input` を追加（`core/protocol`・`core/engine`・`core/server`、golden `120_touch_input.yaml`、Swift 型再生成）。Action 名は keymap の実物（`StartConvert`、`MoveCursor:Left` 形式）に合わせた。`alternatives` は受け取って検証するのみで挙動には使わない（F-1 で Composer に保持）。未知の Action 名は JSON-RPC の `INVALID_PARAMS`。Swift 側は呼び出し 3 箇所に `alternatives: nil` を追加したが、Linux では未ビルドのため macOS での `swift build` / `swift test` が必要。

**D-3 の状況（2026-10-03）**: `.github/workflows/ci.yml` に `mobile-android` / `mobile-ios` ジョブを追加。Linux 上で 2 ターゲットとも engine / romaji / dict / protocol / profile / infer / config / learn / rewriter の `cargo check` が通ることを確認済み（`sysinfo` 0.38.4 もそのままビルドでき、trait での注入への変更は不要）。`nagi-ffi` は C 依存のため NDK（Android）/ Xcode（iOS）が必要で、Linux 開発環境では未検証。CI 上の結果は初回実行で確認する（Android の ffi ステップは検証前のため `continue-on-error`）。

## 3. モバイルのマイルストーン

| M | 内容 | DoD |
| --- | --- | --- |
| M0 | 技術検証（実機） | 下記の実測結果が `docs/decisions.md` に記録されている |
| M1 | FFI 層 + Android MVP | `core/ffi/`（UniFFI）で `handle_line` を公開し、AAR を生成。InputMethodService でフリック/QWERTY 入力・候補バー・確定が動き、`minimal` プロファイルで日常入力に使える。QWERTY のタッチ誤差補正（`fuzzy-input-design.md` F-2）を含む |
| M2 | Android 拡張 | 学習・ユーザー辞書・本体アプリでのデータ更新・更新確認（ed25519 検証）。LLM 変換（可能な端末のみ）とプロファイル自動切替。フリック誤判定の補正（F-3）、補正選択の学習（F-4）、実機でのパラメータ較正（F-5） |
| M3 | iOS | XCFramework。Custom Keyboard Extension + 本体アプリ（App Group 共有）。M0 のメモリ実測に基づき `minimal` から開始し、実測余裕に応じて昇格 |
| M4 | スタイル変換等の拡張 | デスクトップ Phase 4 の機能をモバイルに展開 |

### M0 の実測項目（優先順）

1. iOS キーボード拡張で、mmap した辞書 + Viterbi が動く際の常駐メモリと、拡張が強制終了される閾値
2. iOS 拡張で LLM（zenz-v3.1-small の GGUF）が載るか。載らない場合は本体アプリ側での推論やオンデバイス外の代替は採らず、`minimal` 固定とする
3. SideStore 配布で App Group と Custom Keyboard が実際に動作するか（entitlement の扱い）
4. UniFFI 経由の 1 キーあたりのレイテンシ（JSON 文字列渡しと型渡しの比較）。目標は既存の p95 < 16ms（ひらがな反映）を維持
5. Android で `android:process` 分離した IME サービスのメモリと、コールドスタート時間
6. azooKey のキーボード UI・拡張構成の調査（参考実装）
7. 曖昧入力の較正データ収集（`fuzzy-input-design.md` §10-5）: 実機でのタッチ座標ログ（ローカル保存・明示的に開始）。M1 以降の F-5 で使う

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
