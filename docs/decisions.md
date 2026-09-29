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
- 非同期通知（C→F）は、ライブ変換の推論結果・スタイル変換の結果・Space 変換で予算超過だった LLM 結果の差し替えに限る。通知には `revision` を付け、フロントは現在の revision より古い通知を捨てる
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

> 2026-09-27 追記: ユーザーが「G 節の暫定案はそのまま採用してよい」「`evaluation.tsv` の取得も許可」と承認した。以降 G-1〜G-10 は採用済みとして扱う（変更する場合は新しい項目で上書きする）。

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
- 実測（CPU のみ、n_ctx=512、Windows・24 スレッド、コンテキストは毎回作成。この後 G-13 で使い回すよう変更）: きょうはいいてんきです → 今日はいい天気です（47ms）、なかのです（左文脈「私の名前は」）→ 中野です（33ms）、東京に行く（28ms）ほか 7 文すべて正しい。モデルロード 160ms（mmap）。プロンプト 14〜19 トークン、生成 4〜9 トークン
- 代替案: llama.cpp を更新して pre-tokenizer を追加する（パッチ管理が重い）。GGUF を書き換えて配布する（再配布・SHA-256 検証の扱いが複雑）
- 未実装: ビームサーチ（N-best）、GPU オフロードの検証、`profile` 注入のエンドツーエンド確認（アダプタの組み立てはテスト済み）。KV キャッシュの再利用は実装済み（G-13）

### G-4. システム辞書は Mozc OSS 辞書を暫定採用（Phase 0 の比較は未実施）

- 判断: `github.com/google/mozc` のコミット `b9c3fcbd6d76b19649ef572324fa9da2559bc18e` の `src/data/dictionary_oss/` を使用。`cargo xtask fetch-dict` が 13 ファイルを取得し、SHA-256 を `core/xtask/src/fetch_dict.rs` に記録して検証する（初回取得値をそのまま記録した TOFU 方式）
- 内容: IPAdic（mecab-ipadic-2.7.0）ベース + 沖縄辞書 + Google 追加分。語数 1,223,855（重複除去後）、読みの種類 745,964、バイナリ 53.4MB、構築 2.2 秒、読み込み 約 70ms、変換 1 文あたり 40〜200µs
- ライセンス: README.txt によると IPAdic は「著作権表示と免責条項を全コピーに含めること」が条件。**再配布時は `NOTICE` に IPAdic のライセンス全文を入れる必要がある**（未作成。Phase 1 のインストーラ作成時に対応）。沖縄辞書は README では自由に利用可
- `SPELLING_CORRECTION`（6 列目を持つ 62 行）は誤綴り救済用なので通常の候補から除外した
- 連接コストの並びは `rid(前の語) * N + lid(次の語)`。両方向を実データで試し、逆向きでは「教派イイ天気です」のように崩れることを確認した（`ConnectionLayout::RidMajor`）
- `evaluation.tsv`（Mozc の評価データ、679 行）は当初取得対象外と判断していたが、ユーザーの許可を得て `fetch-dict` に追加した（SHA-256 検証付き）。`nagi-eval` で精度を測る（G-11）
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
- `imserver` は `initialize` の `profile` を常に `minimal`（辞書があってもモデル未統合のため）。`capabilities` は全て false。`reload_config` は常に成功する no-op、`select_candidate` / `reconvert` は `NOT_IMPLEMENTED`（-32003）。**`select_candidate` は G-18 で実装済み。`reconvert` は未実装のまま**
- パラメータ不正のエラーメッセージには serde の詳細を含めない（入力値がキー内容を含み得るため。分類のみ）
- 辞書は `--data-dir` または `NAGI_DATA_DIR` の `dict/nagi-dict.bin`。無い・壊れている場合は読みをそのまま返す `KanaBackend` で起動し続ける（stderr に理由を出す）
- 状態機械のゴールデンテスト用 YAML crate は `serde_yaml_ng`（`serde_yaml` は保守終了のため）

### G-10. その他

- `cargo xtask` のダウンロードは `ureq`（HTTP）と `sha2` を使用。取得は一時ファイル → SHA-256 検証 → rename
- CI の `types` ジョブは `typeshare-cli`（ローカルで確認したのは 1.13.4）を毎回 `cargo install` する。実行時間が長いので、必要ならキャッシュ化する。GitHub Actions 上で全ジョブ成功を確認済み（コミット a3629e4）
- 途中のコミットには、依存 crate の変更が後続コミットに分かれてビルドが通らないものが含まれる（最終状態では fmt / clippy / test が通る）

### G-11. LLM の変換パイプラインへの接続（2026-09-27）

実装: `core/server/src/llm_backend.rs`（`HybridBackend`）、`core/dict/src/align.rs`（強制アライメント）、`core/infer`（`ConversionModel::score`）、`core/server/src/bin/nagi_eval.rs`（精度・レイテンシ計測）。

- **構成**: engine の `ConversionBackend` の実装として辞書 + LLM を server crate に置いた（engine は辞書にもモデルにも依存しない）。縮退は「辞書 + LLM（`balanced`）→ 辞書のみ（`minimal`）→ 読みそのまま」。`--no-llm` で無効化、`--sync-budget-ms`（既定 150）で待ち時間を変更
- **文全体の変換**: 読み全体を zenz で貪欲デコード → 辞書の格子で**強制アライメント**して文節ごとの (読み, 表記) に分ける（G-6 の草案を実装）。アライメントできなければ 1 文節（表記は LLM のまま）。LLM 出力が不正（空・読みの 3 倍+8 文字超・私用領域文字・置換文字）なら辞書の結果
  - 強制アライメント: 表記が与えられた表記の続きに一致する辞書語 / 仮名の素通し（同一文字・ひらがな→カタカナ）/ 未知の連続部分（表記側のひらがなでない連続を、読みの 1〜16 文字に対応づける。高コスト）の 3 種の辺を持つ Viterbi（連接コスト込み）。品詞から文節に区切る規則は G-5 と共通。辞書の逆引き索引は不要だった（読み→表記の順引きで表記との一致を検査する）
- **文節候補**: [LLM の最良変換] + [辞書候補の上位 8 件を LLM で再採点した順] + [残りの辞書候補（コスト順）]。再採点は `score`（プロンプトを 1 回処理し、候補ごとに続きだけデコードして KV キャッシュから外す。終端は EOS 系トークンの確率の和）。同じ表記は出所を併記
- **待たせない（設計書 §3.2 の `sync_budget_ms`）**: 推論は別スレッドで実行し、予算超過時は辞書の結果で即応答。推論は続行して (左文脈, 読み) をキーにキャッシュへ入れ、次回は即座に返る。結果の差し替えは非同期通知で行う（下記「非同期の結果差し替え」）。起動時にダミー入力でウォームアップする
- **長い入力**: 読みが 60 文字を超えるときは、辞書の文節境界で 30 文字前後のチャンクに分け、前のチャンクの結果を左文脈にして順に変換する（ライブ変換の `composing_chunk_len` とは別の暫定）
- **feature**: server の `llama` feature（`nagi-infer/llama`）を有効にしたビルドだけが LLM を使う。既定 OFF（CI では llama.cpp をビルドしない）。配布ビルドでは有効にする
- **LlamaBackend は静的に共有**: llama.cpp の初期化はプロセスで 1 回なので `OnceLock` で共有した
- **計測**（Mozc の `evaluation.tsv` の Conversion 系 609 件。`nagi-eval`。Mozc 自身の OK/FAILED 区分は使わず全件を判定）:

  | 指標 | 辞書のみ（minimal） | 辞書 + LLM（balanced） |
  | --- | --- | --- |
  | Conversion Expected（全文の最良変換が完全一致） | 202 / 302（66.9%） | 238 / 302（78.8%） |
  | Conversion Expected N（読み全体を 1 文節としたときの上位 N 候補に期待値） | 127 / 217（58.5%） | 143 / 217（65.9%） |
  | Conversion Match（最良変換が期待文字列を含む） | 38 / 53（71.7%） | 48 / 53（90.6%） |
  | Conversion Not Match（含まない） | 35 / 37（94.6%） | 36 / 37（97.3%） |
  | **合計** | **402 / 609（66.0%）** | **465 / 609（76.4%）** |
  | `convert_sentence` レイテンシ（release、CPU、24 スレッド） | p50 0.0ms / p95 0.1ms | p50 32ms / p95 56ms / max 81ms |

  Space 1 回の E2E（プロセス起動済み・ローマ字 26 文字入力後）は 46ms。設計書の目標（Space 変換 → 候補表示 p95 < 150ms）を満たす見込み（候補窓は別途 `score` が 40〜50ms 加わる）
- **既知の失敗の傾向**（`nagi-eval --verbose`）: 数詞・助数詞（三千円 / 3000円、十万 / 10万）、固有名詞・当て字、送り仮名の揺れ（見出だす）、表記ゆれ（超え/越え、方/ほう）。数の表記は Rewriter（Phase 4）、その他はモデルの規模・辞書の語彙・学習（Phase 2）の領域。zenz-v3.1-small 単独の限界も含む
- **非同期の結果差し替え**（`update_preedit` / `update_candidates`）: Space 変換で LLM が `sync_budget` を超えたとき、辞書の結果で応答したあと LLM の完了で表示を差し替える
  - 仕組み: engine の `convert_sentence` 呼び出しごとに、セッションが `Origin`（文節範囲・読み・左文脈）を覚える。`Pipeline` は予算超過で待ちをやめた推論が成功して終わったとき、`Completion`（`convert_sentence` に渡された読みと左文脈）を積み、waker で server のループを起こす。`Engine::apply_completions` が一致する `Origin` を `convert_piece` で再変換（キャッシュヒットで即時）し、結果が変わっていれば文節を置き換えて `finish` の差分（revision 加算）を返す。server が `update_preedit` / `update_candidates` に変換して送る
  - 差し替えるのは**未操作のときだけ**: `process_key`・`focus_out`・`reset`・`restore_session` で `Origin` を破棄する。変換中でなくなった・読みや左文脈が一致しない通知は捨てる。`conv.focus` と候補ウィンドウの表示状態は変えない
  - 受け渡しの競合は `Slot`（Waiting / Abandoned / Done）で解決する。待ちが尽きた直後に結果が届いた場合は結果をそのまま使い、通知は出さない
  - server は標準入力を別スレッドで読み（`Server::run_threaded`）、入力行と waker の起床を 1 本のチャンネルで受ける。`run`（同期版）はテスト用に残した
  - 制約: 再変換は engine のスレッド上で行うため、複数チャンクの入力ではチャンクごとに最大 `sync_budget` だけ塞ぐことがある。`segment_candidates`（候補ウィンドウ）の予算超過は対象外（次回は `llm_convert` のキャッシュで即応答）。フロントはキー応答より通知が先に届いても、`revision` の大小で新旧を判定する
- **未実装**: LLM のビームサーチ（文全体の N-best）、`profile`（persona）の注入、環境プロファイルの自動切替。KV キャッシュの再利用は実装済み（G-13）

### G-12. 設定・環境プロファイル・学習・ユーザー辞書（2026-09-27）

実装: `core/config`（`nagi-config`）、`core/profile`（`nagi-profile`）、`core/learn`（`nagi-learn`）、`core/dict/src/{user,overlay}.rs`、`core/server/src/{setup,runtime,pipeline,learning}.rs`。

**設定（`config.toml`）**
- 場所: `NAGI_CONFIG_DIR` > Windows `%APPDATA%\Nagi` / macOS `~/Library/Application Support/com.shiki.nagi` / Linux `$XDG_CONFIG_HOME/nagi`（`~/.config/nagi`）。サーバー引数 `--user-dir`（学習・ユーザー辞書・設定の置き場）、`--config`（設定ファイルのパス）で上書き。優先順位は コマンドライン > 設定ファイル > 既定値
- 全項目は `config/default.toml`（日本語コメント付き。`Config::default()` と一致することをテストで保証）。未指定の項目はプロファイルが決め、明示指定が常に優先（`beam_width` `model` `max_context_length` は `Option`）
- 構文・型・enum の不正値は値の行番号付き、範囲違反は該当キーの行番号付き（`[section]` を追跡して同名キーを取り違えない）、不明キー・不明セクションは警告。問題は全部まとめて返す。壊れた設定では旧設定（起動時は既定値）で動き続け、`notify`（`config_error` / `config_warning`）で行番号付きで知らせる
- 再読み込み: `reload_config` と、リクエスト処理時の更新時刻の確認（最短 1 秒間隔。設定ファイルとユーザー辞書ディレクトリ）。反映されるもの: `input.*`（既存セッションにも）、`conversion.num_candidates` `sync_budget_ms` `max_context_length` `use_context`、`profile.*`、`ranking`、`learning.*`。**次回起動時に反映**: `conversion.model`（通知する）、`n_threads`
- 設計書の「ファイル監視」は、OS のファイル監視 API を使わずリクエスト時の mtime 確認で代用した（スレッド・依存を増やさないため。フォーカス中に別アプリで設定を編集しても、次のキー入力で反映される）

**環境プロファイル（設計書 §4.7）**
- `Profile`: minimal < light < balanced < quality。パラメータ（`ProfileParams`）: LLM 使用、再採点する辞書候補数（12 / 8 / 3 / 0）、左文脈の文字数（40 / 20 / 10 / 0）、ビーム幅・チャンク長（ビームサーチ・ライブ変換の実装後に使う）
- 初回判定（`initial_profile`）: モデル無し → minimal。メモリ 4GB 未満または 1 コア → light。実測（キャリブレーション推論 3 回の中央値）が ≤40ms → quality、≤120ms → balanced、≤300ms → light、それ以上 → minimal。ただしメモリ 8GB 未満は light まで、quality はメモリ 16GB 以上かつ 8 コア以上のみ。実測前は暫定（ハードウェアから balanced 以下）。キャリブレーションは起動直後にバックグラウンドで走り、終わると判定をやり直して `notify` する（この機械: 65ms → balanced）
- 実行時の自動切替（`Controller`、`profile.auto_switch`）: バッテリー駆動・低電力モード・サーマル悪化（serious −1、critical −2）・メモリ逼迫（warning −1、critical で minimal まで）・レイテンシ（直近 20 回の p95 が `max_latency_ms` を超えたら −1）。**下げるのは即座に、戻すのは最短 60 秒のヒステリシス**。レイテンシは p95 が目標の半分未満で 60 秒続けば 1 段戻す。`profile.mode` を明示すると固定（自動切替なし）。切替は `notify`（`profile_changed`）で理由付きで通知し、`get_status` で現在のプロファイルと理由を取れる
- OS の電源・サーマル・メモリ情報の取得はフロントの責務（コアは OS の API を呼ばない）: 新しい通知 `set_environment`（`on_battery` / `low_power_mode` / `thermal` / `memory_pressure`。省略した項目は前回の値を維持）。GPU の検出は未実装
- 未実装: minimal / メモリ逼迫時の**モデルの解放**（現状は LLM を使わなくなるだけでメモリに残る）、`light` でのスレッド数削減、キャリブレーション結果の永続化（毎回起動時に測る）

**学習（設計書 §4.11）**
- `nagi-learn`: 行 = (reading, surface, context_hash)、`score = ln(1+Σcount) × 0.5^(経過日数/14)` + 文脈一致行の `0.5 × ln(1+count)`、`max_entries` 超過は低スコアから削除、TSV（1 行目 `#nagi-learn\tv1`、エスケープ付き、一時ファイル + rename + `File::lock` で原子的保存）、壊れた行は読み飛ばして元ファイルを `.bak` に退避、未知バージョンは読み込まずファイルを壊さない。`context_hash` は左文脈の末尾 4 文字の FNV-1a 64bit
- 記録: engine の `CommitObserver`（変換を経て確定した Japanese 文節。ひらがなのまま確定した入力は記録しない）。**記録しない条件**: `learning.enabled = false`、secure input、`learning.disabled_apps`（既定: 1Password・Bitwarden・KeePassXC・LastPass。フロントが `set_context.app_id` で報告）。保存: フォーカス喪失・`shutdown`・5 分ごと・誤学習削除時
- 反映: 文の変換結果で、優先順が LLM より前の出所（既定では学習 → ユーザー辞書）が文節の読みと完全一致する結果を持てば、その表記に置き換える（学習はスコア 0.3 以上 = 14 日以内に 1 回でも確定していれば有効）。文節候補では優先順どおりに並べ、出所 `learned` を付ける
- **誤学習の削除**: 候補ウィンドウで `Ctrl+Delete`（Action `DeleteLearning`）→ その文節の現在の表記の学習を忘れさせ、候補一覧を作り直す。全削除は `clear_learning`
- 予測変換（前方一致の候補表示）は未実装（`lookup_prefix` は用意済み）

**ユーザー辞書・スニペット（設計書 §4.13）**
- `<user_dir>/user_dict/*.txt|*.tsv` の Mozc TSV（`読み\t単語\t品詞\tコメント`、`#` と MS-IME の `!` ヘッダはコメント、UTF-8 / BOM / UTF-16 LE・BE、カタカナの読みはひらがなに正規化）。不正な行は行番号付きで通知して読み飛ばす。エクスポートは Mozc TSV（`UserDictionary::to_mozc_tsv`）。MS-IME / Google 日本語入力のエクスポートは同じ列構成なので、そのまま置けばインポートになる
- 実装は**オーバーレイ**: システム辞書（不変）を作り直さず、実行時に Viterbi の格子と強制アライメントへユーザー語を追加する（語コスト 1500 でシステム辞書の語より優先）。品詞は Mozc のユーザー辞書の品詞名を `id.def` の品詞 ID に前方一致で対応づける（人名・姓・名・地名・組織・固有名詞・副詞・形容詞・動詞 ほか。未知は名詞,一般）。動詞は基本形の 1 形のみ登録（活用の展開は未実装）
- スニペット: 品詞 `スニペット` の行（単語欄の `\n` `\t` `\` をエスケープとして解釈）。読みが一致する文節の**候補**として出る（変換結果を自動で置き換えない）
- 優先順位: `[ranking] source_order`（既定 `learned, user_dict, llm, system_dict, kana, rewriter`）。優先順に無い出所は既定順で最後に付く

**変換パイプラインの再編**: `HybridBackend` / `DictBackend` を `Pipeline`（辞書なし・辞書のみ・辞書 + LLM を 1 つで扱う）に統合した。LLM 呼び出しごとの実測レイテンシを環境プロファイルの制御器に渡す。`persona`（`profile.persona`）は zenz の `` 欄に注入する

### G-13. 環境プロファイルの残り4項目（2026-09-27）

G-12 で「未実装」としていた4項目を実装した。ユーザー不在の自律作業中の暫定判断。

- **GPU 検出**（`core/profile/src/lib.rs`）: `SystemInfo.has_gpu` を追加。macOS は Metal 対応 GPU が事実上必ずあるとみなし常に `true`（古い Intel Mac の非対応は稀なため簡略化）。Windows / Linux は `PATH` 上に `nvidia-smi`（`.exe`）が存在するかだけを見る（プロセスは起動しない）。GPU オフロード自体は未実装で、検出結果は将来の利用に備えるもの
- **light でのスレッド数削減**: `config.conversion.n_threads`（既定 0 = 自動）を実際に `LlamaOptions` へ反映するよう修正した（従来は常に既定値 0 が使われ設定が無視されていた）。加えて `Runtime::effective()` で `Profile::Light` のとき基準スレッド数を半分（最低 1）に絞る。`ConversionModel::set_n_threads`（既定 no-op）で変換の都度スレッド数を反映する（`LlamaModel` は変換ごとにコンテキストを作るため、モデルの再読み込みは不要）
- **minimal・メモリ逼迫時のモデル解放**: `ConversionModel::set_active(bool)`（既定 no-op）を追加。`LlamaModel` は `model: Option<LlamaModelCore>` として保持し、`false` で解放・`true` で（未読み込みなら）再読み込みする。`Pipeline` がプロファイルの変化を検出して（`Minimal` か否かで）別スレッドで切り替える。再読み込み中に変換要求が来ても既存の `sync_budget` の仕組みで辞書結果にフォールバックするため待たせない。メモリ逼迫（critical）は既存の `Controller` のペナルティ計算で `Minimal` まで下がるため、同じ経路で解放される
- **キャリブレーション結果の永続化**: `<user_dir>/calibration.json`（`core/server/src/calibration.rs`、JSON）に中央値・CPU コア数・メモリ量・バックエンド名を保存する。起動時に一致すれば即座に使い、直後の `warm_up()` が新しい実測値で上書き・再保存する。ハードウェアやバックエンドが変わったら使わない（`CalibrationRecord::matches`）
- 副次的な整理: `Pipeline::new` と `ConversionBackend::name()` に重複していたバックエンド名の判定を `pipeline::backend_name()` に統合した

### G-14. キーマップ TOML 上書き（2026-09-27）

docs/progress.md 優先4「キーマップ TOML」を実装した。ユーザー不在の自律作業中の暫定判断。

- **対象範囲**: `composing`（ローマ字入力中）/ `converting`（変換中）の2文脈に限る。`Direct` 文脈（ほぼ printable の挿入のみで remap の需要が薄い）とグローバルなモード切替キーは対象外
- **TOML 形式**: `[keymap.composing]` / `[keymap.converting]` に `"キー仕様文字列" = "Action仕様文字列"` を並べる。キー仕様は既存の `parse_key`（`Ctrl+Shift+Right` 形式）をそのまま再利用
- **Action 仕様文字列**: 引数なしの Action は名前そのまま（`NextCand` 等）。`Direction` を持つものは `名前:Left` / `名前:Right`。`SelectCand` は `SelectCand:1`〜`9`。**`Insert` / `InsertText` / `SetMode` への上書きは対象外**（文字・モードの値を TOML 文字列に埋め込むエスケープ構文が複雑になるため v1 では見送った）
- **不正な上書きの扱い**: パースできないキー・Action はその1件だけ無視し、`notify`（`keymap_warning`、Warning）で知らせる（既存の `config_warning` と同じ「壊れていても動き続ける」方針）
- **実装**: `core/engine/src/keymap.rs` の `KeyMap` が `HashMap<(KeyContext, KeySpec), Action>` の上書きテーブルを持ち、既定のハードコード分岐より先に確認する（上書きが無い設定では常に既定分岐にフォールバックし、既存のゴールデンテスト・単体テストへの影響はゼロ）
- **設定検証**: `[keymap.composing]` / `[keymap.converting]` サブテーブル自体は既知キーに追加したが、その内側のキー（ユーザーが自由に指定するキー仕様文字列）は既存の「1階層しか見ない」不明キー検出の対象外（意図的）

### G-15. Rewriter 基盤（全角/半角・半角カナ、2026-09-27）

設計書 §4.13 の Rewriter（決定的変換の最終段）を新規実装した。ユーザー不在の自律作業中の暫定判断。

- **スコープ**: v1 は全角/半角英数記号（ASCII 0x21-0x7E ⇔ U+FF01-FF5E、単純な `+0xFEE0` オフセット）と全角カタカナ→半角カナのみ。**日付・簡易計算は対象外**（曖昧さ・安全性の観点で見送り。`Rewriter` trait 経由で後から追加できる形だけ用意）
- **実装**: 新規クレート `core/rewriter`（`nagi-rewriter`）。CLAUDE.md のリポジトリ構成案で「rewriter は各 Phase で分離」とされていた方針に沿う。`Rewriter` trait（`id` / `rewrite(reading) -> Vec<String>`）と `RewriterChain`（複数 Rewriter をまとめて候補を集める）
- **半角カナの対応表**: 手入力ではなく、Python の `unicodedata` で全角カタカナの `<narrow>` 分解を機械的に導出して検証した値をそのまま `HANKAKU_KATAKANA_TABLE` に埋め込んだ（誤りのリスクを排除するため）。半角カナが存在しない文字（`ヮヰヱヵヶヸヹ` 等）を含む場合は変換候補を出さない
- **Pipeline への統合**: `segment_candidates`（候補一覧）にのみ `CandidateSource::Rewriter` として追加する。`convert_sentence`（自動選択される変換結果）は変更しない。辞書の有無に依存しない決定的変換なので、`dict` が無い（`minimal`・辞書無し起動）場合でも動作するよう、辞書ブロックの外で候補を追加する

### G-16. Rewriter: 漢数字変換の追加（2026-09-27）

G-15 の Rewriter 基盤に、アラビア数字→漢数字の2種類を追加した。ユーザー不在の自律作業中の暫定判断。

- **`KanjiDigits`**: 桁の重みを考慮しない1文字ずつの置換（`"2026"` → `"二〇二六"`）。曖昧さが無いので常に候補を出す
- **`KanjiPositionalNumeral`**: 位取り漢数字（`"2026"` → `"二千二十六"`、`"10000"` → `"一万"`）。4桁ごとに 千/百/十/一 と 万/億/兆 を組み合わせる標準的な算出方法で、`十`/`百`/`千` の前の「一」は省略する。**先頭ゼロ（`"0"` 単独を除く）は位取り表記が定義できないため対象外**（`"007"` 等は変換しない）。**兆の桁（10^16 = 京）を超える17桁以上も対象外**（安全側に倒して変換しない）
- 設計書 §4.13 の「数字表記」に相当。「日付」「簡易計算」は曖昧さ・安全性の観点で v1 のスコープ外のまま

### G-17. 範囲操作: 複数文節選択（2026-09-27）

設計書 §4.10「範囲操作」のうち、複数文節選択（`Ctrl+Shift+←/→`）の選択機構のみを実装した。ユーザー不在の自律作業中の暫定判断。

- **対象範囲**: 選択の開始・伸縮・解除（`Preedit.selection` / `StateKind::RangeSelecting` への配線）のみ。**選択範囲に対する再変換（Space での範囲再変換）・`Ctrl+U/I/O` 等の Rewriter 直接書き換えは対象外**（設計書に記載はあるが UX の詳細が未確定なため。特に Rewriter 直接書き換えは、engine が dict/infer/rewriter に依存しない分離原則（decisions.md 記載）を保つには `ConversionBackend` 経由で候補の出所から選ぶ必要があり、`CandidateSource::Rewriter` だけでは「どの Rewriter か」を区別できず実装が曖昧になるため見送った）
- **モデル**: `Conv` に `range_anchor: Option<usize>` を追加。`ExtendSelection` は `focus` を選択の可動端として動かし、押した瞬間の `focus` を固定端（`range_anchor`）にする（Shift+矢印のテキスト選択と同じ考え方）。`focus` が `range_anchor` まで戻ったら選択解除。境界（先頭/末尾）で `focus` が動けない場合は自動的に選択が作られない（`range_anchor == focus` が成立するため）
- **選択中の既存操作**: 選択中でも `NextCand` / `ToKatakana` 等の単一文節操作は `focus` 文節に対してそのまま今まで通り動作する（意図的。範囲全体への操作は未実装なので、単一文節操作を止める理由がない）
- **解除の挙動**: 通常の矢印（`MoveFocus`、Shift 無し）は選択を解除してから移動する。Escape は複数文節選択中ならまず選択だけを解除し、選択していなければ通常通り Composing に戻る
- **テスト**: `core/engine/tests/golden/90_range_select.yaml`。既存のゴールデンテスト・`ir.rs` の `Preedit.selection` の UTF-16 変換テストは変更していない（`ExtendSelection`/`selection`/`Ctrl+Shift` を検査する既存テストが無いことを確認済み）

### G-18. select_candidate メソッドの実装（2026-09-27）

`select_candidate`（JSON-RPC、`NOT_IMPLEMENTED` スタブだった）を実装した。プロトコルの契約（`SelectCandidateParams{session_id, index}`）は既に確定していたので、それに沿って実装した。ユーザー不在の自律作業中の暫定判断。

- **既存の `Action::SelectCand`（数字キー1〜9、ページ相対インデックス）とは別経路**。今回追加したのは候補ウィンドウの表示状態に関わらず候補一覧全体から絶対インデックスで直接選択するAPI（マウス操作等を想定）
- 実装時にメソッド名の衝突が発生したため、既存の private メソッド（数字キー版）を `select_candidate_page_relative` に改名した（ロジックは無変更）
- `reconvert` メソッドは今回変更していない（別途、辞書の逆引き機能が必要なため対象外）

### G-19. update.auto_check の実装（2026-09-27）

`update.auto_check` のアップデート確認クライアントを実装した。ユーザーの回答（「そこら辺は任せる」「ドメインは nagi.shiki-01.com」）に基づく。

- **マニフェスト URL**: 既定 `https://nagi.shiki-01.com/updates/latest.json`（`[update] check_url` で上書き可能）。マニフェストの JSON 形式（`{"version": "x.y.z", "notes_url": "..."}`）はこちらで定義した暫定案（実サーバーは未整備）
- **スコープ**: 確認して `notify`（`update_available`）で知らせるだけ。**ダウンロード・インストールは対象外**（署名検証付きアップデートの実装はインストーラ・配布フローの一部として別途必要）
- **間引き**: `<user_dir>/update_check.json` に前回確認時刻を保存し、24時間以内は確認しない
- **設定から任意のタイミングで確認**（`default.toml` の既存コメントにあった文言）に対応する JSON-RPC メソッドは無いため、今回は起動時の自動確認のみを実装した。オンデマンド確認は別途プロトコルメソッドの追加が必要（未着手）
- 実サーバーが無いため、`nagi.shiki-01.com/updates/latest.json` を実際に用意しないと機能しない（クライアント側は準備済み）

### G-20. Rewriter: 日付・簡易計算の追加（2026-09-27）

G-15/16 で見送っていた日付・簡易計算をスコープに含める旨の回答を受け、Rewriter に追加した。

- **`DigitsToDate`**: 8桁の数字（YYYYMMDD）を `2026/09/27` / `2026年9月27日` に変換する。月（1-12）・日（月の日数以内、うるう年考慮）として妥当でなければ変換しない（システム時刻には依存しない純粋な検証のみ）
- **`SimpleCalculator`**: `数値 演算子 数値` の単一の二項演算（`+` `-` `*` `/`）のみ。括弧・複数演算・先頭の符号は対象外。0除算は候補を出さない。結果は整数ならそのまま、小数は小数点以下4桁に丸めて末尾の0を除去して表示する

### G-21. KV キャッシュ再利用の見送り（2026-09-27）

G-11・G-12・G-13 で未実装として残していた「KV キャッシュの再利用」について、ユーザーから「任せる」との回答を受け、実装しないことを決定した。

- **技術的な問題**: `llama-cpp-2` の `LlamaContext<'a>` は `model: &'a LlamaModel` を借用する。これを `LlamaModel`（`core/infer`）に永続フィールドとして持たせると自己参照構造体になり、素直には安全に書けない（`ouroboros` crate 等の利用、または `unsafe` の個別解禁（G-2 が想定する「infer の FFI 境界」の緩和）が必要になる）
- **判断**: 既存の計測（G-11）で `convert_sentence` は p50 32ms / p95 56ms とすでに設計書の目標（Space 変換 → 候補表示 p95 < 150ms）を満たしている。KV キャッシュ再利用は純粋な性能最適化であり、正誤（誤ったキャッシュ再利用による誤変換）のリスクを負って実装する優先度は現時点で低いと判断し、**見送る**
- 将来、レイテンシが問題になった場合に再検討する。再検討時は `ouroboros` の利用を第一候補とする（手書き `unsafe` より安全側に倒せるため）
- **2026-09-27 追記**: 別セッションでこの判断を覆し実装した（G-32）。`ouroboros` は使わず、`model` を `Box` 化してアドレスを固定し `ctx` の借用を `'static` と偽る手書きの自己参照構造にした（`unsafe` は G-2 の想定どおり `core/infer` の FFI 境界のみに限定）

### G-22. 範囲選択への再変換・ToHankaku（2026-09-27）

G-17 で選択機構のみとしていた範囲操作に、Mozkey（`koyasi777/mozkey`。Mozc + ローカル Zenz 補正の fork）を参照しつつ、ユーザーの回答（「選択状態の維持」「（Rewriter種類の識別問題は）追加していい」「（既定キーは）設定可にすれば良い」）に基づいて残りを実装した。

- **「選択状態の維持」の解釈**: 複数文節選択中に Space で再変換すると、選択範囲は1つの文節に統合される（`Preedit.selection` の「2文節以上」という前提が成り立たなくなるため）。「選択状態を失う」のではなく、**再変換直後に候補ウィンドウを開いた状態にし、そのまま Space で候補をめくれる**（結果への候補アクセスの継続性）という形で実現した
- **`CandidateSource::Rewriter` の種類識別問題の解決**: プロトコルは変更していない。`nagi-rewriter` が stdlib のみ依存の純粋関数クレートであることを踏まえ、**`core/engine` が `nagi-rewriter` に直接依存する**ことにした（`nagi-romaji` に依存しているのと同じ位置づけ。辞書・LLM のような「バックエンド」ではなく、決定的な変換ユーティリティとして扱う）。新しい `Action::ToHankaku` を追加し、候補一覧を経由せず `HankakuKatakana`/`HankakuAlnum` を直接呼ぶ
- **既定キーバインドは割り当てない**（`Ctrl+U/I/O` はハードコードしない）。`parse_action` に `ToHankaku` を追加し、`[keymap.converting]` から利用可能にするだけ（G-17 の「デフォルトキーバインドを決めない」方針を継続）
- `ToKatakana`/`ToHiragana` も選択範囲全体に対応させた（選択中は範囲全体を1文節にまとめてかな化）。**選択していない場合の既存ロジックは一切変更していない**（既存テストへの影響ゼロ）

### G-23. 予測変換（Tab）の実装（2026-09-27）

設計書 §4.11「予測変換」を実装した。ユーザーの回答（表示は候補ウィンドウ、設定可能にする、詳細は任せる）と Mozc/Mozkey の実際の実装を参考にした。

- **設計書からの逸脱**: 設計書は「Tab/↓で選択」としていたが、Mozc の実際のキーマップ（`ms-ime.tsv`/`kotoeri.tsv`）では `Composition Tab → PredictAndConvert`、`Composition Down → MoveCursorToEnd` であり、**Down は予測変換に使われていない**（カーソル移動）。実績のある挙動を優先し、**Tab のみ**を予測変換のトリガーとした。Down は既存の挙動（Composing では何もしない）を変えていない
- **状態遷移**: Mozc の `PredictAndConvert`（`session.cc`）を参考に、Composing 中に Tab を押すと `ConversionBackend::predict`（学習キャッシュ・ユーザー辞書の前方一致）を呼び、見つかれば **Converting 状態へ遷移して候補ウィンドウを開く**（Composing に留まってインライン候補を出す方式ではない）。見つからなければ何もしない。**Converting 中の Tab は `NextCand`（次候補）として働く**（Mozc と同じ。既存の Space と同じ効果になる）
- **供給元**: `Learner::lookup_prefix`（既存）と `Overlay::lookup_prefix`（新規追加）。学習キャッシュを優先し、ユーザー辞書は重複しないものだけ追加する
- **設定**: `[predict] enabled`（既定 true）・`min_chars`（既定 1、設計書の既定値と一致）
- **既存テストの意図的な変更**: `core/engine/src/keymap.rs` の `composing_table`/`converting_table`、`core/engine/tests/golden/40_convert.yaml` の `convert-tab-noop`（→ `convert-tab-next-candidate` に改名）を、Tab の新しい挙動に合わせて更新した

### G-24. 非同期通知の配信基盤（2026-09-27）

`update_preedit`/`update_candidates` 等（ライブ変換・スタイル変換）の非同期通知を、キー入力を待たずに配信できる基盤を実装した。実装方針・通知タイミングはユーザーから「任せる」との回答。

- **設計**: `Server::run` を「標準入力を読む専用スレッド + `mpsc` チャンネルを受信するメインループ」に分離した。`Runtime` に `event_hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>` を追加し、`push_event` が呼ばれた瞬間にフック（チャンネルへの `Wake` 送信）を呼ぶ。メインループは `Msg::Line`（リクエスト処理）と `Msg::Wake`（溜まっている `notify` をすぐ出力）の両方を1つのチャンネルで受け取る
- **既存挙動への影響**: `handle_line`（リクエストへの応答に続けて notify を出す既存の同期経路）は変更していない。今回の変更は「キー入力が無い間でも notify が届くようになる」という追加のみ
- **副作用として直した問題**: `Server::run` は標準入力を別スレッドへ move するため `R: BufRead + Send + 'static` が必要になった。`StdinLock`（`MutexGuard` を内包）は `Send` ではないため、`main.rs` はロックせず `Stdin` をそのまま渡す形に変えた（`Stdin::read` は呼び出しごとに内部で lock/unlock するので、単一スレッドから使う限り挙動は変わらない）
- **今回のスコープ外**: ライブ変換・スタイル変換など、実際にこの基盤を使って非同期通知を送出する機能自体はまだ実装していない（配信基盤のみ）。既存のキャリブレーション完了・アップデート確認・プロファイル変化の通知が、この基盤によって即時配信されるようになった（動作の変化として観測可能）
- **テスト**: `core/server/tests/rpc.rs` に、`push_event` が次のリクエストを送らずに配信されることを検証する統合テストを追加した（`cargo test --workspace` を複数回連続実行して安定性を確認済み）

### G-25. ライブ変換: engine側（Session.live_conversion / Engine::live_convert）（2026-09-27）

設計書 §4.3 のライブ変換を実装した。「進める（推奨）」との回答を受けて着手。

- **キャンセル不要のデバウンス設計**: エンジンはタイマーを持たない。`Engine::live_convert(session_id, expected_reading)` は呼ばれた時点の入力全体の reading と `expected_reading` を比較し、一致しなければ何もしない（`None` を返す）。サーバー側は「デバウンス完了時に呼んだ入力がまだ有効か」をこの一致判定だけで確認できるので、古いデバウンスタイマーをキャンセルする仕組み（世代カウンタ・トークン等）が不要になる。Mozkey（C++ 実装）はタイマーキャンセルを使うが、Rust 側の状態機械はより単純な「内容一致チェック」で同じ目的を達成できると判断した
- **IR 変更なし**: `Preedit`/`Segment` の構造は変更していない。Composing 状態の表示に、既存の Converting 用 `Segment{reading, surface}` をそのまま再利用する（`Session.live_conversion: Option<LiveConversion>` というキャッシュを新設し、`build_preedit` の Composing 分岐がこれを見るだけ）。ふりがな（reading）表示もこの構造のままで実現できる
- 4つのテストケースを追加。カーソル位置の整合性テスト（`live_convert_keeps_cursor_position_consistent`）で、変換後の surface が reading よりUTF-16単位で短くなるケースのカーソルクランプ挙動を確認した（既存の `build_preedit` のクランプロジックは無変更で正しく動作する）

### G-26. ライブ変換: server側（デバウンス・非同期通知配信・設定）（2026-09-27）

G-25 の続き。G-24 の非同期通知基盤に乗せて、実際にライブ変換の結果を配信するようにした。

- **デバウンススレッド方式**: `process_key` のたびに、Composing 状態かつ入力の reading 文字数が `live_conversion_min_key_length` 以上なら、その場でスレッドを1本立てて `live_conversion_delay_ms` だけ sleep してから `Engine::live_convert` を呼ぶ。古いスレッドをキャンセルしない（G-25 の設計により無害）。高速に連続入力すると短命スレッドが積み重なる可能性があるが、遅延（既定228ms）より速いキー入力間隔は現実的にまれであり、各スレッドのコスト（sleep + 1回のロック取得）も小さいため許容する
- **既定値は Mozkey の実測値を採用**: `live_conversion_delay_ms = 228`、`live_conversion_min_key_length = 2`（`koyasi777/mozkey` の `config.proto` 実測。ユーザーから「mozkey を参照」との回答）
- **`Server.engine` を `Arc<Mutex<Engine>>` 化**: デバウンススレッドとメインスレッドが同じ `Engine` を安全に共有するための変更。ロックはポイズン時も中身を使って継続する（`unwrap_or_else(PoisonError::into_inner)`）方針にした。IME サーバーは一部の処理でパニックが起きても全体を落とさず動き続けることを優先する
- **`update_candidates` の非同期配信は対象外**（今回のスコープ外）: ライブ変換は候補ウィンドウを開かない（Mozkey の挙動を参照）ため不要
- テスト: `core/server/tests/rpc.rs` に `MockBackend` を使った統合テストを追加（`KanaBackend` は reading=surface のため差分が出ずテストにならない）。3回連続実行でフレーキーでないことを確認済み

### G-27. ライブ変換: composing_chunk_len によるチャンク処理（2026-09-27）

設計書 §4.6「`composing_chunk_len`（既定30文字）でreadingをチャンク分割し、編集されたチャンクのみ再変換」に対応した。今回実装したのは「チャンク分割して変換する」部分のみ（下記スコープ参照）。

- **「数字の直後で分割しない」制約は自動的に満たされる**: `nagi-romaji` の `Composer::insert_char` は ASCII 数字・大文字・記号を常に `PieceKind::Literal`、それ以外（ひらがな等の非ASCII）を常に `PieceKind::Japanese` として扱い、`Composer::pieces()` は同種の連続文字だけを1つの `Piece` にまとめる。そのため `PieceKind::Japanese` の `Piece.text` に数字が混ざることは構造上あり得ず、単純な固定長チャンク分割（`composing_chunk_len` 文字ごと）で設計書の意図（数字と直後のかなを分断しない）を自動的に満たす。「記号の連続」「Fixed境界」も同じ理由でJapanese Piece内部には出現しない（Fixed はComposing状態にはまだ無い。Phase 3の部分確定機能）
- **スコープ**: 今回実装したのはチャンク分割による変換の「正しさ」のみ。設計書が言う「編集されたチャンクのみ再変換」（前回のチャンク境界を記憶し、編集位置を含むチャンクだけを再変換してレイテンシを下げる）という**キャッシュ最適化は今回のスコープ外**。現時点では毎回のキー入力でチャンク全体を再変換する（既存のデバウンス設計（G-25・G-26）により、キー入力ごとにLLM/辞書を叩くわけではないため実害は限定的と判断したが、実機での長文入力時のレイテンシ計測は未実施）。実機確認後、必要であれば追加する
- `composing_chunk_len` は元々 `config.toml`/`nagi-profile` には存在したが `EngineConfig` に橋渡しされていなかった（未使用のまま放置されていた設定値）。今回 `EngineConfig::composing_chunk_len`（既定30）を追加し `engine_config_from` で接続した。プロファイル別の値（`quality`=40 / `balanced`=30 / `light`=20 / `minimal`=20）への動的追従は他の類似パラメータ（`predict_min_chars` 等）と同様、現時点では未対応（設定ファイルの値を静的に使う。プロファイル自動切替時に反映されない）

### G-28. 確定取消（Ctrl+Backspace）の実装（2026-09-27）

設計書 §4 状態遷移表「確定直後 | Ctrl+Backspace（既定） | 確定取消: 直前の確定を未確定状態へ戻す（アプリが対応する場合）」を実装した。

- **記憶と破棄の単一の choke point**: 確定は必ず `commit_all` を経由する（`focus_out`・Enter・モード切替時の確定・Ctrl/Alt/Meta付きキーでの確定等、すべての確定経路がここに集約されている）ため、ここで確定前の入力（Composer の Piece 列）を `Session.last_commit` に記憶させた。「確定直後のみ有効」という制約は `apply()` の中では実現できない（キーが未割当で `keymap.resolve` が `None` を返す場合、`handle_key` は `apply()` を呼ばずに直接素通しを返すため）。そのため破棄ロジックは `handle_key` に置いた: `Action::UndoCommit` 以外に解決された、または未割当（素通し）だった場合は必ず `last_commit` を捨てる
- **Converting からの確定取消は reading に戻る**: 変換結果（surface）は失われ、ひらがな（reading）の Composing に戻る。既存の Escape での変換取消（`back_to_composing`）と同じ扱いで、一貫性がある
- **プロトコル拡張**: `ProcessKeyResult.delete_before: Option<u32>`（UTF-16長）を追加した。ホストアプリのテキストから確定済み文字列を実際に削除するのはフロントエンド（未実装）の仕事であり、今回実装したのはエンジン側の記憶・復元機構とこの通知のみ。フロント側の対応（Surrounding Text APIでの削除）は別途必要
- **`typeshare-cli` をこの実行環境にインストールして検証した**（`cargo install typeshare-cli --locked`。crates.io は到達可能）。これにより `cargo xtask check-types`/`gen-types` をこのサンドボックス内で実行・検証できることを確認した（以前はモデル・辞書取得（huggingface.co）が到達不可という制約はあったが、typeshare-cli の取得先である crates.io は無関係で問題なく到達できた）。生成された Swift 型（`frontend-macos/Sources/Generated/Protocol.swift`）も本コミットに含めてコミット済み

### G-29. reconvert（確定済みテキストの再変換）の実装（2026-09-27）

`reconvert` JSON-RPC メソッド（`ReconvertParams{session_id, text}`。それまで `NOT_IMPLEMENTED` だった）を実装した。設計書「読みは辞書の逆引きで推定し、不能区間はLLMで補う」のうち、辞書による推定部分を実装した（LLM補完は今回のスコープ外。設計書自体が将来の拡張として記述しているため問題ないと判断）。

- **アルゴリズム**: `nagi-dict` に既存の順方向 Viterbi（読み→表記。`convert.rs`）と対になる逆方向 Viterbi（表記→読み。新規 `reverse.rs`）を実装した。辞書全体を1回舐めて「表記→[(読み, 語 index)]」の逆引き索引を構築し（`HashMap`。fst の2本目のトリエは作らず、前方一致探索は表記の最大文字数までの全長を試す単純な方法にした。速度上の実害は無いと判断: reconvert はユーザー操作起点でキー入力毎ではない）、あとは連接コスト表・文節区切りルール（`starts_bunsetsu`）を順方向と共有する
- **未知語の扱い**: 辞書に登録の無い表記区間（例: 辞書にない漢字）は、既存の順方向 Viterbi の「1文字素通しノード」（`unknown_ids`。どの位置からでも経路が保証される）と同じ仕組みをそのまま流用し、`reading = その文字自身` として通す。正しい読みではないが、クラッシュせず、ユーザーがすぐに打ち直せる状態にはなる
- **遅延構築・メモリコスト**: 逆引き索引は初回 `reconvert` 呼び出し時にだけ構築する。`reconvert` を一度も呼ばなければコストはゼロ
- **2026-09-28 実機追記（Windows、実データ）**: 見積もりの「数十MB」は誤りで、実測は**約300MB**（`HashMap<String, Vec<(String, usize)>>`、辞書語数約120万）。原因は主に、`Vec` が最初の push で容量4を確保するため（`RawVec::MIN_NON_ZERO_CAP`）候補1件の表記でも3件分の未使用領域が生じること、同じ読みが同音異義語の表記ごとに複製されること、`String`/`Vec` 自体が `Box<str>`/`Box<[_]>` より大きいこと。**構造体を圧縮しても（`Box<str>`・`Box<[_]>`・`Arc<str>` 共有・`u32` 化）実測は約270MBまでしか下がらなかった**（1エントリあたりの割当回数（表記・スライス・読みで計3回）は変わらず、Rust/OSのアロケータの割当あたりの固定オーバーヘッドが小さい文字列×120万件で支配的だったため。さらに削るには、割当を1〜2回のアリーナにまとめる設計変更が要る（表記プール `surf_pool` を借用する自己参照構造など）が未着手）。**対処として索引を解放可能にした**: `Dictionary::release_reverse_index()`（`reconvert.rs` の逆引きロックを `OnceLock` から `Mutex<Option<Arc<ReverseIndex>>>` に変更）を追加し、`Pipeline::reconvert` は `minimal`/`light` プロファイルでは使用直後に解放する（次回呼び出し時に約270MBを払って作り直す。速度よりメモリを優先する既存方針・G-13と同じ判断）。`balanced`/`quality` では常駐させたまま（メモリ予算に余裕がある想定）
- **バックエンドが対応できない場合の扱い**: `ConversionBackend::reconvert` の既定実装は空を返す（`KanaBackend` 等、辞書を持たないバックエンドはそのまま非対応）。`Engine::reconvert` はこれを受けて、表記全体を1つの `Literal` セグメント（`reading = surface`）として扱う。読みの再変換はできないが、確定済みテキストを一旦編集可能な状態に戻せる（クラッシュや無反応にはならない）
- **ユーザー辞書（Overlay）は今回の逆引き対象に含めていない**（システム辞書のみ）。将来の拡張余地として残す
- テストは合成辞書（`DictionaryBuilder`）による単体テスト（`nagi-dict`）が中心。実データでの精度・メモリ計測は2026-09-28に実機（Windows）で実施済み（上記追記）

### G-30. ビームサーチ: 辞書ベースの文全体N-best（2026-09-27）

ユーザーの決定に基づき実装した: (1) 実LLMでの本物のビームサーチ（トークン生成、KVキャッシュ管理を伴う）は本環境で検証不能なため、辞書のk-best Viterbiで実装する。(2) 文全体N-best候補を選ぶと、選んだ分割へ丸ごと置き換える（複数の分割を保持し続ける複雑な設計は採用しない）。

- **アルゴリズム**: `nagi-dict` に既存の1-best Viterbi（`convert.rs`）を一般化したk-best版（`nbest.rs::convert_n_best`）を実装した。各セル（文字位置, 右ID）で上位1件だけでなく上位k件を保持しながら格子を前進する標準的な手法。既存の1-best実装は無変更（性能・挙動に影響なし）
- **エンジン側の状態設計**: `initial_view = sentence`（既定）かつ入力全体が1つの日本語Pieceのときだけ、Space変換の1回目でN-best候補を1つの「メガ文節」として候補窓に表示する（`Conv.sentence_alternatives: Option<Vec<Vec<Segment>>>`）。候補選択（NextCand/PrevCand/SelectCand/select_candidate）以外の操作が来たら、選択中の候補の文節分割へ展開する（`ensure_segmented`。`apply_converting` の先頭で全操作に対して呼ぶ、候補選択だけ除外）。複数Pieceが混在する場合（記号・数字と日本語が混じる）は今回のスコープ外で、常に既存の1-best・文節ごとの変換にフォールバックする
- **重要な訂正（実装レビューで発見）**: 当初 `Pipeline::sentence_candidates` は辞書のコスト順だけで候補を並べる実装だったが、これは実際の変換品質を落とす重大な問題だった。既存の `convert_sentence`（LLMが使えるときは再採点済みの1-best。G-11で精度76.4%まで改善したもの）を1件目にし、辞書のN-bestで2件目以降を補う形に修正した。修正前のままだと、LLM搭載環境でも`initial_view=sentence`（既定）が有効な限りLLM再採点を経由しない辞書コスト順が最初の候補になってしまい、精度が66.0%相当まで後退していた（`server/tests/features.rs` の同音異義語（こうえん→公園/公苑/講演/後援）に関するテスト群がこれを検出した）
- `server/tests/features.rs` の `server_in()` は `initial_view = Segment` を明示するよう変更した。これらのテストはLLM再採点・学習・ユーザー辞書優先度（`segment_candidates` 経路）を検証する目的であり、文全体N-bestとは別の関心事のため、テストの対象を明確に分離した
- `conversion.beam_width`（既存だが未接続だった設定値）を `EngineConfig.beam_width` に接続した（文全体N-bestの候補数の起点）。`nagi-profile::ProfileParams.beam_width`（プロファイル別既定値）に基づく `Effective.beam_width`（`nagi-server::Runtime`）も別途接続したが、こちらは辞書ベースのN-bestからは現時点で参照されない（将来、実LLMビームサーチを実装する際に使う想定のプレースホルダー）。つまり `conversion.beam_width` を設定ファイルで指定すればエンジン側のN-best候補数には反映されるが、プロファイル自動切替による値の変動は反映されない

### G-31. アップデート配布物のed25519署名検証（2026-09-27）

設計書「ed25519署名検証付きアップデート」に対応した。`update_check.rs`（G-19）のマニフェストに `download_url`/`sha256`/`signature` を追加し、ダウンロードしたバイト列のSHA-256・ed25519署名の両方を検証する `download_verified()` を実装した。

- **スコープ**: 検証のみ。実際にファイルをディスクへ書き込む・展開する・自己更新する処理はプラットフォームごとの実装が必要なため対象外（G-19の「確認のみ」というスコープの延長）
- **`hex` クレートは使わず自作**: 依存を増やさない方針（既存コードの他の部分と同じ）
- **`UPDATE_PUBLIC_KEY_HEX` はプレースホルダー**: 実装確認のため `openssl genpkey -algorithm ed25519` で使い捨ての鍵ペアを生成し、公開鍵をこの定数に埋め込んだ。**対応する秘密鍵は生成後すぐに破棄し、このリポジトリのどこにも残していない**。実際に配布する前に、新しい鍵ペアを生成し直し、秘密鍵はリリース署名を行う環境（ビルドサーバー等）でのみオフラインで保管する必要がある
- テストは実際にこの使い捨て鍵ペアで署名した既知の値（`openssl pkeyutl -sign`）を使い、`ed25519-dalek` 2.x との相互運用性（署名の受理・改竄検知・鍵不一致検知・不正な長さの検知）を確認した

### G-32. LLM コンテキストの再利用（KV キャッシュ、2026-09-27）

実装: `core/infer/src/lib.rs`（`llama::LlamaModel`）、`core/infer/Cargo.toml`（`unsafe_code` の局所解禁）。

- **課題**: `LlamaModel::convert` / `score` は呼ぶたびに `new_context`（`n_ctx` に比例した KV キャッシュのバッファ確保を含む）でコンテキストを作り直していた。この確保コストは計測値 `prefill_ms` / `total_ms` に混ざっていた（G-3）
- **実装**: `LlamaModel` にコンテキストを 1 つ保持し（`ctx: Option<LlamaContext<'static>>`）、2 回目以降は `clear_kv_cache()` で中身だけ空にして使い回す。`LlamaContext<'a>` はモデルを借用する型のため、同じ構造体にモデルとその借用を同居させる自己参照構造になる
  - `model` を `Box<LlamaModelCore>` にしてアドレスを固定し、`ctx` の借用を `'static` と偽って保持する（実体は `LlamaModel` が生きている間だけ有効）。`ctx` フィールドは `model` より前に宣言し、drop 順（宣言順）で `ctx`（内部の `llama_context`）を先に破棄してから `model` を破棄する
  - `ensure_ctx` は `&mut self` を取るメソッドにせず、`ctx` フィールドだけを借用する関連関数にした。`&mut self` にすると戻り値の生存期間中コンパイラは `self` 全体を借用中とみなし、`self.model` / `self.tokenizer` など他フィールドに（`score` のループ内で候補ごとに）同時にアクセスできなくなるため
  - この自己参照は `unsafe` が要る（G-2 で「`infer` の FFI 境界のみ、必要になった時点で個別に緩和する」と決めていた実例）。ワークスペース既定の `unsafe_code = "forbid"` を `core/infer/Cargo.toml` だけ `allow` に緩和した（`llama` feature 配下、他のクレートは forbid のまま）
  - `LlamaContext` は生ポインタを持つため `Send` が自動導出されない。`ConversionModel: Send` を満たすため `unsafe impl Send for LlamaModel {}` を追加した。安全性の根拠: 呼び出し元（`nagi-server` の `Pipeline`）は常に `Mutex` 越しに使うため同時に複数スレッドから触られない。llama.cpp 自体もコンテキストをスレッドに固定しない
- **正しさ**: 同一モデルインスタンスで異なる reading / 左文脈 / `convert` と `score` の呼び出しを交互に行っても結果が混ざらないことをテストで確認した（`test_llama_context_reuse_does_not_leak_between_calls`）。既存の決定的性・文脈込み再採点のテストも変更なしで通る
- **実測**（同じ 7 文、`test_llama_model_convert` の `prefill_ms`。実装前 → 実装後、CPU のみ・24 スレッド）: 24→28、19→16、19→15、17→13、19→15、17→14、17→14ms。母数が小さく機械のノイズもあるため参考値だが、悪化はしていない。zenz-v3.1-small・n_ctx=512 では確保コスト自体が小さく、改善幅は大きくない（大きいモデル・長い `n_ctx` では差が広がる見込み）
- **未実装**: 複数セッションを跨いだ会話的な KV キャッシュ差分再利用（プロンプトの共通接頭辞を保持したままの継続）は行っていない。`clear_kv_cache` は毎回全消去する暫定

### G-33. LLM のビームサーチ（文全体の N-best、Stage 1: infer 単体、2026-09-27）

実装: `core/infer/src/lib.rs`（`ConvertRequest::beam_width`、`LlamaOptions::max_beam_width`、`llama::LlamaModel::beam_search` / `decode_beams` / `top_k_logprobs`）。段階分けの方針（ユーザー承認済み）: 今回は infer 単体の実装・検証のみ。`server`（`Pipeline::sentence_candidates`）・`engine`（候補ウィンドウの sentence モード）・キーマップへの統合は次段で行う（進捗は `docs/progress.md`）。

- **アルゴリズム**: 設計書 B-4 の「生成途中で上位 k 本の途中結果を残しながら進める」を文字通り実装した（最初のトークンだけ分岐させる簡易版ではない）。各ステップで、生存中の全ビームの上位 `width` 語ずつをプールし、累積対数確率（ソフトマックス正規化後の対数確率の総和。貪欲デコードの `score` と同じ尺度）が高い順に**全体で** `width` 件だけ残す。終了トークンを引いた候補はその時点で確定仮説として抜け、残りが次のビームになる。合流（1 つの親から複数の子が生き残る）が起きたときだけ [`kv_cache_seq_cp`] で分岐先の KV を複製し、使われなくなった seq_id は `free_seq_ids` で使い回す
- **`beam_width` <= 1 は既存の貪欲デコードのまま**（コードも変更していない）。既存の呼び出し元（`Pipeline::llm_convert` / `llm_scores`）は `beam_width: 1` を渡すので、この変更単体では挙動もレイテンシも変わらない
- **`ConvertRequest::beam_width`** を新設。`LlamaOptions::max_beam_width`（既定 4）でコンテキストの `n_seq_max` を決める（コンテキスト作成は 1 回きりなので、実行中に要求され得る最大幅を見積もって渡す。`server` 側の実際の `conversion.beam_width` との連動は次段で行う）。要求された `beam_width` は `max_beam_width` にクランプする
- **`kv_unified = true` が必要だった**: 既定の `kv_unified = false`（分離バッファ）では `kv_cache_seq_cp` が異なるバッファ間のコピーになり、llama.cpp 側が「全域コピーのみ対応（`is_full` アサート）」で拒否する（プロンプト分だけの部分コピーができずクラッシュする）。`with_kv_unified(true)` で 1 つの共有バッファにして解決した。コンテキストは既存の再利用（G-13）と同じ 1 つを使い回すため、`beam_width` を使わない呼び出し（貪欲デコード・`score`）にも影響するが、動作・既存テストに変化はない
- **実測**（zenz-v3.1-small、`width=3`、代表 4 文）: 曖昧性の低い文では貪欲デコードと同じ表記が最上位に来て、2 位以降に表記ゆれ・別の妥当な変換が並ぶ。例: 「こうえん」→ 公園 / 講演 / 公演、「きょうはいいてんきです」→ 今日はいい天気です / 今日は良い天気です / 今日はイイ天気です、「とうきょうにいく」→ 東京に行く / 東京にいく / 東京に行、「かんじへんかんのてすと」→ 漢字変換のテスト / 漢字変換のtest / 感じ変換のテスト。最上位のスコアは貪欲デコードとバッチ化による浮動小数点誤差の範囲（< 0.01）で一致した
- **既知の限界**: 貪欲デコードの経路がビームサーチで必ず生き残る保証は一般には無い（弱い競合を持つ他ビームの累積スコアが上回れば刈られ得る）。実測 4 文では一致したが、理論的な不変条件としては扱わない。early stopping（十分良い確定仮説が出た時点で打ち切る最適化）は未実装で、常に `max_new_tokens` まで回す（`width` が小さく生成長も短いため実害は小さい）
- **未実装（次段）は G-34 で実装済み**

### G-34. LLM のビームサーチ（Stage 2: server / engine への統合、2026-09-27）

実装: `core/server/src/{runtime,pipeline,setup}.rs`（`Effective::beam_width`・`Pipeline::sentence_candidates`・`llm_beam_convert`）、`core/engine/src/{backend,engine}.rs`（`ConversionBackend::sentence_candidates` 既定実装・`InitialView`・`Conv::{whole,sentence}`）。

- **`Effective::beam_width`**: `Tunables::beam_width_override`（`conversion.beam_width`。未指定なら `None`）でプロファイルの既定値（`quality`/`balanced`=3, `light`/`minimal`=1）を上書きする。既存の `max_context_override` と同じパターン
- **`Pipeline::sentence_candidates`**: `eff.beam_width <= 1` または辞書/モデル無し・長文（`MAX_LLM_CHARS` 超）なら `convert_sentence` の結果 1 件に縮退する。それ以外は `llm_beam_convert`（新設。`ConvertRequest::beam_width` を渡して `model.convert` を呼ぶだけの同期呼び出し。`segment_candidates` の再採点と同様、候補ウィンドウを開いたときだけの呼び出しなので非同期完了通知は行わない）で仮説を取り、それぞれ `Dictionary::align_with` で強制アライメントし、`apply_preferences`（学習・ユーザー辞書の優先）を個別に適用する。表記が重複する仮説は除去し、除去後 2 件未満なら 1 件に縮退する
- **`ConversionBackend::sentence_candidates`**（engine 側 trait）: 既定実装は `convert_sentence` の結果を 1 件返す（ビームサーチ非対応のバックエンドはこれで自動的に無効化される）
- **候補ウィンドウの sentence モード**（engine）: `Conv` に `whole: Option<(reading, left_context)>`（`start_converting` 時点で、preedit 全体がちょうど 1 回の `convert_sentence` 呼び出し由来のときだけ設定。`Session.origins` は次のキー入力で消えるため `Conv` 自身に持たせる必要があった）と `sentence: Option<SentenceCandidates>`（遅延生成）を追加。`conversion.initial_view = sentence`（既定）かつ `whole` があり、バックエンドが 2 件以上返したときだけ、候補ウィンドウを開く最初のキーで sentence モードに入る。`←/→`・文節伸縮・カタカナ化・学習削除のいずれかを行うと `whole` ごと消して二度と sentence モードに入らないようにする（既存の文節候補にフォールバック）。数字キー選択・Space 送りは文節候補と同じ挙動（開いた瞬間に 1 件進む）に揃えた
- **プロトコル変更なし**: sentence モードの候補は `CandidatesView`（結合済み文字列 + `sources: [Llm]`）でそのまま表現できるため、`CandidateView`/typeshare の変更は不要だった
- **既知の限界**: Literal との混在など、入力が複数ピースにまたがる場合は sentence モードを提供しない（`whole` が `None` のまま）。ビームサーチ仮説を `apply_preferences` で個別に上書きした結果、複数の仮説が同じ表記に収束しても再重複除去はしない（まれ・実害は小さいと判断）
- **未実装**: キーマップでの sentence モード専用の割当（現状は既存の Space/矢印キーのみ）、`nagi-eval` でのビームサーチ込み精度計測

### G-35. Phase 0: SudachiDict 対応（辞書内部の pos_id 分離・CSV/matrix.def パーサー、2026-09-28）

実装: `core/dict/src/dictionary.rs`（`Entry.pos_id` / `RawParts.pos_ids`、バイナリ形式 v2）、`core/dict/src/builder.rs`（`add_sudachi_csv` / `set_connection_matrix_def_text` / `build_from_sudachi_files`）、`core/dict/src/pos.rs`（`sudachi_pos_to_mozc_like`）、`core/xtask/src/build_dict.rs`（`--source mozc|sudachi`）。

- **前提が崩れていた設計を修正**: 従来の `Dictionary` は「連接コストID（lid/rid）＝品詞ID」という前提（Mozc/IPADIC系の単純化）で設計されていた。SudachiDict では同一の連接コストIDに平均2600種類以上の異なる品詞文字列が対応しており、この前提が成り立たない。`Entry`/`RawParts`/バイナリ形式に `pos_id` を独立フィールドとして追加し（`pos_defs.len()` と連接コスト表の次元 `conn_n` も分離）、Viterbi・アライメント・逆引きの品詞判定を `pos_id` 経由に統一した。Mozc 辞書では `pos_id = lid` として構築するため挙動は変わらない（既存テストは全て無変更で通過）
- **品詞マッピング**: SudachiのPOS体系（名詞/代名詞/動詞/形容詞/形状詞/助動詞/助詞/接続詞/接頭辞/接尾辞/連体詞/副詞/感動詞/補助記号/記号/空白の16分類）を `sudachi_pos_to_mozc_like()` で Mozc `id.def` 互換の文字列に変換してから `pos_defs` に登録し、既存の `Pos::parse` をそのまま再利用する設計にした（`Dictionary` 側のロジックは無変更）
- **ネットワーク到達性**: 当初 `sudachi.s3-website-ap-northeast-1.amazonaws.com`（CSV原本）・`d2ej7fkh96fzlu.cloudfront.net`（matrix.def、CloudFront配信）ともにブロックされていたが、環境のネットワーク許可リストにドメインを追加してもらい解決（S3側は静的サイトホスティングのため HTTP のみ、HTTPS は SSL エラーになる点に注意）。取得したファイルはユーザーがダウンロードして本セッションに提供
- **実データでのビルド確認**: SudachiDict small_lex.csv（226MB）+ core_lex.csv（410MB）+ matrix.def（514MB、5981×5981密行列）から `cargo run --release -p xtask -- build-dict --source sudachi` でバイナリ辞書を構築できることを確認（構築時間 6.6秒、出力 126.69MB、entries=1,573,640、pos_id_count 相当のconn_n=5981 と matrix.def の次元が一致）
- **CSVパースの実装判断**（実データを見て決定）: (1) Headword（col4）が空の行はIndexForm（col0）を表記として使う（実データの約96%でHeadwordが空。SudachiDictの慣例）。(2) LeftId/RightIdが`-1`の行（実データの約4.5%）は複合語のA/B unit分割専用エントリで単独では格子に出現しないため読み飛ばす。(3) フィールド内の `\uXXXX` Unicodeエスケープ（例 `,` = `,`）はデコードせず生のまま格納（実データで該当は約0.05%、主に数字の桁区切り表記のみで通常の日本語文には出現しにくいため精度への影響は小さいと判断。未対応）
- **重要な発見: SudachiDictはIME変換用途に最適化されていない**。実データで検証したところ、同一読みの表記間で明らかに不自然なコスト順位になっている（例: 読み「ほん」でカタカナ表記「ホン」がコスト2456、漢字表記「本」がコスト6561——カタカナの方が低コスト＝優先される。読み「わたし」でも「ワタシ」がカタカナ表記なのに漢字の「私」より低コスト）。生CSVの時点でこの順位になっており、CSVパーサーやコスト計算の実装バグではない。SudachiDictのコストは「既存テキストを最も自然に分割する」形態素解析（分かち書き）用途に調整されたものであり、「読みから最適な表記を選ぶ」かな漢字変換用途の順位付けは想定されていないと考えられる。この結果、Mozc辞書との `nagi-eval` 精度比較では SudachiDict が構造的に不利になる可能性が高い
- **辞書比較の数値化は見送り**（ユーザー判断、2026-09-28）: 上記の通り SudachiDict はコスト設計の時点でかな漢字変換用途に不利なことが明らかなため、`nagi-eval` での定量比較は行わず、Mozc OSS 辞書の継続採用とする。モデル比較（zenz-v3.1 small/xsmall）のみ次段で実施する

### G-36. Phase 0: モデル比較（zenz-v3.1 small/xsmall）と環境プロファイル不可逆降格バグの発見・修正（2026-09-29）

実装（バグ修正）: `core/profile/src/lib.rs`（`Controller::tick`）。

- **重大バグの発見**: `nagi-eval`（Mozc `evaluation.tsv` 609件）で計測すると、LLM込み（backend=dict+llm）でも辞書のみ（backend=dict）からほぼ改善しない（66.2%→66.5〜66.7%。G-11 記録の「LLM込み76〜86%」から大きく乖離）という想定外の挙動を確認。bug-hunter による調査で、`core/profile/src/lib.rs` の `Controller`（環境プロファイル `quality`/`balanced`/`light`/`minimal` の自動切替）に**不可逆の降格バグ**があることが判明した: `observe_latency` は `current == Minimal` なら即 return してサンプル蓄積を止め、`Minimal` は `use_llm=false` のため LLM 呼び出し自体が止まり `observe_latency` が二度と呼ばれなくなる。`nagi-eval` が609件を間隔なく連続変換する過程で（4コアの検証機では zenz-v3.1-small の1回の変換が60〜150msかかり目標100msを超過）`balanced→light→minimal` と降格し、以降残り約400件が実質 LLM 未使用のまま評価されていた。`balanced` 維持中の最初120件だけでは精度92.5%とLLM自体は正しく機能している。G-11 の記録はこの環境プロファイル自動切替機構が統合される前（`HybridBackend`、プロファイル制御なしでLLM無条件使用）に取られた数値で、統合後に再検証されていなかった
- **修正**: `Controller::tick` に、レイテンシ由来で軽くなっている（`latency_steps > 0`）間は保持時間（`upgrade_dwell_ms`、既定60秒）ごとに1段ずつ試しに戻す定期プロービングを追加。まだ遅ければ次の `observe_latency` で速やかに再び下がる。環境由来（バッテリー等、`env_steps`）の降格には影響しない。既存テストは無変更、新規テスト `minimal_from_latency_recovers_via_tick_probing` で `Minimal` から2段階回復することを確認
- **モデル比較の結果**（Mozc辞書、`nagi-eval` 609件、修正後の値）:

  | 構成 | TOTAL | p95レイテンシ |
  | --- | --- | --- |
  | 辞書のみ（LLM無し） | 66.2% (403/609) | 0.1ms |
  | LLM込み・zenz-v3.1-small（73.8MB） | 66.3% (404/609) | 141.0ms |
  | LLM込み・zenz-v3.1-xsmall（21MB） | 68.5% (417/609) | 27.5〜37.5ms |

  xsmall は small よりモデルサイズが1/3以下ながら精度が同等以上、レイテンシは1/4程度。この検証機（4コア）では small の1回の推論が60〜150msかかり `balanced` プロファイル（目標100ms）を維持できず頻繁に降格するのに対し、xsmall は軽量なため常に `balanced` を維持できていることが要因と考えられる。より強力なハードウェア（ターゲット M1/8GB 等）では結果が変わる可能性があるため、この数値はこの検証環境限定の参考値
- **暫定結論**: 現時点のデータでは xsmall が有力候補。ただし (1) 検証機のハードウェアがターゲット外である点、(2) `nagi-eval` が `left_context=""` で呼ぶため LLM の文脈活用効果を十分に測れていない可能性がある点、(3) 辞書のみとの差が両モデルとも小さく評価セット・評価方法自体の感度が低い可能性がある点、を踏まえ、最終的なモデル確定は Phase 0 の他の検証（ターゲットハードウェアでの実測等）と合わせて判断する
- **未実施**: ターゲットハードウェア相当での再計測、`left_context` を使った評価、`docs/decisions.md` の変換モデル最終決定（現時点では暫定）
