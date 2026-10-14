# Nagi iOS 実機検証アプリ

> **【2026-10-14 追記】このアプリは「検証用スパイク」から iOS の製品版（キーボード拡張 + ホストアプリ）に発展しています。**
> 配布は SideStore ではなく TestFlight / App Store です（`docs/decisions.md` H-15）。ビルド・署名・アップロードは
> GitHub Actions `ios-testflight`（`gh workflow run ios-testflight.yml --ref main -f llama=true`）で行います。
> bundle id は `com.shiki01.nagi`（ホスト）/ `com.shiki01.nagi.keyboard`（拡張）、App Group は `group.com.shiki01.nagi`。
> 実機の確認結果と実装の経緯は `docs/decisions.md` の H-6〜H-15 を参照してください。
>
> ホストアプリのタブは 5 つです: ホーム（セットアップ・データ状況・辞書とモデルの取り込み）、入力テスト（キーボードの動作確認用の入力欄）、設定、レイアウト、About（開発者向けに端末情報・App Group・Probes・結果）。
> キーボード拡張は、製品版の 12 キー（フリック）と英字（QWERTY）が既定で、候補バー右端の「検証」ボタンで Probe の画面に切り替えられます。



## ローカル Mac での手動ビルド

### 前提

- macOS での開発環境
- Xcode 15+
- XcodeGen（`brew install xcodegen`）
- 実機（iPhone、シミュレータはメモリ上限が実機と異なるため検証不可）

### ビルド手順

#### 1. Rust コアの XCFramework をビルド

```bash
cd ../core
cargo xtask build-ios-xcframework [--llama]
```

`--llama` フラグを指定すると LLM 対応ビルドになります。ビルド後、`frontend-ios-spike/NagiCore.xcframework` が生成されます。

#### 2. 辞書を準備（オプション）

辞書をコンテナにコピーする場合は先に取得してください：

```bash
cd ../core
cargo xtask fetch-dict
cargo xtask build-dict
```

`../data/dict/nagi-dict.bin` が生成されます。アプリ起動後、「辞書をバンドルからコピー」ボタンで App Group コンテナに配置できます。

#### 3. モデルを準備（オプション）

LLM 検証が必要な場合（Mac から取得する場合）：

```bash
cd ../core
cargo xtask fetch-model
```

`../data/models/` に GGUF ファイルが生成されます。実機への入れ方は次節「モデルの入手とインポート（実機）」を参照してください。

#### 4. Xcode プロジェクトを生成

```bash
cd frontend-ios-spike
xcodegen generate
```

#### 5. Xcode で開く

```bash
open NagiSpike.xcodeproj
```

Xcode の署名設定：
- **Team**: Apple Developer Program のチーム（加入済み。H-15）。配布は TestFlight 用に CI（`ios-testflight`）で署名する。ローカルの実機ビルドは、このチームの証明書を Xcode に入れて行う
- **Bundle ID**: 自動設定（XcodeGen の設定を使う）。ホスト `com.shiki01.nagi`、拡張 `com.shiki01.nagi.keyboard`、App Group `group.com.shiki01.nagi`

#### 6. 実機にビルド・実行

実機を接続して、Build and Run（Cmd+R）します。

## モデルの入手とインポート（実機）

モデル（GGUF）は CI の IPA に同梱されません（辞書のみ）。実機に次の手順で入れます。キーボード拡張でも使うには App Group が使えること（ホームタブの「App Group」が「✓ 利用可能」）が前提です（使えない場合はホストアプリ専用の領域に入り、拡張からは見えません）。

1. iPhone の Safari で HuggingFace の URL を開いてダウンロードする（約70MB。Wi-Fi 推奨）
   - small（標準）: `https://huggingface.co/Miwa-Keita/zenz-v3.1-small-gguf/resolve/ddf6e44b2e05ab7ea9a3e31559c5e7948761365c/ggml-model-Q5_K_M.gguf`（約70.4MB）
   - xsmall（軽量。拡張のメモリ上限が厳しい場合の比較用）: `https://huggingface.co/Miwa-Keita/zenz-v3.1-xsmall-gguf/resolve/5c0c4db8c8cc66a6ef01520475bebd56b41f6236/ggml-model-Q5_K_M.gguf`（約20MB）
2. ダウンロードしたファイルは「ファイル」アプリの「ダウンロード」に入る。**名前が両方 `ggml-model-Q5_K_M.gguf` で同じ**なので、長押し →「名称変更」で `zenz-v3.1-small-Q5_K_M.gguf` / `zenz-v3.1-xsmall-Q5_K_M.gguf` にする（両方置く場合は必須。同名だとインポート時に上書きされる）
3. NagiSpike のホームタブ「データ」→「モデルをインポート（.gguf を選択）」→「ファイル」アプリで上記ファイルを選ぶ
   - iCloud Drive 上のファイルは先にダウンロード済みにしておく
   - コピー中は進捗表示（数十秒）。完了すると「モデルをインポートしました」と表示され、「データ」欄に配置先パスとサイズが出る
   - 辞書が App Group に無ければ同時にバンドルからコピーする（コアは辞書が読めないと LLM を読み込まない）
4. モデルが複数あるときは、「データ」欄のピッカーで llm_load が使うモデルを選ぶ（未選択なら `zenz-v3.1-small-Q5_K_M.gguf`、無ければ最大サイズ）。選択はホストとキーボード拡張で共有される
5. Probes タブの `llm_load` を実行する。結果の note に実行元（app / keyboard）・使用モデル名とサイズ・変換結果が出る

`llm_load` が失敗したときの note の見方:

| 表示 | 意味 / 対処 |
| --- | --- |
| (a) App Group コンテナが取得できません | 再署名で App Group が外れた。`appgroup` プローブで確認し、アプリを再インストール |
| (b) `data/models` がありません | モデル未インポート。上記手順 3 |
| (c) `.gguf` がありません | ディレクトリはあるが空。上記手順 3 |
| (d) LLM のロードに失敗（コアの応答: …） | ファイルはあるがロード失敗。応答文言（ファイル破損・メモリ不足・llama 非対応ビルドなど）を確認。「LLM 非対応ビルド」と出る場合は CI を `llama=true` で実行して IPA を作り直す |

## アプリの使い方

### ホームタブ

- セットアップの状況と案内、辞書・モデルの配置状況・メモリ・LLM の見込みを表示（端末情報・App Group の可否は About の「開発者向け」へ移動）
- 辞書をバンドルからコピーするボタン
- モデルをインポートするボタン（配置済みモデルの一覧・サイズ・使用モデルの選択）

### Probes タブ

9 つの検証項目を実行できます（`Shared/Probes.swift`）：

1. **appgroup**: App Group コンテナの読み書き可否。キーボード拡張との相互検証
2. **baseline_memory**: エンジン未ロード時のメモリ（phys_footprint、利用可能メモリ）
3. **dict_load**: 辞書ロードのメモリ増加量と所要時間。辞書状態（ロード済みか、エントリ数）と変換テスト結果を記録
4. **latency**: ローマ字 1 文字入力～Space 変換～Enter の応答時間を 200 回計測（p50/p95/max）
   - 目標: ひらがな反映 p95 < 16ms、Space 変換 p95 < 150ms
   - 最初の 1 回だけ辞書状態と変換結果を検証
5. **memory_ramp**: 8MB ずつメモリを確保し、強制終了時点を検出。進捗を SharedStore に同期記録
   - **警告**: 実行中に強制終了される可能性があります
   - 上限: ホストアプリ 3072MB、キーボード拡張 1024MB
6. **coldstart**: エンジン生成～最初の process_key 応答までの時間
   - 辞書状態と変換結果（"a" が "あ" になるか）を検証
7. **llm_load**: LLM ロードのメモリ増加量、時間、その後の Space 変換レイテンシ（ローマ字を打ってから Space）
   - 使用したモデル名・サイズ・実行元（app / keyboard）を note に記録。ロード失敗は原因別 (a)〜(d) で表示
   - LLM 非対応ビルド（`llama=false` の CI ビルド）時は skip
8. **llm_load_light**: `llm_load` と同じ手順を、プロファイル `light` を固定して実行する（H-9・H-10 の比較用）
9. **thermal_battery**: システムの熱状態・低電力モード・バッテリー状態を記録

「すべての Probe を実行（memory_ramp 除外）」で全項目を一括実行できます。Memory Ramp は強制終了のリスクがあるため、単独ボタン（警告付き）で実行します。

### 結果タブ

- すべての結果をタイムスタンプ付きで一覧表示
- JSONL 形式でエクスポート（クリップボード）
- 結果をクリア

## キーボード拡張

### 有効化手順

1. ホストアプリ（NagiSpike）を開く
2. ホームタブで「設定 > 一般 > キーボード > 新しいキーボード で NagiSpikeKeyboard を有効化してください」と表示されているので、その指示に従う
3. 設定 → 一般 → キーボード → 新しいキーボード で Nagi を探して「追加」
4. Nagi を許可（「フルアクセスを許可」ダイアログで「許可」）

### キーボード上でのプローブ実行

起動時に自動的に `baseline_memory` が 1 回記録されます（他のプローブはボタンタップで実行）。

推奨実行順序（1 つずつ順番に）：
1. テキスト入力フィールドを開く
2. キーボード下部の地球儀アイコンで「Nagi」に切り替え
3. キーボード上の Probe ボタンをタップして実行：
   - `baseline_memory`（起動時に自動実行済み）
   - `dict_load`（辞書ロードテスト）
   - `latency`（キー入力レイテンシ）
   - `memory_ramp`（メモリ段階的確保、単独実行のみ）
4. 各ボタン右側にチェックマーク（成功）またはエラーマーク（失敗）が表示される
5. **プローブは直列実行されます**（ボタンは実行中に無効化）。複数プローブを同時に実行できません

### 機能

- Probes をキーボード上で実行（タップボタン）
- テスト入力ボタン（あ、か、Space、⌫、次のキーボード）
- 実行結果をホストアプリで確認可能（App Group 共有、または per-process）
- 「結果を入力欄に挿入」ボタン: キーボード拡張で実行した Probe 結果を、入力フィールドに挿入。ホストアプリのテキストフィールドにコピーして確認可能（App Group が使えない環境向け）

## 旧スパイク時代の記録: App Group が使えない場合（無料アカウント・Sideloadly）

> **以下は現行の手順ではありません。** 無料 Apple ID（Sideloadly）で導入していた時代の動作の記録です。
> 現在は Apple Developer Program に加入し、TestFlight / App Store で配布します（`docs/decisions.md` H-15）。App Group は前提で、フォールバックの動作はありません。

以前の検証では、App Group が使えない環境で次の動作をしていました：

### 辞書・モデルの自動フォールバック
- **辞書**: App Group コンテナが無い場合、バンドルに同梱された `Resources/data/dict/nagi-dict.bin` を自動的に使用
- **モデル**: 「モデルをインポート」はホストアプリ専用の領域（`Application Support/com.shiki.nagi/data/models/`）に保存される。キーボード拡張からは見えないため、拡張の llm_load は (a) として失敗を記録（ホストの llm_load は成功しうる）
- 各プローブの metrics には `data_dir_source` を記録（"appgroup" / "bundle" / "none"）

### 結果の記録場所
- App Group が利用可能: `~/Library/Group Containers/group.com.shiki01.nagi/results.jsonl`
- App Group が利用不可（無料アカウント）:
  - ホストアプリ: `~/Library/Application Support/com.shiki.nagi.spike/results.jsonl`
  - キーボード拡張: `~/Library/Application Support/com.shiki.nagi.spike/results.jsonl`（別プロセスなので別ファイル）

### 結果の共有方法（App Group 無しの場合）
- **ホストアプリ**: 結果タブに 1 行サマリを表示。「コピー」ボタンまたは ShareLink で共有可能
- **キーボード拡張**: 「結果を入力欄に挿入」ボタンで、プローブ結果を入力フィールドに挿入。ホストアプリのテキストフィールドにコピーして確認可能

## JSON-RPC プロトコル

エンジンとの通信は改行区切りの JSON-RPC 2.0 です。詳細は `core/protocol/src` と `docs/ime-design-doc.md` を参照してください。

### initialize

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocol_version": 1,
    "client_name": "nagi-ios-spike",
    "client_version": "0.1",
    "os": "ios"
  }
}
```

### process_key

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "process_key",
  "params": {
    "session_id": "unique-id",
    "keysym": 97,
    "keycode": 0,
    "text": "a",
    "modifiers": {
      "shift": false,
      "ctrl": false,
      "alt": false,
      "meta": false
    },
    "kind": "char"
  }
}
```

## トラブルシューティング

### ビルド時に NagiCore.xcframework が見つからない

```bash
cd ../core
cargo xtask build-ios-xcframework
```

### 辞書ロードが失敗する

- `../data/dict/nagi-dict.bin` が存在するか確認
- アプリ内「辞書をバンドルからコピー」でコンテナに配置してから Probe を実行

### モデルロードが失敗する（LLM Probe）

- ホームタブ「データ」欄でモデルの有無・パス・サイズを確認（(a)〜(c) の原因別に表示）
- 「モデルをインポート」で `data/models/` にインポート（手順は「モデルの入手とインポート（実機）」）
- LLM フラグ付きでビルドしたか確認（CI を `llama=true` で実行。`NAGI_LLM` 付き xcodegen + `--llama` の XCFramework）

### キーボード拡張が表示されない

- Project ファイルの署名を再度確認
- 設定アプリをキルして再起動
- 実機を再起動

## 設定ファイル

- コアの設定（`config.toml`）: App Group コンテナの `user/config.toml`（`<App Group>/user/config.toml`）。キーボード拡張とホストアプリが共有する。先頭に `# nagi:generated` の目印がある間は、起動のたびに自動生成し直す（`Shared/ProductSession.swift`）
- 学習・ユーザー辞書: App Group コンテナの `learning.tsv` / `user_dict/`
- 辞書・モデル: App Group コンテナの `data/dict/nagi-dict.bin` / `data/models/`（`Shared/DataPaths.swift`）
- Mac の `~/Library/...` 配下は、実機では存在しない（シミュレータ・旧スパイクの記録用）

## 参考

- Rust コア: `core/`
- 設計書: `docs/ime-design-doc.md`
- プロトコル: `core/protocol/src/`
