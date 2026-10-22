# Nagi 進捗記録

最終更新: 2026-10-22（Windows フロント: OS 非依存部 `nagi-windows` に SessionStore・VK_IME_ON/OFF を追加、TSF 層 `nagi-windows-tsf` の骨組み。決定は H-39）／ 2026-10-16（iOS: 確定方式・注目文節・取り消し・トグル・触覚音・入力欄の種類・キーボード切替の原因、既定モデル small。決定は H-22(c)・H-27〜H-33）／ 2026-10-15（iOS: 設定・レイアウト・ユーザー辞書・次語予測・絵文字候補・堅牢性・レビュー修正。決定は H-16〜H-21）／ 2026-10-14（iOS の実機検証・タッチ入力・製品版キーボードを追記。決定は decisions.md の H-6〜H-17）／ 2026-09-30（G-35〜G-49 の反映: SudachiDict 対応・モデル比較・bundle id 変更・モデル非同期読み込み・設定 RPC/GUI・ライブ変換とローマ字入力の挙動修正。それ以前は 2026-09-27 時点: ライブ変換・範囲操作・reconvert・確定取消・辞書ベースのビームサーチ・ed25519署名検証（origin）と、実LLMビームサーチ・KVキャッシュ再利用（別セッション、G-32〜G-34）を統合後）。決定の詳細・根拠は `docs/decisions.md`（特に G 節）を参照。

## 完了した項目（Phase 1 のコア部分）

すべてコミット済み（`main`）。最終状態で `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` が通る（ローカル Windows 11 で確認。GitHub Actions の結果は下記「CI」参照）。

| # | 項目 | 内容 |
| --- | --- | --- |
| 1 | ワークスペースの土台 | `core/` に engine / romaji / dict / infer / protocol / server / xtask。edition 2024、Rust 1.89.0 固定、rustfmt / clippy 設定、`.gitignore`、`LICENSE-MIT` / `LICENSE-APACHE`、CI（ubuntu / windows / macos + typeshare 差分検査）。ビルド・テスト・lint コマンドは `CLAUDE.md` に追記済み |
| 2 | romaji | Mozc 互換 TSV の標準テーブル（`config/romaji/standard.tsv`、自作）、最長一致 + 保留の変換器、`nn` は保留なしで「ん」（G-48。旧 soft pending は廃止）、Backspace は直前キー入力の取り消し（G-49）、促音、Shift+英字（`literal_continue` / `single_upper`）、数字・記号の Literal 扱い、入力バッファ `Composer`（カーソル移動・削除・`load_pieces`）。66 テスト |
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

## iOS 関連の実装（2026-10-14 追記、2026-10-16 更新。詳細は `docs/decisions.md` H-6〜H-33）

| 項目 | 内容 |
| --- | --- |
| 辞書の mmap 化 | `Dictionary::load` を mmap（非 Windows）。形式は version 2 のまま（H-7）。拡張で +61MB → +0.2MB |
| メモリ上限による LLM 抑止 | `--memory-budget-mb` / `nagi_server_new_with_budget`。150MB 未満は辞書のみ（H-8） |
| 非同期通知の C ABI 配信 | `Server::poll_notifications` / `set_wake_callback`、`nagi_set_waker` / `nagi_poll_notifications`（H-10） |
| タッチ入力 | `insert_reading`（12 キー・フリック・濁点循環）/ `perform_action`（H-11） |
| 入力中の候補と部分変換 | 入力中に候補を常に出し、候補の選択で先頭部分だけ確定（`CandidateView.covers`、H-14） |
| iOS の製品版キーボード | 拡張の UI（`NagiKeyboardView`）、状態（`KeyboardModel`）、型付きセッション（`NagiSession`）、ホストアプリの入力テスト（H-12・H-13） |
| iOS のビルド・配布 | GitHub Actions `ios-testflight`（署名・アップロード・証明書の掃除）。TestFlight へ配布（H-15） |
| 実機の実測 | 拡張の上限 約 177MB、xsmall・small とも動作、応答時間は同期待ち 30ms + 非同期差し替え（H-6・H-9・H-10） |
| 既定モデル | small。未選択時は端末の予算で自動選択（余裕があれば small）、Picker での明示選択を優先（H-22(c)。2026-10-16 ユーザー決定） |
| 確定方式 | `insertText` のみで変換中の範囲を置き換える `insert` を既定に（`keyboard.commit_method`）。手前の本文が消える不具合と部分変換の重複（U-1）は解消（H-28・H-31） |
| 注目文節 | 候補を選ぶと次の文節へ進む（最後の文節で全体を確定）。候補バーの読み表示で注目文節を強調（H-29） |
| 取り消し | フリックは大きく外すと取り消し（表示は出さず選択を消すだけ）、長押しは下 0.3 倍・左右の外で取り消し（H-30・H-31） |
| トグル | 英字 12 キー（`kana` 指定の無い flick キー）もトグル入力で置き換わる（H-31） |
| 触覚・音 | クリック音は `off` / `compat`（既定 `compat`、`AudioServicesPlaySystemSound(1104)`）。互換モードは専用キューで鳴らし速打ちで遅れない（H-30・H-31） |
| 入力欄の種類 | `keyboardType` でレイアウトを自動選択、`returnKeyType` で改行キーの表示を変える。`keyboard.layout_by_field`（既定 true）で切替（H-32） |
| キーボードの切替 | 他社キーボードと併用時に iOS が別のキーボードを選ぶのが原因で、Nagi 側では直せない（H-33。H-27 の LLM 二重化の仮説は撤回） |


## 未完了の項目と次の一手

優先順:

1. **LLM 接続の残り**: ~~非同期通知（`update_preedit`）でのライブ変換結果差し替え~~ → 実装済み（G-25・G-26）。~~ビームサーチによる文全体の N-best~~ → 辞書ベースのk-best Viterbi（G-30）に加え、実LLMでの本物のビームサーチも infer/server/engine とも実装済み（G-33・G-34。別セッションでの実装）。~~KV キャッシュの再利用~~ → 当初見送り（G-21）だったが後日実装（G-32）。~~`update_candidates` の非同期送出~~ → 配信手段を C ABI にも追加（H-10: `nagi_set_waker` / `nagi_poll_notifications`）。スタイル変換等からの新規送出は未着手。~~ライブ変換のチャンク処理~~ → 実装済み（G-27。`composing_chunk_len` による固定長分割。「編集されたチャンクのみ再変換」というキャッシュ最適化は未着手）。~~数詞の表記ゆれ~~ → Rewriter で実装済み（G-16・G-20）。残るのはキーマップでの sentence モード専用割当と `nagi-eval` でのビームサーチ込み精度計測（優先度低）
2. ~~**予測変換**（Phase 2）~~ → 実装済み（G-23）
3. ~~**環境プロファイルの残り**: モデルの解放（minimal・メモリ逼迫時）、`light` でのスレッド数削減、GPU 検出、キャリブレーション結果の永続化~~ → 実装済み（G-13）。残るは `quality` で使う大きいモデル・reranker（Laya 評価。Phase 0 の比較が必要）
4. ~~**キーマップ TOML**: `[keymap]` は `preset` だけ~~ → 実装済み（G-14。`composing`/`converting` の個別キー上書きに対応。`Insert`/`InsertText`/`SetMode` への上書きと `Direct` 文脈は対象外）。~~`update.auto_check` の実装~~ → 実装済み（G-19。起動時の自動確認・notify のみ。実サーバー（`nagi.shiki-01.com/updates/latest.json`）は未整備なので機能するにはサーバー側の用意が必要）。**設定の残り**: 設定 GUI（Phase 4）
5. **Phase 0 の比較**: 変換モデル（zenz 系の他サイズ・量子化）とシステム辞書（SudachiDict 等）の比較を `nagi-eval` で行う。基準機での実測。**一次結果（H-17、2026-10-14）**: zenz-v3.1 small と xsmall を固定プロファイルで比較し、small が xsmall より +4〜7pt（75.0% vs 68.1%、balanced）。G-36 の「xsmall が有力」は auto 降格の影響で、撤回。残り: left_context ありの評価、他の量子化・サイズ、SudachiDict、iPhone/M1 相当の実測
6. ~~辞書形式を mmap 可能な形式へ変更~~ → 実装済み（H-7）。形式（version 2）は変えず、セクションを要素ごとのリトルエンディアン読み出し（アラインメント不要）にして `Dictionary::load` を mmap 化（非 Windows）。実機（iPhone 16e 拡張）で辞書の footprint は +61MB → +0.2MB、xsmall 併用のピークは 124MB → 71MB（H-7）
7. ~~`insert_reading` / `perform_action` のプロトコル設計を core/protocol に追加~~ → 実装済み（H-11。12 キーのトグル・フリック・濁点循環をコアで解決、Swift 型生成済み）。残るのはレイアウトの設定化・`initialize` の capability 拡張
8. ~~**iOS 拡張のメモリ上限実測と LLM 可否判定**（M0 最優先）~~ → 完了（H-6・H-9・H-10）。拡張の上限は約 177MB。mmap 辞書（H-7）で xsmall・small とも拡張内で動作し、iOS の品質基準は `minimal` ではなく LLM あり（`auto` は `light` から開始）。メモリ上限による LLM 読み込みの抑止（`--memory-budget-mb`、150MB 未満は辞書のみ）は実装済み（H-8）。他機種での実測は未確認
9. ~~**SideStore 検証**~~ → 不要（iOS の配布を TestFlight / App Store に変更。H-15）
10. **UniFFI 試作**（ネスト構造のコスト・callback・JSON か型かの計測）: FFI 方式を UniFFI で確定（H-4 参照）。実装時に以下を計測して詳細を決める: Preedit 等のネスト構造を毎キー渡すコスト、非同期通知（revision 付き差し替え）の callback 表現、描画差分を JSON 文字列か UniFFI 型で渡すか（p95 < 16ms 目標への影響）。**iOS は C ABI（`core/ffi/`）で実装済み**で、UniFFI は Android（M1）着手時に再検討（H-10）
11. macOS フロントエンド（IMKit。`frontend-macos/`、CI `macos-frontend.yml`）: **実装あり・実機未確認**。2026-10-22 に以下を追加（H-37。Swift は CI でビルド・テスト済み）: 入力モードの同期（英数モード `<bundle-id>.Roman` の登録、コアの mode → メニューバー、メニュー・英数/かなキー → `perform_action(SetMode)`）、`set_environment`（電源・低電力・熱・メモリ逼迫）、`set_context` の直前テキスト（入力開始時。secure input 中は読まない）、状態の通知表示（右上の小パネル）、入力メニューの状態表示・設定の再読み込み・学習データの削除、確定済みテキストの再変換（かなキー / 変換キー）。`set_context.app_id` と確定取消（`delete_before`）は実装済み。誤学習の削除は既存の Ctrl+Delete（コアのキーマップ）で動く。配布（H-37 追記）: Developer ID 署名・公証つきのリリース（`macos-release.yml`）、`latest.json`（ed25519 署名。`cargo xtask sign-update`）、インストールスクリプト（`web/static/install.sh` → `https://nagi.shiki-01.com/install.sh`）、入力メニューの「アップデートを確認…」（取得・検証・差し替え・再起動）を実装済み。**リリースには Secrets の登録と本物の鍵への差し替えが必要**（下の「macOS のリリース手順」）。辞書・モデルの取得（H-38）: `imserver --install-data` を同梱し、インストールスクリプトが自動で実行する。アプリでは辞書が無いときの最初の入力でダイアログを出し、入力メニューの「辞書とモデルをダウンロード…」からも実行できる。**未着手**: ふりがなのルビ表示。`NOTICE`（IPAdic のライセンス全文、zenz の CC-BY-SA-4.0）は作成済み
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
- G-31 アップデート配布物のed25519署名検証。公開鍵は `config/update-public-key.txt`（2026-10-22 に本番用へ差し替え済み）
- G-32 LLM コンテキストの再利用（KV キャッシュ。自己参照構造・`unsafe_code` の局所解禁・`unsafe impl Send` の根拠・実測値。G-21 の見送りを覆した）
- G-33 LLM のビームサーチ（infer 単体、Stage 1。アルゴリズム・`kv_unified` の必要性・実測・未実装の統合作業）
- G-34 LLM のビームサーチ（server/engine 統合、Stage 2。`Effective::beam_width`・`Pipeline::sentence_candidates`・候補ウィンドウの sentence モード・既知の限界）
- G-35 Phase 0: SudachiDict 対応（辞書内部の pos_id 分離・CSV/matrix.def パーサー）
- G-36 Phase 0: モデル比較（zenz-v3.1 small/xsmall。当時は xsmall が有力としたが H-17 で固定プロファイル比較により small が上と訂正）と、環境プロファイル不可逆降格バグの発見・修正
- G-37 macOS の bundle id を `com.shiki.inputmethod.Nagi` に変更
- G-38 imserver のモデル読み込みを非同期化（`initialize` を即時応答）
- G-39・G-40 ライブ変換中の Enter は表示中の変換結果を確定 / 「ん」重複（`nn` + 子音）の修正
- G-41 GUI 設定 RPC（`get_settings` / `update_settings`）はスキーマ駆動、`config.toml` の書き込みは `toml_edit`
- G-42 ローマ字標準テーブルの欠落 103 件を Mozc 原本との機械照合で修正
- G-43 ライブ変換: 追記中は変換結果を固定表示し末尾のみ再変換（G-47 で全文収束に更新）
- G-44・G-45 変換中の Backspace は変換の取り消し（ライブ変換の表示も破棄）/ Esc の状態別挙動。予測変換の扱いは現状仕様で確定
- G-46 Backspace 等の後はライブ変換を次の文字入力まで抑制
- G-47 ライブ変換は全文再変換に収束・LLM 遅延完了の取り込み・cursor は surface 基準に補正
- G-48 `nn` は保留なしで「ん」を確定（Mozc 準拠。G-8 の soft pending を廃止）
- G-49 Backspace は直前キー入力の取り消し（不一致で確定された文字を保留に戻す）

> G-1〜G-10 は、ユーザーが「暫定案のままで良い」と承認済み（2026-09-27）。
> G-11 以降（G-35〜G-49 を含む）は、ユーザーの承認記録がなく要レビューです。

## Phase 3 以降の要決定事項（UX の詳細が設計書だけでは一意に決まらない項目）

2026-09-27、ユーザーが回答した内容に基づき着手中・着手予定。**Mozkey**（`koyasi777/mozkey`。Mozc + ローカル Zenz 補正の fork。ユーザーが実際に使用しており「使いやすい」と評価）の実装を UX の参考にする方針（ユーザー指示）。

- ~~**予測変換**~~ → 実装済み（G-23。Tab のみ（Down は Mozc でも予測選択に使われないため対象外）。`[predict]` で設定可能）
- **iOS 設定項目（ios-settings.md 段階4）の一部**: `userdict.enabled` / `snippet.enabled`（ユーザー辞書の語・スニペットの使用の有無。登録内容は保持）と `input.dakuten_cycle`（タッチ入力の「゛゜小」キーの循環順。`small_first` が従来の動作）を実装済み。`predict.next_word` / `predict.next_word_count`（確定直後の次語予測。LLM を使わない軽量版: 学習した「直前の語 → 次の語」のバイグラムを `learning-next.tsv` に記録し、確定直後（Direct）の候補として返す。H-19）を実装済み。LLM を使う版は未実装
- **ローマ字の引き継ぎ（曖昧入力。H-23）**: Backspace でかなを消した後、直前に残った未変換のローマ字（`kypu` の `ky` など、有効な接頭辞）を保留に戻して続けて入力できる（`kypu`→BS→`ou`→「きょう」）。設定 `input.romaji_carryover`（既定 true）
- ~~**ライブ変換**~~ → 実装済み（G-25・G-26。既定 ON、`live_conversion_delay_ms`＝228ms・`live_conversion_min_key_length`＝2文字は Mozkey の実測既定値。デバウンス完了時に `update_preedit` で配信）。**未対応**: ふりがな（reading）のルビ風オーバーレイ表示はプロトコル上は可能（`SegmentView.reading` が既に載っている）だが、フロント（macOS/Windows）側の描画実装がまだ無い。Enter は表示中のライブ変換結果を確定する（G-39）。Backspace の細かい挙動（ライブ変換結果からの部分確定・取消）は既存の Composing 用ロジックのまま（Mozkey との詳細比較は未実施）
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
9. ~~**アップデート署名検証の鍵**（G-31）~~ → 本番用の鍵に差し替え済み（2026-10-22。公開鍵は `config/update-public-key.txt`、秘密鍵は Actions Secrets `NAGI_UPDATE_SIGNING_KEY`）

## macOS 実機へのインストール手順（手動）

bundle id は `com.shiki.inputmethod.Nagi`（G-37）。CI の artifact `Nagi.app`（`Nagi.app.zip`）または `cargo xtask bundle-macos` の出力を使う。

1. artifact を展開して `Nagi.app` を得る
2. `~/Library/Input Methods/` に `Nagi.app` を置く
3. `xattr -cr ~/Library/Input\ Methods/Nagi.app`（quarantine 属性の除去）
4. `~/Library/Input\ Methods/Nagi.app/Contents/MacOS/NagiIMEApp --register`
5. `killall TextInputMenuAgent`
6. システム設定 > キーボード > 入力ソース で Nagi（日本語）を追加する。表示されない場合は再ログインする

### macOS のリリース手順（Developer ID 署名・公証、H-37）

1. 初回のみ: `cd core && cargo xtask gen-update-key` で鍵ペアを作る。秘密鍵を Actions Secrets `NAGI_UPDATE_SIGNING_KEY` に登録し、公開鍵で `config/update-public-key.txt` を置き換えてコミットする（2026-10-22 に実施済み）
2. 初回のみ: Secrets に `MACOS_DEVELOPER_ID_P12`（「Developer ID Application」証明書と秘密鍵の .p12 を base64 にしたもの）と `MACOS_DEVELOPER_ID_P12_PASSWORD` を登録する。公証には iOS と共通の `ASC_KEY_ID` / `ASC_ISSUER_ID` / `ASC_KEY_P8` を使う
3. `core/Cargo.toml` の `[workspace.package] version` を上げて main にコミットし、`gh workflow run macos-release.yml --ref main -f version=X.Y.Z` を実行する。リリース `macos-vX.Y.Z`（zip と `latest.json`）が作られ、web ワークフローがサイトの `/updates/macos/latest.json` を更新する
4. 利用者は `curl -fsSL https://nagi.shiki-01.com/install.sh | sh` で入れ、以降は入力メニューの「アップデートを確認…」で更新する（起動時の自動確認はコアが通知する）

### 設定ウィンドウの開き方

メニューバーの入力メニュー（Nagi を選択中に出る入力ソースのアイコン）→「設定…」で「Nagi 設定」ウィンドウが開く。項目はコアが返す記述子（`get_settings`、G-41）から描画され、変更は即時に `config.toml` へ保存・反映される。開発時は IMKServer を起動せず `NagiIMEApp --settings --engine <imserver> --user-dir <dir>` でウィンドウだけ開ける。

### 辞書・モデルと LLM 版の配置（実データで動かす場合）

上記の `Nagi.app` は辞書・モデルを含まず、LLM も無効（読みがそのまま返る）。実データで動かすには以下を行う。CI（`macos-frontend.yml`）の artifact は repo メンバー限定で、公開配布ではない。

1. artifact `nagi-data`（`nagi-data.zip`、保持 7 日）を展開し、中身（`dict/` と `models/`）を `~/Library/Application Support/com.shiki.nagi/data/` に置く（`data/dict/nagi-dict.bin`、`data/models/zenz-v3.1-small-Q5_K_M.gguf` になる）
2. 辞書変換だけなら 1 で足りる。LLM 変換を使うには artifact `Nagi-llama.app`（`Nagi-llama.app.zip`）を展開し、`~/Library/Input Methods/` の `Nagi.app` と差し替える（展開物の名前は `Nagi.app`。手順 3〜5 の `xattr -cr` と `--register` をやり直し、`killall TextInputMenuAgent`。すでにプロセスが動いている場合は `killall NagiIMEApp` 相当で再起動する）
3. 確認: 「きょうはいいてんきですね」等を入力して Space。漢字混じりの候補が出れば辞書が効いている。LLM が効いているかは、辞書のみより文脈に沿った 1 件目になるか（例: 同音語の選び分け）で判断する。読みがそのまま返る場合は data-dir のパスとファイル名を確認する

## Windows フロント（Phase 5、2026-10-22、決定は decisions.md H-39）

| 項目 | 状態 |
| --- | --- |
| `frontend-windows/`（`nagi-windows`、unsafe 禁止・OS 非依存） | 実装済み・Linux でテスト済み（59 件）: キー正規化（VK_IME_ON/OFF・VK_OEM_BACKTAB を含む）、JSON-RPC クライアント、子プロセス通信路、EngineSupervisor（指数バックオフ再起動・`restore_session`）、revision ゲート、SessionStore（セッションごとの preedit 保持。`restore_params` を `set_restore_provider` に渡す） |
| `frontend-windows-tsf/`（`nagi-windows-tsf`、cdylib、unsafe はここに隔離） | **骨組みのみ**。`DllMain` / `DllGetClassObject` / `DllCanUnloadNow` / `DllRegisterServer` / `DllUnregisterServer`、`ITfTextInputProcessorEx` / `ITfKeyEventSink` / `ITfCompositionSink` / `ITfEditSession` の最小実装、キー → エンジン → 確定・preedit 反映。OS 非依存の `render`（応答 → 編集計画）と `engine`（接続のまとめ）は Linux でテスト済み（15 件） |
| 検証 | Linux で `cargo check` / `cargo clippy -D warnings --target x86_64-pc-windows-gnu` が通る（型・シグネチャのみ）。**Windows 実機では未確認**（登録・キー入力・composition・エンジン起動） |
| 未実装 | 候補ウィンドウ、`ITfFnReconversion` と `reconvert_selection`、表示属性（下線）、入力モードの言語バー、非同期通知の即時反映（現状は次のキー入力時）、`set_environment` / `set_context` / 通知表示、アイコン、複数ホストプロセスでのエンジン共有、インストーラ・署名・SmartScreen 対応、CI のビルド・配布 |

## 既知の問題

- ~~CI の `types` ジョブは `typeshare-cli` を毎回 `cargo install` する~~ → `actions/cache` でバイナリをキャッシュするよう修正（`~/.cargo/bin/typeshare`。キー `typeshare-cli-<os>-v1`。バージョンを上げたい場合はキーの `v1` を変える）
- 途中のコミットには、後続コミットの変更に依存してビルドできないものがある（最終状態は通る）
- 文節区切りは暫定ヒューリスティック（サ変名詞 + する、形容詞 + 名詞など）。誤変換の例: 感じ変換の\|テスト、機能病院に（LLM で改善する領域）
- `Preedit` の Composing 表示では、非 ASCII 文字（絵文字など）も japanese 種別の 1 セグメントにまとまる
- `nn` は G-48 で Mozc 準拠（保留なしで「ん」）に変更済み。Mozc 実機との照合は未実施（`konnichiha` → こんにちは、`kanna` → かんな は通る。`nnn` は「んん」）
- ゴールデンテストの期待値は、テスト担当のサブエージェントが仕様から書いた後、実装の挙動と食い違う箇所を「仕様に照らして」修正したもの。仕様の根拠は `docs/decisions.md` G-7。追加のレビュー（標準 IME との挙動照合）は未実施
- macOS / Windows 実機でのフロント接続、標準 IME との挙動照合、レイテンシベンチ（p95）は未実施
- `infer` の `LlamaModel` はコンテキストを 1 つ使い回す（KV キャッシュ再利用・実LLMビームサーチとも server/engine まで接続済み。G-32、G-33、G-34）が、GPU オフロードは未対応

## CI

（最終確認の結果を下に追記する）

## iOS 向けコアの追加対応（2026-10-14）

- `nagi-server` に feature `update-check`（既定 ON）を追加。`ureq` と更新確認・配布物取得の HTTP を裏に置き、`nagi-ffi` は `default-features = false` で依存する（iOS の XCFramework に TLS 実装を入れない。`docs/ios-release.md` §0.2）。無効時は `update.auto_check` が no-op。検証ロジック（ed25519・SHA-256）は feature 外
- `cargo xtask gen-licenses`: nagi-ffi の依存閉包のライセンス一覧を `THIRD_PARTY_LICENSES.md` に生成（`docs/ios-release.md` §0.5）

## iOS の未完了項目（2026-10-14、2026-10-16 更新）

実機未確認だった項目（キーボード UI の細部、入力中の候補・部分変換、`set_context`、レイアウト（JSON・toggle・longPress・shortcut）、設定タブ、ホストの案内・データ・About、堅牢性（`set_environment`・メモリ警告）、コア API への追従）は 2026-10-22 に実機で確認し、問題なしだった。`docs/ios-device-checklist.md` §2 に移した。

残り（詳細は `docs/mobile-roadmap.md` §0 と `docs/ios-device-checklist.md` §0）:

1. ~~本体アプリ: ユーザー辞書の登録・編集 UI（`userdict.entries`）~~ → 実装済み（`App/UserDictionaryView.swift`。実機確認は `docs/ios-device-checklist.md` §2）
2. 配布の準備: 正式なアイコン、プライバシーポリシー、外部テスト（Beta App Review）、フルアクセスの用途の明記
3. 堅牢性: 複数プロセスでの学習データの同時アクセス（D-5。コア側の課題を含む）、他機種での上限・応答時間の実測
4. 設定のコア側新規項目（`docs/ios-settings.md` §4 段階4）
5. 最後にまとめて: 見た目と操作感の調整（実機のフィードバックを集める）
6. ~~要判断（診断ログの既定・iOS の既定モデル）~~ → 決定済み（H-22。診断ログは TestFlight で既定 ON・App Store 版で既定 OFF、既定モデルは small。`RequestsOpenAccess` は H-18 で true。`docs/ios-device-checklist.md` §0）

## キーアイコンの Iconify 対応（2026-10-22、決定は decisions.md H-36）

- 実装済み（実機・CI 未確認）: `icon` の `sf:` / `iconify:` 書式（スキーマ + `LayoutLoader.decode`）、ホストアプリの取り込み時の解決（`App/IconifyResolver.swift`。Iconify API → WKWebView で PNG 化 → App Group `keyboard/assets/icons/`）、拡張の描画（`KeyIconView`）、ライセンス一覧（レイアウトタブ・About）
- サイトエディタ（`web/`）の `iconify:` 対応は完了（`validate.ts` の書式検査、アイコン入力欄、ブラウザ直取得のプレビュー、書き出し JSON は ID のみ）。
- 未完了: Iconify API の実応答の確認（開発環境から接続できず未確認）、実機での取り込み・描画の確認

## iOS 実機確認待ちの一覧（2026-10-21）

実装済みで、実機（TestFlight）での確認が済んでいないもの。確認手順は各節と `docs/ios-device-checklist.md` を参照。

| 項目 | 節 | 確認方法 |
| --- | --- | --- |
| 曖昧入力 M1・M2: 誤補正（固有名詞・俗語の文）、矩形の座標系、タッチ位置、候補バーの「もしかして」の見え方、σ_noise の実測 | H-24・H-25 | 入力テスト、診断ログ。CI での Swift ビルド・テストは 2026-10-21 に成功済み |
| 絵文字・顔文字候補の見え方と設定 UI の追従 | H-21 | 読み（例: えがお）で候補を確認 |
| メモリ上限の閾値（150/1024/2048MB）と LLM 再読み込みの待ち時間（30 秒）の見直し | H-16・H-26 | 診断ログ `unload_model (keyboard hidden)` `reload_model ok=` `llm restored` |
| フルアクセスが要る機能（触覚など）の可否 | H-18 | `docs/ios-device-checklist.md` |
| 他機種での拡張のメモリ上限・応答時間の実測、small の応答時間の実機再確認 | H-17 | 別機種で計測 |
| 複数プロセスでの学習データの同時アクセス（D-5） | mobile-roadmap D-5 | 本体アプリとキーボードで同時に学習・削除 |

確認済み: メール・URL 欄の英字切替、検索欄の改行キー、注目文節まで確定、`insert` 確定（H-31・H-34）、数字欄（`.numberPad` ほか。H-35）。欄ごとの標準キーボードの自動選択は iOS の挙動で対応不可（H-33・H-35）。
2026-10-22 の実機確認で問題なし: クリック音（互換モード）・触覚の設定値と実際の動作の一致（H-30）、iOS の `capabilities` / `get_key_table` / `set_environment` / メモリ警告の連携（「モバイル向けコア API」）。詳細は `docs/ios-device-checklist.md` §2。

## モバイル向けコア API（2026-10-14、決定は decisions.md H-16）

- 実装済み: LLM だけの解放/再読み込み（`unload_model` / `reload_model`、メモリ逼迫 critical・メモリ上限不足での自動解放、normal から 30 秒後の自動再読み込み）、`get_key_table`、D-4（`initialize.capabilities`・`set_environment.memory_budget_mb`・メモリ上限に従うプロファイル判定）
- iOS（Swift）側の追従は済み（起動時の `capabilities`、`get_key_table`、`set_environment`・メモリ警告による `unload_model` 連携。2026-10-22 に実機で問題なし。`KanaKeyTable.swift` の複製表はフォールバックとして残る）
- 未完了: 実機でのメモリ上限の閾値（150/1024/2048MB）と再読み込みの待ち時間（30 秒）の見直し

## 絵文字・顔文字の読みからの候補（2026-10-14、決定は decisions.md H-21）

- 実装済み: Rewriter `EmojiRewriter`（`core/rewriter/src/emoji.rs`）、表 `config/emoji.tsv`（`include_str!` 同梱。絵文字 413 読み・顔文字 101 読み）、設定 `rewriter.emoji` / `rewriter.kaomoji`（既定 true・detail・both）
- 未完了: iOS 側の設定 UI 追従（`IOSSettings.swift`）、実機での見え方の確認

## 曖昧入力 M1（2026-10-15、決定は decisions.md H-24）

- 実装済み: 新クレート `nagi-fuzzy`（QWERTY 隣接表、置換・脱落・挿入・転置・濁点/小書き・同じ行内の隣、辞書の trie による枝刈り付き探索）、`CandidateSource::Fuzzy`、`Pipeline` / エンジンへの統合（入力中の候補・Space 変換の「もしかして」候補）、設定 `input.fuzzy.{enabled,strength,max_candidates,kinds}`（既定オフ）、評価 `nagi-fuzzy-eval`（正解率 約 40%、クリーン入力の誤訂正 0%、展開 p50 1.6ms）
- 未完了: Swift 側（iOS の設定 UI は汎用フォームに自動で出る想定。候補の「もしかして」ラベルの追従）、実機での確認、M2（タッチ座標・実際の打鍵列）、M3（LLM 再採点）、補正後の読みを候補に持たせる表示

## 曖昧入力 M2（2026-10-15、決定は decisions.md H-25）

- 実装済み: Composer が実際の打鍵列（`Stroke`）を保持（BS・G-49 巻き戻し・引き継ぎ・カーソル移動・部分確定に追従）、`set_layout_geometry`（キー矩形から隣接を導出）、`insert_reading` の任意の `touch` / `flick`（`InsertReadingTouchParams`）、座標つきのガウス尤度の置換コスト、`CandidateView.reading`（補正後の読み。選ぶとその読みで確定・学習）、設定 `input.fuzzy.use_touch_coords`（既定 true・detail・ios）、評価 `nagi-fuzzy-eval` に座標ノイズモデル（M1 比で上位 5 件の正解率 +9〜+23pt、クリーン入力の誤訂正 0〜0.13%）
- Swift 側は実装済み（コミット d08893f。geometry・touch / flick の送信、「もしかして: ○○」表示、テスト追従、生成 config で `[input.fuzzy] enabled = true`）
- 未完了: CI での Swift ビルド・テストの確認、実機での確認（矩形の座標系、タッチ位置、候補バーの見え方）、M3（LLM 再採点）
