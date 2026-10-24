# キーボードの「スタイルとトリガー」仕様（v1 draft）

iOS キーボードのキーの見た目を、レイアウト JSON から細かく指定できるようにし、キーごとの状態（押している・長押し・スペースでカーソル移動中など）を
きっかけに見た目を切り替える。サイト（`web/` の `/custom-keyboard`）のエディタに「詳細設定」を足して編集できるようにする。

- 対象: レイアウト JSON（`docs/ios-keyboard-layout.md`）の拡張、iOS 描画（`frontend-ios-spike/Keyboard/`、`Shared/`）、サイトのエディタ（`web/src/lib/keyboard/`、`web/src/lib/components/`）
- **方針（ユーザー指示）**: まだ作者本人しか使っていないため、レイアウト JSON の後方互換は取らない。設計が素直になるなら既存の項目名・構造を変える（破壊的変更可）。新旧の併存・互換コードは作らず、既存データは §12 の手順で一括移行する。ただし「既定の見た目は現行と同じ」は維持する
- この文書は仕様のみ。実装は別の担当が段階ごとに行う（§9）。判断はすべてここで断定し、理由を 1 行ずつ添える

### 進捗

| フェーズ | 状況 |
| --- | --- |
| ①スキーマ ＋ 解決関数 ＋ 単体テスト | **フェーズ 1 実装済み**（ローカルの web テスト・型検査は通過。iOS の `ios-test` / `ios-testflight` は未実行＝CI で確認する）。実装メモは `docs/decisions.md` H-65 |
| ②iOS の描画（全体／個別の `look`） | **フェーズ 2 実装済み**（iOS の `ios-test` / `ios-testflight` は未実行＝CI で確認。実機の目視確認が残る）。実装メモは `docs/decisions.md` H-66 |
| ③状態トリガー | **フェーズ 3 実装済み**（iOS の `ios-test` / `ios-testflight` は未実行＝CI で確認。実機確認が残る）。実装メモは `docs/decisions.md` H-66 |
| ④背景画像 | **フェーズ 4 実装済み**（iOS の `ios-test` / `ios-testflight` は未実行＝CI で確認。実機のメモリ・表示の確認が残る）。実装メモは `docs/decisions.md` H-66 |
| ⑤Web エディタ | **実装済み**（web の test / check / build は通過。実機での書き出し→取り込みの見た目確認は未。decisions H-67） |
- 実装が完了したら `docs/ios-keyboard-layout.md`（構造・kind の節に `look` / `states` / `rules` / `assets` / `metrics` を追記）と `config/keyboard/layout.schema.json` を更新する

## 0. 現状の整理（この仕様が前提にしていること）

| 項目 | 現状（ファイル・関数） |
| --- | --- |
| レイアウトの型 | `Shared/KeyboardLayout.swift` の `KeyboardLayoutDef` / `LayoutRow` / `LayoutKeyDef`。`schema` は 1 のみ受理（`validationError`。この仕様で 2 のみに変える）。Codable は未知のキーを**黙って無視**する |
| 検証 | iOS: `LayoutValidation`（`LayoutLoader.decode`）。web: `web/src/lib/keyboard/validate.ts`（未知のフィールドは**エラー**）と `config/keyboard/layout.schema.json`（`additionalProperties: false`）。`keyboard.test.ts` が ajv と手書き検証の一致を確かめる |
| 検証の流儀 | 不正なレイアウトは**読み込まず次の候補（同梱 → 組み込み）に進む**（入力を失わない）。ただし参照先が消えた `layout.target` など「実行時に確かめない」ものは寛容（`validationError(targets: nil)`）。`visible` は形が違っても無視（`try?`） |
| 既存の `style` | キー定義の `style` は `normal` / `function` / `accent` の**文字列**（色の役割）。`KeyStyle` enum。この仕様で **`role` に改名**する（§1.3、§12） |
| キーの描画 | `Keyboard/NagiKeyboardView.swift`: `LayoutPad` → `RowView` → `KeyCell`（kind ごとに `FlickKeyView` / `RepeatKeyView` / `SpaceKeyView` / `LongPressKeyView` / `TapKeyView`）→ 共通の `KeyFace`（背景 `RoundedRectangle(keyCorner).fill(fill.opacity(pressed ? 0.6 : 1))`、文字 `.font(.system(size: fontSize))`、アイコンは `KeyIconView(size: 17)`） |
| 既定の色 | normal = `systemBackground`、function = `systemGray4`、accent（または Shift の Caps）= `themeAccent`＋文字白、文字 = `UIColor.label`。角丸 = 設定 `keyboard.key_corner_pt`（6）、余白 = `keyboard.key_spacing_pt`（4）。キーボード全体の背景は**塗らない**（システムの背景が見える。H-61） |
| テーマ | 設定 `keyboard.theme`（system/light/dark）は `KeyboardViewController` が `overrideUserInterfaceStyle` で上書き。SwiftUI の `colorScheme` はその結果を反映する |
| 状態のもと | 押下: 各 View の `@GestureState isPressed`（`TapKeyView`/`SpaceKeyView`/`RepeatKeyView`）、`isDown`（`LongPressKeyView`）、`drag != nil`（`FlickKeyView`）。長押し: `LongPressKeyView.startTimer`（`flick.long_press_ms` 後に `popupShown = true`）。スペースのドラッグ: `SpaceKeyView` の `SpaceSwipeTracker.isActive`（`Keyboard/TouchLogic.swift`）。シフト: `KeyboardModel.shiftState`（`.off/.once/.caps`）。入力中: `KeyboardModel.hasPreedit` |
| スペースの移動の有効条件 | `model.ios.spaceSwipe` が真で、スペースキーに `longPress` が無いとき（`KeyCell` の `case .action where key.action == "space" …`） |
| 取り込み | ホストアプリ `App/LayoutManagerView.swift`。JSON の上限 512KB（`maxBytes`）。URL スキームの `data` は 1MB（`LayoutURL.maxDataChars`）。`iconify:` アイコンは取り込み時に `IconifyResolver` が解決 |
| サイトのエディタ | Svelte 5 + SvelteKit。ページ `web/src/routes/custom-keyboard/+page.svelte`（ツールバー、`KeyboardCanvas`、右の「インスペクタ」＝`LayoutMetaForm`/`KeyForm`、下のドロワー＝`JsonPanel`/`ErrorList`）。状態は `EditorStore`（`store.svelte.ts`、undo/redo の `History`、`localStorage` 自動保存）、変更は `ops.ts` の純粋関数。**「カスタム」という名前のタブは現状ない**（ナビの「カスタムキーボード」＝このページ全体を指すと解釈し、インスペクタにタブを足す形にする。§8） |
| テスト | iOS: `Tests/`（XCTest、`.github/workflows/ios-test.yml`。Rust に依存しない純粋な Swift のみ）。web: `keyboard.test.ts`（vitest）。Swift はローカルでコンパイルできず CI で確認 |

## 1. 設計方針

### 1.1 互換の方針（簡素）

- **後方互換は取らない**。`schema` を **2** にし、iOS・web とも **2 だけを受理**する（`schema: 1` は「未対応のスキーマ」エラー。旧形式を読む分岐・変換コードは実装しない）。既存データの移行は §12 の一回限りの手順で行う
- 追加する見た目の項目（`look` ほか）は省略可能。**省略時の見た目は現行と同じ**（既定値は §2.1 の表のとおり現行の値）
- 描画は**新しい 1 経路だけ**（`StyledKeyFace`）にする。現行の `KeyFace` は削除し、その見た目は「指定が何も無いときの既定値」として `StyledKeyFace` が再現する。理由: 二重の描画経路を持つと保守が倍になり、互換コードを作らない方針に反する。見た目の同一性は解決関数の単体テスト（既定値が現行の定数と一致）と、実機で全同梱レイアウトを目視して確認する（§9 ②）
- 理由: 利用者が作者のみで、互換を保つ価値より、実装と仕様を単純にする価値が大きい

### 1.2 スキーマのバージョン方針

- この仕様で `schema` を **1 → 2** にする。以後、**項目の追加（省略可能）ではバージョンを上げない**。意味や名前を変える破壊的変更のときだけ上げ、そのときも旧版を読むコードは持たず移行手順で済ませる
- 受理するのは常に現行版のみ。`schema` が違うレイアウトは読み込まず、従来どおり同梱の既定 → 組み込みにフォールバックする（入力を失わない）
- 「見た目の指定を使っている」ことの判定は項目の有無で行う（フラグは持たない）

### 1.3 名前の整理（破壊的変更）

- キー定義の既存 `style`（`normal`/`function`/`accent`）を **`role`**（色の役割）に改名する。Swift の `KeyStyle` enum は `KeyRole`、web の `KEY_STYLES`/`KeyStyle` は `KEY_ROLES`/`KeyRole` に改名
- 新しい見た目の指定オブジェクトの名前は **`look`** のままとする（`style` にしない理由: 実装・テスト・仕様書で `look` を既に一貫して使っており、`role` との区別も明確なため。`style` にすると `roles`（役割ごとの look）との混同も増える）
- `roles`（役割ごとの look）は **`roles`** に改名（`role` と対応）。`{ "roles": { "function": {…} } }`
- 他の既存項目（`id`・`width`・`label`・`icon`・`kind`・`longPress`・`visible` など）は**変えない**。理由: 変える利点が無く、移行の対象を減らすため

### 1.4 検証の方針（H-22 以降の流儀に合わせる）

- **構造の破損と資源の上限**（`assets` の個数・サイズ・形式、`rules` の個数、JSON 全体の大きさ）は**エラー**にして取り込みを拒否する（取り込み時にユーザーへ日本語で理由を出す）
- **見た目の値**（未知のプロパティ、範囲外、型違い、不正な色、存在しない画像・アイコンの参照）は**エラーにしない**。iOS は**その 1 プロパティだけ無視して下位の値（最終的には既定）に落とす**。レイアウト全体は無効にしない
- サイトのエディタは同じ問題を**警告**（エラーとは別枠、書き出しはブロックしない）として場所つきで出す。JSON Schema は編集時の契約として厳格（範囲・名前を縛る）にし、実行時の iOS は寛容にする
- 理由: 見た目の指定ミスでキーボードが使えなくなるのが最悪のため。一方で資源の上限は拡張のメモリ（約 177MB。H-6）を守るために必須

## 2. スタイルのプロパティ（`look` の中身）

`look` は**平らな**オブジェクト（入れ子にしない）。平らにする理由: 部分上書きを「プロパティ単位の単純な上書き」で定義でき、解決関数・TypeScript の実装・テストベクタが単純になる。

### 2.1 プロパティ表

「既定」は**そのプロパティを誰も指定していないときに現行の見た目になる値**。

| 名前 | 型 | 単位・範囲 | 既定（省略時） | 意味 |
| --- | --- | --- | --- | --- |
| `fontSize` | number | pt、8〜40 | 現行の `fontSize(key)`（`label` 3 文字以上 16、`text` キー 20、他 17。フリックは 22 / 3 文字以上 16） | 文字の大きさ。アイコンの大きさ（現行 17pt）も同じ値に従う |
| `fontWeight` | string | `ultraLight` `thin` `light` `regular` `medium` `semibold` `bold` `heavy` `black` | `regular` | 太さ（SwiftUI の `Font.Weight`） |
| `fontDesign` | string | `system` `rounded` `monospaced` `serif` | `system` | 書体系統（SwiftUI の `Font.Design`。日本語はいずれもシステムの日本語フォントにフォールバックされ、`rounded` で丸ゴシック系になるとは限らない点をエディタの説明に書く） |
| `color` | Color | §2.2 | 役割に応じる（accent は白、他は `@label`） | 文字・アイコンの色 |
| `fill` | Color | §2.2 | 役割に応じる（normal=`@key`、function=`@function`、accent=`@accent`） | キーの背景色 |
| `fillOpacity` | number | 0〜1 | 1（押下中は 0.6。§3.3） | 背景（`fill` と画像）だけの不透明度。文字・枠線・影には掛からない |
| `fillImage` | string | 画像 id（`assets` のキー）または `"none"` | なし | 背景画像（§6）。`"none"` は下位で指定された画像を外す |
| `fillImageFit` | string | `fill` `fit` `tile` | `fill` | 画像の当て方（§6.3） |
| `fillImageOpacity` | number | 0〜1 | 1 | 画像だけの不透明度 |
| `cornerRadius` | number | pt、0〜24（キーの短辺の半分で頭打ち） | 設定 `keyboard.key_corner_pt`（既定 6） | 角丸 |
| `borderWidth` | number | pt、0〜4 | 0 | 枠線の太さ（内側に描く） |
| `borderColor` | Color | §2.2 | `@separator` | 枠線の色。`borderWidth` が 0 なら無効 |
| `shadowColor` | Color | §2.2 | `#000000` | ドロップシャドウの色 |
| `shadowRadius` | number | pt、0〜16 | 0（影なし） | ぼかしの半径。0 のとき影は描かない |
| `shadowX` | number | pt、−8〜8 | 0 | 影の横オフセット |
| `shadowY` | number | pt、−8〜8 | 0 | 影の縦オフセット |
| `shadowOpacity` | number | 0〜1 | 0.3 | 影の不透明度（`shadowColor` のアルファに掛け算） |
| `opacity` | number | 0〜1 | 1 | キー全体（背景・文字・枠線・影）の不透明度 |
| `scale` | number | 0.5〜1.5 | 1 | キーの表示の拡縮（中心基準。当たり判定は変えない） |
| `offsetX` / `offsetY` | number | pt、−20〜20 | 0 | 表示位置のずらし（当たり判定は変えない） |
| `inset` | number | pt、0〜12 | 0 | キーの描画範囲を全方向に縮める（当たり判定はセル全体のまま）。キー同士の見かけの間隔を個別に広げるのに使う |
| `padding` | number | pt、0〜16 | 0 | 文字・アイコンの内側余白（長い文字は `minimumScaleFactor 0.6` の範囲で縮む） |
| `animationMs` | number | ms、0〜1000 | 0（即時。現行と同じ） | この指定への**切り替わり**の長さ。§3.4 |
| `label` | string | 1〜16 文字（`labelMax`） | — | 表示する文字の上書き。**`states` / `rules` の中だけ**で有効（§2.3） |
| `icon` | string | `icon` の書式（`sf:` / `iconify:` / 接頭辞なし。64 文字以内） | — | 表示するアイコンの上書き。**`states` / `rules` の中だけ**で有効 |

- 理由（`fillOpacity` と `opacity` の 2 つ）: 現行の押下表現が「背景だけ 0.6」であり、それを既定の押下の見た目として再現する必要があるため。キー全体を薄くする `opacity` は別に必要
- 理由（`offset`/`scale` は当たり判定を変えない）: 当たり判定が動くと、隣のキーとの境界・曖昧入力のキー矩形（`LayoutGeometry`、H-25）とずれる。見た目だけを変える
- 理由（`shadowOpacity` 既定 0.3）: 影は `shadowRadius > 0` で初めて出る。半径だけ指定した人に「それなりの影」が出る値

### 2.2 色（Color）

次のいずれか。

1. 文字列 `"#RGB"` / `"#RRGGBB"` / `"#RRGGBBAA"`（sRGB。ライト・ダーク共通）
2. 文字列のトークン（現在のテーマ・アクセントに追従する色）:

   | トークン | 意味 | 現行の対応 |
   | --- | --- | --- |
   | `@label` | 文字色 | `UIColor.label` |
   | `@secondaryLabel` | 薄い文字色 | `UIColor.secondaryLabel` |
   | `@key` | 通常キーの背景 | `UIColor.systemBackground` |
   | `@function` | 機能キーの背景 | `UIColor.systemGray4` |
   | `@accent` | アクセントカラー | 設定 `keyboard.accent`（`themeAccent`） |
   | `@separator` | 区切り線 | `UIColor.separator` |
   | `@clear` | 透明 | `UIColor.clear` |

3. オブジェクト `{ "light": <1 または 2>, "dark": <1 または 2> }`。ライト／ダークで別の色にする。**片方だけ**書いた場合、書いていない側の外観ではこのプロパティは**未指定扱い**（下位の値が使われる。もう一方を機械的に反転・複製しない）

- ライト／ダークの判定: SwiftUI の `colorScheme`（拡張の `overrideUserInterfaceStyle` = 設定 `keyboard.theme` の結果が反映される）。解決関数には `scheme: light|dark` を引数で渡す
- **既存のテーマとの関係**: テーマ（`keyboard.theme`）とアクセント（`keyboard.accent`）は従来どおり設定で決まり、レイアウトの色指定が**上書きするのは指定したプロパティだけ**。トークン `@accent` などはテーマに追従するので、色を指定しなければ今までどおり設定の変更が反映される。レイアウトが絶対色（`#...`）を指定した場合は設定のアクセントの影響を受けない（意図どおり）
- 不正な文字列（形式違い・未知のトークン・`light`/`dark` 以外のキー・空）は、そのプロパティを無視して下位の値に落とす
- 理由（トークンを用意）: 「少し薄く」「アクセント色の枠」のように、ダーク／ライトや設定の色に追従したい指定を、色を直書きせずにできるようにするため

### 2.3 ラベルの上書き（`label` / `icon`）

- 既存の `label` / `icon`（キー定義の項目）は**変えない**。`look` の `label` / `icon` は「その状態の間だけの上書き」で、**`states` と `rules` の中でだけ**受理する（`look` 本体・`roles`・行の `look` に書かれたら無視 ＋ web は警告）。理由: 静的な表示は既存の `label`/`icon` で書けるので、二重の書き方を作らない
- 表示の優先順位（高 → 低）: 有効な状態の `icon`（取得できているもの）→ 有効な状態の `label` → 現行の `faceContent` の結果（空白→「変換」、改行→「確定」、Shift の記号、`key.icon`、`key.label` …）
- 状態が `label` だけ指定し `icon` を指定しないときは、その状態の間は `key.icon` を**出さず**状態の `label` を出す（「空白」→「移動」のように文字を差し替えたいのにアイコンが残るのを防ぐ）
- `icon` は既存の書式（`docs/ios-keyboard-layout.md`「アイコン」、`KeyboardIconRef.formatError`）にそのまま従う。`iconify:` は**取り込み時に解決して保存**する必要があるので、ホストの取り込み（`IconifyResolver`）は `key.icon` に加えて `look.icon` / `states.*.icon` / `rules[].look.icon` も走査する。取得できなければ（label があれば）label を使う
- 理由: アイコンの取得は拡張では行わない（H-36）。状態の `icon` だけ取り込み漏れがあると、実機で表示されない不具合になる

## 3. 階層と解決順序

### 3.1 指定の置き場

```
レイアウト
├─ metrics        キー間隔など（§3.5）
├─ background     キーボード全体の背景（§6.4）
├─ assets         画像（§6）
├─ look           全キー共通の見た目（役割を問わない）
├─ roles          役割（role）ごとの見た目 { "normal": look, "function": look, "accent": look }
├─ rules          状態の見た目（全キー／指定キー対象）[ { when, target?, look… } ]
└─ rows[]
   ├─ look        その行の全キーの見た目
   └─ keys[]
      ├─ look     そのキーの見た目
      └─ states   そのキー自身の状態ごとの見た目 { pressed: look, … }
```

### 3.2 解決順序（低 → 高。後のものが先のものを上書き）

1. **既定**（現行の見た目）: 役割（`key.role`）に応じた色・フォント・角丸（設定値）
2. レイアウト `look`
3. レイアウト `roles[<そのキーの役割>]`
4. 行 `look`
5. キー `look`
6. 有効な**状態**の指定を、状態の優先順位（§4.3）の**低い順**に重ねる。同じ状態の中では次の順:
   1. 組み込みの既定（押下中の `fillOpacity` 0.6 だけ。§3.3）
   2. `rules` のうち `when` が一致し、`target` がそのキーに当てはまるもの（**配列の先頭から順**。後の rule が勝つ）
   3. そのキーの `states[<状態>]`

### 3.3 部分上書きのルール

- 上書きは**プロパティ単位**。上位が指定したプロパティだけが下位の値を置き換え、指定していないプロパティは下位の値のまま
- プロパティの値が**無効**（範囲外・型違い・不正な色）または `null` のときは、その指定は**無かったことにする**（下位の値が残る）。`"fillImage": "none"` だけが「下位の画像を外す」明示指定
- **押下の既定**: 現行は押下中に背景が 0.6 になる。これを「組み込みの `pressed` の既定 `fillOpacity: 0.6`」として最下位に置く。ユーザーのどの層（`look` を含む）が `fillOpacity` を指定していても**押下中は 0.6 にならない**わけではなく、`pressed` 状態の中で `rules` / `states` が `fillOpacity` を指定したときだけ 0.6 を置き換える。つまり、`look.fillOpacity: 0.8` と指定したキーの押下中は 0.6（現行どおり）になる
- 理由: 「通常の指定で押下の手応えが消える」事故を避け、押下の見た目を変えたい人は `pressed` で明示する、という一貫した規則にするため
- `look` だけを足したキー（`states` なし）でも押下は 0.6 のまま。押下の手応えを無くしたいときは `pressed` に `"fillOpacity": 1` を書く

### 3.4 アニメーション

- 状態が変わるたびに、解決結果が前と変わった場合に、`max(前の解決結果の animationMs, 後の解決結果の animationMs)` ミリ秒の `easeInOut` で補間する（0 なら即時）
- 理由（前後の最大）: 離したときの戻りを「押下中の指定の animationMs」で滑らかにしつつ、押したときは「押下中の指定」で入る、を片方の指定だけで書けるようにするため
- 既定は 0（現行は即時）。補間の対象は数値・色・不透明度。`label` / `icon` / `fillImage` は補間せず**即座に切り替える**
- 「視差効果を減らす」（`UIAccessibility.isReduceMotionEnabled`）が真のときは `animationMs` を 0 として扱い、`scale` の補間もしない（静的な値は反映する）

### 3.5 位置・サイズの整理（既存項目と新項目）

| 欲しいもの | 手段 | 備考 |
| --- | --- | --- |
| キーの幅 | 既存 `key.width`（行内の比） | 変更なし |
| 行の高さ | 既存 `row.height`（比。合計 4.5 以下） | 変更なし。キーボード全体の高さは設定の高さ倍率で決まる（`NagiKeyMetrics.totalHeight`） |
| キー間・行間の隙間、外周の余白 | 新 `metrics`: `keySpacing`（0〜8pt）、`rowSpacing`（0〜8pt）、`padding`（0〜16pt、外周） | 省略時は設定 `keyboard.key_spacing_pt`（キー間・行間とも）と現行の外周 6pt。指定があれば**設定値より優先**。理由: レイアウトの作者が意図した詰め方を、設定の既定値で崩さないため。曖昧入力の矩形（`LayoutGeometry.rects`）と高さの計算（`padHeight`/`rowsHeight`）は `keySpacing`・`rowSpacing` を別々に受けるよう直す |
| キー 1 つの見た目の位置ずれ・縮小 | 新 `look.offsetX/offsetY/inset/scale` | 当たり判定は動かさない |
| キー 1 つの当たり判定の位置 | 変更しない | 位置の自由配置（絶対座標）は v1 では扱わない。理由: 曖昧入力のキー矩形・行の按分・片手モードと衝突するため |

## 4. 状態（トリガー）

状態は**キーごと（局所）**に入るものと、**キーボード全体（大域）**に入るものに分ける。

### 4.1 状態の一覧

| 名前 | 種類 | 入る | 抜ける | 実装の対応箇所（`Keyboard/NagiKeyboardView.swift`） |
| --- | --- | --- | --- | --- |
| `pressed` | 局所 | 指がそのキーに触れた瞬間 | 指が離れた／ジェスチャが取り消された | `TapKeyView`・`SpaceKeyView`・`RepeatKeyView` の `@GestureState isPressed`、`LongPressKeyView` の `isDown`（`changed` で true、`reset` で false）、`FlickKeyView` の `drag != nil`。**フリック中・長押し中・スペースのドラッグ中も真のまま** |
| `released` | 局所 | `pressed` が**正常な終了**で抜けた瞬間（`onEnded`。入力が実行されたか、範囲外で離して取り消されたかは問わない） | `holdMs`（既定 200、0〜2000）経過後。その間に同じキーが再び押されたら即座に抜ける | 各 View の `onEnded`（`TapKeyView`／`SpaceKeyView`／`RepeatKeyView.stopRepeat`／`LongPressKeyView.ended`／`FlickKeyView.onEnded`）で `releasedUntil` を設定。システムにジェスチャを奪われた取り消し（`onEnded` が呼ばれない経路）では**入らない** |
| `longPress` | 局所 | 長押しと判定されたとき。`longPress` ポップアップを持つキーでは `flick.long_press_ms` 経過で出る瞬間（`LongPressKeyView.startTimer` の `popupShown = true`）。⌫ では連打が始まる瞬間（`RepeatKeyView.startRepeat` の初期待ち `touch.key_repeat_delay_ms` の経過後）。**それ以外のキー（`TapKeyView`・`SpaceKeyView`・`FlickKeyView`）は長押しの判定自体が無いので入らない** | 指が離れた／取り消された（`LongPressKeyView.reset`、`RepeatKeyView.stopRepeat`） | 左記 |
| `flicking` | 局所 | `FlickKeyView` で、移動量が閾値（`flick.threshold_pt`、`FlickKeyView.gesture(for:)` が `.tap` 以外）を超え、かつ取り消しの範囲（`FlickCancel`）の外にいる間 | 閾値内に戻った／取り消しの範囲に出た／離した | `FlickKeyView` の `drag`（`GestureState`）から導出 |
| `spaceDrag` | 大域（起点キーあり） | スペースキーで `SpaceSwipeTracker.isActive` が false → true になった瞬間（左右に `touch.space_swipe_step_pt` 以上動いた時点。`SpaceKeyView.onChanged` の `current.isActive && !wasActive`） | 指が離れた／取り消された（`SpaceKeyView.onEnded` と、`onChange(of: isPressed)` の取り消し経路） | `SpaceKeyView`。起点キーの id（`origin`）を `KeyboardVisualState` に書く。`touch.space_swipe` が OFF、またはスペースに `longPress` があるキーでは入らない |
| `shift` | 大域 | `KeyboardModel.shiftState == .once` | `.off` または `.caps` になった | `KeyboardModel.shiftState`（`@Published`） |
| `capsLock` | 大域 | `shiftState == .caps` | `.off` | 同上 |
| `composing` | 大域 | `KeyboardModel.hasPreedit`（入力中の文字列がある） | なくなった | `KeyboardModel.hasPreedit` |

- 局所の状態は、そのキー自身にだけ入る。`rules` で `when: "pressed"` と書くと「すべてのキー（`target` に当てはまるキー）の**それぞれの**押下中の見た目」になる
- 大域の状態は、そのレイアウトの**全キーに同時に入る**。`spaceDrag` だけは起点キー（スペース）の id を持つ
- 理由（`pressed` が他の状態の間も真）: 「押している間は常に押下の見た目」を基準にして、`longPress` などをその上に重ねる、という説明が一番単純なため
- 理由（`shift`/`capsLock` を別名）: 現行も Caps だけ accent 色で、Shift は記号が変わる、と見た目が違うため

### 4.2 `rules` の書式と `target`

```json
{ "when": "spaceDrag", "target": "all", "holdMs": 200, "look": { … } }
```

| フィールド | 内容 |
| --- | --- |
| `when` | §4.1 の状態名を 1 つ。複数条件の組み合わせ（AND）は持たない。理由: 優先順位（§4.3）と `target` で必要な表現は足りるため |
| `target` | 省略時 `"all"`。`"all"`（全キー）／`"others"`（起点以外の全キー。`when: "spaceDrag"` のときだけ有効。他では `"all"` と同じ扱い）／`{ "keys": ["id", …] }`（キー id、最大 24）／`{ "roles": ["function", …] }`（役割）／`{ "actions": ["space", …] }`（`action` 名） |
| `holdMs` | `when: "released"` のときだけ有効（0〜2000、既定 200） |
| `look` | §2 の `look`（`label`/`icon` も可） |

- `target` に当てはまるキーに `spacer` は含めない（描かない）
- `rules` は最大 32 個。多すぎる場合は取り込みエラー（§1.4）
- `target.keys` の id がレイアウトに無くても無視（エラーにしない）

### 4.3 複数の状態が同時のときの優先順位

有効な状態を、次の順に**低 → 高**で重ねる（後が勝つ。§3.2 の 6）。

`composing` < `capsLock` < `shift` < `released` < `pressed` < `spaceDrag` < `flicking` < `longPress`

- 理由: 持続する状態（入力中・Shift）が一番下、指の状態が上、より特殊な操作（ドラッグ移動・フリック・長押し）がさらにその上。`spaceDrag` を `pressed` より上にしたのは、ドラッグ中のスペースキーに `pressed` の指定（縮小など）が混ざっても、「移動中の見た目」が最終的に勝つようにするため
- `released` は `pressed` の直後にしか入らず、同じキーが再び押されたら消えるので、`pressed` とは実際には重ならない（優先順位は形式的なもの）

### 4.4 「全キー」と「そのキー自身」の書式（例）

- **スペースが移動状態のとき、全キーを薄くする**: `rules` に `{ "when": "spaceDrag", "target": "others", "look": { "opacity": 0.35 } }`
- **スペース自身のラベルを「移動」にして少し薄くする**: スペースキーの `states` に `"spaceDrag": { "label": "移動", "opacity": 0.7 }`（またはスペースを `{ "keys": ["sp"] }` で指す rule）
- 理由（2 つの書き方）: キー自身の指定はキーの定義の近くに置いて読みやすく、全体への指定は 1 か所にまとめたいため。両方同じ解決順序（§3.2）で処理する

## 5. JSON スキーマとサンプル

### 5.1 追加項目の骨子（TypeScript 風）

```ts
type Color = string | { light?: string; dark?: string }
interface Look {                      // 平らなオブジェクト。§2.1 の表のプロパティだけ
  fontSize?: number; fontWeight?: string; fontDesign?: string
  color?: Color; fill?: Color; fillOpacity?: number
  fillImage?: string; fillImageFit?: 'fill' | 'fit' | 'tile'; fillImageOpacity?: number
  cornerRadius?: number; borderWidth?: number; borderColor?: Color
  shadowColor?: Color; shadowRadius?: number; shadowX?: number; shadowY?: number; shadowOpacity?: number
  opacity?: number; scale?: number; offsetX?: number; offsetY?: number
  inset?: number; padding?: number; animationMs?: number
  label?: string; icon?: string       // states / rules の中だけ
}
type StateName = 'pressed' | 'released' | 'longPress' | 'flicking' | 'spaceDrag' | 'shift' | 'capsLock' | 'composing'
type StateLook = Look & { holdMs?: number }   // holdMs は released のみ
type Target = 'all' | 'others' | { keys: string[] } | { roles: KeyRole[] } | { actions: string[] }

interface KeyboardLayout {            // 追加分のみ
  look?: Look
  roles?: { normal?: Look; function?: Look; accent?: Look }
  rules?: { when: StateName; target?: Target; holdMs?: number; look: Look }[]
  metrics?: { keySpacing?: number; rowSpacing?: number; padding?: number }
  background?: Pick<Look, 'fill' | 'fillOpacity' | 'fillImage' | 'fillImageFit' | 'fillImageOpacity'>
  assets?: Record<string, string>     // id → data URI（§6）
}
interface Row { look?: Look }
interface KeyBase { look?: Look; states?: Partial<Record<StateName, StateLook>> }
```

- `config/keyboard/layout.schema.json` に `definitions`（`color`・`look`・`stateLook`・`rule`・`target`・`assets`・`metrics`）を足し、`look` 系は `additionalProperties: false`（名前を縛る）。`web/src/lib/keyboard/types.ts` に同じ定数（プロパティ名の一覧・範囲 `LOOK_LIMITS`）を持つ。範囲の定数は Swift の `KeyLookLimits`、TypeScript の `LOOK_LIMITS`、スキーマの 3 か所で同じ値にする（既存の `LIMITS` と同様に `keyboard.test.ts` の「constants match schema」で突き合わせる）
- 資源の上限: `assets` 4 個以内、`rules` 32 個以内、`rule.target.keys` 24 個以内（§6.2）

### 5.2 例 ①: 全体の `look` だけ（丸み・影・ダーク／ライト別の色）

```json
{
  "schema": 2,
  "id": "soft-flick",
  "name": "やわらか 12キー",
  "mode": "kana",
  "metrics": { "keySpacing": 5, "rowSpacing": 5 },
  "look": {
    "cornerRadius": 12,
    "fontDesign": "rounded",
    "shadowRadius": 2,
    "shadowY": 1,
    "shadowOpacity": 0.2
  },
  "roles": {
    "normal": { "fill": { "light": "#FFFFFF", "dark": "#3A3A3C" } },
    "function": { "fill": { "light": "#D9DCE3", "dark": "#2C2C2E" }, "fontWeight": "medium" },
    "accent": { "fill": "@accent" }
  },
  "rows": [
    { "keys": [
      { "id": "l", "width": 0.7, "label": "←", "kind": "action", "action": "moveLeft", "role": "function" },
      { "id": "a", "kind": "flick", "kana": "あ", "label": "あ" },
      { "id": "bs", "width": 0.85, "label": "⌫", "kind": "action", "action": "backspace", "role": "function" }
    ] }
  ]
}
```

（実在のレイアウトは行・キーをすべて書く。ここでは短縮した。他の例も同じ）

### 5.3 例 ②: キーごとの `look` ＋ `pressed` / `longPress`

```json
{
  "schema": 2,
  "id": "qwerty-glow",
  "name": "QWERTY 光る押下",
  "mode": "alpha",
  "rules": [
    { "when": "pressed", "target": "all", "look": { "scale": 0.94, "fillOpacity": 1, "animationMs": 80 } },
    { "when": "released", "target": "all", "holdMs": 150, "look": { "animationMs": 150 } }
  ],
  "rows": [
    { "keys": [
      { "id": "q", "kind": "text", "text": "q", "shift": "Q" },
      { "id": "w", "kind": "text", "text": "w", "shift": "W",
        "look": { "fill": { "light": "#FFF4D6", "dark": "#4A3F1F" }, "borderWidth": 1, "borderColor": "#E0B341" },
        "states": {
          "pressed": { "fill": "#E0B341", "color": "#000000" },
          "longPress": { "scale": 1.08, "shadowRadius": 8, "shadowOpacity": 0.5, "shadowColor": "#E0B341" }
        },
        "longPress": [
          { "label": "ŵ", "kind": "text", "text": "ŵ" },
          { "label": "ẃ", "kind": "text", "text": "ẃ" }
        ] }
    ] }
  ]
}
```

### 5.4 例 ③: `spaceDrag` で空白を「移動」ラベル＋全体を薄く

```json
{
  "schema": 2,
  "id": "qwerty-alpha-cursor",
  "name": "QWERTY 英字（移動中の表示つき）",
  "mode": "alpha",
  "rules": [
    { "when": "spaceDrag", "target": "others", "look": { "opacity": 0.35, "animationMs": 120 } }
  ],
  "rows": [
    { "keys": [
      { "id": "mode", "width": 1.2, "label": "あ", "kind": "action", "action": "modeSwitch", "role": "function" },
      { "id": "sp", "width": 4.4, "label": "空白", "kind": "action", "action": "space",
        "states": {
          "spaceDrag": { "label": "移動", "opacity": 0.7, "fontWeight": "semibold", "animationMs": 120 }
        } },
      { "id": "en", "width": 1.6, "label": "改行", "kind": "action", "action": "enter", "role": "function" }
    ] }
  ]
}
```

（スペースのドラッグ移動は設定 `touch.space_swipe` が ON、かつスペースに `longPress` が無いときだけ入る。OFF なら `spaceDrag` は入らず、見た目は通常のまま）

### 5.5 検証ルール（早見表）

| 対象 | 扱い |
| --- | --- |
| `schema != 2` | エラー（読み込まない。旧形式 1 の変換コードは持たない。§12） |
| `look`/`roles`/`states`/`rules`/`metrics`/`background` が object でない | その項目全体を無視（エラーにしない）。web は警告 |
| 未知のプロパティ名（`look` 内、`states` のキー、`roles` のキー、`rules[]` のフィールド） | iOS: 無視。web: 警告。JSON Schema: 不一致（外部ツール向けの契約） |
| 範囲外・型違い・不正な色・未知の `fontWeight` 等 | そのプロパティだけ無視して下位の値。丸め込み（クランプ）はしない。理由: 「意図と違う値で動く」より「指定なしで動く」ほうが原因を追いやすいため |
| `fillImage` の id が `assets` に無い／デコードできない | 画像なし（`fill` の色だけ）。web は警告 |
| `label` / `icon` が `look` 本体・`roles`・行の `look` にある | 無視。web は警告 |
| `assets`: 個数 > 4、1 枚 > 256KB（デコード後のバイト）、合計 > 768KB、MIME が `image/png` / `image/jpeg` 以外、base64 不正、id が `^[a-z0-9][a-z0-9_-]{0,31}$` でない | **エラー**（取り込み拒否。web はエラー表示。書き出しをブロック） |
| `rules` > 32 個、`rule.when` が未知、`target.keys` > 24 個 | `rules` の個数超過は**エラー**。`when` が未知の rule は無視（警告） |
| JSON 全体のサイズ | 1.5MB 超は取り込みエラー（ホスト `LayoutManagerView.maxBytes` を 512KB から 1.5MB に上げる。`assets` が無いレイアウトは従来どおり実質 512KB 以下に収まる）。web のファイル読み込み上限（`JsonPanel` の 1MB）も 1.5MB に揃える。**`assets` を含むレイアウトは URL スキーム（上限 1MB）では渡せない**（共有 UI はファイル／コピーを案内する） |

## 6. 背景画像

### 6.1 保存形式

- レイアウト JSON の `assets` に **base64 の data URI** で埋め込む。`{ "assets": { "wood": "data:image/jpeg;base64,/9j/4AAQ…" } }`
- 参照は `look.fillImage: "wood"`、または `background.fillImage`。同じ画像を複数のキー・状態から参照でき、重複して持たない
- 理由: ユーザーの指示（JSON に埋め込む）。共有・書き出し・URL でない取り込みが 1 ファイルで完結し、アイコン（H-36）のような「取り込み時の外部取得」が要らない

### 6.2 形式と上限

| 項目 | 値 | 理由 |
| --- | --- | --- |
| 形式 | `image/png`、`image/jpeg` | ブラウザ（canvas）で生成でき、iOS の ImageIO で確実にデコードできる。HEIC・WebP は web のエディタで加工しにくく（ブラウザ差）、扱わない |
| 1 枚の大きさ | デコード後（base64 を戻したバイト）256KB 以内、長辺 1024px 以内 | 長辺 1024px は、390pt 幅の全面背景（@3x で 1170px）をわずかに間引くだけで足りる大きさ |
| 枚数・合計 | 4 枚以内、合計 768KB 以内 | JSON 全体を 1.5MB（base64 で約 4/3 倍）に収めるため |
| 実行時のメモリ | 画像のデコード結果の合計 12MB 以内 | 拡張の上限（約 177MB。H-6）のうち、LLM を載せる small でもピーク約 78MB（H-10 のモデル分を含む実測）で、余裕が数十 MB しかないため。1024×1024 の RGBA で約 4MB |

### 6.3 デコードとキャッシュ（拡張）

- レイアウト読み込み時には画像を**デコードしない**（data URI の文字列を `KeyboardLayoutDef.assets` に持つだけ）。初めて**描画に必要になった**とき、バックグラウンドのキュー（`DispatchQueue.global(qos: .userInitiated)`）で `CGImageSourceCreateThumbnailAtIndex`（`kCGImageSourceThumbnailMaxPixelSize` = 表示サイズ（pt）× 画面の scale と 1024 の小さいほう、`kCGImageSourceCreateThumbnailFromImageAlways` と `…WithTransform`）でダウンサンプリングしてデコードする。デコード中は `fill` の色だけで描き、完了したら再描画する
- キャッシュ: `NSCache<NSString, CGImage>`（キー = `assetId@maxPixel`、`totalCostLimit` = 12MB、cost = `幅×高さ×4`）。メモリ警告（`didReceiveMemoryWarning` / `UIApplication.didReceiveMemoryWarningNotification`）とキーボードの非表示（H-26 の解放処理）で `removeAllObjects()`
- メモリが逼迫しているとき（`os_proc_available_memory()` が 40MB 未満）は画像のデコードをせず、色だけで描く
- `fit` の意味: `fill` = 縦横比を保って全体を覆い、はみ出しを切る（`scaledToFill` + クリップ）／`fit` = 縦横比を保って全体を収める（余白は `fill` の色が見える。`scaledToFit`）／`tile` = 元の大きさ（1pt = 3px。@3x として扱う）で敷き詰める
- 角丸のクリップ: キーの形（`cornerRadius`、`inset` 後）で画像・背景色をクリップする
- 画像の `tile` は、1 枚が 1pt 未満になる極端に小さい画像を避けるため、1 辺 8px 未満の画像では `fill` として扱う
- 理由（`fill`/`fit`/`tile` の 3 つ）: ユーザーの指示。引き伸ばし（stretch）は縦横比が崩れて見栄えが悪く、省く

### 6.4 画像の範囲

- **キーごと**: 各キーの `look.fillImage`（キーの形の中）。状態でも切り替えられる（押下中だけ別の画像、など）
- **キーボード全体**: `background`（`fill`/`fillOpacity`/`fillImage`/`fillImageFit`/`fillImageOpacity`）。ルートの view 全体（候補バー・キー領域を含む、システムのフッターは含まない）の最背面に描く。省略時は現行どおり**塗らない**（H-61 のシステム背景）。キーの `fill` を `@clear` または低い `fillOpacity` にすると背景が透けて見える
- 理由: 「キー 1 つ 1 つに同じ画像を当てる」より「全体の背景に 1 枚敷いてキーを半透明にする」ほうが、画像が少なくて済み（上限内）、よく使われる見た目のため

## 7. iOS 実装計画

### 7.1 追加・変更するファイル

| ファイル | 内容 |
| --- | --- |
| `Shared/KeyLook.swift`（新規） | 型: `KeyLook`（§2 のプロパティ。すべて Optional）、`LookColor`（`.rgba` / `.token` / `.pair(light, dark)`）、`LookStateName`、`LookRule`、`LookTarget`、`LayoutMetrics`、`LayoutBackground`、`KeyLookLimits`（範囲の定数）。**プロパティ単位の寛容な decode**（`KeyLook.init(from:)` で動的な CodingKey を使い、各プロパティを `try?` で読む。型違いは nil）。色の文字列の解析 `LookColor.parse(_:)` |
| `Shared/KeyLookResolver.swift`（新規・純粋関数） | `LookContext`（`states: Set<LookStateName>`、`origin: String?`、`scheme: LookScheme`）、`ResolvedLook`（全プロパティが Optional。nil は「既定のまま」。色は解決済みの `ResolvedColor`）、`LookResolver.resolve(layout:rowIndex:key:context:) -> ResolvedLook`、`LookSanitizer.sanitize(_:) -> KeyLook`（範囲外・不正を nil に落とす）、`LookDefaults`（現行の見た目の定数。`fontSize(key)` 相当・役割ごとの色・押下の 0.6 など。単体テストで現行値と照合）。UIKit・SwiftUI に依存しない（XCTest で直接検証） |
| `Shared/KeyboardLayout.swift` | `LayoutKeyDef`（`look`、`states`）、`LayoutRow`（`look`）、`KeyboardLayoutDef`（`look`、`roles`、`rules`、`metrics`、`background`、`assets`）を追加。decode は `try?`（失敗は nil）。`LayoutValidation` に assets・rules の上限（§5.5 のエラー）と icon 書式（`look.icon`）の検査を追加。`CodingKeys` と、コードから作る `init` にも追加 |
| `Shared/LayoutGeometry.swift` | `rects` / `snapshot` が `keySpacing` と `rowSpacing` を別に受ける（`metrics` 対応）。既定は同じ値を渡すので結果は変わらない（`LayoutGeometryTests` を維持） |
| `Keyboard/KeyLookRenderer.swift`（新規） | `StyledKeyFace`（SwiftUI）: `ResolvedLook` を受けて背景・画像・枠・影・文字・不透明度・拡縮・オフセットを描く。`LookPalette`（`ResolvedColor` → `Color`。トークンは §2.2 の UIColor、アクセントは `themeAccent`） |
| `Keyboard/KeyboardVisualState.swift`（新規） | `final class KeyboardVisualState: ObservableObject`（`@Published private(set) var spaceDragOrigin: String?`。値が変わるときだけ更新）。`LookCache`（キー × 状態集合 × scheme → `ResolvedLook` のメモ化。レイアウトが変わったら破棄） |
| `Keyboard/KeyImageCache.swift`（新規） | §6.3 のデコードとキャッシュ |
| `Keyboard/NagiKeyboardView.swift` | `KeyFace` を**削除**し、呼び出し箇所を `StyledKeyFace` に置換（解決結果を渡す薄いラッパ `KeyFaceHost` を経由）。`KeyStyle`→`KeyRole` の改名に追従。各キー View に局所状態の報告を追加（§7.3）。`LayoutPad` に `metrics`・`background` を反映。`SpaceKeyView` が `spaceDrag` を `KeyboardVisualState` に書く |
| `App/LayoutManagerView.swift` | `maxBytes` を 1.5MB に。`assets` の検査結果の表示。`IconifyResolver` が `look.icon` / `states.*.icon` / `rules[].look.icon` も走査（`App/IconifyResolver.swift` の走査対象の拡張） |
| `Tests/KeyLookTests.swift`（新規）ほか | §7.5 |
| `config/keyboard/layout.schema.json` | §5.1 |
| `config/keyboard/test-looks/`（新規） | 解決のテストベクタ（§9 ①）。Swift と web の両方が読む |

`project.yml` の `NagiSpikeTests` の `Shared/` 参照は既存の指定に含まれるので、新規ファイルは追加設定なしで入る（`Keyboard/` の純粋な部分をテストしたい場合は `KeyLookResolver.swift` を `Shared/` に置くことで足りる）。

### 7.2 レンダラの構造

- **スタイル解決は純粋関数**（`LookResolver.resolve`）。入力 = レイアウト定義・行・キー・文脈（有効な状態・起点・外観）。出力 = `ResolvedLook`（Equatable）。副作用なし・UI 非依存
- `KeyFaceHost`: キーの `ResolvedLook` を `LookCache` から取り、`StyledKeyFace` に渡すだけ。指定が無ければ既定値（現行の見た目。`LookDefaults`）で描く（§1.1）
- `StyledKeyFace` の描画順: ①影（背景の形に付ける）→ ②背景色（`fill` × `fillOpacity`）→ ③背景画像（クリップ、`fillImageOpacity` × `fillOpacity`）→ ④枠線（内側）→ ⑤文字／アイコン（`padding`、`color`、`fontSize`、`fontWeight`、`fontDesign`、長い文字は `minimumScaleFactor(0.6)`）。これらを `inset` で縮めた領域に描き、全体に `opacity`・`scaleEffect`・`offset` を掛ける。`contentShape` は元のセル全体のまま（当たり判定を変えない）
- 文字の内容: `faceContent(key, model)`（現行）の結果に、有効な状態の `label` / `icon` の上書き（§2.3）を適用する
- 押下の既定（背景 0.6）: `ResolvedLook` に組み込みの `pressed` 既定を含めて解決する（§3.3）

### 7.3 状態の購読と再描画の局所化

- **局所の状態**（`pressed`/`released`/`longPress`/`flicking`）は、各キー View が自分の `@State` / `@GestureState` で持つ（現在の `isPressed` などをそのまま使う）。他のキーを再描画しない
  - `released`: `onEnded` で `releasedUntil = now + holdMs` を設定し、`Task { try? await Task.sleep(...) ; releasedUntil = nil }`。再押下で即クリア。View が消える（`onDisappear`）ときはタスクを cancel
  - `longPress`: `LongPressKeyView` は `popupShown`、`RepeatKeyView` は連打開始（`startRepeat` の初期待ちが終わった瞬間に `@State isLongPress = true`、`stopRepeat` で false）
  - `flicking`: `FlickKeyView` の `drag` から導出（`gesture(for:) != .tap` かつ `!isCancelled`）
  - 局所状態を使うスタイルが**そのキーに無い**（`resolve` が状態で変わらない）ときは、状態の更新を行わない（`@State` への書き込みを省く）。理由: ほとんどのキーは何も指定せず、無駄な再描画を作らないため
- **大域の状態**: `shift`/`capsLock`/`composing` は `KeyboardModel` の既存の `@Published`（`shiftState`/`preedit`）から取る。`KeyCell` は既に `@ObservedObject model` なので追加の購読は不要。`spaceDrag` だけ `KeyboardVisualState` を環境（`EnvironmentObject`）で渡す
- **全キーを再描画しない工夫**:
  1. `LookCache`: `(keyId, 状態集合, origin の一致, scheme)` → `ResolvedLook` をメモ化。状態が変わっても未関係のキーは同じ `ResolvedLook` を返す
  2. `StyledKeyFace` は `Equatable`（`ResolvedLook` と内容が同じなら再描画しない）にして `.equatable()` を付ける
  3. 大域の状態を使う `rules` が**レイアウトに無い**とき（`hasGlobalRules == false`）、`KeyboardVisualState` の変化を読まない（`KeyFaceHost` を 2 種類に分け、大域の状態を読まない側は環境オブジェクトを参照しない）
  4. `spaceDragOrigin` は `nil ⇄ 値` の変化時にしか publish しない（ドラッグ中の毎フレームは更新しない）
- **アニメーション**: `StyledKeyFace` に `.animation(duration > 0 ? .easeInOut(duration: duration) : nil, value: resolved)`（`duration` は §3.4）。`label`/`icon`/`fillImage` の切り替えは `.transaction { $0.animation = nil }` を適用した別のレイヤーで即時にする。Reduce Motion のときは nil

### 7.4 Swift をローカルでコンパイルできないことへの配慮

- 純粋な部分（`KeyLook`・`KeyLookResolver`・`LookColor`・画像の上限検査）を UIKit/SwiftUI に依存させず、XCTest（`ios-test` ワークフロー）で検証する。SwiftUI の描画部分は薄く保ち、`ios-testflight` のビルド成功と実機で確認する
- 各段階は 1 つの PR 相当（1 コミット）に収め、CI（`ios-test` → `ios-testflight`）を通してから次へ進む

### 7.5 XCTest の方針

| テスト | 内容 |
| --- | --- |
| `KeyLookDecodeTests` | 寛容な decode（型違い・未知のキー・null は該当プロパティだけ nil）。`look` が object でない場合の無視。`schema: 1` は拒否 |
| `LookColorTests` | `#RGB`/`#RRGGBB`/`#RRGGBBAA`、トークン、`{light,dark}`（片方だけ）、不正（`#GG`、`@unknown`、空、数値）→ nil |
| `LookSanitizeTests` | 各プロパティの範囲の境界値（内側・外側）。範囲外は nil（クランプしない） |
| `LookResolverTests` | 階層の優先順位（§3.2）、部分上書き、`null` と無効値、押下の既定 0.6、`label` のみ指定時の `icon` 抑制、`rules` の `target`（all/others/keys/roles/actions）、状態の優先順位（§4.3）、`released` の `holdMs`、`animationMs` の最大。`config/keyboard/test-looks/*.json` のテストベクタを読んで期待値と突き合わせる |
| `PresetLayoutTests`（追加） | 移行後の全プリセットが `schema: 2` で検証を通り、`look` 系の指定を持たない（`LookDefaults` だけで描かれる）。`LookDefaults` が現行の値（役割ごとの色・角丸 6・fontSize の規則・押下 0.6）と一致 |
| `LayoutValidationTests`（追加） | `assets` の上限・形式・id、`rules` の個数、`look.icon` の書式 |
| `LayoutGeometryTests`（追加） | `keySpacing` と `rowSpacing` を別にしたときの矩形、同じ値なら従来と同一 |
| `KeyImageCacheTests` | 上限（cost）で古いものが追い出される、ダウンサンプリング後の大きさ ≤ 指定。小さな PNG の data URI をテスト内で生成して使う |

## 8. Web エディタ実装計画

### 8.1 画面（`/custom-keyboard` のインスペクタに「詳細設定」を足す）

- インスペクタ（`+page.svelte` の `aside.inspector`）の見出しの下に**タブ**「基本」「詳細設定」を置く。「基本」は現行の `LayoutMetaForm`／`KeyForm` をそのまま入れる（変更なし）。「詳細設定」を新設コンポーネント `LookInspector.svelte` にする。理由: 現行のページに「カスタム」というタブは無く、ユーザーの言う「カスタム」タブ＝カスタムキーボードのエディタ。既存の画面構成を壊さず、インスペクタ内で完結させる
- **スコープの選択**（`LookInspector` の上部）: 「全体」「行」「キー」。キャンバスで選択したものに自動で追従する（キー未選択 → 全体。1 つ選択 → キー。複数選択 → キー（選択した全部に同じ値を書く）。行の選択は行見出しのボタンから）
- **状態タブ**（スコープの下）: 「通常」「押下」「離した直後」「長押し」「移動中」「フリック中」「入力中」「Shift」「Caps」。「通常」はスコープの `look`（全体なら `look` と、役割ごとの `roles`）、他は状態の指定（キーなら `states[<状態>]`、全体なら `rules` で `when` が一致するもの）
  - スコープが「全体」で状態タブ（通常以外）のとき、対象を選ぶ行を出す: 「全キー」「他のキー（移動中のみ）」「指定キー」「役割」「操作」。1 つの状態に複数の rule を持てるよう「対象を追加」ボタンで rule を足す（各 rule は `target` と `look` を持つ）
  - 「全体」の「通常」では、全キー共通の `look`、役割ごとの `roles`（セグメント切替）、`metrics`、`background` をセクションで並べる
- **プロパティ入力**（共通部品 `LookField.svelte`）: プロパティごとに 1 行。左のチェックボックス＝「指定する」（外すと省略＝下位を継承）。外れている間は解決結果（継承される値）を薄く表示する。右に入力（数値＝スライダー＋数値欄＋単位、列挙＝セレクト、色＝カラー入力＋アルファスライダー＋「ライト／ダークを分ける」切替（分けると 2 つの色入力）＋トークンのプルダウン、`label`/`icon`＝テキスト欄（`icon` は既存のアイコン入力欄を再利用）、`fillImage`＝画像の選択）。行の右端に「リセット」（その指定を消す）
- **画像**: 「画像を追加」でファイルを選ぶ → `<canvas>` で長辺 1024px 以内に縮小 → 透過がなければ JPEG（品質 0.85）、あれば PNG → サイズが 256KB を超えれば品質を下げて再試行（JPEG のみ。PNG は縮小を強める）→ data URI にして `assets` に追加。「使用量 xxx / 768KB」を表示。削除は参照している指定も外す確認つき。`fit` の切替と不透明度スライダー
- **プレビューでの状態切替**: プレビュー（`KeyboardPreview` / `KeyboardCanvas`）の上に「プレビューの状態」のチェックボックス列（押下／離した直後／長押し／移動中／フリック中／入力中／Shift／Caps）。局所状態（押下・離した直後・長押し・フリック中）は**選択中のキー（無ければ全キー）**に、大域状態（移動中・入力中・Shift・Caps）は全キーに適用して表示する。移動中の起点は `action: space` のキー（無ければ最初のキー）。状態タブを切り替えると、プレビューの状態も同じものを自動でオンにする（手動で変更も可）
- プレビューの描画は CSS で近似する（背景＝`background`、影＝`box-shadow`、枠＝`box-shadow: inset`、拡縮＝`transform`、画像＝`background-image`＋`background-size`: `fill`→`cover`、`fit`→`contain`、`tile`→`画像の実寸 / 3 px`。書体＝`ui-rounded` / `ui-monospace` / `ui-serif` を先頭にしたフォントスタック）。「実機とは細部が異なります」の注記を出す。色トークンは CSS 変数（`--kb-*`、既存の `--key-bg` などに対応）にマップし、ライト／ダークは既存のプレビューの配色切替に従う
- JSON との双方向同期: エディタの状態は `store.layout`（JSON の実体）が唯一の正。フォームの変更は `ops.ts` の純粋関数を介して `store` に反映され、`JsonPanel` の出力が即座に変わる。JSON の貼り付け・読み込みで `store` が置き換わればフォームも追従する（フォームは常に `store.layout` から導出する）。空になった `look`/`states`/`rules` は取り除く（JSON を汚さない）

### 8.2 影響するファイル

| ファイル | 変更 |
| --- | --- |
| `web/src/lib/keyboard/types.ts` | `Look` / `StateName` / `Rule` / `Target` 等の型、`LOOK_PROPS`（プロパティ名・型・範囲の表）、`LOOK_LIMITS`、`LOOK_STATES`（名前・表示名・種類・優先順位）、`TOKENS`、`ASSET_LIMITS` |
| `web/src/lib/keyboard/validate.ts` | `ValidationError` に `severity: 'error' \| 'warning'` を追加（既定 `'error'`）。`validateLooks`（§5.5 の表のとおり。look 系は warning、assets・rules の上限は error）。既存の `extra()` は look 系には使わず別処理（警告にするため） |
| `web/src/lib/keyboard/look-resolve.ts`（新規） | Swift の `LookResolver` と**同じ規則**の TypeScript 実装（`resolveLook(layout, rowIndex, key, ctx)`）。エディタが「継承される値」を出すのと、プレビューの描画に使う |
| `web/src/lib/keyboard/look-color.ts`（新規） | 色の解析・トークン・ライト／ダーク別の解決 |
| `web/src/lib/keyboard/look-assets.ts`（新規） | 画像の縮小・data URI 化・サイズ計算（ブラウザ依存部分は薄く分離し、サイズ計算・上限判定は純粋関数） |
| `web/src/lib/keyboard/ops.ts` | `setLookProp(layout, scope, state, prop, value)`、`clearLookProp`、`addRule`/`updateRule`/`removeRule`、`addAsset`/`removeAsset`、`setMetrics`、`setBackground`、空の掃除。すべて入力を変更しない純粋関数（既存の流儀） |
| `web/src/lib/keyboard/store.svelte.ts` | 上記 ops を呼ぶメソッド（`History` の合体＝スライダー操作を 1 つの undo 単位にまとめる既存の仕組みを使う）、`inspectorTab`・`lookScope`・`lookState`・`previewStates` の UI 状態 |
| `web/src/lib/components/LookInspector.svelte`（新規）・`LookField.svelte`（新規）・`ColorField.svelte`（新規）・`ImageAssets.svelte`（新規）・`RulesEditor.svelte`（新規） | §8.1 |
| `web/src/lib/components/KeyboardPreview.svelte` | `previewStates` と `look-resolve` に基づいて CSS 変数・インラインスタイルを適用（指定なしのキーは既定値＝現行の見た目） |
| `web/src/lib/components/ErrorList.svelte` | `severity` で「エラー」「警告」を分けて表示。警告は書き出しをブロックしない。クリックで該当の入力へジャンプ（既存の `onjump`） |
| `web/src/lib/components/JsonPanel.svelte` | 読み込み上限を 1.5MB に。書き出し時に `assets` を含む旨と、URL スキームでは渡せない旨を表示。`invalid` は error のみで判定 |
| `web/src/routes/custom-keyboard/+page.svelte` | インスペクタのタブ |
| `config/keyboard/layout.schema.json` | §5.1 |
| `config/keyboard/test-looks/*.json` | 解決のテストベクタ（§9 ①） |
| `web/README.md`・`docs/ios-keyboard-layout.md` | 仕様の追記（実装完了時） |

### 8.3 iconify 書式との整合

- `look.icon` / `states.*.icon` / `rules[].look.icon` は、キーの `icon` と**同じ検査**（`validateIcon`。`sf:` / `iconify:` / 接頭辞なし。64 文字以内）を通す。入力欄は既存のアイコン入力部品を再利用する
- プレビューは既存どおり、`iconify:` をブラウザから `api.iconify.design` で読んで表示する（失敗時は label 表示）。書き出し JSON には ID のみ
- 理由: アイコンの書式の検査・表示は 1 か所（`validateIcon`、アイコン入力部品）に保つ

### 8.4 テスト（既存の流儀: vitest、`keyboard.test.ts` に追記または `look.test.ts` を新設）

- `validateLooks`: §5.5 の表の各行（warning と error の別、パスの形式 `rows[0].keys[1].states.pressed.opacity`）
- `schema agreement`: ajv と `validateLayout` が look 系の良い／悪い入力で一致（warning もスキーマ違反として数える）。`LOOK_LIMITS` とスキーマの範囲が一致
- `look-resolve`: Swift と同じテストベクタ（`config/keyboard/test-looks/*.json`）。階層、部分上書き、状態の優先順位、`target`
- `ops`: 入力を変更しない、空の掃除、`setLookProp` → `clearLookProp` で元の JSON に戻る
- 画像: サイズ計算・上限判定（純粋関数）。canvas 部分は手動確認
- `npm run check`（svelte-check）と `npm test`（`.github/workflows/web.yml` が走る）を通す

## 9. 実装の順序と受け入れ条件

### フェーズ ①: スキーマ ＋ 解決関数 ＋ 単体テスト（描画は変えない）

- 作業: `layout.schema.json`、`types.ts`、`validate.ts`（警告対応）、`look-resolve.ts` / `look-color.ts`、Swift の `KeyLook.swift` / `KeyLookResolver.swift` / `KeyboardLayout.swift` の項目追加と上限検査、`config/keyboard/test-looks/` のテストベクタ、テスト（§7.5、§8.4）
- 受け入れ条件:
  1. §12 で移行した全プリセット・テスト用レイアウト・組み込みの JSON が `schema: 2` で検証を通る（iOS `PresetLayoutTests`、web の `presets` テスト）。`schema: 1` は拒否される
  2. Swift と TypeScript が、同じテストベクタで**同じ解決結果**を出す（`ios-test` と `npm test` が両方通る）
  3. §5.2〜5.4 の 3 つの例が検証を通り、不正な値の例は「そのプロパティだけ無視」「資源の上限はエラー」になる
  4. `style`→`role` の改名が Swift・web・スキーマ・テストのすべてで揃っている（`LayoutValidationTests`・`keyboard.test.ts` が通る）
  5. 描画は未変更（`KeyFace` のまま）で、`role` 改名に追従するだけ。`ios-testflight` のビルドが通る

### フェーズ ②: iOS の描画（全体／個別の `look`）

- 作業: `KeyLookRenderer.swift`（`StyledKeyFace`）、`KeyFace` の削除と `StyledKeyFace` への置換（`KeyFaceHost`）、`metrics`・`background` の反映、`LayoutGeometry` の `keySpacing`/`rowSpacing` 分離、押下の既定 0.6、`animationMs`（状態なしでは使わないが骨組みだけ）
- 受け入れ条件:
  1. 指定の無い全同梱レイアウトの見た目・押下・長押し・フリック・曖昧入力が**置換前と変わらない**（実機で `qwerty-alpha`／`flick-kana`／`symbols`／`numbers`／`emoji` を確認、ライト・ダーク・各アクセント。`ios-device-checklist.md` に確認項目を追記）
  2. §5.2 の例で、角丸・影・書体・ライト／ダーク別の色が反映され、設定のテーマ切替（system/light/dark）に追従する
  3. `inset`/`offset`/`scale` を指定しても当たり判定は変わらない（端のタップで隣のキーに入らない）
  4. `ios-test`・`ios-testflight` のビルドが通る
  5. `metrics` を指定しない限り `LayoutGeometry` の結果が従来と同一（既存テスト）

### フェーズ ③: 状態トリガー

- 作業: `KeyboardVisualState`、各キー View の局所状態（`pressed`/`released`/`longPress`/`flicking`）、`spaceDrag`、`shift`/`capsLock`/`composing`、`rules` の適用、`LookCache`、`.equatable()`、アニメーション、Reduce Motion、`label`/`icon` の上書き（ホスト取り込みの `IconifyResolver` 走査の拡張を含む）
- 受け入れ条件:
  1. §5.4 の例: スペースを左右に `touch.space_swipe_step_pt` 以上ドラッグした瞬間に、スペースが「移動」ラベル＋薄くなり、他のキーが薄くなる。離すと元に戻る（指を離した後にスペース入力は入らない既存の動作は不変）
  2. `touch.space_swipe` OFF のとき、`spaceDrag` は入らない
  3. §5.3 の例: 押下で縮小、`longPress` ポップアップが出た瞬間に光る、離した後 150ms かけて戻る
  4. ⌫ の連打開始で `longPress` が入る
  5. ジェスチャがシステムに奪われた場合も、状態が残らない（`released` は入らない。実機で通知センターの引き下ろしなどで確認）
  6. 押下ごとに他のキーが再描画されない（Instruments の SwiftUI body 回数、または `Self._printChanges()` を一時的に入れて確認。確認後に消す）
  7. 状態を使わない既存レイアウトでは、パフォーマンス（入力から画面反映）が悪化しない
  8. `iconify:` を状態の `icon` に指定したレイアウトを取り込むと、アイコンが解決・保存される

### フェーズ ④: 背景画像

- 作業: `assets` の検査（取り込み時のエラー表示）、`KeyImageCache`、`fillImage`／`background` の描画、メモリ逼迫時のスキップ、`LayoutManagerView.maxBytes` の引き上げ
- 受け入れ条件:
  1. §6.2 の上限を超えるレイアウトは取り込みで日本語の理由つきで拒否される
  2. 4 枚・合計 768KB のレイアウトで、拡張のメモリ増加が 12MB 以内（`MemoryProbe` で測る。LLM を載せた状態でも `jetsam` で落ちない。H-26 と同じ観点）
  3. `fill`/`fit`/`tile` の 3 通りと `background` が実機で意図どおり表示される。デコード前は色だけで描かれ、チラつかない
  4. キーボードを非表示にするとキャッシュが解放される
  5. 参照先の画像が無い／壊れているときは色だけで表示される（落ちない）

### フェーズ ⑤: Web エディタ

- 作業: §8（`LookInspector` ほか、プレビューの状態切替、画像の追加、警告の表示、書き出し）
- 受け入れ条件:
  1. 「詳細設定」で、全体・行・キー・各状態の `look` を GUI で編集でき、JSON パネルに即反映される。JSON を貼り付けると GUI が追従する
  2. §5.2〜5.4 の 3 つの例を GUI だけで作れる
  3. プレビューで状態（押下・長押し・移動中ほか）をオンにして見た目を確認できる
  4. 警告（無視される値）と エラー（資源の上限）が場所つきで分かれて表示され、警告だけなら書き出せる
  5. 画像は縮小され、上限の使用量表示が正しい。上限超過は追加できない
  6. 書き出した JSON を iOS の取り込みに通して、同じ見た目になる（§5.2〜5.4 で実機確認）
  7. `npm run check`・`npm test`・`npm run build` が通る

## 10. 決めたこと（理由つき）

1. 後方互換は取らず `schema` を 2 に上げ、2 だけを受理 — 利用者が作者のみで、新旧併存の互換コードより実装の単純さを優先する（ユーザー指示）
2. 既存 `style`（色の役割）を `role` に、`styles` を `roles` に改名し、見た目のオブジェクトは `look` — 名前の衝突を避け、役割と見た目を別の語にするため
3. 見た目の値の不正は無視してエラーにしない、資源の上限だけエラー — キーボードが使えなくなるのを防ぎ、拡張のメモリを守るため
4. `look` は平らなプロパティ — 部分上書きの規則と、Swift／TypeScript／テストベクタの実装が単純になる
5. 描画は `StyledKeyFace` の 1 経路、既定値が現行の見た目 — 二重経路を持たない。同一性は `LookDefaults` の単体テストと実機の目視で担保する
6. 状態は局所（キー単位）と大域（全体）に分け、`rules` と `states` の 2 つの書式 — 「全キー」への指定は 1 か所に、「そのキー自身」は定義の近くに書けるようにするため
7. 状態の優先順位は `composing < capsLock < shift < released < pressed < spaceDrag < flicking < longPress` — 持続する状態を下に、特殊な操作を上に置くと説明が一つで済むため
8. `pressed` は `longPress`/`flicking`/`spaceDrag` の間も真 — 「押している間は常に押下」を基準にして、特殊な状態をその上に重ねるため
9. 押下の既定（背景 0.6）は組み込みの `pressed` の既定として最下位 — 現行の見た目を保ちつつ、`look.fillOpacity` が押下の手応えを消さないようにするため
10. `label`/`icon` は `states`/`rules` の中だけ — 静的な表示はキー定義の `label`/`icon` が既にあり、二重の書き方を作らないため
11. 状態が `label` だけ指定したら `key.icon` を隠す — 「空白」→「移動」の差し替えでアイコンが残らないようにするため
12. 色は `#hex`・トークン・`{light,dark}`、片方だけの指定は反対の外観では未指定 — 設定のテーマ・アクセントに追従したい指定と絶対色の指定を両立し、勝手な反転を避けるため
13. 範囲外はクランプせず無視 — 「意図と違う値で動く」より「指定なしで動く」ほうが原因を追いやすいため
14. `offset`/`scale`/`inset` は当たり判定を変えない — 曖昧入力のキー矩形（H-25）・行の按分・片手モードと衝突しないため
15. 位置の絶対座標配置は v1 に入れない、`metrics`（キー間・行間・外周）だけ足す — 同上。キー間隔の自由度は `metrics` と `inset` で足りる
16. 画像は PNG/JPEG、1 枚 256KB・長辺 1024px・4 枚・合計 768KB、デコード結果 12MB — ブラウザ生成の容易さと、拡張のメモリ上限（約 177MB。H-6）に対する余裕のため
17. 画像は遅延デコード・ダウンサンプリング・`NSCache`、逼迫時はスキップ — LLM を載せたときの余裕が小さいため（H-26 と同じ方針）
18. 画像の当て方は `fill`/`fit`/`tile`（`tile` は 1pt = 3px）— ユーザーの指示。引き伸ばしは見栄えが悪い
19. キーボード全体の画像は `background` で別に持つ — 全体に 1 枚敷いてキーを半透明にするほうが、画像数が少なくて済む
20. `assets` を含むレイアウトは URL スキームで渡せない — URL スキームの上限 1MB（`LayoutURL.maxDataChars`）を変えないため。ファイル・コピーで渡す
21. アニメーションは既定 0（即時）、前後の `animationMs` の最大で補間、Reduce Motion で無効 — 現行が即時なので既定を変えず、戻りの滑らかさを片方の指定で書けるようにするため
22. 複数条件（AND）の状態は持たない — 優先順位と `target` で必要な表現が足りるため。複雑さに見合わない
23. popup（長押し・フリックの案内）と候補バー・ツールバーは対象外 — まずキー本体の見た目に絞り、範囲を広げるのは使われ方を見てから
24. web の look の問題は警告、書き出しをブロックしない — iOS が寛容に無視する挙動と揃えるため
25. 解決関数は Swift と TypeScript の二重実装＋共有テストベクタ — 言語が違い共有コードにできないため、ベクタで同一性を保証する
26. web は既存ページのインスペクタにタブ「詳細設定」を足す — 現状「カスタム」タブは存在せず、既存の画面構成を壊さないため

## 11. 決定記録に追記する案（`docs/decisions.md` へは追記していない）

```
### H-?? キーボードのスタイルとトリガー（レイアウト JSON の `look` / `states` / `rules` / `assets`、日付、ユーザー要望、要実機確認）

- 要望: スペースのドラッグ移動中だけ見た目（薄く・ラベル「移動」）を変えたい。キーの高さ・幅・位置だけでなく、文字サイズ・色・角丸・ドロップシャドウ・透明度・背景画像まで
  レイアウトで指定でき、サイトのエディタ（`/custom-keyboard`）の「詳細設定」で全体・キーごとに編集できるようにしたい。
- 決定: 仕様は `docs/keyboard-style-spec.md`。要点 —
  - 後方互換は取らず `schema` を 2 に（2 のみ受理）。`style`→`role`、`styles`→`roles` に改名、新しい見た目は `look`。追加項目は省略可能で、省略時の見た目は現行と同じ（`KeyFace` は削除し `StyledKeyFace` に一本化）。既存データは移行スクリプトで一括変換
  - 見た目の値の不正は無視（レイアウトは無効にしない）、資源の上限（画像の個数・サイズ、rules の個数）だけ取り込みエラー
  - 状態: pressed / released / longPress / flicking（局所）、spaceDrag / shift / capsLock / composing（大域）。優先順位 composing < capsLock < shift < released < pressed < spaceDrag < flicking < longPress
  - 背景画像は JSON に base64 data URI（PNG/JPEG、1 枚 256KB・合計 768KB・4 枚まで）。拡張では遅延ダウンサンプリング＋12MB 上限のキャッシュ
  - 解決関数は純粋関数で Swift と TypeScript の二重実装、共有テストベクタで一致を保証
- 実装段階: ①スキーマ＋解決関数＋テスト ②iOS 描画（全体／個別）③状態トリガー ④背景画像 ⑤Web エディタ
- 未確認（実機）: 押下ごとの再描画コスト、画像を載せたときの拡張のメモリ、Reduce Motion、システムにジェスチャを奪われたときの状態の解除
```

## 12. 既存データの移行手順（一回限り。新旧併存のコードは作らない）

### 12.1 変更点（schema 1 → 2）

| 変更 | 内容 |
| --- | --- |
| `schema` | `1` → `2` |
| キーの `style` | `role` に改名（値は `normal`/`function`/`accent` のまま。省略時 `normal`） |
| `popup`（`longPress` の項目）の `style` | 同じく `role`（あれば） |
| それ以外 | 既存項目は変更なし。`look`・`states`・`rules`・`roles`・`metrics`・`background`・`assets` は新規の省略可能項目 |

### 12.2 移行スクリプト

- `web/scripts/migrate-layout-v2.mjs`（Node。`cargo xtask` は JSON の薄い変換には過剰なため web 側に置く）。引数のファイル／ディレクトリの `*.json` を走査し、`schema: 1` のものを上記の規則で書き換える（インデント 2 スペース・末尾改行を維持。`schema` が 2 のものは触らない）。冪等
- 実装担当はフェーズ ① で、このスクリプトと `vitest` のテスト（1 → 2 の変換、冪等、`style` を持たないキー）を追加する

### 12.3 移行の対象と手順（フェーズ ① のコミットで一括）

1. `config/keyboard/presets/*.json`（8 ファイル）、`config/keyboard/test-layouts/*.json` を、スクリプトで変換してコミット
2. `frontend-ios-spike/Shared/KeyboardLayout.swift` の組み込み JSON（`builtinKanaJSON` / `builtinAlphaJSON`）を手で書き換え（`"schema":2`、`"style"`→`"role"`）。`KeyStyle` → `KeyRole`、`CodingKeys.style` → `.role`、`validationError` の `schema != 2`
3. Swift の `KeyStyle` の参照（`NagiKeyboardView.swift` の `KeyFace` の `style` 引数ほか）と `Tests/` のレイアウト JSON リテラル（`LayoutValidationTests`・`LayoutGeometryTests`・`TestSupport.swift` など）を `role`／`schema: 2` に直す。`grep -rn '"style"\|KeyStyle\|\.style' frontend-ios-spike` で漏れを確認
4. web: `types.ts`（`SCHEMA_VERSION = 2`、`KEY_STYLES`→`KEY_ROLES`）、`validate.ts`、`ops.ts`・`key-display.ts`・`KeyForm.svelte`（「見た目の種類」の入力）・`KeyboardPreview.svelte`（クラス名）、`presets.ts` のサンプル、`keyboard.test.ts` のリテラル、`layout.schema.json`（`schema` の const を 2、`style`→`role`）
5. `localStorage` の保存キー `nagi.custom-keyboard.v1` を `nagi.custom-keyboard.v2` に変える。旧キーは読まない（保存済みの編集中レイアウトは消える。作者本人のみの利用で、必要なら先に JSON を書き出して移行スクリプトを通してから読み込む）
6. ユーザーのレイアウト（App Group の `keyboard/layouts/*.json`）は、実機の作者のものを一度書き出し → 移行スクリプト → ホストアプリで取り込み直す。`schema: 1` のファイルは読み込まれず、同梱の既定にフォールバックする（入力は失われない）。ホストアプリの取り込み画面は `schema: 1` に「未対応のスキーマ 1（schema 2 に移行してください）」と表示する
7. `docs/ios-keyboard-layout.md` の `style` を `role` に、`schema` を 2 に更新し、`look` 系の節を追記（実装完了時）。`docs/ios-device-checklist.md` の確認項目を更新

- 移行後の確認: `PresetLayoutTests`・`keyboard.test.ts` の全プリセット検証、`git grep '"style"' config/` が空であること
- 理由（スクリプトを残す）: 作者の手元に残る旧形式の JSON を再変換できるようにするため。互換コードをアプリに持たせる代わり
