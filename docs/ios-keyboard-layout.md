# キーボードレイアウト仕様（v0 draft）

iOS のキーボードの配置とキーの動作を、データ（JSON）で定義する。既定のレイアウトも同じ形式で持ち、
サイト（`nagi.shiki-01.com/custom-keyboard`）の編集 GUI とキーボード拡張が同じファイルを読み書きする。
ロジックはコアに置く原則に従い、レイアウトは「どのキーが何を送るか」だけを持つ（かな変換・トグル・フリックの解決はコア）。

- 置き場: 既定 `config/keyboard/presets/*.json`、スキーマ `config/keyboard/layout.schema.json`
- ユーザーのレイアウト: ホストアプリが App Group の `keyboard/layouts/<id>.json` に保存。サイトで編集 → JSON を書き出し →
  ホストアプリで読み込み（ファイル / 共有 / 貼り付け）。将来 URL スキームで 1 タップ取り込み
- 値の範囲はスキーマで検証する。不正なレイアウトは読み込まず、既定にフォールバックする（入力を失わない）

## 構造

```json
{
  "schema": 1,
  "id": "qwerty-ja",
  "name": "QWERTY（かな）",
  "mode": "kana",               // kana | alpha | number | symbol | emoji | custom（変換エンジンの扱いが変わる）
  "rows": [
    { "height": 1.0,            // 行の高さの比
      "keys": [
        { "id": "q", "width": 1, "label": "q",
          "kind": "text", "text": "q", "shift": "Q" }
      ] }
  ]
}
```

### キー共通

| フィールド | 内容 |
| --- | --- |
| `id` | レイアウト内で一意 |
| `width` | 行内の幅の比（既定 1）。`height` は行側 |
| `label` / `icon` | 表示。`label` は文字列、`icon` は下の「アイコン」の書式（SF Symbols または Iconify）。両方あるときは `icon` を優先し、Iconify のアイコンを取得できていなければ `label` を出す |
| `style` | `normal` / `function` / `accent`（色はテーマ側） |
| `kind` | 下表 |
| `longPress` | 長押しで出るポップアップ（`[{ "label", "kind"... }]` のキー定義の配列） |
| `visible` | 条件（`{"field":"returnKeyType","in":["search"]}` 等。v1 では省略可） |

#### アイコン（`icon`）

| 書式 | 意味 |
| --- | --- |
| `sf:<symbol 名>` | SF Symbols（明示） |
| `iconify:<prefix>:<name>` | Iconify のアイコン（例 `iconify:mdi:home`）。prefix / name は小文字英数字とハイフン区切り |
| 接頭辞なし | SF Symbols 名（従来どおり。後方互換） |

- 全体で 64 文字以内。書式はスキーマと `LayoutLoader.decode` の両方で検査する（`sf:` / `iconify:` で始まるのに書式が合わないものは拒否）
- キーボード拡張は通信しない。Iconify のアイコンは**ホストアプリのレイアウト取り込み時にだけ**取得し、画像（PNG）に変換して App Group の `keyboard/assets/icons/` に保存する。拡張はそのファイルを読むだけ
- 共有・書き出しする JSON には ID（`iconify:...`）だけが入り、画像は含まれない。受け取り側が取り込み時に解決する（URL スキームの上限も変わらない）
- 取得は使うアイコンだけを 1 件ずつ。取得済みは再取得しない。取得できなかった場合、そのキーに `label` があれば label を表示して取り込みを続け、無ければ取り込みエラー（キーとアイコン名を表示）
- ライセンス: Iconify のアイコンセットごとに作者・ライセンスを保存し（`keyboard/assets/icons/licenses.json`）、ホストアプリの「レイアウト」タブと About で一覧できる。CC BY 系のライセンスの帰属表示義務に対応するためで、アイコンはライセンス情報と一緒にしか保存しない
- 多色のアイコンはテンプレート画像（単色。色はテーマの前景色）になる
- 経緯と変換手段の選択理由は `docs/decisions.md` H-36

## kind

| kind | 追加フィールド | 動作 |
| --- | --- | --- |
| `text` | `text`, `shift?` | 文字を送る（QWERTY のローマ字入力は、かなモードではコアのローマ字変換に渡す） |
| `flick` | `center`（`kana` 指定時は省略可）, `up?`, `down?`, `left?`, `right?`（各 `{ "text" }`）, `kana?` | フリック。`kana` に `"あ"` のような 12 キー名を指定すると、コアのトグル・フリック・濁点循環に任せる |
| `toggle` | `cycle: [string...]` | 押すたびに順に切り替わる（ガラケー式）。確定は他のキー・時間経過 |
| `action` | `action` | `space` / `enter` / `backspace` / `shift` / `capslock` / `convert` / `moveLeft` / `moveRight` / `dakuten` / `smallKana` / `modeSwitch` / `globe` / `dismiss` ほか（コアの `TouchAction` に対応） |
| `shortcut` | `steps: [{ "action" or "text" }...]` | 複数操作の連続（例: 「ありがとうございます」の定型挿入、カーソルを括弧の間へ） |
| `layout` | `target` | 別レイアウトへ切り替え |
| `spacer` | — | 空白（幅のみ） |

## 検証（スキーマ）で保証すること

- `rows` は 1〜8 行、1 行あたり幅の合計が 1〜20、`id` の重複なし、`layout.target` は存在するレイアウトまたは組み込み名
- 行の高さ比の合計は 4.5 以下（キーボード全体の高さに収めるため）
- `flick` の方向は 4 方向以内、`toggle.cycle` は 1〜16 要素
- `text` は 16 文字以内（`shortcut` 内の `text` は 256 文字以内）

## 組み込みレイアウト（既定）

`qwerty-kana`（QWERTY かな）/ `qwerty-alpha`（QWERTY 英字）/ `flick-kana`（12 キー かな）/ `flick-alpha`（12 キー 英字）/
`symbols`（記号）/ `numbers`（数字）/ `emoji`（絵文字）。ユーザーは各モードにどのレイアウトを割り当てるかを設定で選ぶ（App Group の `keyboardLayoutKana` / `keyboardLayoutAlpha`）。
`layout.target` の特別名 `kana` / `alpha` は「割り当て済みのかな / 英字レイアウト」を指す。

## 動作の詳細（段階 2）

- `toggle`: 同じキーを `input.toggle_timeout_ms`（iOS 設定ファイルに値があれば。無ければ 1000ms 固定。200〜3000）以内に押すと、
  直前の文字を消して `cycle` の次の文字を入れる（`cycle` が 1 要素なら置き換えず追加）。他のキー・レイアウト切替・フォーカス変化で確定。
  かなモードは直前の文字をコアの backspace で消してから `insert_reading(text)` を送る（`key12` のトグルは 12 キーの行の表で
  コアが解決するため、任意の cycle には使えない）。かな以外はホストの deleteBackward + insertText。英字ではシフト状態を最初の押下で適用する
- `longPress`: `flick.long_press_ms`（既定 350）押し続けるとキーの上にポップアップを出し、指を滑らせて選び、離すと実行する
  （`text` / `action` / `shortcut`、実装上は `toggle` / `layout` も可）。ポップアップが出る前に離せば通常のタップ。キーの下に大きく外れて離すと取り消し。
  ポップアップは行の左右からはみ出さないよう寄せる。**`flick` / ⌫ キーの `longPress` は未対応（無視）**
- `shortcut`: 実行前に入力中の文字列を確定する。`steps` は先頭から順に実行し、`text` はコアに通さずホストへそのまま挿入する
  （定型文）。未知の `action` や、`action` も `text` も無い手順に当たったらそこで中断する（それまでの手順は実行済み）
- 片手モード（設定 `keyboard.one_handed` が `left` / `right`）: 空いた側に「⇄（左右の切替）」と「↔（片手モードを解除）」を出す

## 取り込み（ホストアプリ）

- 「レイアウト」タブ: 同梱とユーザーレイアウトの一覧、JSON の貼り付け / ファイル（`fileImporter`）/ URL スキームでの取り込み、
  書き出し（共有・コピー）、削除、かな / 英字への割り当て、簡易プレビュー。検証は `LayoutLoader.decode`（エラーは日本語で表示）
- URL スキーム: `nagi://layout?data=<base64url の JSON>`。同じ id のユーザーレイアウトは更新する。同梱と同じ id でもユーザーのものが優先される
- アイコン（`iconify:`）: 検証のあと、保存の前に使うアイコンを解決する（通信が入るため、取り込みの完了まで少し待つ）。取得できずに `label` で代替したキーは完了メッセージに出る。取得できず `label` も無いキーがあれば、レイアウトは保存しない
- 取り込んだアイコンの作者・ライセンスは「レイアウト」タブ >「アイコンの出典・ライセンス」と About で見られる

## 実装の段階

1. スキーマ + 既定レイアウトの JSON 化 + キーボード拡張が JSON から描画（`text` / `flick` / `action` / `layout` / `spacer`）— 実装済み
2. `toggle` / `shortcut` / `longPress` — 実装済み（実機確認待ち。`docs/ios-device-checklist.md` §3）
3. サイトの編集 GUI（プレビュー、キーの追加/削除/並べ替え/サイズ、種類の切替、JSON の書き出し/読み込み）— 実装済み（`web/`。`web/src/routes/custom-keyboard`）
4. ホストアプリでの取り込み・管理・割り当て — 実装済み（`App/LayoutManagerView.swift`。実機確認待ち）
