# Nagi 進捗記録

最終更新: 2026-09-27（ライブ変換・範囲操作・reconvert・確定取消・辞書ベースのビームサーチ・ed25519署名検証（origin）と、実LLMビームサーチ・KVキャッシュ再利用（別セッション、G-32〜G-34）を統合後）。決定の詳細・根拠は `docs/decisions.md`（特に G 節）を参照。

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
| 10 | 予算超過した LLM 結果の非同期差し替え | Space 変換で LLM が `sync_budget` を超えたとき、辞書の結果で応答したあと、完了時に `update_preedit` / `update_candidates` で表示を差し替える（未操作のときだけ）。`Engine::apply_completions`、`Pipeline` の `Completion` 通知、`Server::run_threaded`（入力読み取りスレッド + waker）。詳細は G-11 |
| 11 | LLM コンテキストの再利用（KV キャッシュ） | `llama::LlamaModel` が `convert` / `score` のたびにコンテキストを作り直すのをやめ、1 つを使い回して `clear_kv_cache` で空にする。自己参照構造（`model` を `Box` で固定・`ctx` を `'static` と偽って保持）のため `core/infer` の `unsafe_code` を局所解禁（G-2 が想定していた緩和）。`convert` / `score` を交互に呼んでも結果が混ざらないことをテストで確認。詳細は G-32 |
| 12 | LLM のビームサーチ（infer 単体、Stage 1） | `ConvertRequest::beam_width` を新設し、`llama::LlamaModel::convert` に設計書 B-4 どおりの真のビームサーチ（各ステップで上位 `width` 本に絞りながら進める）を実装。KV キャッシュの分岐・合流は `kv_cache_seq_cp`（要 `kv_unified = true`）。`beam_width <= 1` は既存の貪欲デコードのまま（挙動・レイテンシとも無変更）。実モデルで曖昧性の低い文でも表記ゆれ・別の妥当な変換が上位に出ることを確認。詳細は G-33 |
| 13 | LLM のビームサーチ（server/engine 統合、Stage 2） | `Effective::beam_width`（プロファイル既定 + `conversion.beam_width` 上書き）、`Pipeline::sentence_candidates`（仮説を強制アライメント + 学習/ユーザー辞書の優先を適用、重複除去、縮退条件）、engine の候補ウィンドウ sentence モード（`Conv::whole`/`sentence`、`conversion.initial_view`、`←/→` 等での恒久的な離脱）。プロトコル・Swift 型の変更は無し。詳細は G-34（辞書ベースの N-best との統合方法は G-30 の追記を参照） |

## 未完了の項目と次の一手

優先順:

1. **LLM 接続の残り**: ~~非同期通知（`update_preedit`）でのライブ変換結果差し替え~~ → 実装済み（G-25・G-26）。~~ビームサーチによる文全体の N-best~~ → 辞書ベースのk-best Viterbi（G-30）に加え、実LLMでの本物のビームサーチも infer/server/engine とも実装済み（G-33・G-34。別セッションでの実装）。~~KV キャッシュの再利用~~ → 当初見送り（G-21）だったが後日実装（G-32）。`update_candidates` の非同期送出（スタイル変換等。未着手）。~~ライブ変換のチャンク処理~~ → 実装済み（G-27。`composing_chunk_len` による固定長分割。「編集されたチャンクのみ再変換」というキャッシュ最適化は未着手）。~~数詞の表記ゆれ~~ → Rewriter で実装済み（G-16・G-20）。残るのはキーマップでの sentence モード専用割当と `nagi-eval` でのビームサーチ込み精度計測（優先度低）
2. ~~**予測変換**（Phase 2）~~ → 実装済み（G-23）
3. ~~**環境プロファイルの残り**: モデルの解放（minimal・メモリ逼迫時）、`light` でのスレッド数削減、GPU 検出、キャリブレーション結果の永続化~~ → 実装済み（G-13）。残るは `quality` で使う大きいモデル・reranker（Laya 評価。Phase 0 の比較が必要）
4. ~~**キーマップ TOML**: `[keymap]` は `preset` だけ~~ → 実装済み（G-14。`composing`/`converting` の個別キー上書きに対応。`Insert`/`InsertText`/`SetMode` への上書きと `Direct` 文脈は対象外）。~~`update.auto_check` の実装~~ → 実装済み（G-19。起動時の自動確認・notify のみ。実サーバー（`nagi.shiki-01.com/updates/latest.json`）は未整備なので機能するにはサーバー側の用意が必要）。**設定の残り**: 設定 GUI（Phase 4）
5. **Phase 0 の比較**: 変換モデル（zenz 系の他サイズ・量子化）とシステム辞書（SudachiDict 等）の比較を `nagi-eval` で行う。基準機での実測
6. macOS フロントエンド（IMKit。`set_environment` の送信、`set_context.app_id` の報告を含む。**未着手**: 本環境にXcode等が無く、Swift/IMKitのコードを書いてもコンパイル確認ができないため）。~~アップデート確認（署名検証）~~ → 検証ロジックは実装済み（G-31。ed25519署名検証。ダウンロード・インストールの実処理はプラットフォーム実装待ち）。インストールスクリプトは、対象の`.app`バンドル（フロントエンド本体）が無いため未着手（フロントエンド着手時に合わせて作る）。`NOTICE`（IPAdic のライセンス全文、zenz の CC-BY-SA-4.0）は作成済み
7. Phase 3 以降: ~~範囲操作（`RangeSelecting` / `Ctrl+Shift+←/→`）~~ → 選択・再変換・ToHankaku まで実装済み（G-17・G-22）。~~確定取消~~ → エンジン側は実装済み（G-28。`ProcessKeyResult.delete_before` を追加。ホストアプリのテキストを実際に削除するフロント側の対応は別途必要）。スタイル変換は未着手。~~Rewriter~~ → 全角/半角英数記号・半角カナ・漢数字・日付・簡易計算まで実装済み（G-15・G-16・G-20）
8. `select_candidate`（JSON-RPC）実装済み（G-18）。~~`reconvert`~~ → 実装済み（G-29。辞書の逆引きによる専用Viterbi。ユーザー辞書は対象外。実データでのメモリ増加量は実機（Windows）で実測済み: 約270MB。`minimal`/`light` では使用後に自動解放するよう対応済み）
9. ~~KV キャッシュ再利用は見送り~~ → 別セッションで実装した（G-32。既存の見送り理由（G-21）にあった自己参照構造の問題を、手書きの `unsafe` で解決した）

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
- G-14 キーマップ TOML 上書き（composing/converting の個別キー上書き）
- G-15 Rewriter 基盤（全角/半角英数記号・半角カナ）
- G-16 Rewriter: 漢数字変換（digit-for-digit・位取り）
- G-17 範囲操作: 複数文節選択（選択の開始・伸縮・解除のみ。再変換・Rewriter直接書き換えは未着手）
- G-18 select_candidate メソッドの実装（絶対indexでの候補選択。reconvertは未着手）
- G-19 update.auto_check の実装（アップデート確認クライアント。実サーバーは未整備）
- G-20 Rewriter: 日付・簡易計算の追加
- G-21 KV キャッシュ再利用の見送り（理由・再検討条件を記載）
- G-22 範囲選択への再変換・ToHankaku（Mozkey を参照。engine が nagi-rewriter に直接依存する方針転換を含む）
- G-23 予測変換（Tab）。設計書の「Tab/↓」を Mozc の実際のキーマップに合わせて Tab のみに変更した経緯を記載
- G-24 非同期通知の配信基盤（`Server::run` の非同期化。`StdinLock` が `Send` でない問題の対処を含む）
- G-25 ライブ変換: engine側（`Session.live_conversion` / `Engine::live_convert`。タイマーキャンセル不要のデバウンス設計）
- G-26 ライブ変換: server側（デバウンススレッド・`Arc<Mutex<Engine>>` 化・`update_preedit` 配信。Mozkey の実測既定値を採用）
- G-27 ライブ変換: `composing_chunk_len` によるチャンク処理（数字直後の分割を避ける制約が Composer の Piece 構造上自動的に満たされる理由を記載。「編集されたチャンクのみ再変換」の最適化は対象外）
- G-28 確定取消（Ctrl+Backspace）。`ProcessKeyResult.delete_before` の追加。`typeshare-cli` をこの実行環境にインストールして型生成・差分検査まで検証済み（crates.io は到達可能）
- G-29 reconvert（確定済みテキストの再変換）。辞書の逆引きによる専用Viterbi（`nagi-dict::reverse_convert`）。ユーザー辞書は対象外、未知語はreading=surfaceのまま通す（LLM補完は今回のスコープ外）
- G-30 ビームサーチ: 辞書ベースの文全体N-best（`nagi-dict::convert_n_best`。k-best Viterbi）。1件目をLLM再採点済みの1-bestに揃えることで変換品質の後退を防いだ経緯を記載
- G-31 アップデート配布物のed25519署名検証。`UPDATE_PUBLIC_KEY_HEX` はプレースホルダー（実配布前に鍵の差し替えが必要）
- G-32 LLM コンテキストの再利用（KV キャッシュ。自己参照構造・`unsafe_code` の局所解禁・`unsafe impl Send` の根拠・実測値。G-21 の見送りを覆した）
- G-33 LLM のビームサーチ（infer 単体、Stage 1。アルゴリズム・`kv_unified` の必要性・実測・未実装の統合作業）
- G-34 LLM のビームサーチ（server/engine 統合、Stage 2。`Effective::beam_width`・`Pipeline::sentence_candidates`・候補ウィンドウの sentence モード・既知の限界）

> G-1〜G-10 は、ユーザーが「暫定案のままで良い」と承認済み（2026-09-27）。

## Phase 3 以降の要決定事項（UX の詳細が設計書だけでは一意に決まらない項目）

2026-09-27、ユーザーが回答した内容に基づき着手中・着手予定。**Mozkey**（`koyasi777/mozkey`。Mozc + ローカル Zenz 補正の fork。ユーザーが実際に使用しており「使いやすい」と評価）の実装を UX の参考にする方針（ユーザー指示）。

- ~~**予測変換**~~ → 実装済み（G-23。Tab のみ（Down は Mozc でも予測選択に使われないため対象外）。`[predict]` で設定可能）
- ~~**ライブ変換**~~ → 実装済み（G-25・G-26。既定 ON、`live_conversion_delay_ms`＝228ms・`live_conversion_min_key_length`＝2文字は Mozkey の実測既定値。デバウンス完了時に `update_preedit` で配信）。**未対応**: ふりがな（reading）のルビ風オーバーレイ表示はプロトコル上は可能（`SegmentView.reading` が既に載っている）だが、フロント（macOS/Windows）側の描画実装がまだ無い。Enter/Backspace の細かい挙動（ライブ変換結果からの部分確定・取消）は既存の Composing 用ロジックのまま（Mozkey との詳細比較は未実施）
- ~~**非同期通知の配信基盤**~~ → 実装済み（G-24。`Server::run` の非同期化）。`update_preedit` の送出（ライブ変換）は実装済み（G-26）。**`update_candidates` の送出（スタイル変換等）はまだ無い**
- ~~**ビームサーチ（文全体 N-best）**~~ → 実装済み（G-30。辞書ベースのk-best Viterbi。実LLMでの本物のビームサーチは本環境で検証不能なため未着手。候補選択時は選んだ分割へ丸ごと置き換える設計）
- ~~**範囲選択への再変換・Ctrl+U/I/O 直接書き換え**~~ → 実装済み（G-22）

## 実機で確認してほしいこと

このセッションで実装した機能は、実データ（Mozc辞書・zenzモデル）もフロントエンドも無い環境で開発したため、以下は実機（`cargo xtask fetch-dict && cargo xtask build-dict`、必要なら `fetch-model` の後）でしか確認できない。詳細な経緯は各Gエントリを参照。**具体的な手順・コマンド付きのチェックリストは `docs/real-hardware-checklist.md` を参照。**

1. **ライブ変換の基本動作**（G-25・G-26）: 2文字以上のかな入力後、キー入力を止めて約228ms（既定の `live_conversion_delay_ms`）待つと `update_preedit` 通知が届くこと。1文字だけの入力では届かないこと。
2. **ライブ変換の変換品質**: 実データでの変換結果が Space 変換と比べて違和感が無いか。
3. **デバウンススレッドの負荷**: 高速タイピング時に入力ごとに1スレッド立つ設計（G-26）。実際の入力速度で問題にならないか。
4. **長文のライブ変換のレイテンシ**（G-27）: `composing_chunk_len` でチャンク分割しているが、「編集されたチャンクのみ再変換」というキャッシュ最適化は未実装。長い読みを一度に再変換する遅延が気になるレベルか。気になる場合はキャッシュ最適化の実装を検討する。
5. **確定取消（Ctrl+Backspace）**（G-28）: エンジン側の機構のみで、フロント側で実際にホストアプリのテキストを削除する対応（`ProcessKeyResult.delete_before` を使う）はまだ無い。フロント実装時に対応が必要。
6. ~~**reconvert の逆引き辞書のメモリ増加量**~~ → 実機（Windows、実データ）で実測済み: 約270MB（見積もりの「数十MB」は誤りだった。G-29 追記）。`minimal`/`light` プロファイルでは `reconvert` 使用後に自動解放するよう対応済み（`Dictionary::release_reverse_index`）。`balanced`/`quality` では常駐させたまま。
7. **reconvert の推定精度**: 辞書のみでの読み推定（LLM補完なし）が実際の確定済みテキストに対してどの程度使えるか。未知語（辞書に無い表記）の頻度も含めて確認してほしい。
8. **ビームサーチ（文全体N-best）の実際の見え方**（G-30）: 実データ・実LLMで、Space変換の1回目に候補窓が開く挙動（設計書どおりの `initial_view = sentence`）が実際に使いやすいか。1件目はLLM再採点済みの結果に揃えたが、2件目以降は辞書コスト順（LLM再採点なし）なので、この並びが不自然に感じられないか確認してほしい。
9. **アップデート署名検証の鍵**（G-31）: `UPDATE_PUBLIC_KEY_HEX` は実装確認用の使い捨て鍵。実際に配布する前に本物の鍵ペアを生成し直し、秘密鍵をビルド・署名環境にオフラインで保管する必要がある。

## 既知の問題

- ~~CI の `types` ジョブは `typeshare-cli` を毎回 `cargo install` する~~ → `actions/cache` でバイナリをキャッシュするよう修正（`~/.cargo/bin/typeshare`。キー `typeshare-cli-<os>-v1`。バージョンを上げたい場合はキーの `v1` を変える）
- 途中のコミットには、後続コミットの変更に依存してビルドできないものがある（最終状態は通る）
- 文節区切りは暫定ヒューリスティック（サ変名詞 + する、形容詞 + 名詞など）。誤変換の例: 感じ変換の\|テスト、機能病院に（LLM で改善する領域）
- `Preedit` の Composing 表示では、非 ASCII 文字（絵文字など）も japanese 種別の 1 セグメントにまとまる
- `nn` の soft pending は Mozc の実挙動と一致するか未確認（`konnichiha` → こんにちは、`kanna` → かんな は通る。`nnn` は「んん」）
- ゴールデンテストの期待値は、テスト担当のサブエージェントが仕様から書いた後、実装の挙動と食い違う箇所を「仕様に照らして」修正したもの。仕様の根拠は `docs/decisions.md` G-7。追加のレビュー（標準 IME との挙動照合）は未実施
- macOS / Windows 実機でのフロント接続、標準 IME との挙動照合、レイテンシベンチ（p95）は未実施
- `infer` の `LlamaModel` はコンテキストを 1 つ使い回す（KV キャッシュ再利用・実LLMビームサーチとも server/engine まで接続済み。G-32、G-33、G-34）が、GPU オフロードは未対応

## CI

（最終確認の結果を下に追記する）
