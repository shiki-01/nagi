# Nagi

ローカルLLM（GPT-2級の小型モデル）によるニューラルかな漢字変換を中核とする日本語IME。
コアエンジン（プラットフォーム非依存）とフロントエンド（OS組み込み層）を分離して開発する。

- 設計書: `ime-design-doc.md`（v0.1 draft）。本ファイルはその要約であり、詳細・根拠は設計書を参照すること
- bundle id: `com.shiki.nagi` / リポジトリ名: `nagi`
- 対象: macOS（先行）/ Windows（後続）/ Linux（将来検討）

## 現状

- リポジトリは空（ソースコード・ビルド設定・テスト未作成）。Phase 0（技術検証）着手前
- ビルド・テスト・lintコマンド: **未確認**（実装が存在しないため未定義）。確定したら本ファイルに追記する
- 使用モデル・ローマ字テーブル初期セット・ライセンスは未決定（設計書 付録A）

## 差別化点

1. 範囲操作を第一級市民としたIR設計（プリエディットを最初からセグメント列で持つ）
2. 範囲 × LLMアクション（スタイル変換）のプラグイン化

## アーキテクチャ

```
Frontend (OS組み込み層)  ◀── JSON-RPC 2.0 (改行区切り, stdin/stdout) ──▶  Core Engine (Rust)
 macOS: IMKit + Swift                                                       状態機械 / IR / ローマ字変換
 Windows: TSF + Rust(C++薄層)                                               候補生成 / llama.cpp(GGUF) / 辞書 / 学習
                                                                            Rewriter / キーバインド解決 / スタイル変換
```

### 分離の原則

- コアに置くもの: 状態機械、キーバインド解決、変換・学習・辞書のすべてのロジック
- フロントに置くもの: OSイベントの正規化、描画、プロセスライフサイクル管理**のみ**
- フロントは「keysym+modifierを送り、描画指示を受け取る」thin clientとする。ロジックをフロントに持ち込まない
- エンジンはフロントの子プロセス。クラッシュ時は指数バックオフで自動再起動、スリープ復帰時にも再起動する
- 状態機械はOSに一切依存しない純粋なRustコードとし、ヘッドレステスト対象とする

### プロトコル

- 正のスキーマ定義は Rust 側（`core/protocol/`）に置き、Swift側は型ミラーを持つ
- 主要メソッド: `process_key` / `set_context` / `commit_text` / `update_preedit` / `update_candidates` / `update_mode` / `reconvert` / `reload_config`

### リポジトリ構成（案・未作成）

```
nagi/
├── core/            # Rust workspace
│   ├── engine/      # 状態機械・IR・パイプライン
│   ├── romaji/      # ローマ字変換（テーブル外部定義）
│   ├── infer/       # llama.cpp binding・投機的デコーディング
│   ├── dict/        # trie辞書・Mozc TSV・ユーザー辞書
│   ├── learn/       # 学習キャッシュ
│   ├── rewriter/    # 決定的変換
│   ├── style/       # スタイル変換プラグイン
│   ├── keymap/      # キーバインド解決
│   ├── protocol/    # JSON-RPCスキーマ（正）
│   └── server/      # imserverバイナリ
├── frontend-macos/  # Swift + IMKit
├── frontend-windows/# TSF
├── config/          # default.toml / romaji tables / style presets
├── bench/           # 精度・レイテンシベンチ
└── docs/
```

## 設計上の不変条件

- プリエディットは `Preedit { segments, cursor, selection }` / `Segment { reading, surface, kind, candidates, chunk_id }` のセグメント構造。**Phase 1の時点で導入する**（後付けにしない）
- `reading`（ひらがな）が常に真実で、`surface` はその射影。範囲操作・再変換・スタイル変換はすべて「readingの部分列を選んで surface を再計算する」操作として統一的に定義する
- `SegmentKind`: `Japanese` / `Literal`（英数・記号・数字。変換パイプラインを素通し）/ `Fixed`（部分確定済み）
- 状態: `Direct` / `Composing`（ライブ変換はここ）/ `Converting` / `RangeSelecting` / `StyleAction`（LLM実行中・キャンセル可能な非同期状態）
- 状態遷移はすべて `Action` enum で駆動し、キーとの対応はキーマップ（TOML）が解決する

## 変換パイプライン

`key → romaji変換 → Preedit更新 → 候補生成 → ランキング → preedit/候補通知`

候補生成の優先順位: 学習キャッシュ → ユーザー辞書/スニペット → LLM推論（文脈・プロファイル込み）→ システム辞書 → ひらがな/カタカナ → Rewriter。
候補には出所アノテーション（📝/👤/🤖/📚 等）を付与する。

- ライブ変換: `composing_chunk_len`（既定30）でreadingをチャンク分割し、編集されたチャンクのみ再変換。記号・数字の連続とFixedセグメントで強制分割。キーリピート中は推論をスキップ
- 推論: llama.cpp (GGUF) をFFI経由で利用。`strategy = adaptive | light | main`、`max_latency_ms` 超過で軽量モデルへ切替
- 投機的デコーディング（Zenzai方式）はPhase 6。MVPでは素朴推論 + adaptive + チャンクで対応

## 設定・データ

- 設定: `config.toml`（全項目は `config/default.toml` に日本語コメント付きで管理）。ホットリロード対応（モデル変更は次回変換開始時に遅延ロード）
- 設定パス: XDG / `~/Library/Application Support/com.shiki.nagi/` / `%APPDATA%\Nagi\`
- ユーザー辞書: Mozc TSV互換（インポート/エクスポートはMozc TSVを正とする）
- 学習ファイル: TSV1。commit毎ではなくIME切替・フォーカス喪失時にflush

## プライバシー・セキュリティ

- 全処理ローカル完結。ネットワークアクセスはモデル/辞書の初回ダウンロードのみ
- secure input / シークレットモード / パスワードマネージャ系アプリでは学習・Surrounding Text取得・ログを停止する
- ログに入力内容を残さない（デバッグビルドのみopt-inで許可）
- 学習データはワンコマンドで全削除可能にする

## テスト戦略

- 状態機械ゴールデンテスト: キー列 → (preedit, commit) の期待値をYAMLで定義
- JSON-RPC統合テスト: サーバーバイナリにイベント列を流すヘッドレステスト
- 変換精度ベンチ: 読み→正解表記の一致率（AJIMEE-Bench形式を参考）
- レイテンシベンチ: チャンク長・戦略別のp50/p95をCIで記録
- フロントは手動E2Eチェックリスト（ブラウザ/ターミナル/エディタ/Office系）

## パフォーマンス目標

| 指標 | 目標 |
| --- | --- |
| キー入力→preedit反映（ライブ変換ON） | p95 < 50ms |
| Space変換→候補表示 | p95 < 150ms |
| スタイル変換（1文） | < 1.5s |
| アイドルメモリ（light戦略） | < 500MB 目安 |
| コールドスタート | < 2s（mmap + 遅延ロード） |

## ロードマップ

各フェーズ完了時点で日常利用できることをDoDに含める。

| Phase | 内容 |
| --- | --- |
| 0 | 技術検証（llama.cpp推論、モデル比較・ライセンス調査、IMKitスケルトン） |
| 1 | MVP（macOS・一括変換。状態機械 + IR + ローマ字 + JSON-RPC + 最小config） |
| 2 | ライブ変換 + 学習 + ユーザー辞書/スニペット |
| 3 | 範囲操作 + 確定済みテキストの再変換 + カスタムキーバインド |
| 4 | スタイル変換 + Rewriter拡張 + プロファイル注入 |
| 5 | Windows対応（TSF）。コア側の変更が原則ゼロであること |
| 6 | パフォーマンス最適化（投機的デコーディング等） |

## 非目標

かな入力レイアウト / 手書き・音声入力 / クラウド変換 / iOS・Android / 候補ウィンドウのマウス操作（v1）。

## 参考実装

karukan（コア分離・プロトコル・チャンク・adaptive）、azooKey / Zenzai（投機的デコーディング・スタイル変換）、rakukan（Windows TSF・別プロセス化）、riq0h fork（部分変換）、Mozc（辞書TSV・Rewriter）。

## サブエージェント委任方針

機械的・定型的な作業（ファイル探索、チェック実行、テスト雛形作成、ドキュメントの下読みなど）は
Task toolでサブエージェントに委任し、可能な限り自身のコンテキストを消費しないこと。
設計判断・アーキテクチャ選定・デバッグ方針の決定など、判断力が必要な作業は自身で直接処理すること。

委任するか迷った時点で、原則は委任する。委任しない場合はそれが例外であり、理由を一言添える。

利用可能なサブエージェント（`~/.claude/agents/`）:

| 名前 | 用途 |
|---|---|
| bug-hunter | エラー・テスト失敗・想定外挙動の原因調査（再現・切り分け・証拠集め）。修正はしない |
| check-runner | lint・型チェック・ビルドの実行と、失敗箇所のみの圧縮報告。修正・原因推測はしない |
| codebase-explorer | 「どこに何があるか」の読み取り専用探索。パスと行番号で返す。設計判断・レビューには使わない |
| docs-reader | `docs/` の下読みと必要な節の要約。根拠を文書名＋節番号で返す |
| implementer | 方針確定後の実装（新規ファイル・機能追加・リファクタリング） |
| test-scaffolder | 既存のテストの書き方に合わせたテストケース雛形の作成。プロダクションコードは触らない |
