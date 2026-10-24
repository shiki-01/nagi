# Nagi 紹介サイト（web/）

`nagi.shiki-01.com` 用。Svelte 5 (runes) + SvelteKit 2 (adapter-static) + Master CSS 2.0.0-rc.88 + TypeScript。

- `/` 紹介、`/support` サポート（App Store の「サポート URL」に使う）、`/privacy` プライバシーポリシー、`/custom-keyboard` キーボードレイアウト編集 GUI
- `sitemap.xml` / `robots.txt` は `src/routes/*/+server.ts` で prerender。公開ページの一覧は `src/lib/site.ts` の `PAGES`（ページを足したらここにも追加）。メタ情報・OGP は `lib/components/Meta.svelte`
- 404 は `+error.svelte` と adapter-static の `fallback: '404.html'`（ホスティング側で 404.html を未存在パスの応答に設定する）
- レイアウト仕様: `../docs/ios-keyboard-layout.md`、スキーマ: `../config/keyboard/layout.schema.json`
  （TypeScript 型と検証は `src/lib/keyboard/types.ts` / `validate.ts`。`keyboard.test.ts` が ajv でスキーマと突き合わせる）
- プリセットは `../config/keyboard/presets/*.json` をビルド時に取り込む（無ければサンプル 1 つ）
- 見た目の指定（`look` ほか。仕様は `../docs/keyboard-style-spec.md`）の解決は `src/lib/keyboard/look-resolve.ts`。iOS の `KeyLookResolver.swift` と、
  `../config/keyboard/test-looks/*.json` の共有テストベクタで同じ結果になることを確かめる（`look.test.ts`）。編集 GUI は未実装（フェーズ 5）
- レイアウト JSON の schema 1 → 2 の移行: `node scripts/migrate-layout-v2.mjs <ファイル or ディレクトリ>...`（冪等）

## デザイン

- 色・書体のトークンは `src/app.css` の `:root`。色はアプリアイコン（藍・水平線・朝の日）から取り、ライトは生成りの紙、ダークは夜の海。見出しは OS の明朝体（Web フォントは読み込まない）
- アイコン: `static/icon.svg`（= `favicon.svg`）・`static/icon-dark.svg` は `../frontend-ios-spike/icon-src/` の角丸版、`apple-touch-icon.png` は iOS の `icon-light.png` を 180px に縮小したもの。アイコンを変えたらここも作り直す
- `og.png` は `static/og.svg` をブラウザで 1200×630 に描画したもの
- サイト用の部品は `src/lib/components/site/`（`Horizon` = アイコンの線画、`ConversionDemo` = トップの変換デモ。台本の再生のみでエンジンは動かさない）

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

## デプロイ（GitHub Actions → Cloudflare Pages）

`.github/workflows/web.yml` の `deploy` ジョブが、`check`（check・test・build）の成功後に `build/` を Cloudflare Pages（プロジェクト名 `nagi`）へ公開する。手動実行（`gh workflow run web.yml --ref main`。`macos-release` も最後に起動する）のときだけ動く。main への push や PR では公開しない（check だけ走る）。プロジェクトが無ければ作成し、カスタムドメイン `nagi.shiki-01.com` の追加も試みる。

Secrets が未設定の間は、`deploy` ジョブは警告を出してデプロイだけを飛ばす（CI は赤くならない）。

初回だけ必要な作業（リポジトリの Settings > Secrets and variables > Actions）:

1. `CLOUDFLARE_ACCOUNT_ID`: Cloudflare ダッシュボードの右側（Workers & Pages の概要）にあるアカウント ID
2. `CLOUDFLARE_API_TOKEN`: My Profile > API Tokens で作成。権限は Account > Cloudflare Pages > Edit（ドメイン追加の API 用に Account Settings > Read も付けると確実）
3. DNS: `shiki-01.com` が Cloudflare 管理なら、Pages の Custom domains にドメインを追加した時点で CNAME が作られる。他社 DNS なら `nagi` の CNAME を `nagi.pages.dev` に向ける
4. ワークフローを実行し、`https://nagi.shiki-01.com/`・`/privacy`・`/support`・未存在パスの 404 を確認する（下の【要確認】）

以下は Git 連携（ダッシュボード）で公開する場合の手順。Actions で公開するなら不要。

### ダッシュボードから Git 連携する場合（決定: decisions.md H-22）

`build/` の静的出力を Cloudflare Pages に置く。独自ドメインは `nagi.shiki-01.com`（DNS・カスタムドメインの設定は後で行う）。

想定手順（Cloudflare ダッシュボード > Workers & Pages > Create > Pages > Git に接続）:

1. リポジトリ `nagi` を接続し、本番ブランチを `main` にする
2. ビルド設定:
   - フレームワーク プリセット: なし（None）
   - ルートディレクトリ（Root directory）: `web`
   - ビルドコマンド: `npm run build`
   - ビルド出力ディレクトリ: `build`
3. 環境変数: `NODE_VERSION` = `22`（CI の `.github/workflows/web.yml` と同じ。`web/.node-version` を置いてもよい）
4. デプロイ後、Pages プロジェクトの「カスタムドメイン」に `nagi.shiki-01.com` を追加する。`shiki-01.com` の DNS が Cloudflare 管理なら CNAME が自動で作られ、他社 DNS なら `nagi` の CNAME を `<プロジェクト名>.pages.dev` に向ける
5. 公開後、`https://nagi.shiki-01.com/privacy` と `/support` が 200 で開くことを確認する（App Store Connect の URL が 404 だと審査に通らない）

404.html の扱い: adapter-static の `fallback: '404.html'`（`svelte.config.js`）で `build/404.html` が出力される。Cloudflare Pages は `404.html` があると、**未存在パスにそのファイルを 404 ステータスで返す**（SPA 扱いにはならない）。そのため `_redirects` による全パスの書き換えや SPA フォールバックの設定は**不要**で、置かないこと（置くと 404 が 200 になる）。`prerender` された各ページは `/privacy` のようにディレクトリなしのパスで開く（Pages は `privacy.html` を拡張子なしで配信する）。

【要確認】`trailingSlash` の設定と Pages の配信パスの組み合わせは、実際にデプロイして `/privacy`・`/support`・`/sitemap.xml`・未存在パスの 404 を確認すること（ローカルでは未検証）。

プレビュー（ブランチごとの `*.pages.dev`）は検索に載せたくないため、必要なら `static/robots.txt` ではなく Pages のプレビュー保護（Cloudflare Access）で制限する。
