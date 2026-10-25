# test-looks: 見た目の解決（look）の共有テストベクタ

`docs/keyboard-style-spec.md` の解決規則を、Swift（`frontend-ios-spike/Shared/KeyLookResolver.swift`）と
TypeScript（`web/src/lib/keyboard/look-resolve.ts`）が**同じ結果**で実装していることを確かめるためのデータ。
どちらの単体テスト（`Tests/KeyLookVectorTests.swift`、`web/src/lib/keyboard/look.test.ts`）も、このフォルダの `*.json` をすべて読む。

## 形式

```json
{
  "description": "説明",
  "layout": { "schema": 2, "id": "...", "name": "...", "mode": "alpha", "rows": [ ... ] },
  "cases": [
    {
      "name": "ケース名",
      "key": "キー id",
      "states": ["pressed"],
      "origin": "sp",
      "scheme": "light",
      "expect": { "プロパティ名": 値 },
      "expectHoldMs": 150,
      "expectContent": { "label": "移動", "icon": null, "hidesKeyIcon": true }
    }
  ],
  "transitions": [
    { "name": "...", "from": 120, "to": null, "reduceMotion": false, "expect": 120 }
  ]
}
```

- `states` / `origin` / `scheme` は省略可（既定は空・なし・`light`）。`key` はレイアウト内のキー id（行は id から求める）
- `expect` は解決結果の**全プロパティ**（過不足なく一致を見る）。色は `#RRGGBBAA`（大文字 16 進 8 桁）か `@トークン名` の文字列に正規化して書く。
  `fillImage` が `"none"` に解決された場合は、プロパティ自体が無い（画像なし）
- `expectHoldMs` は `released` の保持時間（ms）。`expectContent` は `label` / `icon` の上書き（`null` は無し）。どちらも省略可
- `transitions` の `from` / `to` は各解決結果の `animationMs`（`null` は未指定）、`expect` は補間の長さ（ms）
- ツールバー（`layout.toolbar.keys`）のキーも `key` に書ける（キー id は rows と合わせて一意）。解決では行番号 -1 として扱い、
  rows[].look の代わりに `toolbar.look` を使い、最下位に既定（`fontSize: 14`, `inset: 6`）を重ねる（`resolve-toolbar.json`）。
  既定は `layout.look` / `roles` / `toolbar.look` / キーの look で上書きできる
