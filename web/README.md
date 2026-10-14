# Nagi 紹介サイト（web/）

`nagi.shiki-01.com` 用。Svelte 5 (runes) + SvelteKit 2 (adapter-static) + Master CSS 2.0.0-rc.88 + TypeScript。

- `/` 紹介、`/support` サポート（App Store の「サポート URL」に使う）、`/privacy` プライバシーポリシー、`/custom-keyboard` キーボードレイアウト編集 GUI
- `sitemap.xml` / `robots.txt` は `src/routes/*/+server.ts` で prerender。公開ページの一覧は `src/lib/site.ts` の `PAGES`（ページを足したらここにも追加）。メタ情報・OGP は `lib/components/Meta.svelte`
- 404 は `+error.svelte` と adapter-static の `fallback: '404.html'`（ホスティング側で 404.html を未存在パスの応答に設定する）
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

## App Store Connect 申告との整合メモ

`/privacy` と `/support` の文面は、次の申告内容と矛盾しないようにしてある。文面か実装を変えるときは両方を見直すこと。

- **App のプライバシー（栄養ラベル）**: 「データを収集しない」を選ぶ。入力内容・学習データ・診断ログは端末内のみで、開発者は受け取らない（診断ログの共有はユーザーが手動で行う）。解析・広告 SDK なし。トラッキングなし。
  - アプリ側の `frontend-ios-spike/App/PrivacyInfo.xcprivacy` も `NSPrivacyTracking=false`・収集データなし・API 理由（UserDefaults / FileTimestamp）で整合している。
- **プライバシーポリシー URL**: `https://nagi.shiki-01.com/privacy`、**サポート URL**: `https://nagi.shiki-01.com/support`。
- **フルアクセス**: 用途は「共有領域の辞書・モデルの読み込み」のみ。キーボードは通信しない。審査メモにも同じ説明を書く。
- **ネットワーク**: 辞書・モデルの取得と更新確認は本体アプリがユーザー操作で行う（無効化可）。入力内容は含めない。
- **要確認（実装との差）**: `/privacy` と `/support` は設計（`docs/mobile-design.md` §5.2・§6、`docs/ios-settings.md` §2.6〜2.9）に基づく。
  現状の `project.yml` では拡張の `RequestsOpenAccess: true`（H-18）、アプリ内の辞書・モデルのダウンロードは未実装（取り込みのみ）、診断ログは既定 ON（TestFlight 暫定）。
  フルアクセスを要求する前提で、サポート文面・ポリシーと審査メモの整合を維持する。
- 公開前に `/privacy` の【要確認】表示（施行日・提供者名・アクセスログ・子どもの年齢区分）を埋めて取り除く。

## デプロイ（未定。候補）

`build/` をそのまま置ける静的ホスティング: Cloudflare Pages / GitHub Pages / Netlify / 自前サーバー。
独自ドメイン `nagi.shiki-01.com` の DNS（CNAME）設定が必要。
