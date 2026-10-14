# Nagi 紹介サイト（web/）

`nagi.shiki-01.com` 用。Svelte 5 (runes) + SvelteKit 2 (adapter-static) + Master CSS 2.0.0-rc.88 + TypeScript。

- `/` 紹介、`/privacy` プライバシーポリシー（草案）、`/custom-keyboard` キーボードレイアウト編集 GUI
- レイアウト仕様: `../docs/ios-keyboard-layout.md`、スキーマ: `../config/keyboard/layout.schema.json`
  （TypeScript 型と検証は `src/lib/keyboard/types.ts` / `validate.ts`。`keyboard.test.ts` が ajv でスキーマと突き合わせる）
- プリセットは `../config/keyboard/presets/*.json` をビルド時に取り込む（無ければサンプル 1 つ）

## 開発

```sh
npm install
npm run dev        # 開発サーバー
npm run check      # 型・Svelte 検査
npm test           # vitest
```

## ビルド

```sh
npm run build      # build/ に静的出力
npm run preview    # build/ を確認
PW_CHROMIUM=/path/to/chrome npm run shots   # preview 起動中にスクリーンショット（shots/）
```

## Master CSS の注意

rc.88 の `@master/css.svelte` は `mode: 'static'` で使っている（`vite.config.ts`）。
推奨の `hooks.server` を入れると prerender 時にマニフェスト読み込みで `Invalid URL` となりビルドが落ちるため。
SvelteKit は 2.x に固定（3.x は設定 API が変わっており未検証）。

## デプロイ（未定。候補）

`build/` をそのまま置ける静的ホスティング: Cloudflare Pages / GitHub Pages / Netlify / 自前サーバー。
独自ドメイン `nagi.shiki-01.com` の DNS（CNAME）設定が必要。
