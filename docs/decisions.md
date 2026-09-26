# Nagi 決定事項ログ

設計書 `ime-design-doc.md`（v0.1）の未決事項に対する決定を記録する。
本ファイルと設計書が食い違う場合は本ファイルを優先し、設計書は次版（v0.2）で反映する。

- 記録日: 2026-09-27
- 「Phase 0 で確定」とある項目は、比較・検証の結果をもって本ファイルに追記する

---

## A. IR・状態機械・プロトコル

### A-1. reading と surface の対応（アライメント）

- 用語: `reading` = ひらがなの読み（例: `とうきょうにいく`）、`surface` = 表示・確定される表記（例: `東京に行く`）
- Segment の粒度は **文節相当** とする。1 Segment = 1 文節（`東京に` / `行く`）
- LLM 出力（surface のみ）と reading の対応は、コア側の **強制アライメント** で求める
  - surface 中のひらがな（送り仮名・助詞）を reading 上の固定アンカーとして一致させる
  - アンカー間の漢字列には、システム辞書の読みを使った DP で reading の部分列を割り当てる
  - 割り当てられない区間は 1 Segment にまとめる（それ以上分割しない）
- 位置の内部表現は reading 基準（`segment index + reading 内 offset`）とする。プロトコルでフロントへ送るのは surface 上の表示位置（A-4 の単位）
- 結果だけを返す変換エンジンやモデルも、候補の供給元として利用してよい。アライメントはコア側で行うため、供給元が対応情報を出さなくても IR は成立する（ライセンス条件は B-1 と同じ基準で判断する）
- Jev / Laya の調査結果（2026-09-27）:
  - Jev は TypeSafe が提供する有料の「判断」API。状態テキストと選択肢付きの質問を渡すと、確率付きで答えを返す。Jev IME（Windows 用）はこれで文節候補を文脈順に並べ替えている
  - Laya（`convaiinnovations/laya`）は Jev 互換のローカル実装で、Apache-2.0。中身は ModernBERT / mmBERT 系の非生成モデル（英語版 421M、多言語版 322M）。3 つの checkpoint で合計約 2.2GB、重みは safetensors、Apple Silicon で 1 回あたり約 20ms
  - 判断: Laya はかな漢字変換（生成）そのものには使えない。一方、候補の再ランク（reranker）と英日判定には適合し得る。日本語での精度は未確認で、GGUF 以外のランタイムも必要になるため、コアの経路には入れない。Phase 0 で評価し、効果があれば `quality` プロファイル限定の任意機能にする（設計書 §4.8）

### A-2. 変換単位（文全体 + 文節）

両方を実装し、**Phase 1 に含める**。

- Space で変換を開始すると、文全体の最良候補を文節に区切って表示する
- 候補ウィンドウの初期表示は `conversion.initial_view` で選ぶ
  - `sentence`（既定）: 文全体の N-best を表示する
  - `segment`: 先頭文節の候補を表示する（MS-IME / Mozc と同じ挙動）
- `←/→`: 注目する文節を移動する。候補ウィンドウはその文節の候補に切り替わる
- `Shift+←/→`: 注目文節の区切りを reading 1 文字単位で伸縮する。伸縮後はその文節と後続の文節を再変換する
- `Ctrl+Shift+←/→`: 複数文節にまたがる選択を作る（`RangeSelecting`）。範囲の再変換やスタイル変換の対象になる
- 設計書 §4.9 の「`Shift+←/→` = 範囲選択」は上記に置き換える。いずれのキーもキーマップで変更できる
- 状態の整理: `Converting` では注目文節が暗黙の selection になる。`RangeSelecting` は「2 文節以上にまたがる selection」がある状態と定義する

### A-3. プロトコルの同期・非同期

- `process_key` は同期リクエストとし、応答で差分をまとめて返す
  `{consumed, commit?, preedit?, candidates?, mode?, revision}`
- 非同期通知（C→F）は、ライブ変換の推論結果とスタイル変換の結果に限る。通知には `revision` を付け、フロントは現在の revision より古い通知を捨てる
- Space 変換の同期待ちは最大 `sync_budget_ms`（既定 150）とする。間に合わない場合はひらがなのまま応答し、結果を非同期通知で送る
- タイムアウト: 応答がない状態が 500ms 続いたら、フロントはエンジン異常とみなす。そのキーは Direct 状態なら素通しし、それ以外なら消費する。エンジンを再起動し、preedit を `restore_session` で復元する（F で改訂）
- 追加するメソッド: `initialize`（プロトコルバージョンと capability の交換）/ `shutdown` / `ping` / `focus_in` / `focus_out` / `reset`
- フォーカス喪失時は、表示中の surface を確定する（macOS 標準 IME と同じ挙動）
- セッション: エンジンはフロント 1 プロセスにつき 1 つとする。クライアント（IMKInputController）ごとに `session_id` を振り、状態をセッション単位で持つ
- キーイベント: 送るのは押下と修飾キー変化（`flags_changed`）のみとする。離上は送らない。右⌘の単独タップは `flags_changed` 列からコア側で判定する
- エラー: JSON-RPC 標準コードに加えて、アプリ固有コードを `-32000` 台で定義する

### A-4. 位置の単位

- プロトコル上の offset はすべて **UTF-16 コード単位** とする。コア内部の表現との変換はコアが行う
- 絵文字・結合文字・サロゲートペアをゴールデンテストに含める

### A-5. キー表現

- `{keysym, keycode, text, modifiers, kind: "down" | "flags_changed"}`
  - `keysym`: XKB keysym の数値
  - `text`: OS の配列で生成された文字（Dvorak 等への対応用）
- JIS の英数キー・かなキーには XKB の `Eisu_toggle` / `Hiragana_Katakana` 相当を割り当てる

### A-6. Swift 側の型生成

- `typeshare` を使い、Rust の型から Swift の型を生成する
- 生成物はコミットし、CI で再生成して差分がないことを検査する
- `typeshare` で表現しにくい型が出た場合は、schemars で JSON Schema を出し、そこから生成する方式に切り替える

### A-7. 状態遷移の個別挙動

| 状態 | 入力 | 挙動 |
| --- | --- | --- |
| Composing | Enter | 表示中の surface を確定する（ライブ変換 OFF なら reading） |
| Composing | ESC | preedit を全破棄する（`composing.esc_behavior` で変更可） |
| Converting | ESC / Backspace | Composing に戻る（変換を取り消す） |
| Converting | 文字キー | 現在の変換結果を確定し、新しい入力を始める |
| 任意 | Shift+英字 | 大文字を Literal セグメントとして挿入する。変換・確定・ESC まで、後続の小文字も英字のまま続ける（`Nagi` と入力できる）。`input.shift_letter = "literal_continue" \| "single_upper"` で変更可 |
| Composing / Converting | Ctrl+K | 対象をカタカナにする単発操作（トグルではない） |
| 任意 | 英数キー / かなキー | 入力モードを切り替える |

完全な遷移表はゴールデンテスト YAML と一緒に Phase 1 冒頭で作成する。

### A-8. Literal と数字

- Literal は **surface を変更しない** ことだけを意味する。推論の入力（文脈）には含める
- 数字 + 助数詞（`3じ` → `3時`）が変換できるように、数字の直後ではチャンクを分割しない。チャンクを強制分割するのは記号の連続と Fixed の境界に限る

## B. モデル・推論・辞書

### B-1. 変換モデル — Phase 0 で確定

- 選定基準（優先順）
  1. 無料で使え、再配布の制約が小さいこと。表示義務はあってよい
  2. 変換品質
  3. 速度とサイズ
- `infer` にモデルアダプタ層を置き、モデルごとのプロンプト形式（区切りトークン、文脈欄、プロファイル欄）の違いを吸収する

### B-2. スタイル変換モデル

- 変換モデルとは別の指示追従モデルを使い、**任意ダウンロード** とする
- 使うときにロードし、一定時間使わなければ解放する（`style.unload_after_sec`）
- アイドルメモリ目標（< 500MB）は変換モデルだけを対象にする
- IR 上の扱い: スタイル変換の結果は reading を持たない `Fixed` 相当のセグメントとする。元の Segment 列を Undo 用に保持する

### B-3. プロファイル注入

- モデルアダプタが対応を宣言した入力欄（プロファイル欄など）にだけ注入する。非対応モデルでは無視し、起動時にログで警告する

### B-4. N-best

- 用語: N-best = 最良の 1 案だけでなく、上位 N 案を出すこと
- 文全体の候補: ビームサーチ（生成途中で上位 k 本の途中結果を残しながら進める方式）で出す。幅は `conversion.beam_width`（既定 3）
- 文節の候補: 辞書でその読みの候補を列挙し、上位の数件を LLM で文脈込みに再採点する

### B-5. ランキング

- 既定の優先順は設計書 §4.3 のとおり
- 設定で出所の順序と重みを上書きできるようにする（`[ranking] source_order` / `[ranking.weights]`）
- 出所の異なる候補は、同じ尺度に正規化したスコアで並べる。重複は表記が同じなら 1 つにまとめ、出所アノテーションは複数を保持する

### B-6. llama.cpp バインディング

- 広く使われている既存の Rust バインディング crate を使い、バージョンを固定する
- `infer` の trait の裏に隠し、crate の差し替えや自前 bindgen への移行ができるようにしておく

### B-7. システム辞書 — Phase 0 で確定

- 候補: Mozc OSS 辞書 / SudachiDict / その他。B-1 と同じ基準（無料・再配布の制約が小さい）で比較する
- ユーザー辞書の「自前バイナリ形式の自動判定」は v1 から外す（Mozc TSV のみ）

### B-8. 性能の計測条件・対応環境

- 基準機: Apple Silicon M1 / メモリ 8GB。量子化レベルは Phase 0 で確定する
- 対応 OS: macOS 13 以降（Apple Silicon。Intel Mac は動けばよい扱い）/ Windows 10 22H2 以降・11（x64）

## C. Phase 2 以降の仕様

### 予測変換
- 一般的なスマートフォン・PC の IME と同じ挙動にする。読みを `predict.min_chars`（既定 1）文字以上入力したら候補を出し、Tab / ↓ で選ぶ
- 供給元は学習キャッシュとユーザー辞書の前方一致。LLM による続きの生成は後で追加する

### 学習ファイル
- TSV。1 行目にスキーマバージョン。列は `reading, surface, context_hash, count, last_used(unix秒), source`
- タブと改行はバックスラッシュでエスケープする
- flush は IME 切替とフォーカス喪失のときに加え、5 分ごとにも行う。一時ファイルに書いて rename する方式で書き込みを不可分にし、ファイルロックをかける
- スコア: `ln(1+count)` に、最終使用からの経過時間による指数減衰（半減期 14 日）を掛け合わせる。パラメータは設定で変更できる

### 学習のキー
- マッチは reading の完全一致・前方一致で行う。`context_hash` はフィルタではなく、一致したときにスコアを加点するだけに使う

### ライブ変換
- 編集されたチャンクだけを再推論する。直前のチャンクの surface を、最大 `max_context_length` 文字まで左文脈として渡す
- デバウンスは 30ms。キーリピート中は推論しない

### `max_context_length`
- 単位は文字（Unicode スカラー値）

### 再変換
- 一般的な IME と同じ挙動にする。読みは辞書の逆引きで推定し、推定できない区間は LLM で補う

### キーマップ TOML
- キー名は `Ctrl+Shift+Right` 形式で、修飾キーの順序は問わない。Action は `Name` または `Name(arg)` と書く
- `[keymap.common]` を全モードが継承する。プリセットより個別定義を優先する
- 不正な行はその行だけ無視し、フロントに通知する

### 設定のホットリロード
- 読み込みに失敗したら旧設定を維持し、フロントに通知して表示させる

### アノテーション
- プロトコルでは enum（`learned` / `user_dict` / `llm` / `system_dict` / `rewriter` …）で送る。アイコンはフロントで割り当てる

## D. macOS フロントエンド

- 入力モード: 状態の正はコアの内部モードとする。表示は OS の入力モード（ひらがな / 英数）へ同期し、メニューバーに「あ / A」を出す（Mozc と同じ方式）
- 候補ウィンドウ: 独自の NSPanel で実装する
- preedit の属性がアプリごとにどこまで反映されるかは、Phase 0 で実機確認する（下線スタイル、選択範囲の強調）
- 配布: Apple Developer Program には加入しない
  - アプリには ad-hoc 署名だけを付ける
  - 主な導入手段はインストールスクリプト（`curl -fsSL https://shiki-01.com/nagi/install.sh | sh`）。`~/Library/Input Methods` に配置する。curl で取得したファイルには quarantine 属性が付かないため、Gatekeeper の警告は出ない
  - 手動用に zip と手順書も配布する
- アップデート: 起動時と、メニューから任意のタイミングで確認する
  - `https://shiki-01.com/nagi/latest.json` を取得する
  - 配布物は ed25519 署名で検証する（公開鍵はアプリに埋め込む）
  - 検証後に差し替え、IME プロセスを再起動する
  - 自動確認は設定でオフにできる
- エンジン停止中: キーを素通しし、停止したことを通知する。preedit はフロントが保持しておき、再起動後に復元する（F で改訂）

## E. 開発基盤

- 開発環境: macOS と Windows のどちらでもコアを開発・テストできるようにする。補助スクリプトは OS 依存のシェルを避け、`cargo xtask` で書く。フロントのビルドはそれぞれの OS 上でのみ行う
- Rust: edition 2024。`rust-toolchain.toml` で stable の版を固定する
- ワークスペース: Phase 1 は `engine` / `protocol` / `server` / `infer` / `romaji` の 5 crate から始め、必要になったら分割する
- CI: GitHub Actions。ubuntu（コアのテスト）/ macos（コアのテスト + Swift のビルド）/ windows（コアのテスト）。clippy と rustfmt は必須、SwiftLint を併用する
- 大きいファイル: モデルと辞書はリポジトリに入れない。取得スクリプトと SHA-256 検証で扱う
- 配布元・ダウンロード元: `shiki-01.com` を使う
- ベンチのコーパス: 公開ベンチ（ライセンスが許すもの）と、自前の評価セットを併用する。自前の評価セットは、ドッグフーディング中に集めた誤変換を手作業で読み・正解付きに整えて蓄積する。収集はローカル限定・opt-in とする
- ライセンス: コードは `MIT OR Apache-2.0` のデュアルライセンス（Rust 界隈の慣例）
  - モデルと辞書は別途ダウンロードとし、それぞれのライセンスに従う。同梱しないため、コードのライセンスに影響しない
  - 表示義務は `NOTICE` とアプリの「このアプリについて」画面で満たす
  - ローマ字テーブルは自作し、コードと同じライセンスにする
- ローマ字テーブル: Mozc 互換の TSV（`入力\t出力\t保留`）。v1 は標準テーブルのみ。AZIK は同じ形式の追加テーブルとして後から同梱する

## F. 基本方針（2026-09-27 追加）

- 品質優先: 開発コストがかかっても良いものを作る。環境に合わせた精度・軽さの自動調整と、全体の気配りを重視する
- 既存資産の使い方: コード（ロジック）は自作する。データ資産（辞書・連接コスト・モデル）は既存のものを使う。アルゴリズムは既存実装を読んで参考にする
  - 状態機械・IR・プロトコルは差別化点そのもので、既存実装の IR はセグメント構造を前提にしていない。改造しても作り直しと同じになる
  - 辞書の語彙・コストと変換モデルは、個人で同じ品質を再現できない
  - Mozc（C++・Bazel）や azooKey の変換器（Swift）を丸ごと組み込むことはしない。Rust コアと Windows 展開に合わないため
  - MIT / Apache / BSD のコードを部分的に移植する場合は、出典を明記する
- 環境プロファイル（`quality` / `balanced` / `light` / `minimal`）: 初回起動時に自動判定し、実行中もバッテリー・温度・レイテンシに応じて自動で切り替える（設計書 §4.7）
- `minimal` 経路（辞書 + Viterbi のみで動く経路）は Phase 1 に前倒しする。モデルがなくても、障害時でも入力を続けられるようにするため
- エンジン再起動時は `restore_session` で preedit を復元する（入力を失わない）。D の「preedit は破棄する」を置き換える

## 設計書への反映

上記の A〜F は、設計書 v0.2（2026-09-27）に反映済み。

---

## G. Sonnet 暫定判断（要レビュー）

2026-09-27、ユーザー不在の自律作業中に行った暫定判断。各項目に判断・理由・代替案を記す。Opus によるレビュー対象。

### G-1. Rust ツールチェーンは 1.89.0 に固定

- 判断: `core/rust-toolchain.toml` を `channel = "1.89.0"`、`profile = "minimal"`（rustfmt / clippy のみ追加）とした
- 理由: 作業機に導入済みの stable が 1.89.0 だったため。「stable の版を固定する」（E 節）に沿う
- 代替案: 最新 stable に更新して固定する。上げる場合は `rust-toolchain.toml` と `workspace.package.rust-version` / `clippy.toml` の `msrv` を同時に更新する

### G-2. ワークスペースの lint 方針

- 判断: `unsafe_code = "forbid"` をワークスペース全体に課す（`infer` の FFI 境界のみ、必要になった時点で個別に緩和する）。`missing_docs = "warn"`（公開 API は rustdoc 必須の品質基準に対応）。clippy は `-D warnings`
- 理由: 「unsafe は infer の FFI 境界に限定する」に対応

### G-3. llama.cpp バインディングは `llama-cpp-2 =0.1.157`（feature `llama`、既定 OFF）

- 判断: crates.io の `llama-cpp-2`（utilityai）を `=` 固定で optional 依存にした。`infer` の `ConversionModel` trait の裏に隠し、既定ビルド・CI では llama.cpp の C++ ビルドを走らせない
- 理由: DL 数が多く（累計 138 万・直近 70 万）、直近 5 日以内にも更新されている。安全な API で unsafe が不要。同梱の llama.cpp が新しい（2026 年 9 月時点）
- ビルド前提（Windows）: cmake、MSVC、libclang（`LIBCLANG_PATH="C:\Program Files\LLVM\bin"`）。初回の C++ ビルドは約 25 秒〜2 分
- 判明した問題と対処: 同梱の llama.cpp は zenz-v3.1 の `tokenizer.ggml.pre = gpt2-small-japanese-char` を未知の pre-tokenizer として拒否する
  - ロード時のみ KV override で `tokenizer.ggml.pre` を `default` に上書きして通す
  - zenz の語彙は GPT-2 のバイトエスケープ表記の BPE（merges 5,764 件、6,000 トークン）で、私用領域文字（U+EE00〜）は複数のバイトトークンに分かれる。llama.cpp のトークナイザ／デトークナイザを使うと化けるので、`infer::gguf` に最小の GGUF リーダー（語彙・merges）と `CharBpeTokenizer`（1 文字ずつ BPE）を自作し、トークン化・デコードを自前で行う
- 実測（CPU のみ、n_ctx=512、Windows・24 スレッド、コンテキストは毎回作成）: きょうはいいてんきです → 今日はいい天気です（47ms）、なかのです（左文脈「私の名前は」）→ 中野です（33ms）、東京に行く（28ms）ほか 7 文すべて正しい。モデルロード 160ms（mmap）。プロンプト 14〜19 トークン、生成 4〜9 トークン
- 代替案: llama.cpp を更新して pre-tokenizer を追加する（パッチ管理が重い）。GGUF を書き換えて配布する（再配布・SHA-256 検証の扱いが複雑）
- 未実装: ビームサーチ（N-best）、KV キャッシュの再利用、GPU オフロードの検証、`profile` 注入のエンドツーエンド確認（アダプタの組み立てはテスト済み）

### G-4. システム辞書は Mozc OSS 辞書を暫定採用（Phase 0 の比較は未実施）

- 判断: `github.com/google/mozc` のコミット `b9c3fcbd6d76b19649ef572324fa9da2559bc18e` の `src/data/dictionary_oss/` を使用。`cargo xtask fetch-dict` が 13 ファイルを取得し、SHA-256 を `core/xtask/src/fetch_dict.rs` に記録して検証する（初回取得値をそのまま記録した TOFU 方式）
- 内容: IPAdic（mecab-ipadic-2.7.0）ベース + 沖縄辞書 + Google 追加分。語数 1,223,855（重複除去後）、読みの種類 745,964、バイナリ 53.4MB、構築 2.2 秒、読み込み 約 70ms、変換 1 文あたり 40〜200µs
- ライセンス: README.txt によると IPAdic は「著作権表示と免責条項を全コピーに含めること」が条件。**再配布時は `NOTICE` に IPAdic のライセンス全文を入れる必要がある**（未作成。Phase 1 のインストーラ作成時に対応）。沖縄辞書は README では自由に利用可
- `SPELLING_CORRECTION`（6 列目を持つ 62 行）は誤綴り救済用なので通常の候補から除外した
- 連接コストの並びは `rid(前の語) * N + lid(次の語)`。両方向を実データで試し、逆向きでは「教派イイ天気です」のように崩れることを確認した（`ConnectionLayout::RidMajor`）
- `evaluation.tsv`（Mozc の評価データ）は許可された取得対象外と判断して取得していない。精度ベンチには有用なので、ユーザーの許可があれば `fetch-dict` に追加する
- trie は `fst`（有限状態トランスデューサ）を採用した。指示は double-array trie だったが、`yada` は構築に 80 秒かかり（74 万キー）、`daachorse` は読み込みが unsafe だったため。`fst` は構築 1.7 秒・読み込みほぼ即時・safe で、共通接頭辞検索（`Fst::root` から 1 バイトずつ辿る）を自前で実装した
- 精度の目安（Viterbi のみ、言語的に正しいと確認できた文は `dict/tests/real_data.rs` で固定）: 今日は|いい天気です、私の|名前は|中野です、東京に|行く、寿司を|食べたい ほか。誤変換の例: 感じ変換の|テスト（漢字変換）、機能病院に（昨日病院に）。文脈を使う LLM で改善する領域

### G-5. 文節区切りの暫定規則（`dict/src/bunsetsu.rs`）

Mozc の `segmenter.def` が取得許可外のため、`id.def` の品詞だけで区切る暫定ヒューリスティック。
- 助詞・助動詞・接尾・非自立語は直前の文節に付く。接頭詞は直後の語と 1 文節。句点・読点・括弧閉は前に付き、括弧開・その他の記号は新文節
- 名詞が連続する場合（複合名詞・姓+名・数）は 1 文節。ただし非自立名詞（こと・の 等）の後は区切る
- 動詞・形容詞は自立のとき新文節（`せる` `れる` 等の動詞,接尾 は前に付く）
- 既知の弱点: 「勉強|しています」（サ変名詞 + する）、「行ったそうです」の内部、形容詞 + 名詞（実データでは「いい」が連体詞扱いで 1 文節になるが、辞書によっては「いい|天気です」）。Mozc の統計的な境界コストとは一致しない

### G-6. アライメント（設計書 §4.2）の草案 — 未実装（要レビュー）

今回は実装していない（Phase 1 の LLM 統合時に必要。辞書 Viterbi 経路は文節ごとに reading と surface を持つので、`minimal` 経路では不要）。草案:
1. 入力 `(reading, surface)`。surface を「ひらがな連続 / それ以外（漢字・カタカナ・英数）連続」の run に分ける
2. ひらがな run を reading 上のアンカーとして左から右へ一致させる。同じ仮名列が reading に複数回出てくるときは、アンカー間に挟まる非仮名 run が reading を 1 文字以上消費する制約のもとで候補を全列挙し、非仮名 run の読みが辞書に存在するもの（下記 3）を優先、なお同点なら最左を採る
3. アンカー間の非仮名 run に reading の部分文字列 R が割り当たったら、辞書の逆引き（表記 → 読み。現在のバイナリ辞書には無いので追加が必要）で `R` が読みとして存在するか確認する。run が複数漢字の場合は、run の部分列ごとに辞書逆引きを使う DP（Viterbi）で reading を分割する。分割不能なら run 全体を 1 セグメントにする
4. カタカナ run は reading のひらがなをカタカナ化して一致させる（長音 `ー` は reading の `ー` と一致）。英数 run は Literal として扱い reading と 1 対 1 で一致させる
5. 文節区切りは辞書 Viterbi の文節境界（G-5）を初期値とし、アライメントの分割結果と矛盾しないよう境界を合わせる。矛盾したら文節単位（アライメントの粒度を落とす）を優先する
6. テスト: 絵文字・結合文字・サロゲートペアを含む (reading, surface) → セグメント対応の期待値表（設計書 §9）

### G-7. 状態遷移表の草案（状態 × Action）— 要レビュー

実装は `core/engine/src/engine.rs`、テストは `core/engine/tests/golden/*.yaml`。`-` は到達しない、`pass` は素通し（consumed=false）。

| 状態 | Insert | StartConvert/NextCand | PrevCand | Commit | Cancel | Backspace | Move* | ToKatakana/Hiragana | SetMode/Toggle | CommitAndPassThrough | 割当なし |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Direct（かな） | → Composing | pass | pass | pass | pass | pass | pass | pass | モード更新 | - | pass |
| Direct（英数） | pass | pass | pass | pass | pass | pass | pass | pass | モード更新 | - | pass |
| Composing | 挿入 | → Converting（候補窓なし、focus 0） | - | 確定 → Direct | 破棄 → Direct | 削除、空なら Direct | カーソル移動 | → Converting（カタカナ/ひらがなを選択） | 確定 → Direct、モード更新 | 確定 → Direct、consumed=false | 消費のみ |
| Converting | 確定して Composing で新規入力 | 次候補（窓表示）| 前候補 | 確定 → Direct | → Composing（reading 復元） | → Composing | MoveFocus / ResizeSegment / ExtendSelection（NoOp） | 注目文節を変更 | 確定 → Direct、モード更新 | 確定 → Direct、consumed=false | 消費のみ |

- `SelectCand(n)`: 候補窓表示中のみ現在ページの n 番目を選択（確定はしない・窓を閉じる）。非表示なら「確定して数字を新規入力」。Mozc / MS-IME は選択と同時に文節を確定するが、部分確定（Fixed）は Phase 3 なので、今回は選択だけとした
- `RangeSelecting`（Ctrl+Shift+←/→）と `StyleAction` は enum に宣言のみ。Converting での `ExtendSelection` は消費して何もしない
- ライブ変換 OFF なので Composing の Enter は reading を確定する（設計書 §4.3）
- Converting で Left / Right は端で止まる（循環しない）。候補は先頭が現在の表記、次にバックエンド候補、末尾にひらがな・カタカナ（重複除く）
- 入力中に未割当の Ctrl/Alt/Meta 付き文字キー（Cmd+C など）は、確定してからキーを素通しする（入力を失わない）
- 「文全体の N-best」（`conversion.initial_view = sentence`）は未実装。候補窓は注目文節の候補のみ

### G-8. ローマ字・入力バッファの判断

- `nn` は「ん」+ soft pending `n`。直後が母音・`y` なら結合する（`konnichiha`→こんにちは、`shinnyuu`→しんにゅう、`kanna`→かんな）。確定時・後続が他の文字のときは捨てる（`honn`→ほん、`nn`→ん）。soft の `n` は preedit の pending 表示に含めない。Mozc の標準テーブルの挙動との一致は未確認（`nn` を即確定する実装との差に注意）
- テーブルにない ASCII 文字（`q` 単独、`kq` の `k` など）は Literal として preedit に残る。数字・ASCII 記号は Literal（surface 不変）で、数字は英字継続モードを開始しない。`,` `.` `-` `[` `]` `/` `?` `!` `~` `(` `)` は全角記号へ変換し、`input.punctuation` で `,` `.` の表記を切替える
- Composer の内部表現は「1 文字 + 種別」の平坦な列とカーソル位置。保留は常にカーソル位置に付随し、カーソル移動・確定の前に flush する
- `Preedit` に設計書 §4.1 に無い `pending`（未確定ローマ字）を追加した。プロトコルでは `PreeditView.text` にカーソル位置へ挟み込み、`pending` に範囲を持たせる

### G-9. プロトコル・サーバーの判断

- `revision` は typeshare が 64 ビット整数を扱えないため、プロトコル上は `u32`（エンジン内部は `u64`、送出時に切り詰め）。フロントは差の符号でなく「等しい・新しい」で判定する前提（wrapping）
- `Modifiers` の各フィールドは struct 単位の `#[serde(default)]`（Swift 生成型で `Bool` にするため）
- `imserver` は `initialize` の `profile` を常に `minimal`（辞書があってもモデル未統合のため）。`capabilities` は全て false。`reload_config` は常に成功する no-op、`select_candidate` / `reconvert` は `NOT_IMPLEMENTED`（-32003）
- パラメータ不正のエラーメッセージには serde の詳細を含めない（入力値がキー内容を含み得るため。分類のみ）
- 辞書は `--data-dir` または `NAGI_DATA_DIR` の `dict/nagi-dict.bin`。無い・壊れている場合は読みをそのまま返す `KanaBackend` で起動し続ける（stderr に理由を出す）
- 状態機械のゴールデンテスト用 YAML crate は `serde_yaml_ng`（`serde_yaml` は保守終了のため）

### G-10. その他

- `cargo xtask` のダウンロードは `ureq`（HTTP）と `sha2` を使用。取得は一時ファイル → SHA-256 検証 → rename
- CI の `types` ジョブは `typeshare-cli`（ローカルで確認したのは 1.13.4）を毎回 `cargo install` する。実行時間が長いので、必要ならキャッシュ化する。CI 全体は GitHub Actions 上では未検証（ローカルでは同じコマンドを実行済み）
- 途中のコミットには、依存 crate の変更が後続コミットに分かれてビルドが通らないものが含まれる（最終状態では fmt / clippy / test が通る）
