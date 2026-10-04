# Nagi for iOS（キーボードアプリ）

Nagi のコアエンジン（Rust）を iOS のカスタムキーボード拡張として動かすアプリ。本体アプリ（`Nagi`）とキーボード拡張（`NagiKeyboard`）の 2 ターゲットで構成する。
実機検証用の `frontend-ios-spike/`（メモリ・レイテンシ計測）とは別物で、Bundle ID も別（本アプリ: `com.shiki.nagi` / `com.shiki.nagi.keyboard`、スパイク: `com.shiki.inputmethod.NagiSpike` 系）。

- 辞書のみ（`minimal` プロファイル、LLM なし）で動く。辞書は拡張にバンドル同梱する
- **フルアクセス不要**（`RequestsOpenAccess = false`）。ネットワークは使わない
- 入力: QWERTY ローマ字（Space で変換・候補バー・確定）と 12 キーのフリック（かな直接挿入）、英数、記号・数字
- 曖昧入力（タッチ位置の誤差・フリックの誤判定の補正候補）を既定で有効にする（`config.toml` の `[fuzzy] enabled = true`）
- 設計: `docs/mobile-design.md`（§4.2・§5）、`docs/fuzzy-input-design.md`（§4.1・§4.2）、`docs/mobile-roadmap.md`

## ビルド

### GitHub Actions（推奨）

`.github/workflows/ios-app.yml`（手動実行）を Run workflow する。`build` ジョブが XCFramework のビルド（LLM なし）→ 辞書の取得・構築 → XcodeGen → 未署名アーカイブ → ldid で entitlement（App Group `group.com.shiki.nagi`）を埋め込み → IPA 化を行い、artifact `Nagi-unsigned.ipa` を出力する。`test` ジョブはシミュレータ上の単体テストで、IPA の成否とは独立（失敗しても IPA は出る）。

### ローカル Mac

前提: macOS、Xcode 15 以降、XcodeGen（`brew install xcodegen`）、Rust（`core/rust-toolchain.toml` に従う）、`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`。

```bash
cd core
cargo xtask build-ios-xcframework --out-dir ../frontend-ios   # frontend-ios/NagiCore.xcframework を生成
cargo xtask fetch-dict && cargo xtask build-dict              # data/dict/nagi-dict.bin
mkdir -p ../frontend-ios/Resources/data/dict
cp ../data/dict/nagi-dict.bin ../frontend-ios/Resources/data/dict/
cd ../frontend-ios
xcodegen generate
open Nagi.xcodeproj
```

実機に入れるときは Xcode の Signing で Team（Personal Team 可）を選ぶ。単体テストは `xcodebuild test -project Nagi.xcodeproj -scheme NagiLogicTests -destination 'platform=iOS Simulator,name=<端末名>'`（XCFramework 不要）。

## SideStore での導入

1. `Nagi-unsigned.ipa` を端末へ取り込み、SideStore の「+」から IPA を選んでインストールする
2. **App Group は使えないことがある**（無料 Apple ID）。その場合もキーボードは動く（下記）。本体アプリの「状態」に App Group の可否が表示される
3. 無料 Apple ID の制限: 署名は 7 日で失効（SideStore の自動更新で再署名）、App ID は週 10 個まで（本体 + 拡張で 2 個消費）、有効アプリ数は 3 個まで

## キーボードの有効化

1. 設定 > 一般 > キーボード > キーボード > 新しいキーボードを追加 > 「Nagi」
2. 追加した「Nagi」をタップする。**「フルアクセスを許可」はオフのままでよい**（不要）
3. 入力欄で 🌐 を長押しして「Nagi」を選ぶ。本体アプリの「テスト入力」欄で試せる

操作: QWERTY ではローマ字を打って Space で変換、候補バーの候補をタップで選択（最後の文節なら確定、途中なら次の文節へ進む）、⏎ で確定。変換中はバー右端の ◀ ▶ で注目文節の移動、縮 / 伸 で文節の区切りの伸縮。12 キーではフリックで段を選び、「小゛゜」で濁点・半濁点・小書きを切り替える。メール・URL・数字欄では自動で英数 / 数字面になり、変換しない。

## フルアクセスが不要な理由とデータの置き場

- コアは拡張プロセス内にライブラリとして読み込む（子プロセスなし）。辞書は拡張のバンドルから読む。ネットワークは使わない
- ユーザーデータ（`config.toml`・学習・ユーザー辞書・ユーザー定義レイアウト）は `user_dir` に置く
  - App Group が使えれば、その中の `user/`（本体アプリと拡張で共有）
  - 使えなければ、拡張の `Application Support/com.shiki.nagi/`（本体アプリとは別の領域。拡張専用になる）
- App Group の ID は実行時に解決する（`embedded.mobileprovision`・`ALTAppGroups`・固定値の順に候補を試す）。SideStore が ID を書き換えても追従できる
- 辞書の探索順: App Group の `data/` → 自プロセスのバンドル → 本体アプリ内の拡張のバンドル

## レイアウト JSON の書き方（カスタム配列）

キーボードの見た目と動作はすべて `Layouts/*.json` から作る。同梱: `qwerty-romaji`、`flick-12`、`qwerty-direct`（英数）、`symbols`、`symbols-direct`。ユーザー定義は `<user_dir>/layouts/<id>.json` に置くと、同じ `id` は同梱を上書きし、新しい `id` は追加される（本体アプリの「クリップボードのレイアウト JSON を取り込む」で検査して保存できる。**拡張から読めるのは App Group が使えるときだけ**）。

```json
{
  "id": "my-qwerty",
  "name": "自作",
  "direct": false,
  "rows": [
    { "height": 1.0, "keys": [
      { "id": "q", "tap": { "type": "char", "text": "q" } },
      { "id": "gap", "spacer": true, "width": 0.5 },
      { "id": "ka", "label": "か", "tap": { "type": "kana", "text": "か" },
        "flick": { "left": "き", "up": "く", "right": "け", "down": "こ" } },
      { "id": "bs", "label": "⌫", "style": "function", "width": 1.5, "repeats": true,
        "tap": { "type": "key", "name": "backspace" } }
    ] }
  ]
}
```

| 項目 | 内容 |
| --- | --- |
| `id` / `name` | レイアウト ID（英数字・`-`・`_`）/ 表示名 |
| `direct` | true ならコアを通さずそのまま挿入（英数入力） |
| `rows[].height` | 行の相対の高さ（既定 1） |
| `keys[].width` | 行内の相対の幅（既定 1。行ごとに合計で正規化される） |
| `keys[].spacer` | true なら空白（描画・タッチ判定の対象外） |
| `keys[].id` | キー ID（レイアウト内で一意。空白は省略可） |
| `keys[].label` | 表示ラベル（省略時は `tap` の文字） |
| `keys[].style` | `normal`（既定）/ `function` / `accent` |
| `keys[].tap` | タップ時の動作（下表） |
| `keys[].flick` | `left` / `up` / `right` / `down` ごとの出力文字。出力の種類は `tap` と同じ（`char` か `kana`） |
| `keys[].longPress` | 長押しの動作（`tap` と同じ書式） |
| `keys[].repeats` | true なら押し続けると連打（Backspace 用） |

`tap` / `longPress` の `type`:

| type | 指定 | 動作 |
| --- | --- | --- |
| `char` | `text` | ローマ字 1 文字（`process_key`。QWERTY の近傍キーを曖昧入力の代替として送る） |
| `kana` | `text` | かなの直接挿入（`insert_reading`。フリックの近傍ゾーンを代替として送る） |
| `key` | `name`: `space` `enter` `backspace` `left` `right` `shift` `globe` `dakuten` | 特殊キー |
| `layout` | `layout`: 切替先の `id` | レイアウト切替 |
| `action` | `name`: コアの Action 名（`ToKatakana`、`MoveCursor:Left` 等） | `perform_action` |

曖昧入力の距離は、描画しているキー矩形から計算する（配列を決め打ちしない）。QWERTY は「タッチ位置から各キー中心までの距離 ÷ キー幅」が 1.0 以下の近傍キー（最大 3 件）、フリックは「他の方向ゾーン（中心から短辺 × 0.8 離れた位置）と、隣接キーの中央ゾーン」を同じ基準で送る。方向ゾーンの離し幅は `FlickGeometry.spreadRatio` の 1 箇所で決めている（実機で較正する）。

## 診断のとり方

- 本体アプリの「状態」: エンジンのバージョン・辞書の有無（出所）・App Group の可否・`config.toml` の状態
- 「自己テストを実行」: 本体アプリ内でエンジンを生成し、`nihongo` を Space 変換できるか・レイテンシ・メモリ（`phys_footprint`）を表示する
- 「診断情報をコピー」: 状態・自己テスト・（App Group が使えれば）キーボード拡張の診断ログをクリップボードへコピーする
- キーボード上部バー左端の **ⓘ** をタップ: 診断（エンジンの状態・プロファイル・パス情報・メモリ・レイテンシ p50/p95・ログ）を入力欄に挿入する。App Group が使えず本体アプリから拡張のログを読めないときはこちらを使う
- **ⓘ を長押し**: 未確定文字列の出し方を切り替える（入力欄内の下線表示 ⇔ バー上部のみ。確定時だけ入力欄へ挿入）。`setMarkedText` が合わないアプリでの回避策
- ログには入力内容を含めない（イベント・エラー・統計のみ）

## 既知の制限・未実装

- 記号・数字面は最小限（全角記号・絵文字・数字のフリックは未対応）
- 文節操作はバーのボタンのみ（ドラッグでの区切り伸縮・範囲選択は未実装）
- 非同期通知（`update_preedit` 等）は未処理。LLM とライブ変換が無効の現構成では発生しない
- 外部要因でカーソルが動いた場合（ホスト側のタップ等）の preedit 再同期は未実装（キーボードが隠れる時に確定する）
- フリック中の方向プレビューは簡易（押しているキーのラベルを確定予定の文字に置き換えるだけで、吹き出しは出さない）
- iOS 側の入力欄種別による自動切替は `keyboardType` / `isSecureTextEntry` を見るだけ（secure 欄は iOS が標準キーボードへ切り替えるため通常は呼ばれない）
- 辞書はヒープに全量読み込む（mmap 化は未実施）。拡張のメモリ上限は実機で要確認（診断のメモリ値を参照）
- アップデート確認・辞書の更新は未実装（`docs/mobile-design.md` §6.3）
- 🌐 の長押し（入力モード一覧）は未対応。タップで次のキーボードへ切り替える
