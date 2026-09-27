# Nagi 進捗記録

最終更新: 2026-09-27（設定・環境プロファイル・学習・ユーザー辞書の実装後）。決定の詳細・根拠は `docs/decisions.md`（特に G 節）を参照。

## 完了した項目（Phase 1 のコア部分）

すべてコミット済み（`main`）。最終状態で `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通る（ローカル Windows 11 で確認。GitHub Actions の結果は下記「CI」参照）。

| # | 項目 | 内容 |
| --- | --- | --- |
| 1 | ワークスペースの土台 | `core/` に engine / romaji / dict / infer / protocol / server / xtask。edition 2024、Rust 1.89.0 固定、rustfmt / clippy 設定、`.gitignore`、`LICENSE-MIT` / `LICENSE-APACHE`、CI（ubuntu / windows / macos + typeshare 差分検査）。ビルド・テスト・lint コマンドは `CLAUDE.md` に追記済み |
| 2 | romaji | Mozc 互換 TSV の標準テーブル（`config/romaji/standard.tsv`、自作）、最長一致 + 保留の変換器、`nn` の soft pending、促音、Shift+英字（`literal_continue` / `single_upper`）、数字・記号の Literal 扱い、入力バッファ `Composer`（カーソル移動・削除・`load_pieces`）。66 テスト |
| 3 | protocol | 設計書 §3.2 の全メソッド・型、JSON-RPC エンベロープ、エラーコード、UTF-16 ユーティリティ、XKB keysym 定数。`#[typeshare]` 付与、`cargo xtask gen-types` / `check-types`、Swift 型を `frontend-macos/Sources/Generated/Protocol.swift` に生成・コミット済み（差分なしを確認）。24 テスト + doctest |
| 4 | engine | IR（Preedit / Segment / SegmentKind / CandidateSet）、`Action` enum、キーマップ（内蔵既定）、Direct / Composing / Converting の状態機械（文節移動・伸縮・候補・カタカナ/ひらがな・確定取消・モード切替・focus_out・reset・restore_session）、差分応答と revision、Surrounding Text の左文脈。ゴールデンテスト基盤（YAML）と 159 ケース、API テスト 7、単体 39 |
| 5 | server | `imserver`: 改行区切り JSON-RPC ループ。`initialize` / `process_key` / `focus_in` / `focus_out` / `reset` / `restore_session` / `set_context` / `reload_config`（no-op）/ `ping` / `shutdown`。実プロセスを起動する統合テスト（クラッシュ → `restore_session`、EOF、辞書 E2E を含む）17 件 |
| 6 | dict | `cargo xtask fetch-dict`（Mozc OSS 辞書、コミット固定 + SHA-256 検証）、`build-dict`（53MB、2 秒）、`fst` による trie、連接コスト、Viterbi、POS ベースの文節区切り（暫定）、文節候補。実データで「今日は\|いい天気です」等を確認。単体 13 + 実データ 4 テスト。`minimal` 経路として server に接続済み（`--data-dir`） |
| 7 | infer | `ConversionModel` / `PromptAdapter` trait、zenz-v3.1 アダプタ、`MockModel`、`llama-cpp-2 =0.1.157`（feature `llama`）による実推論、`cargo xtask fetch-model`（SHA-256 検証）。zenz-v3.1-small で 7 文すべて正しく変換（28〜47ms/文）。自前の GGUF 語彙リーダーとトークナイザを実装（G-3）。既定 feature では llama.cpp をビルドしない |
| 8 | LLM の変換パイプラインへの接続 | `HybridBackend`（server）: LLM で文全体を変換 → 辞書の格子で強制アライメントして文節に分ける（`Dictionary::align`）、文節候補は LLM の最良変換 + 辞書候補の LLM 再採点（`ConversionModel::score`）、`sync_budget` 超過時は辞書の結果 + 結果キャッシュ、縮退（LLM → 辞書 → 読み）、`--no-llm` / `--sync-budget-ms`。`nagi-eval`（Mozc `evaluation.tsv` での精度・レイテンシ計測）。評価データ 609 件で辞書のみ 66.0% → 辞書 + LLM 76.4%、`convert_sentence` p50 32ms / p95 56ms。詳細は G-11。LLM を使うには server を `--features llama` でビルドする |
| 9 | 設定・環境プロファイル・学習・ユーザー辞書 | `nagi-config`（行番号付きエラー・`default.toml`・再読み込み）、`nagi-profile`（初回判定・キャリブレーション・自動切替）、`nagi-learn`（学習の記録・削除・原子的保存）、ユーザー辞書（Mozc TSV・スニペット・オーバーレイ）、`Pipeline` の優先順統合、`set_environment` / `get_status` / `clear_learning`、`Ctrl+Delete`。詳細は G-12 |

## 未完了の項目と次の一手

優先順:

1. **LLM 接続の残り**: 非同期通知（`update_preedit` / `update_candidates`）での結果差し替え、ビームサーチによる文全体の N-best（`conversion.initial_view = sentence`）、KV キャッシュの再利用、ライブ変換のチャンク処理（Phase 2）。数詞の表記ゆれは Rewriter（Phase 4）
2. **予測変換**（Phase 2）: 学習・ユーザー辞書の前方一致（`Learner::lookup_prefix` は用意済み）、候補ウィンドウの Tab / ↓ 選択
3. ~~**環境プロファイルの残り**: モデルの解放（minimal・メモリ逼迫時）、`light` でのスレッド数削減、GPU 検出、キャリブレーション結果の永続化~~ → 実装済み（G-13）。残るは `quality` で使う大きいモデル・reranker（Laya 評価。Phase 0 の比較が必要）
4. **設定の残り**: キーマップ TOML（Phase 3。`[keymap]` は `preset` だけ）、設定 GUI（Phase 4）、`update.auto_check` の実装
5. **Phase 0 の比較**: 変換モデル（zenz 系の他サイズ・量子化）とシステム辞書（SudachiDict 等）の比較を `nagi-eval` で行う。基準機での実測
6. macOS フロントエンド（IMKit。`set_environment` の送信、`set_context.app_id` の報告を含む）、インストールスクリプト、アップデート確認、`NOTICE`（IPAdic のライセンス全文、zenz の CC-BY-SA-4.0）
7. Phase 3 以降: 範囲操作（`RangeSelecting` / `Ctrl+Shift+←/→`）、確定取消、Rewriter、スタイル変換

## G 節（`docs/decisions.md`）に記録した要レビュー事項

- G-1 Rust 1.89.0 固定 / G-2 lint 方針
- G-3 llama.cpp バインディングの選定、pre-tokenizer の回避（KV override + 自前トークナイザ）、実測値
- G-4 システム辞書は Mozc OSS を採用（ライセンス表記義務、`fst` 採用理由、精度の目安）。`evaluation.tsv` は許可を得て取得
- G-5 文節区切りの暫定規則と弱点
- G-6 アライメントの草案 → G-11 で実装済み
- G-7 **状態遷移表の草案**（`SelectCand` は選択のみで確定しない判断を含む）
- G-8 ローマ字・入力バッファの判断（`nn` の soft pending、Preedit への `pending` 追加）
- G-9 プロトコル・サーバーの判断（`revision` を u32、`reload_config` no-op など）
- G-10 その他（CI、xtask、コミット粒度）
- G-11 LLM の変換パイプライン接続（構成・計測結果・未実装事項）
- G-12 設定・環境プロファイル・学習・ユーザー辞書（仕様・優先順位・未実装事項）
- G-13 環境プロファイルの残り4項目（GPU 検出・light のスレッド数削減・minimal 時のモデル解放・キャリブレーション永続化）

> G-1〜G-10 は、ユーザーが「暫定案のままで良い」と承認済み（2026-09-27）。

## 既知の問題

- CI の `types` ジョブは `typeshare-cli` を毎回 `cargo install` する（時間がかかる。キャッシュ化の余地あり）
- 途中のコミットには、後続コミットの変更に依存してビルドできないものがある（最終状態は通る）
- 文節区切りは暫定ヒューリスティック（サ変名詞 + する、形容詞 + 名詞など）。誤変換の例: 感じ変換の\|テスト、機能病院に（LLM で改善する領域）
- `Preedit` の Composing 表示では、非 ASCII 文字（絵文字など）も japanese 種別の 1 セグメントにまとまる
- `nn` の soft pending は Mozc の実挙動と一致するか未確認（`konnichiha` → こんにちは、`kanna` → かんな は通る。`nnn` は「んん」）
- ゴールデンテストの期待値は、テスト担当のサブエージェントが仕様から書いた後、実装の挙動と食い違う箇所を「仕様に照らして」修正したもの。仕様の根拠は `docs/decisions.md` G-7。追加のレビュー（標準 IME との挙動照合）は未実施
- macOS / Windows 実機でのフロント接続、標準 IME との挙動照合、レイテンシベンチ（p95）は未実施
- `infer` の `LlamaModel` は変換ごとにコンテキストを作る（KV キャッシュ再利用・ビームサーチ・GPU 未対応）

## CI

（最終確認の結果を下に追記する）
