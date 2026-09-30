# Nagi

ローカルLLM（GPT-2級の小型モデル）によるニューラルかな漢字変換を中核とする日本語IME。
コアエンジン（プラットフォーム非依存）とフロントエンド（OS組み込み層）を分離して開発する。

- 設計書: `docs/ime-design-doc.md`（v0.2 draft）。本ファイルはその要約であり、詳細・根拠は設計書を参照すること
- 決定事項ログ: `docs/decisions.md`（決定の経緯・理由。設計書と食い違う場合はこちらが新しい）
- IME の bundle id: `com.shiki.inputmethod.Nagi`（macOS が入力メソッドとして認識するには `.inputmethod.` を含む必要があるため。設定/データの識別子は `com.shiki.nagi` のまま） / リポジトリ名: `nagi` / 配布・DL元ドメイン: `shiki-01.com`
- 対象: macOS（先行）/ Windows（後続）/ Linux（将来検討）/ iOS・Android（将来。設計のみ `docs/mobile-design.md`）

## 現状

- Phase 0（技術検証）は未着手。Phase 1 のコア部分（romaji / protocol / engine / server / dict / infer）は実装済み（進捗・未完了は `docs/progress.md`、要レビューの暫定判断は `docs/decisions.md` G 節）
- Rust ワークスペースは `core/` 以下（edition 2024、`core/rust-toolchain.toml` で 1.89.0 固定）。コマンドはすべて `core/` で実行する
- 未決定: 変換モデル・システム辞書（Phase 0 の比較で確定。基準は「無料・再配布制約が小さい → 品質 → 速度/サイズ」）。暫定は Mozc OSS 辞書 + zenz-v3.1-small（`docs/decisions.md` G 節）

### ビルド・テスト・lint コマンド（`core/` で実行）

| 用途 | コマンド |
| --- | --- |
| ビルド | `cargo build --workspace --all-targets` |
| テスト | `cargo test --workspace` |
| lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| フォーマット検査 | `cargo fmt --all --check` |
| 補助タスク | `cargo xtask <task>`（OS 非依存。bash/ps1 は書かない） |

- CI（`.github/workflows/ci.yml`）は ubuntu / windows / macos で fmt・clippy・build・test を実行する
- 取得データ（モデル・辞書）は `data/`（.gitignore 済み）に置く。取得は `cargo xtask` のタスクで行う: `fetch-dict` → `build-dict`（`data/dict/nagi-dict.bin`）、`fetch-model`（`data/models/`）、`gen-types` / `check-types`（Swift 型の生成・差分検査。`typeshare-cli` が必要）
- サーバー: `cargo run -p nagi-server --bin imserver -- --data-dir ../data [--user-dir <dir>] [--config <file>] [--no-llm]`（JSON-RPC は stdin/stdout。辞書が無ければ読みそのまま返す。`--user-dir` は `config.toml`・`learning.tsv`・`user_dict/` の置き場で、未指定なら OS の設定ディレクトリ（`NAGI_CONFIG_DIR` で上書き）。**開発中は実ユーザーの学習データを汚さないよう `--user-dir` を指定する**）
- LLM 推論（zenz）は feature `llama`（infer・server とも既定 OFF）: `LIBCLANG_PATH` と cmake が必要。`cargo test -p nagi-infer -p nagi-server --features llama`（モデル未取得なら skip）。LLM を使うサーバーは `cargo run --release -p nagi-server --features llama --bin imserver -- --data-dir ../data`（`--no-llm` で辞書のみ、`--sync-budget-ms` で推論の待ち時間）
- 精度・レイテンシ計測: `cargo run --release -p nagi-server --features llama --bin nagi-eval -- --data-dir ../data [--no-llm] [--verbose]`（Mozc の `evaluation.tsv`。`cargo xtask fetch-dict` で取得）

## 基本方針

- 品質優先: 開発コストがかかっても良いものを作る。暫定実装は置き換え可能な境界（trait・プロトコル）の裏に置く
- 環境適応: 環境プロファイル `quality` / `balanced` / `light` / `minimal` を自動判定・実行時自動切替（設計書 §4.7）
- 気配り: 入力を失わない / キー入力を待たせない / 標準IMEと挙動を揃える / 状態が見える（設計書 §4.14 のチェックリスト）
- オープン: コードは `MIT OR Apache-2.0`。ロジックは自作、辞書・モデル等のデータ資産は既存を利用し同梱せず別途取得（設計書 §2.3）

## 差別化点

1. 範囲操作を第一級市民としたIR設計（プリエディットを最初からセグメント列で持つ）
2. 範囲 × LLMアクション（スタイル変換）のプラグイン化

## アーキテクチャ

```
Frontend (OS組み込み層)  ◀── JSON-RPC 2.0 (改行区切り, stdin/stdout) ──▶  Core Engine (Rust)
 macOS: IMKit + Swift                                                       状態機械 / IR / アライメント / ローマ字変換
 Windows: TSF + Rust(C++薄層)                                               候補生成 / llama.cpp(GGUF) / 辞書+Viterbi / 学習
                                                                            Rewriter / キーバインド解決 / 環境プロファイル / スタイル変換
```

### 分離の原則

- コアに置くもの: 状態機械、キーバインド解決、変換・学習・辞書・環境適応のすべてのロジック
- フロントに置くもの: OSイベントの正規化、描画、プロセスライフサイクル管理、アップデート**のみ**。ロジックをフロントに持ち込まない
- エンジンはフロントの子プロセス。クラッシュ時は指数バックオフで自動再起動、スリープ復帰時にも再起動。再起動後はフロントが保持した preedit を `restore_session` で復元する
- 状態機械はOSに一切依存しない純粋なRustコードとし、ヘッドレステスト対象とする

### プロトコル

- 正のスキーマは Rust 側（`core/protocol/`）。Swift の型は `typeshare` で生成してコミットし、CI で差分検査
- `process_key` は同期要求で、応答に描画差分 `{consumed, commit?, preedit?, candidates?, mode?, revision}` をまとめて返す。非同期通知はライブ変換・スタイル変換結果・Space 変換の遅延した LLM 結果の差し替え等のみ（`revision` で古いものを破棄）
- 文字位置は UTF-16 コード単位。キーは `{keysym(XKB), keycode, text, modifiers, kind}`
- メソッド一覧は設計書 §3.2

### リポジトリ構成（案・未作成）

Phase 1 は `engine` / `romaji` / `dict` / `infer` / `protocol` / `server` から始め、`learn` / `rewriter` / `keymap` / `style` は各Phaseで分離する。
補助タスクは `cargo xtask`（OS非依存。bash/ps1 を書かない）。macOS / Windows のどちらでもコアを開発・テストできること。

```
nagi/
├── core/            # Rust workspace（engine, romaji, dict, infer, protocol, server, xtask, ...）
├── frontend-macos/  # Swift + IMKit
├── frontend-windows/# TSF
├── config/          # default.toml / romaji tables / keymap presets / style presets
├── bench/           # 精度・レイテンシベンチ
└── docs/
```

## 設計上の不変条件

- プリエディットは `Preedit { segments, cursor, focus, selection, revision }` / `Segment { reading, surface, kind, candidates, chunk_id }` のセグメント構造。**Phase 1の時点で導入する**（後付けにしない）
- `reading`（ひらがな）が常に真実で、`surface` はその射影。1 Segment = 1 文節相当。reading と surface の対応はコア側のアライメントで求める（設計書 §4.2）
- `SegmentKind`: `Japanese` / `Literal`（英数・記号・数字。surface は変えないが推論の文脈には含める）/ `Fixed`（部分確定済み）/ `Styled`（スタイル変換結果。reading を持たず Undo 用に元を保持）
- 状態: `Direct` / `Composing`（ライブ変換はここ）/ `Converting`（注目文節あり）/ `RangeSelecting`（2文節以上の選択）/ `StyleAction`（LLM実行中・キャンセル可能）
- 状態遷移はすべて `Action` enum で駆動し、キーとの対応はキーマップ（TOML）が解決する
- 変換操作: `←/→` 注目文節移動、`Shift+←/→` 文節区切り伸縮、`Ctrl+Shift+←/→` 複数文節選択（キーマップで変更可）

## 変換パイプライン

`key → romaji変換 → Preedit更新 → 候補生成 → アライメント → ランキング → 差分通知`

候補の供給元（既定順、`[ranking]` で上書き可）: 学習キャッシュ → ユーザー辞書/スニペット → LLM推論 → システム辞書(Viterbi) → ひらがな/カタカナ → Rewriter。
出所アノテーションはプロトコル上 enum、アイコンはフロントで割り当てる。

- N-best: 文全体は LLM ビームサーチ（既定幅3）、文節は辞書列挙 + LLM再採点
- ライブ変換: `composing_chunk_len`（既定30）でチャンク分割し編集チャンクのみ再変換。強制分割は記号連続と Fixed 境界のみ（数字の直後では分割しない）
- 辞書 + Viterbi のみで動く `minimal` 経路を Phase 1 から持つ（モデル無し・障害時の最後の砦）
- 投機的デコーディング（Zenzai方式）は Phase 6

## 設定・データ

- 設定: `config.toml`（全項目は `config/default.toml` に日本語コメント付き）。未指定項目はプロファイルが決め、明示指定が常に優先。ホットリロード失敗時は旧設定維持＋行番号付き通知
- 設定パス: XDG / `~/Library/Application Support/com.shiki.nagi/` / `%APPDATA%\Nagi\`
- ユーザー辞書: Mozc TSV互換（インポート/エクスポートは Mozc TSV を正とする）
- 学習ファイル: TSV（スキーマバージョン付き）。IME切替・フォーカス喪失時 + 5分毎に原子的 flush
- ローマ字テーブル: Mozc 互換 TSV。v1 は標準のみ
- モデル・辞書はリポジトリに入れない（取得スクリプト + SHA-256 検証）

## プライバシー・セキュリティ・配布

- 全処理ローカル完結。ネットワークはモデル/辞書のダウンロードとアップデート確認（無効化可）のみ
- secure input / シークレットモード / パスワードマネージャ系アプリでは学習・Surrounding Text取得・ログを停止する
- ログに入力内容を残さない（デバッグビルドのみopt-inで許可）
- 学習データはワンコマンドで全削除可能にする
- macOS 配布は Apple Developer Program 非加入: ad-hoc 署名 + インストールスクリプト（`shiki-01.com`）+ ed25519 署名検証付きアップデート
- モバイル（将来）: iOS は SideStore、Android は APK 直接配布。どちらも本体アプリで更新確認し、データ資産は再インストールなしで更新（`docs/mobile-design.md` §6）

## テスト戦略

- 状態機械ゴールデンテスト: キー列 → (preedit, commit) の期待値をYAMLで定義（遷移表と1対1）
- 標準IME（macOS標準 / MS-IME / Mozc）との挙動照合、アライメントテスト
- JSON-RPC統合テスト（再起動・`restore_session` 含む）
- 変換精度ベンチ・レイテンシベンチ（プロファイル別）
- フロントは手動E2Eチェックリスト（ブラウザ/ターミナル/エディタ/Office系/チャット系）

## パフォーマンス目標（基準機 M1/8GB・balanced）

| 指標 | 目標 |
| --- | --- |
| キー入力→ひらがな反映 | p95 < 16ms |
| キー入力→ライブ変換反映 | p95 < 50ms |
| Space変換→候補表示 | p95 < 150ms |
| スタイル変換（1文） | < 1.5s |
| アイドルメモリ（light） | < 500MB 目安 |
| コールドスタート | < 2s（minimal は < 300ms） |

## ロードマップ

各フェーズ完了時点で日常利用できることを DoD（完了条件）に含める。

| Phase | 内容 |
| --- | --- |
| 0 | 技術検証（推論、モデル/辞書比較、アライメント検証、Laya評価、IMKitスケルトン） |
| 1 | MVP（macOS・文全体変換 + 文節変換。状態機械 + IR + 辞書Viterbi + LLM + JSON-RPC + プロファイル自動判定） |
| 2 | ライブ変換 + 学習 + ユーザー辞書/スニペット + プロファイル実行時切替 |
| 3 | 範囲操作 + 確定済みテキストの再変換 + カスタムキーバインド |
| 4 | スタイル変換 + Rewriter拡張 + プロファイル注入 + 設定GUI |
| 5 | Windows対応（TSF）。コア側の変更が原則ゼロであること |
| 6 | パフォーマンス最適化（投機的デコーディング等） |

## 非目標

かな入力レイアウト / 手書き・音声入力 / クラウド変換 / 候補ウィンドウのマウス操作（v1）。

iOS・Android は v1 では実装しないが、将来対応する前提で設計する（`docs/mobile-design.md`）。コアの実装・レビューでは同文書 §3 の制約（コアをライブラリとして完結させる・パスとプラットフォーム情報は注入・stdio/`std::process` に依存しない・モバイルでビルドできる依存に留める）を守る。

## 参考実装

karukan（コア分離・プロトコル・チャンク・adaptive）、azooKey / Zenzai（投機的デコーディング・スタイル変換）、rakukan（Windows TSF・別プロセス化）、riq0h fork（部分変換）、Mozc（辞書・Rewriter・文節操作）、Laya（reranker / 英日判定の評価対象）。

## Git 運用

- 指定がない限り `main` ブランチで直接作業・コミット・プッシュする（作業ブランチは切らない）
- ブランチや PR を使うのは、ユーザーから指定があった場合のみ

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
