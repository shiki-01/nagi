# Nagi iOS 実機検証アプリ

Nagi（日本語IME）の iOS 実機上での動作検証用スパイク実装。メモリ使用量、レイテンシ、LLM ロード、システム状態などを測定できます。

## クイックスタート（推奨：GitHub Actions 経由）

### 1. CI ビルド（未署名 IPA）を取得

GitHub Actions ワークフロー `.github/workflows/ios-spike.yml` を実行：

1. GitHub リポジトリの **Actions** タブを開く
2. **iOS Spike Build** ワークフローを選択
3. **Run workflow** → **Dispatch** で実行
4. 完了後、**Artifacts** から `NagiSpike-unsigned.ipa` をダウンロード

### 2. 実機に導入（SideStore 使用）

- **SideStore** アプリをインストール（App Store）
- SideStore を開き、ダウンロードした IPA を「Add IPA」で追加
- 実機を接続して、SideStore から NagiSpike をインストール

これで実機検証が可能です。

#### App Group 動作確認（SideStore で App Group ID が書き換わるかテスト）

SideStore は署名時に App Group ID を書き換える可能性があります。以下の手順でアプリの App Group 解決機能を検証してください：

1. **Sideloadly 版があれば削除**: 異なる署名者のアプリは上書きできないため、事前に削除（設定 > 一般 > iPhone ストレージ > NagiSpike を削除）
2. **SideStore から IPA をインストール**: IPA ファイルを「Add IPA」で追加して導入
3. **ホストアプリで App Group 状態を確認**:
   - ホームタブの「App Group」セクションで「Resolved ID」と「Candidates」を確認
   - 署名者が App Group ID を書き換えた場合、複数の候補が表示されます
4. **キーボード拡張を追加**: 設定 > 一般 > キーボード > 新しいキーボード で NagiSpike Keyboard を有効化
5. **相互通信テスト**: ホームタブの Probes から「appgroup」を実行
   - App Group が利用可能なら、結果に「Resolved ID」と「I/O test: OK」が表示されます
   - 相手プロセス（キーボード拡張）の最新実行時刻も表示されます
6. **キーボード拡張でも実行**: テキスト入力フィールドでキーボード拡張を開き、appgroup プローブを実行
   - App Group が使えるなら、相手プロセス（ホストアプリ）の結果が見えるはずです

**無料 Apple ID での制約**: 
- アプリ上限: 3 個（SideStore のアプリ枠も含む）
- App ID 作成上限: 週 10 個（拡張ごとに消費）
- 有効期限: 7 日間（定期的に再署名が必要）

---

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

LLM 検証が必要な場合：

```bash
cd ../core
cargo xtask fetch-model
```

`../data/models/` に GGUF ファイルが生成されます。アプリ内から「モデル(GGUF)をファイルから取り込む」でインポートできます。

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
- **Team**: Personal Team（無料アカウントで OK）
- **Bundle ID**: 自動設定（XcodeGen の設定を使う）

#### 6. 実機にビルド・実行

実機を接続して、Build and Run（Cmd+R）します。

## アプリの使い方

### ホームタブ

- 端末情報（機種・iOS）を表示
- App Group コンテナの可否を確認
- 辞書・モデルの配置状況を表示
- 辞書をバンドルからコピーするボタン
- ファイルからモデルをインポートするボタン

### Probes タブ

8 つの検証項目を実行できます：

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
7. **llm_load**: LLM ロードのメモリ増加量、時間、その後の Space 変換レイテンシ
   - LLM 非対応ビルド時は skip
8. **thermal_battery**: システムの熱状態・低電力モード・バッテリー状態を記録

「すべての Probe を実行（memory_ramp 除外）」で全項目を一括実行できます。Memory Ramp は強制終了のリスクがあるため、単独ボタン（警告付き）で実行します。

### 結果タブ

- すべての結果をタイムスタンプ付きで一覧表示
- JSONL 形式でエクスポート（クリップボード）
- 結果をクリア

## キーボード拡張

### 有効化手順

1. ホストアプリ（NagiSpike）を開く
2. ホームタブで「設定 > 一般 > キーボード > 新しいキーボード で NagiSpikeKeyboard を有効化してください」と表示されているので、その指示に従う
3. 設定 → 一般 → キーボード → 新しいキーボード で Nagi Spike Keyboard を探して「追加」
4. Nagi Spike Keyboard を許可（「フルアクセスを許可」ダイアログで「許可」）

### キーボード上でのプローブ実行

起動時に自動的に `baseline_memory` が 1 回記録されます（他のプローブはボタンタップで実行）。

推奨実行順序（1 つずつ順番に）：
1. テキスト入力フィールドを開く
2. キーボード下部の地球儀アイコンで「Nagi Spike Keyboard」に切り替え
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

## App Group が使えない場合（無料アカウント）

Sideloadly + 無料 Apple ID での導入など、App Group が使えない環境では以下の動作をします：

### 辞書・モデルの自動フォールバック
- **辞書**: App Group コンテナが無い場合、バンドルに同梱された `Resources/data/dict/nagi-dict.bin` を自動的に使用
- **モデル**: App Group が無いため、ファイルインポート機能は無効化（ホストアプリの「モデルをインポート」ボタンがグレイアウト）
- 各プローブの metrics には `data_dir_source` を記録（"appgroup" / "bundle" / "none"）

### 結果の記録場所
- App Group が利用可能: `~/Library/Group Containers/group.com.shiki.nagi.spike/results.jsonl`
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

- `../data/models/` に GGUF ファイルが存在するか確認
- アプリ内「モデルをファイルから取り込む」で `data/models/` にインポート
- LLM フラグ付きでビルドしたか確認（project.yml に `NAGI_LLM` フラグ）

### キーボード拡張が表示されない

- Project ファイルの署名を再度確認
- 設定アプリをキルして再起動
- 実機を再起動

## 設定ファイル

- アプリのユーザー設定: `~/Library/Application Support/Nagi/` 下に保存
- App Group コンテナ: `~/Library/Group Containers/group.com.shiki.nagi.spike/`

## 参考

- Rust コア: `core/`
- 設計書: `docs/ime-design-doc.md`
- プロトコル: `core/protocol/src/`
