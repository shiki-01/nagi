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
| `label` / `icon` | 表示（SF Symbols 名または文字列） |
| `style` | `normal` / `function` / `accent`（色はテーマ側） |
| `kind` | 下表 |
| `longPress` | 長押しで出るポップアップ（`[{ "label", "kind"... }]` のキー定義の配列） |
| `visible` | 条件（`{"field":"returnKeyType","in":["search"]}` 等。v1 では省略可） |

### kind

| kind | 追加フィールド | 動作 |
| --- | --- | --- |
| `text` | `text`, `shift?` | 文字を送る（QWERTY のローマ字入力は、かなモードではコアのローマ字変換に渡す） |
| `flick` | `center`, `up?`, `down?`, `left?`, `right?`（各 `{ "text" }`）, `kana?` | フリック。`kana` に `"あ"` のような 12 キー名を指定すると、コアのトグル・フリック・濁点循環に任せる |
| `toggle` | `cycle: [string...]` | 押すたびに順に切り替わる（ガラケー式）。確定は他のキー・時間経過 |
| `action` | `action` | `space` / `enter` / `backspace` / `shift` / `capslock` / `convert` / `moveLeft` / `moveRight` / `dakuten` / `smallKana` / `modeSwitch` / `globe` / `dismiss` ほか（コアの `TouchAction` に対応） |
| `shortcut` | `steps: [{ "action" or "text" }...]` | 複数操作の連続（例: 「ありがとうございます」の定型挿入、カーソルを括弧の間へ） |
| `layout` | `target` | 別レイアウトへ切り替え |
| `spacer` | — | 空白（幅のみ） |

## 検証（スキーマ）で保証すること

- `rows` は 1〜8 行、1 行あたり幅の合計が 1〜20、`id` の重複なし、`layout.target` は存在するレイアウトまたは組み込み名
- `flick` の方向は 4 方向以内、`toggle.cycle` は 1〜16 要素
- `text` は 16 文字以内（`shortcut` 内の `text` は 256 文字以内）

## 組み込みレイアウト（既定）

`qwerty-kana`（QWERTY かな）/ `qwerty-alpha`（QWERTY 英字）/ `flick-kana`（12 キー かな）/ `flick-alpha`（12 キー 英字）/
`symbols`（記号）/ `numbers`（数字）/ `emoji`（絵文字）。ユーザーは各モードにどのレイアウトを割り当てるかを設定で選ぶ。

## 実装の段階

1. スキーマ + 既定レイアウトの JSON 化 + キーボード拡張が JSON から描画（`text` / `flick` / `action` / `layout` / `spacer`）
2. `toggle` / `shortcut` / `longPress`
3. サイトの編集 GUI（プレビュー、キーの追加/削除/並べ替え/サイズ、種類の切替、JSON の書き出し/読み込み）
4. ホストアプリでの取り込み・管理・割り当て
