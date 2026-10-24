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
- 配布: Apple Developer Program には加入しない【2026-10-14: 加入済み。iOS は TestFlight / App Store で配布（H-15）。macOS の配布方針は見直し可能】
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
| Composing | 挿入 | → Converting（候補窓なし、focus 0） | - | 確定 → Direct | ライブ変換の表示中は変換だけ破棄してひらがな表示に戻る（G-45）、ひらがな表示なら破棄 → Direct | 削除、空なら Direct（変換表示は破棄、ライブ変換を抑制。G-46） | カーソル移動 | → Converting（カタカナ/ひらがなを選択） | 確定 → Direct、モード更新 | 確定 → Direct、consumed=false | 消費のみ |
| Converting | 確定して Composing で新規入力 | 次候補（窓表示）| 前候補 | 確定 → Direct | → Composing（reading 復元、ライブ変換は抑制。G-45/46） | → Composing（reading 復元、削除なし、ライブ変換の表示も破棄。G-44） | MoveFocus / ResizeSegment / ExtendSelection（NoOp） | 注目文節を変更 | 確定 → Direct、モード更新 | 確定 → Direct、consumed=false | 消費のみ |

- `SelectCand(n)`: 候補窓表示中のみ現在ページの n 番目を選択（確定はしない・窓を閉じる）。非表示なら「確定して数字を新規入力」。Mozc / MS-IME は選択と同時に文節を確定するが、部分確定（Fixed）は Phase 3 なので、今回は選択だけとした
- `RangeSelecting`（Ctrl+Shift+←/→）と `StyleAction` は enum に宣言のみ。Converting での `ExtendSelection` は消費して何もしない
- Composing の Enter は「表示中の surface」を確定する。ライブ変換の結果が反映されていればその変換結果（追記中は固定部分 + 追記のかな。G-43）、無ければ reading（G-39 で変更。当初はライブ変換 OFF 前提で reading 固定だった）
- Composing の Backspace / DeleteForward は、表示中のライブ変換（固定部分を含む）をすべて破棄して reading 表示に戻してから削除し、次に文字を入力するまでライブ変換を再開しない（G-46）。文中挿入・Cancel・新規入力の開始でも破棄する（G-43）
- Converting で Left / Right は端で止まる（循環しない）。候補は先頭が現在の表記、次にバックエンド候補、末尾にひらがな・カタカナ（重複除く）
- 入力中に未割当の Ctrl/Alt/Meta 付き文字キー（Cmd+C など）は、確定してからキーを素通しする（入力を失わない）
- 「文全体の N-best」（`conversion.initial_view = sentence`）は未実装。候補窓は注目文節の候補のみ

### G-8. ローマ字・入力バッファの判断

- **【G-48 で改訂: `nn` は保留なしの「ん」に変更。以下の soft pending の記述は旧設計】** `nn` は「ん」+ soft pending `n`。直後が母音・`y` なら結合する（`konnichiha`→こんにちは、`shinnyuu`→しんにゅう、`kanna`→かんな）。確定時・後続が他の文字のときは捨てる（`honn`→ほん、`nn`→ん）。soft の `n` は preedit の pending 表示に含めない。Mozc の標準テーブルの挙動との一致は未確認（`nn` を即確定する実装との差に注意）
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
- **レビュー完了（2026-09-29）**: 4項目とも実装がこの記述通りであることをコード上で確認した（GPU検出: `core/profile/src/lib.rs:136,151-166`。スレッド数削減: `core/infer/src/lib.rs:863-868`・`core/server/src/runtime.rs:472-507`。モデル解放: `core/infer/src/lib.rs:377-393,872-883`・`core/server/src/pipeline.rs:149-176`（`reconcile_active`）。キャリブレーション永続化: `core/server/src/calibration.rs:8-46`）。各項目にテストもある（`detect_gpu_returns_a_bool`、`test_llama_model_set_active_reloads`、`calibration.rs` の round-trip テスト等）

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
- **後の決定**: H-17 の固定プロファイル比較で small が xsmall より上と訂正した。iOS の既定モデルは small（H-22(c)）
- **未実施**: ターゲットハードウェア相当での再計測、`left_context` を使った評価、`docs/decisions.md` の変換モデル最終決定（現時点では暫定）

### G-37. macOS の bundle id を `com.shiki.inputmethod.Nagi` に変更（2026-09-30）

- **経緯**: 当初の `com.shiki.nagi` でバンドルを作り `~/Library/Input Methods` に置いて登録しても、TIS（Text Input Sources）が入力ソースとして認識しなかった（該当0件）。Karukan など実在の IME の bundle id は `.inputmethod.` を含んでいる
- **実機検証**: `.inputmethod.` を含む `com.shiki.inputmethod.Nagi` で bundle を作ると、登録・有効化に成功した（CI で両 ID の bundle を作って比較）
- **決定**: IME の bundle id は `com.shiki.inputmethod.Nagi`。モード ID は `<id>.Japanese`、IMKServer 接続名は `<id>_Connection`（`cargo xtask bundle-macos --bundle-id` の既定値）
- **変更しないもの**: 設定/データの識別子 `com.shiki.nagi`（`~/Library/Application Support/com.shiki.nagi`、core の設定ディレクトリ名）。製品のデータ識別子であり、bundle id とは別物として維持する。フロントの内部 DispatchQueue ラベルも同様

### G-38. imserver のモデル読み込みを非同期化（2026-09-30）

- **背景**: `setup::build()` が GGUF の読み込みを同期的に行い、`initialize` の応答が読み込み完了後になっていた。macOS フロントは `initialize` を 10 秒で打ち切って再起動するため、コールド時（読み込みに約30秒以上）にキーが素通しになり、再起動ループの末に minimal で起動していた。「キー入力を待たせない」「コールドスタート < 2s」に反する
- **方針**: `ServerOptions::async_model_load`（既定 false。`imserver` の `main` のみ true）が真なら、`build()` は辞書だけで即座に返る（LLM なし＝`minimal` 相当）。モデルの読み込みは別スレッド（`nagi-model-load`）で行い、完了後に `Pipeline::attach_model` で取り付ける。`Pipeline.llm` は `OnceLock<Llm>`（一度だけ設定）。取り付け後は `Runtime::set_llm_loaded(true)` でプロファイルを再判定し（既存の `profile_changed` 通知）、続けて既存の `warm_up()`（ウォームアップ＋キャリブレーション）を開始する。読み込み中の変換は辞書のみ（LLM を待たない）。読み込み失敗（`Err`）は辞書のみのまま動作し、理由を stderr に日本語で出す（入力内容は出さない）
- **キャリブレーション記録**: 非同期時は、LLM ありの前提で `calibration.json` を照合し、一致すれば実測値を `set_calibration_hint`（判定・保存はしない）で置く。`attach_model` 時の判定が一度で確定し、プロファイルのちらつき（minimal → 仮判定 → 実測判定）を減らす
- **ログ**: stderr に `imserver: モデル読み込み完了 (N ms)` と `imserver: ウォームアップ/キャリブレーション完了 (N ms)`。同期版（`nagi-eval`）にも出る
- **テストの継ぎ目**: `ModelLoader`（`Box<dyn FnOnce() -> Result<Box<dyn ConversionModel>, String> + Send>`）と `build_with_loader(options, loader)`。`build(options)` は `loader = None`（既定の GGUF ローダー）。`server/tests/async_load.rs` が feature `llama` なしで、遅いローダーでの即時応答・完了後の LLM 経路とプロファイル通知・失敗時の辞書のみ動作を検証する
- **影響**: `nagi-eval`・既存テストは同期版のまま（`async_model_load = false`）で挙動不変。読み込み完了までの間（コールドで数十秒）は辞書のみの変換になり、完了時にプロファイルが minimal から上がる。読み込み中のスレッドが残っている間にプロセスが終了しても、状態の破棄で問題は起きない（ファイルは書かない）
- **未対応**: 読み込み完了を待たずに終了した場合の読み込みスレッドの中断（GGUF 読み込み中は中断できない）。読み込み失敗の `notify`（フロントへの通知）は出していない（stderr のみ）

### G-39. ライブ変換中の Enter は表示中の変換結果を確定する（2026-09-30）

- **不具合**: ライブ変換で変換結果が表示されていても、Composing の Enter（`commit_all`）は Composer の reading（ひらがな）を確定していたため、Space で Converting にしないと変換結果が確定できなかった。画面の表示と確定内容が食い違っていた
- **修正**: `commit_all` の Composing 分岐で、`build_preedit` と同じ条件（`LiveConversion.source_reading` が現在の reading と一致）のときだけ、ライブ変換の segments の surface を確定する。一致しない（LLM 待ちで未反映・反映後にさらに入力した）ときは表示も reading なので reading を確定する（表示と確定が常に一致する）。`commit_all` を経由するすべての確定（Enter・モード切替・CommitAndPassThrough・focus_out・restore）に同じ規則が効く
- **学習**: 変換結果を確定した場合は Converting 確定と同じく `notify_commit` を呼ぶ（`notify_commit` は `&[Segment]` を受ける形にした。文節ごとの reading / surface / 左文脈は既存経路と同じ）。reading の確定（未反映）では従来どおり通知しない
- **保留ローマ字**: 単独の `n` が表示されたまま Enter した場合、flush で末尾に付く「ん」を変換結果の後ろに足す（表示 `蚊n` → 確定 `蚊ん`）。カーソルが途中で flush 分が途中に入る場合は対応を取れないので reading で確定する（未対応の端ケース）
- **確定取消**: `last_commit.pieces` は従来どおり reading から作るので、Ctrl+Backspace は変換結果ではなく reading に戻る（Converting 確定後の取消と同じ扱い）
- **テスト**: `engine/tests/golden/110_live_conversion.yaml`（golden に `action: live_convert` / `reading:` を追加）、`engine/tests/api.rs`（学習フック・取消）
- **未確認**: macOS 標準のライブ変換との詳細比較（Enter 以外の Shift+Enter 等）は未実施

### G-40. 「ん」の重複（`nn` + 子音）の原因と修正（2026-09-30）

- **症状**: 「ん」の直後（確定前）に次の文字を打つと「んん」になる
- **原因（コア側の `nagi-romaji`）**: `nn` は「ん」を emit して soft pending の `n` を残す（G-8）。この soft の `n` は、次が母音・`y` なら結合し（`nna` → んな）、それ以外では捨てる設計だったが、`RomajiConverter::feed` は soft の `n` に構わず次の文字を足していた。`nk`・`nj` 等はテーブルに「ん + 保留 k」のエントリがあるため、`nn` + `k` → `nk` として「ん」が再度 emit され「んんk」になった（`nn` + `t` / `kan` の後の `nn` → `j` なども同様）。engine 側（ライブ変換の reading は保留を含まない Piece 列）には二重処理は無く、Composer の出力自体が誤りだったので、フロントの marked text 再描画の問題ではない。修正前の engine でも `nnk` の preedit が「んんk」になることを確認済み
- **修正**: `feed` の冒頭で、soft pending 中に母音・`y` 以外が来たら soft の `n` を捨ててから通常処理する。`n'` は soft の `n` を捨てて `'` を消費する（`nn'` が「んん」や `'` の Literal にならない）。soft は結合を試みる時点で解除する（`nny` の `ny` は通常の保留として表示。従来は soft のまま `ny` が表示から隠れていた）。`konnichiha`・`hannou`・`onna`・`nnn` 系は従来どおり
- **Mozc との対応**: `nn` → ん、`n` + 子音 → ん + 子音、`n` + 母音 → な行、`n'` → ん。`nnn` は「んん」（2つ目の soft を捨て、3つ目の `n` を新規の保留として扱い、flush で「ん」）。progress.md の「`nn` の soft pending は Mozc と一致するか未確認」は、上記の範囲で解消（Mozc 実機との照合は未実施）
- **テスト**: romaji の `test_n_sequences_do_not_duplicate_nn`、engine の `n_input_never_duplicates_nn_in_composing_reading`、golden の `10_romaji.yaml`（`romaji-nn-*`）と `110_live_conversion.yaml`（`live-soft-n-*`）

### G-41. GUI 設定 RPC はスキーマ駆動、`config.toml` の書き込みは `toml_edit`（2026-09-30）

- **判断**: 設定画面向けに `get_settings` / `update_settings` を追加した。コアが設定項目を「記述子」（`key` はドット区切りの TOML パス、`group` / `label` / `description`、型 `SettingKind`（`bool` / `int{min,max,step}` / `float` / `choice{options}` / `text`）、現在値・既定値・`explicit`・`requires_restart`）として返し、フロントは汎用フォームを描くだけにする。値の検証（型・範囲・選択肢）と書き込みはコアが行い、不正な項目だけエラーにして他は適用する。適用後は既存のホットリロード（`reload_all`）を即時に呼ぶ
- **理由**: 「ロジックをフロントに持ち込まない」原則（設計書 §3.1）のとおり、項目・範囲・既定値の知識をフロントに複製しない。項目の追加・範囲の変更がコアだけで済み、Windows 版（Phase 5）の設定画面も同じ記述子を使える。検証をコアの一箇所に置くので、手編集とGUIで基準が食い違わない
- **`toml_edit` の採用**: `config.toml` はユーザーが手で書くコメント・未知のキー・`[keymap.*]` を含むため、`toml`（0.8）の再シリアライズでは失われる。`toml_edit`（0.22。既に `toml` の依存で、純Rustなのでモバイルでもビルドできる）でキーの直前コメント・行末コメント・書式を保って値だけを書き換える。依存の追加は `nagi-config` に限定した。書き込みは同じディレクトリの一時ファイルへ書いてから rename（原子的）。ファイルが無ければ空の文書から作る（`default.toml` の全体は複製しない）
- **値の表現**: `SettingValue` は `{"type":"bool|int|float|text","value":…}`、`SettingKind` は `{"type":"int","params":{min,max,step}}`（typeshare が Swift の enum を生成できる隣接タグ形式）。選択肢の値は `text`。「未指定でプロファイルが決める」項目（`conversion.beam_width`）は `value` / `default` が `null`（`update_settings` で `null` を送ると明示指定を削除）
- **範囲の重複**: 記述子の範囲は `Config::validate` の範囲と同じ値を手で揃えている（二重管理。将来 `validate` 側を記述子から引く形に統合できる）。記述子の既定値は `Config::default()` から取り、`default.toml` との一致をテストで検査する
- **`conversion.use_llm` を新設**: 「LLM 推論の有効/無効」を GUI から切り替える設定キーが無かったため追加した（既定 true。false なら辞書のみ。`Tunables.llm_enabled` → `Effective.use_llm`）。`--no-llm` は従来どおり起動時にモデルを読まない指定で別物
- **壊れた設定ファイル**: TOML の構文が壊れているときは、書き込まず全項目をエラーにし、`get_settings` は既定値と `error` を返す（ユーザーの手書きの設定を上書きで壊さない）
- **`requires_restart`**: 現状で真なのは `update.auto_check`（起動時にだけ確認するため）のみ。他はホットリロードで反映される
- **未対応**: `--sync-budget-ms`（CLI）で上書きされている間、`conversion.sync_budget_ms` の実効値と設定値がずれる（GUI は設定値を表示）。`ranking.source_order`・`keymap.*`・`learning.disabled_apps`・`conversion.model` などの複雑な項目は対象外

---

### G-42. ローマ字標準テーブルの欠落を Mozc 原本との機械照合で修正（2026-09-30）

- **症状**: 実機で `cya` が「ちゃ」にならない
- **原因**: `config/romaji/standard.tsv` を手書きで作った際、Mozc の標準テーブルを網羅しなかった。`cya`・`cye` のほか、`kyi`/`kye` 等の `yi`/`ye` 系、`qa`〜`qo`、`kwa`/`gwa`/`swa`/`zwa` 系、`tsa` 系、`twa`/`dwa` 系、`wha` 系、`hwa` 系、`fya` 系、`vya` 系、`xka`/`xke`、`wu`/`ye`、`qq`/`ll`/`xx`/`yy`/`tch`、`xn`、`t'i` 系など計 103 エントリが欠けていた（`cyi`/`cyu`/`cyo` だけ「Ritsuryou-compatible」節に入っていて `cya` だけ漏れていた）
- **照合方法**: Mozc の原本（`src/data/preedit/romanji-hiragana.tsv`、`cargo xtask fetch-dict` と同じ commit `b9c3fcb` の raw を curl で取得。323 行）と、キー→(出力, 保留) の辞書同士で機械的に差分を取った。取得は xtask には組み込んでいない（テーブルは同梱ソースなので、今回は一度だけ照合）。結果: 欠落 105 件、出力が異なるのは `nn` のみ（Nagi は soft pending 用に保留 `n` を持つ。G-8/G-40）
- **対応**: 欠落のうち `n`（単独 `n` は flush で「ん」にする既存の設計）と `www`（`ww` の確定を 1 キー遅らせてしまう）を除く 103 件を追加した。Nagi 独自の `n` + 子音（`nk` 等）は維持し、Mozc の `n` 系に合わせて `nq`/`nl`/`nx` を追加した。`/ ? ! ( )` は Mozc の TSV に無く Nagi 独自（従来どおり）。Mozc の `n` と `www` は未採用
- **ライセンス**: テーブルはリポジトリ内のソースなので、`NOTICE` に Mozc（BSD-3-Clause、Copyright Google Inc.）の帰属とライセンス全文を追記した。従来の「ローマ字テーブルは自作」（G の初期判断）は、標準テーブルは Mozc 由来（自作部分を含む）に改める
- **テスト**: romaji の `MOZC_CASES`（c 系・拗音・小書き・外来音・促音・撥音・記号・z 系を約 300 件）、`test_cya_row_explicit`、テーブルの形式検査（出力が空でない・保留の整合・重複キー）。Mozc 実機との挙動照合は未実施（テーブル内容のみ照合）

## H. モバイル対応（2026-09-29）

ユーザー指示「スマホ版の対応を視野に入れる。実装はまだ不要」に基づく。詳細は `mobile-design.md`。

### H-1. 決定事項

- iOS / Android は非目標から外し、「v1 では実装しないが将来対応する前提で設計する」に変更する
- デスクトップ開発中から `mobile-design.md` §3 の制約を守る（コアのライブラリ完結、パス・プラットフォーム情報の注入、stdio / `std::process` 非依存、モバイルでビルドできる依存）
- JSON-RPC スキーマはモバイルでも共通とし、トランスポート（stdio / FFI 関数呼び出し）のみを差し替える

### H-2. 暫定判断（要レビュー）

- Android 先行、iOS 後続（理由: 子プロセス制約・メモリ上限・配布障壁がいずれも Android の方が小さい）
- **後の決定**: H-15（TestFlight / App Store 配布への変更）以降、実際は iOS を先行している（`docs/mobile-roadmap.md` §0）
- FFI バインディングは UniFFI を第一候補（Swift / Kotlin を同時生成できるため）。代替は手書き C ABI + cbindgen
- 12キーのトグル・フリック方向の解決はコアで行う（ロジックをフロントに持ち込まない原則）

### H-3. 未決事項と決定（2026-09-30）

- **タッチ入力のプロトコル表現**（決定済み）: 案A採用。`insert_reading` / `perform_action` を新設し、タッチ由来の入力はこの系統で渡す。物理キーは従来どおり `process_key`。`process_key.kind` は拡張しない。12キーのトグル・フリック解決はコア側（H-2 のとおり）で行う
- **辞書形式の mmap 対応**（決定済み）: 前倒しする。デスクトップ v1 のうちに、辞書ファイル形式を mmap 可能（アラインメント・オフセット表現）にしておく。読み込み実装の mmap 化自体は形式対応後でよい。現状は `core/dict/src/dictionary.rs:203` の `std::fs::read` による全量読み込み。ユーザー辞書の `core/dict/src/user.rs:153-176` の read_dir 走査も、配置の注入に寄せる余地あり（任意の改善として併記）
- **iOS の LLM 可否**（決定済み）: 実測を Phase M0 の最優先項目とし、早期に着手する。実測が出るまで iOS の品質基準は `minimal`（辞書+Viterbi）。Phase M1 以降は実測結果に基づいて判定する

### H-4. 追加決定（2026-09-30）

> 【H-15 で注記】下の「SideStore 検証」は H-15 により不要になった（iOS は TestFlight / App Store で配布する）。iOS の FFI は UniFFI ではなく C ABI で実装済み（H-10）。以下は当時の決定の記録として残す。

- **SideStore 検証**: entitlement / App Group の維持（再署名後）、無料 Apple ID の制限の数え方、更新時の URL scheme 対応を、デスクトップ作業と独立に早期に実施する。H-2 の「FFI バインディングは UniFFI」の前提で、Swift のバインディングとして同時に検証する
- **FFI 方式**: UniFFI で確定（H-2 の第一候補を確定に格上げ）。確認事項として以下を実装時に計測して決める旨を添える: (1) Preedit 等のネスト構造を毎キー渡すコスト、(2) 非同期通知（revision 付き差し替え）の callback 表現、(3) 描画差分を JSON 文字列か UniFFI 型で渡すか（p95 < 16ms 目標への影響）

### H-5. 配布・更新・アプリ仕様（2026-09-29 ユーザー決定）

- iOS は SideStore によるサイドロードで配布する（Apple Developer Program 非加入の方針を維持）【H-15 で TestFlight / App Store に変更】。更新は AltStore 互換のソース JSON を `shiki-01.com` に置いて配信する
- Android は署名済み APK の直接配布とする（公式ストアは使わない）
- 両 OS とも本体アプリに更新確認を入れる。データ資産（辞書・モデル・プリセット）は再インストールなしで更新できるようにする。アプリ本体は Android が `PackageInstaller` で上書き、iOS は SideStore の更新へ誘導する（iOS は署名済みでないコードを実行できないため）
- アプリ仕様（キーボード UI・本体アプリの構成・設定項目）は azooKey を参考にする。変換エンジン本体は参考にしない（F の方針どおり）
- 詳細は `mobile-design.md` §5.0・§6

### G-43. ライブ変換: 追記中は変換結果を固定して表示し、末尾だけ再変換する（2026-09-30）

- **背景**: ライブ変換の表示条件が「`source_reading` と現在の reading の完全一致」（G-25 / G-39）だったため、変換結果の表示後に1文字打つたびに preedit 全体がひらがなに戻り、デバウンス後の結果でまた変換表示に戻る、というちらつきが実機で出た
- **方針**: 現在の reading が `source_reading` を接頭辞として持ち（末尾への追記）、かつカーソルが末尾なら、既存のライブ変換結果を「固定部分」として保ち、追記分を後ろに未変換の segment（かな・Literal は piece の種類どおり。保留ローマ字は従来どおり `pending`）として足して表示する（`Engine::live_view`）。全体をひらがなに戻さない。`reading` が真実という不変条件は、固定 segment の reading 連結 + 追記 segment の reading = 入力全体の reading で保たれる
- **末尾のみ再変換は打鍵中の暫定表示にとどめる（G-47 で更新）**: 当初は `live_convert` が固定部分を再変換せず追記分だけを変換して連結していたが、これだと固定部分の文節区切り・変換内容が最初の変換時のままになり、Space の全文変換（AI 変換）と食い違う不具合になった。現在は、打鍵中（デバウンス満了前）は「固定部分 + 追記のかな」の暫定表示を保つだけで、デバウンス満了時の `live_convert` は reading 全体を（`composing_chunk_len` 分割は既存どおり）LLM 込みで再変換し、その結果で表示を置き換える（変換→変換の入れ替えなのでひらがなには戻らない）
- **Backspace**: 表示中の変換（固定部分を含む）をすべて破棄して reading 表示に戻し、通常の Backspace として最後の1文字（保留ローマ字があればその1つ）を削除する。デバウンス後のライブ変換の再開は G-46 で抑制するようにした（次の文字入力まで再開しない）。DeleteForward も同様
- **文中編集のフォールバック**: カーソルが末尾でないときの挿入は変換結果を破棄する。カーソルが末尾でない状態では追記の固定表示も使わず、従来どおり reading 表示（完全一致のときのみ変換表示）。文中の部分再変換は範囲操作（Phase 3）で扱う。Cancel と新規入力の開始でも古い結果を持ち越さない
- **古い結果の破棄**: `live_convert(expected_reading)` は現在の reading と一致しなければ何もしない（従来どおり）。表示側は `source_reading` が現在の reading の接頭辞として一致する範囲だけ固定部分（暫定表示）に採用し、不整合なら reading 表示にする
- **確定（G-39 との関係）**: Enter は固定部分 + 追記のかなを、表示どおりに確定する。学習通知（`notify_commit`）は変換済みの固定部分のみ。追記分は未変換のかななので学習しない。確定取消は従来どおり reading に戻る
- **影響**: 既存 golden の2ケース（`live-enter-after-more-input-commits-what-is-displayed`、`live-soft-n-then-consonant-no-duplicate`）の期待値を、追記後もひらがなに戻らない仕様に合わせて変更。追加テストは `engine/tests/golden/110_live_conversion.yaml` と `engine/tests/api.rs`

### G-44. 変換中（Converting）の Backspace は「変換の取り消し」、ライブ変換の表示も破棄する（2026-09-30）

- **背景**: 「Space で変換した後に Backspace を打つとひらがなに戻り、その後 Space でもう一度 AI 変換に入る」という要望。Converting の Backspace は既に `Cancel` にマップされ、reading を Composing に復元する（文字は削除しない）挙動だったが、直前に ライブ変換（G-43）の結果が残っていると、Composing に戻った直後に `source_reading` が一致して変換表示に戻ってしまい、「ひらがなに戻る」になっていなかった
- **Backspace の2段階**: (1) Converting の Backspace（Ctrl+h も同じ）= 変換の取り消し。Composing のひらがな表示に戻るだけで文字は削除しない（標準 IME と同じ）。ライブ変換の固定表示も破棄する（`back_to_composing` で `live_conversion` を捨てる。Escape も同じ関数なので同様にひらがな表示に戻る）。(2) Composing の Backspace は従来どおり（G-43: 変換表示を破棄して1文字削除）
- **再度の Space**: `start_converting` の冒頭でも `live_conversion` を捨てる。Space 変換は常に `convert_sentence` を新しく呼ぶので、AI（LLM）を含む通常の変換になる（サーバー側の LLM 結果キャッシュはキー（左文脈・読み）一致時に LLM 結果を返すだけで、辞書のみの結果には固定されない。モデル未ロード時は辞書のみ）。デバウンス後のライブ変換が再開して変換表示に戻っていても、Space は Converting に入り先頭候補は新しい変換結果になる
- **予測変換**: 現状、予測変換（`predict`）は Composing での Tab のみが入口で、Converting 中の Space（NextCand）は通常候補（現在の表記・バックエンド候補・ひらがな・カタカナ）を循環するだけで、予測候補には移らない。「通常候補の後に予測候補へ移る」は未実装で、`predict.min_chars` などの仕様は変えていない（当面は現状の仕様で確定。ユーザー確認済み 2026-09-30。将来必要になれば別途検討）
- **状態遷移表（G-7）**: Converting × Backspace は「→ Composing（reading 復元、削除なし、ライブ変換の表示は破棄）」
- **影響**: `engine/src/engine.rs`（`back_to_composing`・`start_converting`）。追加テストは `engine/tests/golden/40_convert.yaml`（`convert-backspace-cancels-then-space-converts-again`、`convert-backspace-cancel-then-backspace-deletes-one`）と `engine/tests/api.rs`（`converting_backspace_cancels_and_discards_live_view`）

### G-45. Esc の状態別挙動（2026-09-30）

- **不具合**: Composing で Esc を押すと、ライブ変換の結果が表示されていても入力全体（reading）が破棄されていた（`apply_composing` の `Cancel` が無条件に `composer.clear()`）。標準 IME / Mozc の「ひらがな表示で Esc = 破棄」を、ライブ変換の表示中にも適用してしまっていた
- **挙動**: Composing でライブ変換の変換結果が見えている（`live_view` が有効で、固定部分に surface と reading が異なる文節がある。追記中の固定表示 + 追記も含む）→ 変換だけ破棄して reading（ひらがな）表示に戻す（削除しない）。ひらがな表示（変換なし、または変換結果が reading と同一）→ 従来どおり入力全体を破棄して Direct。Converting → Composing の reading に戻る（削除しない。既存）。RangeSelecting（Converting の `range_anchor` あり）→ まず選択だけ解除、次の Esc で Composing に戻る（既存）
- ライブ変換を破棄した Esc の後は、G-46 のとおり次の文字入力までライブ変換を再開しない（再開すると Esc の効果が打ち消されるため）
- **影響**: `engine/src/engine.rs`（`apply_composing`・`live_displays_conversion`）。テストは `engine/tests/api.rs`（`esc_*`）と `engine/tests/golden/110_live_conversion.yaml`（`live-esc-discards-conversion-then-clears`）。G-7 の表を更新

### G-46. Backspace 等の後はライブ変換を次の文字入力まで抑制する（2026-09-30）

- **不具合**: G-43/G-44 で Backspace は変換を破棄してひらがなに戻すが、Backspace 後のデバウンス満了で `live_convert` が再び走り、すぐ変換表示に戻っていた
- **修正**: `Session.live_suppressed` を追加。Composing の Backspace / DeleteForward、ライブ変換表示中の Esc、Converting → Composing の取り消し（Esc・Backspace。`back_to_composing`）で立て、文字入力（`insert_char`）で下ろす。抑制中は (1) `composing_reading` が `None` を返す（サーバーは debounce スレッドを立てない）、(2) `live_convert`（デバウンス満了時の照合・変換）が何もしない、(3) 非同期の LLM 遅延完了（G-47 の `replace_live`）を反映しない。Space 変換・Enter 確定・Tab（予測）など明示操作には影響しない（Enter は抑制中は reading を確定する）
- カーソル移動（←/→/Home/End）では解除しない（「次に文字を入力するまで」）
- **影響**: `engine/src/engine.rs`。テストは `engine/tests/api.rs`（`backspace_suppresses_*` 他）、`engine/tests/golden/110_live_conversion.yaml`（`live-suppressed-after-backspace-until-next-input`）、`server/tests/rpc.rs`（`live_conversion_is_not_delivered_after_backspace_until_next_input`）

### G-47. ライブ変換は全文再変換に収束させる・LLM 遅延完了の取り込み・cursor は surface 基準（2026-09-30）

- **不具合**: G-43 以降、ライブ変換の表示が Space の全文変換（AI 変換）と食い違い、一度 Space を押さないと適切な変換にならなかった。原因は2つ: (1) 追記後の `live_convert` が末尾だけの変換で、固定部分の内容・区切りが最初の変換時のまま残った。(2) ライブ変換のバックエンド呼び出しが同期予算（`sync_budget_ms`）を超えると辞書寄りの結果で表示され、その後 LLM が完了しても、非同期の差し替え（`apply_completions`/`Origin`）は Converting にしか働かないため、Composing のライブ変換には最終結果が反映されなかった（Space を押すと LLM 結果のキャッシュが効いて初めて正しい結果が出る）
- **修正 (a)**: `live_convert` は常に reading 全体を（`composing_chunk_len` 分割で）変換し直し、結果で表示を置き換える。打鍵中は G-43 の固定部分 + 追記かなの暫定表示。G-43 の記述を更新済み
- **修正 (c)**: `LiveConversion.chunks` に変換したチャンクの `(reading, left_context)` を持ち、`apply_completions` が Composing のセッションも対象にする。完了通知が表示中のチャンクと一致し、入力が変換時から変わっていない（reading 一致・非抑制）場合に全文を変換し直して差し替える（`replace_live`）。入力が変わっていれば、デバウンス後の `live_convert` が全文を変換するので何もしない。`live_convert` 側の `expected_reading` 不一致による破棄は、古い入力への結果を捨てる正しい動作で変更なし（サーバーは打鍵ごとに debounce スレッドを立てるので、最後の入力に対する結果は必ず届く）
- **修正 (b)**: プロトコルの `PreeditView.cursor` は `text`（surface 連結）上の UTF-16 位置。engine の `Preedit.cursor` は reading 基準（segment + reading 内の文字数）で、`to_view` が reading の文字数をそのまま surface の先頭から数えていたため、surface が reading と異なる文節（ライブ変換・変換結果。特に surface が reading より長いとき）で cursor が文節の途中にずれていた。`to_view` で、surface が reading と異なる文節では cursor を文節の先頭か末尾（reading 上の位置が半分未満なら先頭）に寄せるようにした（surface と reading が同一の文節は文字数どおり）。Swift（`NagiInputController.renderPreedit`）は `preedit.cursor` を UTF-16 の `selectionRange` にそのまま使っており、定義と一致しているので変更なし
- **未対応**: ライブ変換表示中に ←/→ でカーソルを動かしたとき、変換済み文節の途中には止まれず端に寄る（表示上のみ。reading 上のカーソルは正しく動く）。全文再変換は毎回チャンク全体を変換するので、長文でのレイテンシは未計測（G-27 の最適化は依然として未実装）
- **影響**: `engine/src/engine.rs`（`convert_live`・`replace_live`・`try_apply_live_conversion`）、`engine/src/ir.rs`（`to_view`）。テストは `engine/tests/api.rs`（`live_convert_after_append_*`・`completion_replaces_live_conversion_*`・`live_*cursor*`）と golden 110。既存 golden の `live-append-*` 2 ケースと `api.rs` の `live_append_reconverts_only_tail_*`（`live_convert_after_append_reconverts_whole_reading_*` に置換）は、末尾のみ再変換 → 全文再変換の仕様変更に合わせて期待値を変更

### G-48. `nn` は保留なしで「ん」を確定する（Mozc 原本どおり。G-8 の soft pending を廃止）（2026-09-30）

- **背景**: 実機で「nnei」が「んねい」になった。ユーザーの期待は「んえい」（`nn` で「ん」が確定し、続く `ei` が「えい」）。G-8 の設計は `nn` を「ん」+ 保留 `n`（soft pending）とし、直後の母音・`y` を保留 `n` と結合させていた（`konnichiha`→こんにちは の目的）が、これが上記の症状の原因
- **決定**: ユーザーの明示要望を優先し、Mozc 標準（`romanji-hiragana.tsv` の `nn	ん`、保留なし。G-42 の照合で唯一の相違点だった）に揃える。`config/romaji/standard.tsv` の `nn` の保留 `n` を外し、`nagi-romaji` の soft pending の機構（`soft_pending` フラグ、feed 冒頭の捨て処理、flush・backspace の特別扱い）を削除した。単独 `n` の保留（表示は pending `n`、flush で「ん」）、`n`+子音（`nk`→ん+保留 k）、`n'`→ん、`xn`→ん は従来どおり。G-40 の「ん」重複は、`nn` が保留を残さなくなったため構造的に起こらない（`n`→n、`nn`→ん、`nnk`→んk、`konn`→こん、`konni`→こんい など、`test_n_sequences_do_not_duplicate_nn` で確認）
- **トレードオフ**: G-8 の目的だった「`konnichiha` を素直に打てる」を失う。`nn` の直後の母音は独立した音になる。「んな」「んに」等を続けたいときは `nnn`（`n` を3つ）か `n'` を使う。Mozc・MS-IME でも同じ運用で、`konnnichiha` / `kon'nichiha` と打つのが一般的（macOS 標準 IME の `nn`+母音の挙動は実機未確認。ここでは Mozc 原本のテーブルを根拠とした）
- **影響する典型語（変更前 → 変更後）**:

  | 入力 | 変更前 | 変更後 | 変更前の結果を得る打ち方 |
  | --- | --- | --- | --- |
  | nnei | んねい | んえい | nnnei |
  | nna | んな | んあ | nnna |
  | konnichiha | こんにちは | こんいちは | konnnichiha / kon'nichiha |
  | hannou | はんのう | はんおう | hannnou / han'nou |
  | onna | おんな | おんあ | onnna / on'na |
  | kannei | かんねい | かんえい | kannnei |
  | kanna | かんな | かんあ | kannna |
  | shinnyuu | しんにゅう | しんゆう | shinnnyuu / shin'nyuu |
  | honn / nnk | ほん / んk | 変わらず | |
  | n' / xn | ん | 変わらず | |

  末尾の `nn`（`honn`、`kouenn` 等）や `nn`+子音は影響なし。`nn` の後に母音・`y` が続く語だけが変わる
- **テスト**: `nagi-romaji` の `test_n_sequences_do_not_duplicate_nn`・`MOZC_CASES`（nn 行）・`test_konnichiha` 等、engine の golden `10_romaji.yaml`（`romaji-nn-*`、`romaji-multiple-segments`）と `40_convert.yaml`（打鍵列に `n` を1つ追加）、`api.rs` の `n_input_never_duplicates_nn_in_composing_reading` の期待値を新挙動に更新
- **G-8 との関係**: G-8 の `nn` の記述は本項で改訂（G-8 側に注記済み）。soft pending に依存していた G-40 の説明（soft の `n` を捨てる修正）は履歴として残すが、現在は機構自体が無い

### G-49. Backspace は「直前のキー入力の取り消し」（不一致で確定された文字を保留に戻す）（2026-09-30）

- **背景**: 実機で「へんかん」を打つつもりで `hennkp` と打ち、Backspace で 1 つ消して `a` を打つと「へんkあ」になった（期待は「へんか」）。`k`+`p` は変換規則に無いため `k` が確定済みの文字（Literal）として reading に出て、保留は `p` のみになる。Backspace で `p` を消しても `k` は確定済みのままで、保留に戻らなかった
- **決定**: Backspace は「直前のキー入力の取り消し」とする。直前のキーで保留ローマ字が不一致のまま確定（Raw の素通し）された場合、その出力を消して保留を「直前のキーを打つ前の状態」に戻す。`hennkp`→BS→`へん`+保留 `k`（`a` で「へんか」）。促音（`kk`→っ+k、BS→保留 k のみ）、`n`+子音（`nk`→ん+k、BS→保留 n）、`tch`（→っ+ch、BS→保留 tc）も同様
- **巻き戻す条件（`RomajiConverter::feed`）**: 直前の `feed` が (a) Raw を出力した、または (b) 何か確定して保留が残った、のどちらか。確定して保留が空になる通常の入力（`ka`→か、`nn`→ん）は巻き戻さず、従来どおり確定済みの文字を 1 文字消す（標準 IME と同じ）。何も確定せず保留に足しただけのキーは、巻き戻しでも従来の「保留を 1 文字戻す」でも同じ結果になる
- **履歴の持ち方**: `RomajiConverter` が直前 1 キー分だけ `Rollback { prev_buffer（feed 前の保留）, emitted_chars（feed が出力した文字数）}` を持つ。`backspace()` は `Backspace::{Nothing, Pending, Rollback{retract}}` を返し、`Composer` は `Rollback` のときカーソル直前の `retract` 文字（種別が Literal の Raw を含む）を消す。使った時点で履歴は消えるので、2 回目以降の Backspace は従来どおり（保留があれば保留を 1 文字、無ければ reading の末尾 1 文字）。`feed` 以外の操作（`flush`・`clear`・`load_pieces`、カーソル移動・削除・確定・変換・大文字/数字/非 ASCII の入力）は履歴を破棄する。したがって巻き戻しは常にカーソル直前の出力に対して働く
- **ライブ変換（G-43/G-46）との整合**: engine は変更なし。Backspace でライブ変換の表示が破棄されひらがなに戻り、次の文字入力まで再開しないのは従来どおり。reading（保留を含まない）は巻き戻しで「直前キー前の状態」に一致する。不一致で Literal になっていた `k` は巻き戻しで消え、保留に戻る（Segment の kind に Literal は残らない）
- **影響**: `romaji/src/lib.rs`（`Backspace`・`RomajiConverter::{feed,backspace}`・`Composer::backspace`）。テストは romaji の `test_backspace_*`・`test_converter_backspace_outcomes`、engine の `api.rs`（`backspace_rolls_back_*`・`second_backspace_is_conventional`・`cursor_move_discards_backspace_history`・ライブ変換ケース）と golden `30_composing_edit.yaml`（`composing-backspace-rollback-*`）。既存テストの期待値変更なし
- **未対応**: Undo（確定後の取り消し）や複数キーにわたる巻き戻しは対象外。`k`+`-`（k が Raw、`-` が「ー」で保留が空）も Raw を含むため巻き戻し対象（BS で保留 k に戻る）


### H-6. iOS キーボード拡張のメモリ実測（2026-10-14、ユーザー実測・要レビュー）

端末: iPhone 16e / iOS 26 系、TestFlight ビルド（`ios-testflight`、llama=ON）。拡張内で Probe を実行した結果。

- **拡張のメモリ上限は約 177MB**: `baseline_memory` で `available_memory_mb=166.2`・`phys_footprint_mb=10.8`（`os_proc_available_memory` は上限までの残量）。ホストアプリは 3GB 超（`memory_ramp` を 3072MB まで確保して正常終了。上限は未特定）
- `dict_load`（Mozc 辞書の全量読み込み）で footprint が +60.9MB（合計 72.0MB）。残りは約 105MB
- `coldstart` は 48.5ms で成功
- 一括実行は `latency` の実行中に拡張が終了（メモリ超過の疑い）。`llm_load` は開始のみで完了の記録なし（終了の疑い。未確定）。`memory_ramp`（拡張側の上限 1024MB）も実行中に終了
- アプリ側の `llm_load`（small、70MB）は footprint が増えず（mmap のため未計上と推測）、ロードに 17 秒。推論時のメモリは未測定

判断（暫定）:
- 拡張に zenz small は載せられない見込み。xsmall + 辞書は拡張内で動作した（iPhone 16e）。ただし他機種の上限が未測定のため、iOS の品質基準は `minimal` を既定とし、起動時の `os_proc_available_memory` で十分な残量（要閾値決定）がある場合だけ xsmall を有効にする方針を候補とする（環境プロファイルの自動判定、設計書 §4.7 の延長）
- **後の決定**: mmap 辞書（H-7）で small も拡張内で動作した（H-9）。iOS の `auto` は `light` から開始し、既定モデルは small（H-22(c)）
- 辞書の mmap 化（progress.md 未完了項目 6）の優先度を上げる。全量読み込み（約 61MB）は上限の約 1/3 を占める
- **追試（同日）: xsmall（20.0MB）は拡張内で `llm_load` が成功**: footprint_after 119.4MB（増加 106.4MB）、残り 57.6MB（`available_after_mb`）、ロード 110ms、`first_space_ms` 76.1ms。small は拡張で完了記録がなく、載らない見込みは変わらない
- 一方 `latency` は再び実行中に終了（一括実行時）。多数回の変換で残り約 58MB を超える可能性。単独実行・`llm_load` 後の実行での切り分けが必要
- `latency`（辞書のみの経路）を新規プロセスで単独実行すると成功（ひらがな p95 0.0ms、Space p95 0.1ms）。一括実行時の終了は、複数 Probe のメモリが同一プロセスに積み上がったことによる計測上の問題で、実使用の上限超過を示すものではない
- **xsmall + 連続変換 20 回（拡張内、新規プロセス、同日）**: ロード後 footprint 111.0MB（残り 66.0MB）、ピーク 124.2MB、残量の最小 52.8MB。Space 応答は p50 0.2ms / p95 22.0ms / max 22.0ms。強制終了なし。ロード後からピークまでの増加は 13MB（推論用バッファの初回確保と見られ、リークかは 20 回では判別できない）
- 未確認: CPU のみ（Metal 無効）での差、mmap 辞書での成立可否、長時間・多数回の入力でのメモリ推移、実際のキーボード UI（候補表示・描画）を載せた状態での余裕、他機種での上限（本実測は iPhone 16e / RAM 8GB のみ。上限は非公開で機種により異なる見込み）

### H-7. 辞書の mmap 読み込み（2026-10-14、要レビュー）

背景: H-6 の実測で、iOS キーボード拡張の上限（約 177MB）に対し、辞書の全量読み込みが +61MB を占めた。

- **形式は変えない（version 2 のまま）**: H-3 は「アラインメント・オフセット表現を持つ形式」を想定していたが、`u16` / `u32` 配列を要素ごとに `from_le_bytes` で読めばアラインメントは不要で、形式変更・辞書の再生成・unsafe な型変換が要らない。読み出しは境界検査付きで、リトルエンディアン固定
- **`Dictionary` はファイル全体を 1 つのバイト列（`Backing`: `Vec` または mmap）で保持**し、各セクションはその範囲として参照する。fst の `Map` も同じバイト列を参照する（従来は trie を二重に保持していた）。`to_bytes` は保持しているバイト列そのもの
- **`Dictionary::load` は非 Windows で mmap、失敗時または Windows は全量読み込み**。Windows はマップ中のファイルを置換・削除できず、辞書更新（データ資産の更新）と相性が悪いため。デスクトップはメモリに余裕があり問題にならない
- **unsafe の範囲**: `nagi-dict` のみ `unsafe_code = "deny"` とし、`map_file` 1 関数だけ `allow`。他のクレートの forbid は変えない
- **UTF-8 検査は参照時**: 表記プール全体の検査は mmap の全ページを触るため読み込み時には行わず、`surface()` で都度検査する（壊れたデータでは空文字。panic しない）。オフセット表の単調性・ID の範囲検査は従来どおり読み込み時に行う
- **運用上の注意**: マップ中のファイルを同じ inode のまま書き換え・短縮すると SIGBUS になる。辞書の更新は一時ファイルに書いて rename で置き換えること
- テスト: 既存の往復・破損データに加え、ファイルからの `load`（unlink 後も参照可）、全長での切り詰め拒否を追加。実辞書（real_data）での検証は未実施（この環境から Mozc を取得できない）
- **実機の実測（iPhone 16e、キーボード拡張、新規プロセス、同日）**: `dict_load` の footprint 増加 +60.9MB → +0.2MB（合計 72.0MB → 10.8MB）、ロード 59.8ms → 37.7ms。`llm_load`（xsmall + 連続変換 20 回）のピーク 124.2MB → 70.8MB、上限までの残り最小 52.8MB → 106.2MB。`latency`（辞書のみ）は単独で成功（Space p95 0.2ms）。LLM 併用時の Space p95 は 25.3ms（max 同値）で、mmap 前（22.0ms）と同程度

### H-8. プロセスのメモリ上限による LLM 読み込みの抑止（2026-10-14、要レビュー）

背景: iOS のキーボード拡張は搭載メモリ（iPhone 16e で 8GB）に関係なく、プロセスごとの上限（同機で約 177MB）が課される。`nagi-profile` の判定は搭載メモリ（`SystemInfo.total_memory_mb`）を見るため、このままでは上限の小さい環境でも LLM を読み込んでしまう。

- **フロントエンドが上限を注入する**: `ServerOptions.memory_budget_mb`（`--memory-budget-mb`）と、C ABI の `nagi_server_new_with_budget(.., memory_budget_mb)`（0 は指定なし）。iOS では `phys_footprint + os_proc_available_memory()`（Swift: `MemoryProbe.processBudgetMB()`）を渡す。コアは OS の API を直接呼ばない原則（設計書 §4.7、mobile-design §3）に沿う。既存の `nagi_server_new` は上限なしで従来どおり
- **`MIN_MEMORY_BUDGET_FOR_LLM_MB` = 150（暫定）未満なら LLM を読み込まない**。読み込み前に判定し、`notes` に理由を残す（プロファイルは `minimal`）。根拠は iPhone 16e の拡張で xsmall + 辞書が上限 177MB に対しピーク 124MB に収まった実測（H-6）。辞書の mmap 化（H-7）で余裕が増える見込みのため、実測後に下げる余地がある
- **モデルの選択（xsmall / small）は今回の対象外**: 現状はフロントエンド（または `config.toml` の `conversion.model`）が決める。上限が大きい環境で small を選ぶ判断は、small の拡張外での実測（メモリ・速度）が揃ってから設計する
- 上限は起動時の 1 回のみ判定する。実行中の逼迫は従来どおり `set_environment` の `memory_pressure` で扱う（フロントが `os_proc_available_memory` の残量から報告する）
- 未実施: 実製品のキーボード（変換エンジンを実際に使うもの）が無いため、`processBudgetMB()` はまだ呼び出し元が無い（Probe は計測のため上限判定なしで生成する）。他機種での上限の実測

### H-9. small（70MB）も拡張内で動作（2026-10-14、実測・要レビュー）

mmap 辞書（H-7）の実機で、small（`zenz-v3.1-small-Q5_K_M.gguf`、70.4MB）を拡張内で `llm_load` した結果（iPhone 16e、新規プロセス）: ロード後 footprint 43.0MB、ピーク 67.4MB、上限までの残り最小 109.6MB、ロード 27.4ms。xsmall（ピーク 70.8MB）と同程度で、モデルは mmap のため触れたページだけが計上される。強制終了なし。

- **計測の限界**: 連続変換 20 回の Space 応答は p50 0.3ms で、最初の 1 回だけ 155ms。プロファイルの自動切替（遅い変換で `minimal` に降格）が働き、2 回目以降は LLM が動いていない可能性が高い。つまり上記ピークは「持続的な推論中のメモリ」ではない。`llm_load` Probe をプロファイル固定（balanced・自動切替なし）に変更して再計測する
- 判断は再計測後に行う。xsmall と small にメモリ差がほぼ無いなら、iOS の既定モデルは small（精度優先）とし、`MIN_MEMORY_BUDGET_FOR_LLM_MB`（150MB）の見直しは他機種の実測が出てから行う

**再計測（プロファイル固定 balanced、LLM 有効のまま連続変換 20 回、拡張内、新規プロセス、同日）**:

| 項目 | small（70.4MB） | xsmall（20MB） |
| --- | --- | --- |
| ロード後 footprint | 56.3MB | 38.0MB |
| ピーク footprint | 76.2MB | 45.6MB |
| 上限までの残り最小 | 100.8MB | 131.4MB |
| Space 応答 p50 / p95 / max | 155.4 / 318.2 / 318.2ms | 103.2 / 308.7 / 308.7ms |

- **メモリは制約にならない**: どちらもピークは上限（約 177MB）の半分以下。強制終了なし。small で残り 100MB 超
- **制約は応答時間**: balanced（beam 3・再採点 8）の同期変換は p50 が 100〜155ms、p95 が約 310ms で、目標（Space 変換→候補表示 p95 150ms 未満、基準機 M1）を満たさない。入力した文が 19〜29 文字と長いことにも注意（短文は速い見込み）
- 次の計測: プロファイルを `light`（beam 1・再採点 3）に固定した `llm_load_light` Probe を追加して、応答時間がどこまで下がるかを見る。非同期の候補差し替え（`update_candidates`、未実装）で、まず辞書の候補を即表示して LLM の結果を後から差し替える構成にすれば、同期の応答時間は辞書のみの値（0.1〜0.3ms）に収まる見込み

**light 固定の計測（拡張内、新規プロセス、同日）**:

| 項目 | small（70.4MB） | xsmall（20MB） |
| --- | --- | --- |
| ピーク footprint | 62.4MB | 70.0MB |
| 上限までの残り最小 | 114.6MB | 107.0MB |
| 最初の Space 応答 | 155.5ms | 91.7ms |
| Space 応答 p50 / p95・max | 0.3 / 150.7ms | 0.3 / 27.5ms |

- **メモリ**: balanced / light・small / xsmall のどの組み合わせでもピークは 76.2MB 以下（上限約 177MB の 43%）。強制終了なし。**モデル・プロファイルの選択にメモリは効かない**（iPhone 16e の場合）
- **応答時間の読み方（要注意）**: p50 が 0.3ms で、最初の 1 回だけ遅い。0.3ms で LLM 推論が終わることはないため、2 回目以降の多くは LLM が同期経路で使われていない。原因の候補は、(a) 同期の待ち時間（`sync_budget_ms`）を超えた推論が裏で続いていて、次の変換では LLM が使用中のため辞書のみになる、(b) 同じ 5 文の繰り返しによるキャッシュ（balanced では p50 が 100〜155ms だったため、これだけでは説明できない）。どちらかは未切り分けで、`update_candidates` の実装と合わせて調べる
- **暫定の iOS 既定**: モデルは small（存在しなければ xsmall）、プロファイルは `light`。根拠はメモリに余裕があること、small の方が変換精度が高い見込みであること。最初の変換の遅さ（155ms、モデルのページ読み込みと見られる）は、起動時のウォームアップで隠す。**精度は未比較**（zenz small と xsmall の変換精度の差は Phase 0 の比較項目）

### H-10. 非同期通知の C ABI 配信と、同期の待ち時間（2026-10-14、要レビュー）

H-9 の応答時間の謎の原因: `conversion.sync_budget_ms` の既定が 150ms。balanced の p50 が 100〜155ms・p95 が約 310ms だったのは、LLM が予算（150ms）に収まらず、毎回 150ms 待ってから辞書の結果を返していたため。light で p50 が 0.3ms になったのは、最初の 1 回の推論が裏で走り続けている間、バックエンドが使用中で LLM を使わず辞書のみで返していたため（上限 150ms が p95 / max に出る）。つまり**待つ時間を短くし、LLM の結果は非同期で差し替える**構成（既に実装済みの G-11）が、iOS の応答時間の解になる。

- **課題**: 非同期の差し替え通知（`update_preedit` / `update_candidates` / `notify`）は、stdio の `run` ループでは Wake で配信されるが、C ABI には取り出す手段が無く、次のリクエストを待つ必要があった
- **`Server::poll_notifications` / `Server::set_wake_callback`（新設）**: `run` の Wake 処理を取り出した公開 API。`run` もこれを使う（挙動は同じ）
- **C ABI**: `nagi_set_waker(handle, callback, user_data)`（通知が取り出せるときに任意のスレッドから呼ばれる）と `nagi_poll_notifications(handle)`（通知行を返す。`initialize` の前は積んだまま空）。`nagi_server_free` は先にコールバックを外し、実行中のコールバックの完了を待ってから `Server` を捨てるため、解放後に `user_data` が使われない
- **Swift**: `NagiEngine` の呼び出しをすべて 1 本の直列キューで行う（C ABI はハンドルの同時アクセス不可）。`startNotifications { lines in .. }` でメインキューに通知を渡す。ハンドラの中から `send` を呼ばないこと（デッドロック。メインに積み直す）
- **`llm_load` Probe**: `sync_budget_ms = 30` に変更し、連続変換のあと非同期通知（`update_preedit` / `update_candidates`）が届いた件数（`loop_async_updates`）と待ち時間（`loop_async_wait_ms`）を記録する。以前の数値（既定 150ms）とは条件が違う
- 未実施: iOS 製品の既定 `sync_budget_ms` の決定（30ms は計測用。実測で決める）、`update_candidates`（候補ウィンドウの差し替え）の実機での確認

**sync_budget_ms = 30 の実測（拡張内、新規プロセス、同日）**:

| 項目 | small・light | xsmall・balanced |
| --- | --- | --- |
| 最初の Space 応答 | 49.0ms | 83.0ms |
| Space 応答 p50 / p95・max | 35.3 / 43.1ms | 36.4 / 77.7ms |
| ピーク footprint | 78.5MB | 48.4MB |
| 上限までの残り最小 | 98.6MB | 128.6MB |

- **応答時間は目標（p95 150ms 未満）を満たす**。p50 が約 35ms なのは、毎回待ち時間（30ms）を使い切って辞書の結果を返しているため（LLM は 30ms に収まらない）。待ち時間を短くすれば、さらに速くできる
- **非同期通知は 0 件だった（想定内）**: ループは各変換の直後に Enter で確定するため、差し替えが意図的に捨てられる（G-11: 未操作のときだけ差し替える）。到達の確認は「入力を続けない変換」で行うよう Probe を改め（`tail_async_arrival_ms`、-1 は不達）、次のビルドで確認する

**非同期通知の到達確認（拡張内、新規プロセス、`llm_load_light`、sync_budget_ms = 30、同日）**:

| 項目 | small（70.4MB） | xsmall（20MB） |
| --- | --- | --- |
| 入力を続けない変換の同期応答（`tail_space_ms`） | 36.3ms | 19.8ms |
| 非同期通知の到達（`tail_async_arrival_ms`） | **621.6ms で到達** | 不達（-1） |
| ピーク footprint / 残り最小 | 61.5 / 115.5MB | 70.5 / 106.5MB |

- **C ABI 経由の非同期差し替えは動作した**: small は 30ms の待ちで辞書の結果を返し、約 620ms 後に LLM の結果が `update_preedit` / `update_candidates` として届いた（`nagi_set_waker` → `nagi_poll_notifications` → メインキュー）
- **xsmall の「不達」は不具合ではない**: 同期応答が 19.8ms で、待ち時間（30ms）に収まって LLM の結果をその場で返したため、差し替える結果がなかった
- **iOS の暫定既定を訂正**: 先に「small + light」としたが、応答時間を踏まえて **xsmall を既定**とする。xsmall は待ち時間内に LLM の結果が返る（差し替えのちらつきが無い）。small は結果が約 0.6 秒遅れて差し替わる。**変換精度の差（zenz small と xsmall）は未比較**で、small が明確に良い場合に限り、設定で small を選べるようにする（Phase 0 の比較項目。デスクトップで `nagi-eval` により比較できる）
- **後の決定**: xsmall を既定とする判断は H-22(c) で廃止した。既定は small（未選択時は端末の予算で自動選択、Picker の明示選択を優先）

### H-11. タッチ入力のプロトコル `insert_reading` / `perform_action`（2026-10-14、要レビュー）

H-3 の決定（案A: タッチ入力は `insert_reading` / `perform_action` の系統で渡し、物理キーは `process_key`）を実装した。12 キーのトグル・フリックの解決はコア側（H-2）。

- **`insert_reading`**（応答は `process_key` と同じ描画差分）: `input` は隣接タグ付きの列挙（`{"type": ..., "content": ...}`）。
  - `text`: 読み（かな・記号）をそのまま挿入する。ローマ字変換を通らない（非 ASCII は直接、ASCII はローマ字扱い）
  - `key12 { key, gesture }`: 12 キーのキー（`a ka sa ta na ha ma ya ra wa punct`）と操作（`tap` / `toggle` / `flick_left|up|right|down`）。文字の対応はコア（`engine/src/touch.rs`。中央・左・上・右・下の順で、や行は `や （ ゆ ） よ`、わ行は `わ を ん ー 〜`、句読点は `、 。 ？ ！ …`）。`tap` は連続してもトグルしない（フリック入力）。`toggle` は同じキーを続けて押すと、直前の文字を同じ行の次の文字に置き換える（あ→い→う→え→お→あ）。カーソルが動く・別の入力が入る・物理キーが来る・フリックを挟むと、トグルは続かない
  - `variant`: 直前の文字の濁点・半濁点・小書きの循環（か→が→か、は→ば→ぱ→は、つ→っ→づ→つ、う→ぅ→ゔ→う など）。未入力なら消費しない
- **`perform_action`**（同上）: `action` は `space` `enter` `start_convert` `predict` `next_candidate` `prev_candidate` `commit` `cancel` `backspace` `delete_forward` `move_cursor` `move_cursor_home` `move_cursor_end` `move_focus` `resize_segment` `extend_selection`（方向は `left` / `right`）`delete_learning` `to_katakana` `to_hiragana` `to_hankaku` `toggle_mode` `set_mode`（`hiragana` / `alphanumeric`）`undo_commit`。`space` は入力中なら変換、変換中なら次の候補。**未入力（Direct）では `set_mode` / `toggle_mode` / `undo_commit` 以外を消費しない**（`consumed = false`。フロントが空白・改行・1 文字削除などの既定の処理をする）
- **選択肢の追加は `select_candidate`（既存）**。候補バーのタップは絶対インデックスで送る
- Swift の型は `cargo xtask gen-types` で生成してコミット済み（`frontend-macos/Sources/Generated/Protocol.swift`。iOS も同じ型を使う）
- 未実装: 12 キー以外のレイアウト（QWERTY は既存の `process_key` を使う）、`initialize` の capability 拡張（`platform` / `memory_limit_mb` など。メモリ上限は `nagi_server_new_with_budget` で渡している）、レイアウトの配列を設定で差し替える仕組み（現状は固定表）

### H-12. iOS 製品版キーボードの構成（段階 1、2026-10-14、要レビュー）

`frontend-ios-spike/` の拡張に、製品版のキーボードを追加した。検証用の画面（Probe）は残し、候補バー右端の「検証」ボタンで切り替える（実測を続けるため）。

- **ロジックはコアに置く**: Swift は「キー操作をコアへ送り、返った差分を反映する」だけ。`insert_reading`（12 キー: タップ・フリック・濁点循環）と `perform_action`（空白・改行・削除・カーソル・モード）を使い、`select_candidate` で候補を選ぶ（H-11）
- **型**: `frontend-macos/Sources/Generated/Protocol.swift`（typeshare）を、ホストと拡張の両ターゲットのソースに含める（`project.yml`）。JSON-RPC の封筒だけ Swift 側で持つ（`Shared/NagiSession.swift`）
- **構成**: `NagiSession`（型付きの JSON-RPC。非同期通知は C ABI の `nagi_set_waker`、H-10）→ `KeyboardModel`（状態。`KeyboardHost` 経由でホストの入力欄を操作）→ `NagiKeyboardView`（SwiftUI の 12 キーと候補バー）。`ProductSession` が App Group の辞書・モデルからセッションを作る
- **変換中の表示**: ホストの入力欄に `setMarkedText`（下線付き）で出し、確定は「確定文字列を変換中の表示に置き換えて `unmarkText`」。未入力のとき、コアが消費しない空白・改行・削除（`consumed = false`）はフロントがホストに直接入れる
- **起動時の設定**: `<App Group>/user/config.toml` を自動生成（先頭の `# nagi:generated` 行があるファイルだけ起動のたびに作り直す）。`sync_budget_ms = 30`、更新確認なし、モデルは xsmall を優先（H-10）。メモリ上限は `nagi_server_new_with_budget` で渡す（H-8）
  - **後の決定**: 「xsmall を優先」は H-22(c) で廃止した。既定は small（未選択時は端末の予算で自動選択、Picker の明示選択を優先）
- **未実装（次の段階）**（当時の状態。現状は `ios-device-checklist.md` §3）: BackSpace の長押し連打、トグル入力の切り替え設定（現状はフリック入力固定）、`set_context`（直前のテキストを LLM の文脈に渡す）、ホストの入力欄が外部から変わったときの追従、QWERTY・英数・記号のレイアウト、候補の展開表示、本体アプリ（有効化の案内・設定）。**Swift はローカルでコンパイルできず、確認は CI（Xcode 26.3）のみ**

### H-13. iOS キーボード段階 1 の実機結果と修正（2026-10-14）

実機（TestFlight）の結果: フリック入力・濁点/小書き・ライブ変換は動作。次の不具合を修正した。

- **変換の候補が増えない / ←→が効かない**: 変換を始めても候補ウィンドウは 2 回目の Space まで開かず（キー操作の仕様）、変換中の ←→ は「カーソル」を送っていたため効かなかった。**タッチ UI では変換中は常に注目文節の候補の一覧を出す**（`perform_action` の後に候補を作って表示状態にする。エンジン側）。フロントは変換中（`preedit.focus` あり）の ←→ を `move_focus`、変換前の入力中を `move_cursor`、未入力をホストの入力欄のカーソル移動（`adjustTextPosition`）にする
- **確定しても下線が残る**: 「確定文字列を変換中の表示にして `unmarkText`」では下線が消えなかった。**変換中の表示を空にして（`setMarkedText("")` + `unmarkText`）から、確定文字列を通常のテキストとして `insertText`** に改めた。変換中のカーソル位置も `preedit.cursor` を使う
- **日英の切替が効かない**: 切替はコアの入力モードではなく、キーボードのレイアウトの問題だった（かなキーはモードに関係なくかなを入れる）。**英字（QWERTY）のレイアウトを追加**し、あ/A キーでレイアウトを切り替える（入力中の文字列は確定してから切り替える）。英字は変換を通さずホストに直接入れる。数字・記号のレイアウトは未実装

### H-14. 入力中（変換前）の候補と部分変換（モバイル向け、2026-10-14、要レビュー）

実機の指摘（変換の選択肢が少ない・常に候補バーに変換を出したい・一覧を展開したい・矢印で文節を動かす操作は不要）を受け、**PC 型の「文節を動かして変換」ではなく、一般的なスマートフォンのキーボード型**にした。

- **入力中（Composing）に、候補を常に出す**: 読み全体の最良の変換、先頭の文節の候補、ひらがな・カタカナの順（最大 12 件）。入力のたびに、25ms のデバウンスのあとバックグラウンドで作り（ライブ変換と同じ仕組み）、`update_candidates` で届ける。入力が変わったあとの古い候補は、表示にも選択にも使わない（読みの一致で判定）。LLM の遅延完了（`sync_budget_ms` 超過分）が入力中の読みと一致すれば、候補を作り直して差し替える
- **部分変換**: `CandidateView.covers`（先頭から何文字の読みを置き換えるか。`None` は読み全体）。**候補を選ぶ（`select_candidate`）と、その部分だけが確定され、残りの読みは入力中のまま続く**。全体の候補なら全部を確定。部分確定は学習に通知する。確定の取消（`undo_commit`）は、部分確定では対象外
- **変換キー（空白）は従来どおり**: 変換中（Converting）に入り、文全体の N-best を候補の一覧に出す。変換中は候補を選んでから確定（フロントが `select_candidate` → `commit`）
- **矢印**: 入力中はカーソル移動、変換中は注目文節の移動、未入力ならホストのカーソル移動。文節の細かい操作（区切り伸縮・範囲選択）は、Phase 3 の範囲操作 UI（セグメント列、mobile-design §5.1）で改めて設計する
- **UI**: 候補バー右端の「﹀」で、候補の一覧を縦のグリッドで広げる（キーの領域に重ねる）。選ぶと閉じる
- **ロードマップ上の位置づけ（今後の実装順、実機のフィードバックの分類）**: 構造に関わるもの（候補の仕組み・展開）はこの H-14 で対応。ほかは (1) 英字シフトの 2 回押しでの大文字固定、(2) フリックの押下時に 4 方向の文字を出す案内、⌫の長押し連打・数字記号のレイアウトと合わせて「キーボード UI の実装」、(3) 直前のテキストの文脈、(4) 本体アプリ、(5) 最後に見た目と操作感の調整

### H-15. iOS の配布を TestFlight / App Store に変更（2026-10-14、ユーザー決定）

- **Apple Developer Program に加入済み**で、iOS は **TestFlight（内部テスト → 外部テスト）→ App Store** で配布する。H-5 の「SideStore によるサイドロード」と、`docs/decisions.md` の「Apple Developer Program には加入しない」（iOS について）は、この決定で置き換える。SideStore 向けの検証項目（再署名後の App Group、無料 Apple ID の制限、7 日失効、AltStore 互換ソース JSON）は不要になった
- bundle id は `com.shiki01.nagi`（ホスト）、`com.shiki01.nagi.keyboard`（キーボード拡張）、App Group は `group.com.shiki01.nagi`。macOS の IME（`com.shiki.inputmethod.Nagi`）とは別
- ビルド・署名・アップロードは GitHub Actions `ios-testflight`（手動実行。App Store Connect API キーのクラウド署名。表示バージョンは `ios-vX.Y.Z` タグ、ビルド番号は run_number。CI の開発証明書は前後で失効させて上限を避ける）
- macOS の配布方針は、加入済みの前提で Developer ID 署名 + 公証へ見直せる（未着手。現状の実装は ad-hoc 署名）。Android の配布（APK 直接配布）は変更なし。**SideStore は使わない**（ユーザー決定。関連ワークフロー `ios-spike.yml`・`ios-signing-diag.sh` は削除）

### H-16. モバイル向けコア API: LLM の解放/再読み込み・12 キーの文字表・D-4（2026-10-14、要レビュー）

- **LLM だけの解放・再読み込み**: `Runtime::unload_model` / `reload_model`（実行先は `ModelControl` trait。`Pipeline` が実装し、`build` が登録）。解放は変換モデルの `set_active(false)`（重みの解放）+ 辞書の逆引き索引の解放（Minimal 化）。**待たない・破棄**: 解放を要求したらすぐに辞書のみの応答になり、実行中の推論は待たずに、世代番号で結果（キャッシュ・遅延完了通知）を捨てる。解放中に始まる推論はモデルに触れない（`LlamaModel` は `convert` で重みを読み直すため、印を見て弾く）
- **自動の解放・再読み込み**: `set_environment` の `memory_pressure = critical`、またはメモリ上限が 150MB 未満のとき解放。逼迫が `normal` に戻って **30 秒**後（`MODEL_RELOAD_DELAY`。暫定値）、`light` 以上で動ける場合に再読み込み（`warning` の間は戻さない）。再読み込みの失敗は `llm_reload_failed` を通知して辞書のみで継続し、**自動では再試行しない**（次の逼迫サイクルか明示の `reload_model` まで）。再読み込みが起動時の非同期読み込みと重なる場合は、取り付けた直後に解放する
- **JSON-RPC の追加（加算的）**: `unload_model` / `reload_model`（params なし、応答 `{}`）。明示の `unload_model` は `reload_model` まで自動では戻さない。状態は `get_status.llm`
- **`get_key_table`**: 12 キーの文字表（`center/left/up/right/down`・`toggle` の循環）と濁点・小書きの循環（`variants`）を返す。エンジンの `touch.rs` の表から作る（単一の情報源）。iOS の `KanaKeyTable.swift` の複製表はこれで置き換えられる
- **D-4**: `initialize.params.capabilities`（任意。`input_method: touch|physical`、`environment`＝`set_environment` と同じ項目）と `set_environment.memory_budget_mb`（任意）を追加。プロファイル判定（`nagi_profile::initial_profile_with_budget`）はメモリ上限に従う: **150MB 未満は Minimal、1024MB 未満は Light まで、2048MB 未満は Balanced まで**（暫定値。実機で見直す）。`profile.mode` での固定は上限より優先。`input_method` は記録するのみで、現状は挙動を変えない
- **Swift の追従が必要**: 生成された `InitializeParams` / `SetEnvironmentParams` のメモリワイズ init に引数が増えたため、既存の呼び出し（macOS `EngineSupervisor.swift`、iOS `NagiSession.swift`・`NagiEnvironment.swift`）に `capabilities: nil` / `memory_budget_mb: nil` が要る

### H-17. Phase 0: モデル比較の一次結果（zenz-v3.1 xsmall / small、2026-10-14、要レビュー）

`nagi-eval`（Mozc `evaluation.tsv` の Conversion 系 609 件、`left_context=""`、Mozc OSS 辞書）で、辞書のみ・xsmall・small を比較した。**G-36 の「small は xsmall と同等以下」は、`nagi-eval` 中の環境プロファイル自動降格（`minimal` 化）の影響を受けた値で、プロファイルを固定すると逆転する**ことが分かった。

- 測定環境: Linux x86_64（Intel Xeon 2.8GHz・4 コア・RAM 16GB）、release ビルド、`--features llama`、モデルは `zenz-v3.1-{small,xsmall}-Q5_K_M.gguf`（SHA-256 検証済み）。**iPhone / Apple Silicon とは CPU 性能が異なるため、レイテンシは参考値**。各条件 1 回の測定（ばらつきは未評価）。`user_dir` は空の一時ディレクトリ（学習なし）。モデル指定は `config.toml` の `conversion.model`、プロファイル固定は `profile.mode`
- 取得: この環境では `cargo xtask fetch-*` の HTTP クライアント（ureq）がプロキシ経由・追加 CA に対応せず TLS 検証に失敗するため、同一 URL を curl（CA は環境のバンドル）で取得し、`cargo xtask fetch-dict` / `fetch-model` を再実行して SHA-256 一致を確認した（xtask のコードは未変更）

| 条件 | モデルサイズ | 精度（TOTAL） | Expected（全文） | Expected N（文節候補） | Match | convert_sentence p50 / p95 / max |
| --- | --- | --- | --- | --- | --- | --- |
| 辞書のみ | - | 66.2%（403/609） | 66.9% | 59.0% | 71.7% | 0.0 / 0.1 / 0.4ms |
| xsmall・balanced 固定 | 20.0MB | 68.1%（415/609） | 65.6% | 64.5% | 77.4% | 15.5 / 40.4 / 154ms |
| xsmall・light 固定 | 20.0MB | 68.3%（416/609） | 65.9% | 64.5% | 77.4% | 25.7 / 49.8 / 151ms |
| xsmall・auto | 20.0MB | 68.5%（417/609） | 65.9% | 65.0% | 77.4% | 18.0 / 40.7 / 126ms |
| small・balanced 固定 | 70.4MB | **75.0%**（457/609） | 76.8% | 65.0% | 90.6% | 65.8 / 150.8 / 351ms |
| small・light 固定 | 70.4MB | 72.7%（443/609） | 74.8% | 65.0% | 79.2% | 90.2 / 150.5 / 155ms |
| small・auto | 70.4MB | 66.2%（403/609） | 66.9% | 59.0% | 69.8% | 0.0 / 110.3 / 154ms |

- 所見 1（auto の small は辞書のみと同値）: auto では small がレイテンシ超過で降格し、辞書のみと全く同じ結果（403/609）になる。G-36 で修正した定期プロービングの後も、連続変換のベンチではほぼ常時降格している。**small の本来の精度は固定プロファイルでしか測れない**。G-36 の表（small 66.3%）はこの現象で、small のモデル自体の精度ではなかった
- 所見 2（small と xsmall の差）: 固定プロファイルで small は xsmall より全体で +4.4〜6.9pt（balanced 75.0% / light 72.7% vs 68.1〜68.3%）。差の主因は全文変換（Expected: 76.8% vs 65.6%、light でも 74.8%）と Match（90.6% vs 77.4%）。文節候補（Expected N）はどちらも 65% で差がない（辞書列挙が支配的）。xsmall は辞書のみに対し全文 Expected でむしろ -1.3pt（65.6% vs 66.9%）で、改善は文節候補と Match が中心。Expected の失敗数は xsmall 43 件のみ・small 9 件のみ（キー=読み+期待値で比較）
- 所見 3（誤変換の傾向）: xsmall のみ誤り・small は正解の例は、あおぞらぎんこう（xsmall: 青空銀行、期待: あおぞら銀行）、いっきょうたじゃく（一強多弱、期待: 一強他弱 ※small は正解）、いちまん（１万、期待: 一万）、いいごえんがあれば（良いご縁があれば）。固有名詞・慣用句・数字表記で、モデル容量の差が出やすい。small のみ誤りの例は、きかいがとまる（機会が止まる、期待: 機械が止まる）、くうぐんしょうこう（空軍商工）、へんきんまち、ゆうせんでもむせんでも。同音異義語の文脈判定でも small が全勝ではない。両者とも誤る例は、千円・五百円札などの数詞（9000円/500円札と算用数字が優先される）、新字体でない異体字（残置・接続などの Mozc 期待値）で、モデルの問題ではなく評価データ・数字の扱い方針の問題
- 所見 4（レイテンシ）: この環境（4 コア Xeon）で small は p50 66〜90ms、p95 約 150ms。**p95 が約 150ms で頭打ちなのは `sync_budget_ms` による打ち切り**で、長い文では LLM の結果が間に合わず辞書の結果になっている可能性がある（打ち切り率は未計測）。xsmall は p95 40〜50ms で余裕がある。iPhone 16e の実測（H-9: small の Space 応答 p50 155ms・p95 318ms、xsmall 103/309ms）の方が現実に近く、いずれも同期経路では目標（p95 150ms）に収まらない
- **xsmall を iOS 既定とする判断の妥当性**: 速度・メモリ（H-9: 差はほぼない）の面では xsmall が有利だが、**精度の面では small が明確に上**（特に全文変換）。H-9 の「暫定の iOS 既定は small（存在しなければ xsmall）」は、この結果とは矛盾しない。xsmall を既定にする根拠は「小さい・速い」のみで、精度を優先するなら small を既定にし、同期の待ち時間は非同期の候補差し替え（`update_candidates`）で隠すのが筋。**ただし**この環境の CPU は iPhone と違い、small が `light` でも p50 90ms かかる点（実機は H-9 の値）を、最終判断の前に実機で再確認する必要がある
- 限界・未実施: (1) `left_context` なし（文脈の効果は未評価）、(2) 1 条件 1 回の測定（ばらつき未評価）、(3) 評価セットは Mozc の Conversion 系 609 件のみで、期待値の一部（異体字・数字）は精度の比較に不向き、(4) `segment_candidates` のレイテンシ・同期打ち切り率は未計測、(5) 量子化違い（Q8 等）・他サイズ・SudachiDict との組み合わせは未比較、(6) iPhone 相当の CPU での計測。**`nagi-eval` で auto を使う比較は降格の影響を受けるため、固定プロファイル（`profile.mode`）で行うこと**。再現用の設定は `config.toml` に `[profile] mode = "balanced"` と `[conversion] model = "<gguf の絶対パス>"` を書いた `--user-dir` を渡す


### H-18. キーボード拡張の RequestsOpenAccess を true にする（2026-10-14、実機で判明）

- `RequestsOpenAccess: false` のままでは、設定アプリにフルアクセスのトグルが出ず、ホストアプリのセットアップ状況が『未検出』のままになる（ユーザー報告）。`true` に変更した。
- フルアクセスの用途は、App Group 共有領域の辞書・モデルの読み取りと、拡張から App Group への書き込み（セットアップ状況のマーカー、設定の反映）。入力内容は端末外へ送信しない（プライバシーポリシー・サポートの文面と整合）。
- 触覚フィードバックなど、フルアクセスが要る機能の可否は引き続き実機で確認する（`docs/ios-device-checklist.md`）。
- App Store 審査では、フルアクセスを求める理由の説明が必要（`web/README.md` の申告メモ）。

### H-19. 確定直後の次語予測（`predict.next_word`、軽量版、2026-10-14、要レビュー）

モバイルの候補バー向け。確定直後（入力中の読みが空の Direct 状態）に、直前に確定した語から次に来そうな語を出す。**LLM は回さず**、学習したバイグラムを引くだけの軽量版にした。

- **記録**: 変換を経て確定した日本語の文節ごとに「直前の語 → この語」（表記どうし）を記録する。直前の語は、同じ確定内の前の文節、または同じセッションで直前に確定した最後の語（`Session.last_word`）。ひらがなのままの確定・Literal・未変換のかなが続く確定・フォーカス喪失・`reset`・`set_context` の Surrounding Text が直前の語で終わっていないときは文脈を断つ。`CommittedSegment.prev_word` で observer に渡す
- **ファイル**: 通常の学習ファイル `learning.tsv` は変えず、同じディレクトリの別ファイル **`learning-next.tsv`**（1 行目 `#nagi-learn-next\tv1\tgen=N`、行は `prev\tnext\tcount\tlast_used`）。旧版は `learning.tsv` しか読まないので壊れない。`nagi_learn::NextWordLearner` が D-5 と同じ流儀（`.lock` の flock・基準との 3 者マージ・世代・原子的置換）で flush する。ファイルが無い・世代なしでも読める。読めないファイルは上書きせず空で始め、`next_word_load_failed` を通知する。上限は `learning.max_entries`、`learning.forget_days`・半減期も共通
- **表示**: `ConversionBackend::predict_next(ctx, prev, limit)`（既定は空。LLM 版はこの境界の裏に置く）。`Pipeline` は `learning_allowed`（設定・secure input・停止アプリ）と `predict.next_word` を確認して学習済みの上位を返す。engine は確定があったときだけ作り、既存の `candidates` 差分（`visible = true`、出所 `learned`、`covers` なし）で返す。入力が始まる・確定のない操作が来ると `visible = false` の差分で消える。**プロトコルの型は変更なし**（Direct で候補が出る点を `CandidatesView` の doc に追記）
- **選択**: Direct 中の `select_candidate(index)` は、次語候補ならその語を `commit` として返し、バイグラム（`reading` が空の文節。単語の学習はしない）を記録して、選んだ語を新しい直前の語にする（続きの次語候補が出る）
- **無効時**: `predict.next_word`（既定 false）が false の間は**記録もしない**（現状維持。有効にした時点から貯まる）。学習が無効・secure input・停止アプリでも記録・表示しない
- **削除**: `clear_learning` は両ファイルを消す。候補ウィンドウの学習削除（`Ctrl+Delete`）は、その表記を次語とするバイグラムも消す（読みが違う同表記も消える。粗い扱いだが誤学習の除去を優先）
- **暫定**: スコアは通常の学習と同じ `ln(1+count)` の指数減衰。`learning.strength` は次語の順位に影響しない（順序に効かないため）。Swift 側の追従: `docs/ios-settings.md` の `predict.next_word` / `predict.next_word_count` の設定 UI、Direct 状態の `candidates` を候補バーに出して `select_candidate` で挿入する処理

### H-20. コードレビュー指摘への対応（2026-10-14、要レビュー）

- **モデルの解放と再読み込みの競合**（`pipeline.rs` / `runtime.rs`）: 再読み込みスレッドは `set_active(true)` の直後に、モデルのロックを握ったまま世代を再確認し、変わっていれば `set_active(false)` で戻して `unloaded` は true のまま通知も出さずに終える（状態反映の後にもう一度確認する）。`spawn_set_active` は世代つきで、古い世代の要求は捨てる。`Runtime::evaluate_model`（状態の更新から `unload_model` / `reload_model` の指示まで）と `attach_model` の判定は `model_apply` の Mutex で直列化する。回帰テスト: `server/tests/model_unload.rs`
- **secure input と次語予測**（`engine.rs`）: `notify_commit` は secure input 中に `last_word` / `next_words` を消し、`prev_word` を空で通知する（observer には secure フラグつきで従来どおり通知）。`set_context` は secure の出入り・`app_id` の変化・Surrounding Text の不一致で `last_word` / `next_words` を消す
- **古い次語の挿入**: `reset` / `focus_out` / 不一致の `set_context` で `next_words` を消す。確定のない消費で候補を保つのは、モード切替（`SetMode` / `ToggleMode`）だけにした。`set_context` は戻り値が無いので、候補バーを消す差分は次の呼び出し（`select_candidate` など）で返る（`reset` / `focus_out` は自身の戻り値で返す）
- **学習ファイルの flush**（`runtime.rs` / `learn`）: 他プロセスの flock を待つ間 learner の Mutex を握らない。`nagi_learn::FlushLock::acquire(path, timeout)` で先にロックを取り（定期は 100ms、明示の忘却・全削除は 2 秒で打ち切り、取れなければ dirty のまま次回へ）、Mutex は書き出しの間だけ握る（`flush_with_lock`）。書き出し自体（ファイルの読み込み・マージ・書き込み）は引き続き Mutex 内。**暫定**: スナップショット方式はマージの基準の管理が複雑になるため採らなかった
- **flush での解釈できない行**: 書き戻して消える前に `.bak` を 1 度だけ作る（load 時に作った場合は作らない）。スキーマは変更なし
- **`cargo xtask build-dict`**: `.tmp` に書いて rename（mmap 済みの辞書を同じ inode で上書きしない）
- **FFI**: `nagi_handle_line` の `line` の NULL を検査。`nagi_server_free` で `Server` を捨てる前に学習のフラッシュを試みる
- **ユーザー辞書オーバーレイ**: `lookup_prefix` を読みの登録順で安定化（`order` を併用）

### H-21. 絵文字・顔文字の読みからの候補（2026-10-14、要レビュー）

- **実装**: `nagi_rewriter::EmojiRewriter` が読みの完全一致で候補を返し、`Pipeline::segment_candidates` の Rewriter グループの末尾に足す（出所は既存の `CandidateSource::Rewriter`。プロトコル型は不変）。順位は `ranking.source_order` の rewriter の位置に従うので、既定ではユーザー辞書・学習・LLM・システム辞書より後ろ。ユーザー辞書が同じ表記を持てば先に出て、出所は両方付く
- **表**: `config/emoji.tsv`（自作。外部辞書の転記なし）。`[emoji]` / `[kaomoji]` のセクション行で種類を分ける（設定で別々に切れるため。仕様の `読み<TAB>候補...` に対する追加）。読みはひらがなのみ。肌色修飾なし。同じ読みが両セクションにあるのは可（絵文字 → 顔文字の順）
- **入力中の候補**: 既存の `compute_composing_items` が先頭文節の `segment_candidates` を呼ぶので、先頭文節の読みが表に完全一致するときだけ絵文字が出る（前方一致・部分一致なし）。エンジンの変更なし。ただし入力中候補は上限 12 件なので、辞書候補が多い読みでは末尾の絵文字が切れる
- **設定**: `rewriter.emoji` / `rewriter.kaomoji`（bool、既定 true、level=detail、platforms=both、グループ「表記の候補」）。ホットリロード対応（`Tunables` → `Effective`）。`docs/ios-settings.md` の旧 `convert.emoji_candidates` はこの 2 項目に置き換え


### H-22. iOS リリース準備のユーザー判断（2026-10-14）

`docs/ios-device-checklist.md` §0 と `docs/ios-release.md` §5 の要判断事項への回答。

- **(a) 診断ログ**: TestFlight 期間中は**入力内容を含めて既定 ON**（現状どおり。判定は `DiagLog.isTestBuild`＝DEBUG または `sandboxReceipt`）。App Store 配布版は**既定 OFF・内容を含めない**。パスワード欄は内容を伏せる（`DiagLog.quote(redact:)`、現状どおり）。`CLAUDE.md` の「ログに入力内容を残さない」に対する、**テスト配布期間に限った例外**として `CLAUDE.md` に一文を追記した。App Store 版への切り替えは `isTestBuild` の判定で自動（コード変更不要）。実装済みの範囲は変更なし。「入力内容を含まない」ログ形式（件数・時間・エラーコードのみ）は App Store 版では未実装で、既定 OFF にして記録自体を止めている（ユーザーが明示的に ON にした場合は内容を含む）
- **(b) `RequestsOpenAccess`**: `true` のまま（H-18）。審査メモでの用途説明は `ios-release.md` §1.4
- **(c) iOS の既定モデル**: **small。ただし端末ごとに自動選択**する。`NagiModelRecommendation.recommend(budgetMB:, available:)`（純粋関数。`Shared/ModelRecommendation.swift`）が、`予算 − (モデルのサイズ + 20MB) ≥ 40MB` を満たす最大のモデルを返す。定数: 実行時の見込み 20MB（実測の small ピーク約 78MB に対し、モデル全体 70MB を足しても余る見積り）、安全マージン 40MB。実機の拡張（約 177MB）では small（要 130MB 以上）が選ばれ、xsmall は 80MB 以上で選ばれる。どれも載らなければ推奨なしで、`DataPaths.selectModel` は最小のモデルにする（コア側も 150MB 未満では LLM を読まない。H-8）
  - **優先順位**: Picker での明示選択 → 推奨 → （予算が測れないとき）既定名 small → 最大サイズ。従来の「未選択のとき xsmall 優先」（H-10 の暫定）は廃止
  - **予算の取得**: 拡張は `ProductSession.make` で `MemoryProbe.processBudgetMB()`（footprint + `os_proc_available_memory`）を計算して使い、`DataPaths.recordExtensionBudgetMB` で App Group の UserDefaults に記録する。ホストアプリの `os_proc_available_memory` は数 GB を返し拡張の予算の代用にならないため、ホストの表示・Probes・設定は**記録済みの拡張の予算**を使い、拡張が一度も起動していなければ**仮定値 177MB**（H-6/H-9 の実測）で計算する。これにより、ホストの「使用」表示と拡張が実際に読むモデルが（拡張起動後は）一致する
  - **表示**: データ画面のモデル欄に「推奨: small（この端末の予算 177MB に対して余裕あり）」を出す
  - **残る確認**: small にすると同期の待ち 30ms を超えた結果が約 0.6 秒後に差し替わる（H-10）。他機種（予算の小さい端末）での実測、予算のばらつき（拡張の起動時のページ状況で footprint + 残りが揺れる）は未確認
- **暗号化申告**: `ITSAppUsesNonExemptEncryption=false` を維持する。TLS（`ureq`/rustls/ring）は `nagi-ffi` から外し済み（`ios-release.md` §0.2）で、残る `ed25519-dalek`・`sha2` は認証・ハッシュ用途のみ。最終バイナリの確認（`nm`/`strings` で `rustls`/`ring` が無いこと）は CI のビルド後に残る
- **PrivacyInfo / AppleKeyboards**: `AppleKeyboards` の読み取りは削除済みのまま、マーカーのみで判定する。`CA92.1` の追加も維持
- **iPad**: iPhone 専用（`TARGETED_DEVICE_FAMILY: "1"`、設定済み）。13 インチのスクリーンショットは不要
- **表示名**: `Nagi`（設定済み）。App Store のアプリ名 `Nagi 日本語キーボード` は App Store Connect での作成時に確保できるか分かる
- **診断ログ・Probes（App Store 版）**: About の「開発者向け」セクション（端末情報・Probes・結果）を `DiagLog.isTestBuild` のときだけ表示する（App Store 版では隠す）。キーボード側の検証 UI は元から `isTestBuild` 限定
- **サイト**: Cloudflare Pages に置く（ドメイン `nagi.shiki-01.com`、DNS・カスタムドメインの設定は後で）。手順は `web/README.md`。`/privacy` の【要確認】欄（提供者名・施行日・アクセスログ・年齢区分・連絡窓口）は**空けておく指示のため触らない**（公開前に埋める必要がある点は変わらない）
- **未決のまま**: フルアクセス OFF での動作（§0.3）、モデルの配布方針（§4）、U-1、ed25519 更新鍵、カテゴリ・連絡先・TestFlight 外部テスターの扱い

### H-23. Backspace で残った未変換のローマ字を保留に戻す（曖昧入力・ローマ字の引き継ぎ）（2026-10-14、要レビュー）

- **背景**: 「kyou」を打つつもりで `kypu` と打つと、`ky` は `p` で変換規則に当たらず未変換の ASCII（Literal 扱い）として確定し、`pu` が「ぷ」になる（`kyぷ`）。「ぷ」を消しても `ky` は固定のままで、続けて `ou` と打っても「kyおう」になっていた。G-49（直前の 1 キーの巻き戻し）は「ぷ」のように保留が空になる入力には効かない
- **決定**: `Composer::backspace` が確定済みのかな（種別 Japanese）を 1 文字消した結果、カーソル直前に「ローマ字の取りこぼし」の ASCII 小文字が連続して残っていれば、その末尾から**有効な接頭辞（テーブルに、それより長いエントリがある）になる最長の部分**を、確定済みの文字から取り除いて保留に戻す。`kypu`→BS→保留 `ky`→`ou`→「きょう」。戻した後の BS は保留を 1 文字ずつ消す（`ky`→`k`→空。G-49 と同じ経路）。断片が無効（続きが無い）なら従来どおり固定のまま
- **「取りこぼし」の判定**: `Composer` が各文字に「変換器が素通し（`Emit::Raw`）した ASCII 小文字か」の印を持つ（`chars` の 3 つ目の要素）。Shift 入力の大文字・literal_continue 中の英字・数字・`load_pieces` で復元した文字には付かない（戻さない）。保留の確定（カーソル移動など）で素通しされた断片にも付く
- **条件**: 消したのが Japanese の文字のときだけ働く（取りこぼしそのものを消したときは戻さない）。カーソルが文中でも、直前が取りこぼしなら同じ（保留はカーソル位置に付随）。Rollback（G-49）や保留の削除では何もしない
- **設定**: `input.romaji_carryover`（bool、既定 true、詳細モード、platforms=both）。`config/default.toml`・記述子・`EngineConfig.romaji_carryover`・`Composer::set_carryover`。`false` で従来の挙動
- **タッチ入力**: QWERTY の `insert_reading(Text)` も同じ Composer を通るので同様（`touch.rs` のテスト）。12 キーのかなは非 ASCII なので対象外
- **ライブ変換**: engine の Backspace 処理は変更なし（変換表示の破棄・抑制は従来どおり）。reading は保留を含まないので、保留に戻した分だけ短くなる
- **テスト**: romaji の `test_carryover_*`、golden `30_composing_edit.yaml`（`composing-carryover-*`。`config.romaji_carryover` を追加）、`touch.rs`、config の既定値テスト

### H-24. 曖昧入力 M1（キー近接 + 辞書による「もしかして」候補）（2026-10-15、要レビュー）

- **方針（確定済み）**: 読み（ひらがな）が真実で、入力中のプリエディットは打った通りの読みを保つ（自動置換しない）。補正は「もしかして」候補として候補に混ぜる（`CandidateSource::Fuzzy`）。M1 はキー近接の隣接表 + 辞書だけ（座標も LLM も使わない。M2 でタッチ座標、M3 で LLM 再採点）。既定オフ・候補のみ
- **新クレート `core/fuzzy`（`nagi-fuzzy`）**: 依存は `nagi-romaji` のみ（辞書には依存せず、`ReadingIndex` トレイトで trie を問い合わせる）。
  - 隣接表: `KeyLayout { keys: [{id, label, x, y, w, h}] }` から中心間距離で近傍と距離順位を計算（`DEFAULT_REACH` = 1.3 キー幅）。M1 は標準 QWERTY の矩形配置（段のずれ 0 / 0.25 / 0.75）を内蔵。j の近傍は h・k・u・n・m・i
  - 編集操作（**利用者の打ち間違い**の側の名前）: 置換（隣接キー。順位に応じて 2.0〜3.0）、脱落（打ち忘れ。補正は任意の 1 文字の補い。3.0）、挿入（余計な打鍵。補正は削除。直前文字の重複と前後キーに隣接する文字だけ。3.0）、隣接 2 文字の転置（2.0）。かな直接入力は濁点・半濁点・小書きの付け忘れ/付け過ぎ（1.5）と同じ行内の隣の文字（中央↔各方向と周回で隣の方向 2.5、反対方向 3.0）。コストは負の対数尤度風の相対値で、定数はコードのコメントに根拠と調整可能性を記載（`edit.rs`）
  - 探索: コスト順の最良優先探索（状態 = 入力位置・出力した読み・変換器の保留。同じ状態は最小コストだけ残す）。累積コスト上限と最大ノード数・時間（2ms）で打ち切る。決定的（時間による打ち切りだけが環境に依存する）
  - 辞書による枝刈り: 展開途中の読みが「辞書の語の連なり + 最後は語の接頭辞」として成立し得るかを `Frontier` で差分更新しながら判定し、成立しない枝は作らない。辞書には 1 文字の読みがほぼ全部あり、何でも語の連なりになってしまうため、**2 文字以上の語か助詞・活用語尾の 1 文字だけ**を語の片として数える（ヒューリスティック。`SINGLE_KANA_PIECES`）。最初の実装（読み全体を毎回走査）は 1 ノード約 12µs で、2ms の間に約 150 ノードしか進めず正解率が約 30% だった。差分更新 + ローマ字テーブルの接頭辞集合化（`RomajiTable::has_longer_prefix` が全エントリ走査だった。キー入力経路も速くなる）で数倍速くし、2ms の打ち切りでも正解率が約 40% に届くようになった
  - 読みから打鍵列の復元: バックエンドの入力は読みだけなので、`ReverseRomaji` が仮名を最短の綴り（si / shi なら si。促音は子音を重ね、ん は次が母音・y・n なら nn）に戻す。実際の綴りと違うと隣接キーの当て推量がずれる（M2 以降で実際の打鍵列を渡す余地）。未変換のまま残った英小文字（`kamji` の m）はそのまま打鍵列に入る
  - 展開の対象: 読みが 3〜24 文字で、ひらがな・長音・英小文字だけ。1〜2 文字は何でも辞書語になり誤補正が増えるため対象外
- **統合（`nagi-server`）**: `fuzzy.rs` の `FuzzyEngine::suggest`。(1) 読みが辞書語・ユーザー辞書・スニペット・学習として既知なら何も出さない（クリーンな入力を補正しない）、(2) 展開した補正読みごとに辞書の Viterbi で最良の表記と総コストを求め、**「補正後の読みのコスト + 編集コスト × 500」が元の読みのコストより小さい**ものだけを残す（雑音のある通信路の考え方。500 は Mozc の語コスト `-500 ln p` の尺度に換算する係数）、(3) 元の読みがかなだけで辞書の語の連なりとして成立する場合は、別の語への補正がクリーン入力の誤訂正になりやすいので編集コストに 5 倍の係数を掛ける（評価で係数 1 だと辞書語でない文節の約 2 割に補正候補が出た）。補正後の表記は 1 つの補正読みにつき最良の 1 件、重複と通常候補にある表記は除く、`input.fuzzy.max_candidates` 件まで
  - `Pipeline::segment_candidates` が通常の候補の後ろ（`merge_by_order` の既定順の末尾。`ranking.source_order` に `fuzzy` を書けば位置を変えられる）に `Fuzzy` の候補を足す。`limit` で切るときも補正候補は押し出されない（通常候補の方を切る）。`ConversionBackend::fuzzy_candidates`（既定は空）を新設し、文節に割れた入力の読み全体に対する補正に使う
  - `BackendCandidate` は表記と出所だけで、「補正後の読み」は持たせていない（struct の全生成箇所に影響するため見送り。UI の「もしかして: かんじ」の表示は M2 以降で、`covers` のように任意項目として足せる）
- **エンジン側の変更（最小限）**: 未変換のまま残った英小文字（`kamji` の m）は Composer では Literal 扱いで、`candidate_reading` が None になり入力中の候補が出ず、Space 変換でも m が独立した文節に割れて補正が届かない。そこで `EngineConfig.fuzzy_input`（`input.fuzzy.enabled`）が true のときだけ、(a) `Composer::leftover_reading`（保留なし・日本語の文字と「ローマ字の取りこぼし」だけから成り、取りこぼしを 1 文字以上含む入力の全体）を入力中の候補の読みにする、(b) Space 変換で補正候補があれば、入力全体を 1 文節（表記は打った通りの読み）にして候補ウィンドウを開く（先頭が打った通りなので Enter で確定すれば打った通り）、(c) 入力中の候補は補正候補の枠を 3 件まで確保してから通常候補を詰める、(d) 文全体の N-best 表示（`initial_view = sentence`）の末尾に補正候補を足す。Shift 入力・数字・literal モードの英字を含む入力は対象外。無効のときの挙動は変わらない
- **設定**: `input.fuzzy.enabled`（bool、既定 false、simple、both）、`input.fuzzy.strength`（gentle=3.0 / normal=4.5 / aggressive=6.5 の累積コスト上限。既定 normal、simple）、`input.fuzzy.max_candidates`（1〜5、既定 3、detail）、`input.fuzzy.kinds`（`substitute,delete,insert,transpose,voicing` のカンマ区切り。既定は全部、detail、記述子の型は Text）。TOML は入れ子の `[input.fuzzy]`。記述子のキーが 3 階層になるため、`settings.rs` のキー分割・書き込み・読み取りを入れ子のテーブルに対応させた（最後の `.` で分ける）
- **`CandidateSource::Fuzzy` の追加**（加算的）: Swift の型を再生成。macOS の `CandidatePaging.label(for:)` の `switch` が網羅的なので `case .fuzzy: return "もしかして"` を足した（iOS は `switch` なし）
- **辞書クレートへの追加（読み取りの API のみ）**: `Dictionary::has_reading_prefix`（読みが prefix で始まる語があるか）と `reading_prefix_lens`（`common_prefix_lookup` の語を作らない版）。枝刈りは「展開した読みを接頭辞とする語が辞書にあるか」を引く必要があり、既存の `common_prefix_lookup`（読みが入力の接頭辞である語）では答えられないため
- **評価**（`nagi-fuzzy-eval`。Mozc の evaluation.tsv 602 文を辞書の文節に分けた 793 文節（3〜24 文字、dedup 済み）。各文節をローマ字に戻し、1 打鍵ごとに確率 p で隣接置換・脱落・挿入・転置のどれかを注入。ノイズは補正側と同じ隣接表から作るので実際の打ち間違いより楽観的。`--seed 1`、release、このサンドボックスの CPU）:
  - normal（累積コスト上限 4.5、候補 5 件）: 正解の表記が上位 5 件に入る率 p=0.02: 39.3%（46/117）、p=0.05: 40.1%（107/267）、p=0.10: 34.0%（147/432）。1 位は 34.2% / 36.3% / 30.1%。何か出た率は 54〜60%。seed 7 でも 42.5% / 42.8% / 36.6%
  - gentle（3.0）: 40.2% / 41.6% / 34.3%（探索が速く、normal と同程度）。aggressive（6.5）: 26.5% / 20.6% / 19.2%（探索空間が広く 2ms で打ち切られて悪化）
  - **クリーンな入力での誤訂正率**: クリーンな文節 0/793（0.00%）、辞書語そのものでない文節 0/394、24 文字以下の文 1/602（0.17%）。5 倍の係数を入れる前は 10.3%（文節）/ 20.8%（辞書語でない文節）で、係数を 1.0→3.0→5.0 と変えて選んだ（`VALID_READING_PENALTY`。正解率とのトレードオフ: 係数 1: 約 48% / 誤訂正 10%、係数 3: 約 44% / 0.6%、係数 5: 約 40% / 0%）
  - 展開のレイテンシ（辞書の枝刈り込みの展開のみ）: p50 1.6ms、p95 3.5ms。表記への変換まで含む提案全体: p50 2.2ms、p95 4.6ms、max 約 6ms。2ms の打ち切りは 1 ノードの処理が終わるごとに判定するため p95 が 2ms を超える。Space 変換の予算（`sync_budget_ms` = 150ms）には余裕で収まるが、「展開は 2ms 目安」は p50 でしか満たしていない
  - 取りこぼし（上位 5 件に入らなかったもの。`--verbose` の目視による）の主因: ノイズが 2 回以上入って累積コスト上限を超える、元の綴りの当て推量のずれ（si / shi 等）、補正後の読みが別の語に変換される（同音の別表記）、文脈が要る
- **未解決・次の段階**: M2 でタッチ座標（実際の打鍵位置の確率）と実際の打鍵列の受け渡し、M3 で LLM による再採点。ライブ変換への反映なし（候補のみ）。1 つの補正読みから複数表記を出さない。補正候補の選択を学習に回すか（現状は通常の確定と同じ扱いで、`かmじ` → 感じ が学習される）の判断。`SINGLE_KANA_PIECES` と係数 500 / 5 は暫定値
- **テスト**: `nagi-fuzzy` の単体テスト 24 件（隣接表・各操作のコスト・上限・枝刈り・種類の切り替え・決定性）、`server/tests/fuzzy.rs`（設定オフ・クリーン入力・max_candidates・strength・kinds・limit での保護・ユーザー辞書・エンジン経由の入力中の候補と Space 変換）、config の既定値・入れ子 TOML の読み書き、romaji の `leftover_reading`

### H-25. 曖昧入力 M2（実際の打鍵列・タッチ座標・キー矩形・補正後の読み）（2026-10-15、要レビュー）

- **方針**: H-24 の続き。M1 の「読みからの最短綴りの復元（`ReverseRomaji`）」を、実際の打鍵列があるときの**フォールバック**に格下げし、座標つきの確率モデルを加算的に足す（プロトコルの追加はすべて任意。既存フロントはそのまま動く）
- **実際の打鍵列の保持（`nagi-romaji`）**: `Stroke { ch, touch, flick }`。`Composer` が `chars` と並行して、各文字を作った打鍵（`keys: Vec<Vec<Stroke>>`）と保留の打鍵（`pending_keys`）を持つ。割り当て: 変換器の `feed` / `flush` の出力になった打鍵（保留に残らなかった分）を、素通し（`Raw`）は 1 文字に 1 打鍵、変換結果は先頭の文字にまとめて（`kya` → きゃ は き に `k,y,a`、ゃ は空）。BS は保留の打鍵を 1 つ戻す、G-49 の巻き戻しは「消した出力の打鍵 + 保留の打鍵」から最後の 1 つ（今消した打鍵）を除いたものを保留の打鍵に戻す、H-23 の引き継ぎは取り除いた取りこぼしの打鍵をそのまま保留の打鍵にする、Delete・部分確定（`remove_front`）・`clear` は文字と同時に消える。かな直接入力・数字・Shift 英字・`load_pieces`（変換取消・`restore_session`）・`replace_before_cursor`（トグル / 濁点）の文字は打鍵なし。**確認方式**: 打鍵列を使う側（`Fuzzy::strokes_reproduce`）が、打鍵列を変換器に通して `flush` した結果が読みと一致するときだけ使い、一致しなければ（`きゃ` の `ゃ` だけ消した等で対応が崩れたときを含め）`ReverseRomaji` に落とす。これで割り当ての細部が崩れても誤った補正にはならない
- **座標は preedit の間だけ**: `Stroke.touch` は Composer の `keys` にあるだけで、確定・破棄・`focus_out`・部分確定（`clear` / `remove_front`）で消える。ログ・`Debug` 出力のどこにも出していない（サーバーは入力パラメータをログに出さない）。候補を作るときだけ `TypingSnapshot`（所有）にコピーして `TypingHint`（借用）でバックエンドへ渡し、保持しない
- **座標つき確率モデル（`nagi-fuzzy`）**: `TouchModel::substitute_cost(pressed, intended, rel, σ)` = `TOUCH_PRIOR(1.2) + max(0, d_intended² - d_pressed²) / (2σ²)`（上限 6.0）。`d` は指の位置（押されたキーの矩形内の相対位置 `rel` をキー座標に直したもの）からキー中心までの距離を**キー間隔（ピッチ）**で割ったもの（横・縦を別々に正規化）。押されたキーを狙った場合と隣を狙った場合の等方ガウスの対数尤度の差に、置換という誤りの事前コストを足したもの。境界に近いほど `d_intended² - d_pressed²` が 0 に近づき安い（単調）。座標のない打鍵は M1 の離散コスト（`substitute_cost(rank)`）。置換以外（脱落・挿入・転置）は座標を使わない。`Params.use_touch` / `touch_sigma` を新設
  - **σ の既定値 = 0.5（キー間隔比）**。実際の指のばらつき（0.2〜0.35 程度）より広く見積もる。評価で、σ をノイズと同じ 0.25〜0.35 にすると、累積コスト上限（4.5）に対して座標つきの置換が高くなり過ぎ、ノイズが大きい（σ_noise=0.32）ときは M1 より悪化した（σ=0.25 で 22.7% < M1 35.8%）。σ を 0.25/0.35/0.45/0.55/0.7 と上げると上位 5 件の正解率は単調に上がり（σ_noise=0.32: 22.7 → 35.8 → 44.0 → 50.1 → 55.2%）、クリーン入力の誤訂正は 0.55 から現れる（0.13%、0.7 で 0.25%）。0% のまま頭打ちに近い 0.5 を採った
  - **隣接の導出**: `KeyLayout::pitch()`（同じ段の隣のキーの中心間隔の中央値 = 横、段の間隔の中央値 = 縦）で距離を正規化してから `DEFAULT_REACH`(1.3) 以内を近傍とする。隙間のある縦長のキー（実機の QWERTY）でも、横 2 + 斜め 4 が近傍に入る（内蔵 QWERTY は従来と同じ結果）
- **`set_layout_geometry{session_id?, layout_id, mode, keys:[{id,label,x,y,w,h}], width, height}`**: 座標系はキーボード view のローカル座標（左上が原点、`width` × `height`、単位は問わない。正規化座標でもよい）。`label` が 1 文字の英字のキーだけを使い（大文字表示でもよい）、**英字キーが 10 個未満のレイアウト（12 キーのかななど）は無視**する。`session_id` 省略は既存の全セッションと以後の新規セッションの既定。受け取るまでは内蔵の QWERTY。`Engine::set_layout_geometry` が `Arc` で保持し、サーバー（`FuzzyEngine::fuzzy_for`）が内容が同じ間はキャッシュした `Fuzzy` を使い回す。`mode` は診断用で、挙動には使っていない
- **座標の渡し方**: 既存の `InsertReadingParams` に項目を足すと Swift の `init` が変わり iOS のコードが壊れるため、`InsertReadingTouchParams { session_id, input, touch?, flick? }`（上位互換）を新設し、サーバーは `insert_reading` をこの型で受ける（従来の形も読める）。`touch: {x, y}` は**押されたキーの矩形内の相対位置（0..1）**で、QWERTY の 1 文字（`ReadingInput::Text`）にだけ意味がある（複数文字では無視）。絶対座標は採らない（キー矩形の座標系とフロントの座標系を合わせる必要がなくなる）。`flick: {angle_deg, distance}` は 12 キーのフリック（`Key12`）用
- **フリック角度（最小実装）**: `Stroke.flick` を Composer が非 ASCII の文字（かな）に付けて持つ。`KanaTable::flick_cost`: 隣り合う方向（左-上-右-下の周回）への取り違えを、角度が 2 方向の境界（中間の 45° 刻み）にあるとき `COST_FLICK_BOUNDARY`(1.5)、境界から 45° 離れれば通常の `COST_ROW_NEAR`(2.5) になる直線補間にする（それ以上離れれば割り引かない）。反対方向・中央との取り違えは通常のコスト。`distance` は現状使っていない（受け取るだけ）。読みと同じ長さのフリック情報があるときだけ使う
- **補正後の読みを候補に持たせる**: `CandidateView.reading: Option<String>`（`BackendCandidate.reading`、エンジン内部の `Candidate.reading` / `ComposingItem.reading` も）。`Fuzzy` 由来の候補だけに付く（フロントは「もしかして: かんじ」と表示できる）。選ぶと**補正後の読みで確定・学習**する: 入力中の候補は `select_composing` が学習通知の `Segment.reading` を補正後の読みにし、変換中は `notify_commit` が選択中の候補の `reading` を使う。ユーザーが選んだ以上は通常の確定と同じ扱いで、確定取消（`UndoCommit`）で学習から外れる既存の仕組みに従う。文全体 N-best の補正候補は文節の `reading` を補正後の読みにしたので、Esc で変換を取り消すと入力が補正後の読みに戻る（選んだ以上は補正を受け入れたとみなす）
- **バックエンド境界**: `ConversionBackend::fuzzy_candidates(reading, typing: &TypingHint, left_context, limit)`。`TypingHint { strokes, flicks, geometry }` はすべて任意。エンジンは**入力中の読みが Composer の入力全体のときだけ**打鍵列を渡す。入力中の候補は、1 文節に収まる入力では `segment_candidates` ではなく `fuzzy_candidates`（打鍵列つき）の結果を使う。Space 変換（`start_converting_fuzzy`）は文節の候補の補正候補を、打鍵列つきで求めたものに差し替える。複数文節に割れた入力の「先頭文節だけの補正」と、2 文節目以降の候補（`ensure_candidates`）は、文節と打鍵列の対応を持たないので従来どおり読みからの復元で推定する
- **設定**: `input.fuzzy.use_touch_coords`（bool、既定 true、detail、platforms=ios）。false で座標・フリック角度を使わず離散コストにする。`config/default.toml`・記述子・`FuzzySection.use_touch_coords`・`FuzzySettings.use_touch`
- **`input.fuzzy.enabled` の既定**: コアの既定は false のまま（デスクトップ）。評価（下記）のクリーン入力の誤訂正は 0.00〜0.13%（< 0.5%）なので、**iOS は `ProductSession.swift` が書く config.toml に `[input.fuzzy] enabled = true` を明示する**方式にする（要 Swift 側の変更。コアに「iOS の既定値上書き」の仕組みは足していない）。注意: 評価の「クリーン」は辞書に載る語の文節なので、固有名詞・俗語・新語を多く含む実際の文では誤訂正がこれより多いはず（実機で確認する）
- **評価**（`nagi-fuzzy-eval`、release、`--seed 1`、793 文節。各打鍵を狙ったキーの中心からのガウス乱数の位置に置き、中心が最も近いキーが押されたとする。同じ入力を M1（座標なし・離散コスト）と M2（座標つき）で補正。通常変換で正解済みの文節は除く。σ_noise はキー間隔比）:
  - σ_noise=0.18: ノイズ入り 64 件。上位 5 件の正解率 M1 51.6% → M2 75.0%、1 位 45.3% → 71.9%
  - σ_noise=0.25: 360 件。M1 50.0% → M2 66.4%、1 位 44.2% → 61.1%
  - σ_noise=0.32: 609 件。M1 36.0% → M2 48.4%、1 位 31.4% → 44.0%
  - seed 7: σ_noise=0.18: 66.0% → 85.1%、0.25: 48.6% → 66.2%、0.32: 35.8% → 45.2%
  - **クリーン入力の誤訂正（M2、座標つき。誤打鍵が出ない試行に限る）**: seed 1 で 0/793（0.00%）、seed 7 で σ_noise=0.32 のとき 1/753（0.13%）、他は 0%。座標なし（M1）は H-24 のとおり 0/793、文（24 文字以下）は 1/602（0.17%）
  - p 注入のノイズ（H-24 の評価）は座標を持たないので M1 と同じ（上位 5 件 38.5% / 39.3% / 34.0%、p=0.02/0.05/0.10）。レイテンシは H-24 と同じ（提案全体 p50 約 2.3ms、p95 約 5ms）
  - 評価の限界: ノイズは補正側と同じ幾何（隣接キー・ガウス）から作るので、実際の指の挙動より楽観的。σ_noise の実測値は実機で取る必要がある
- **テスト**: romaji の `strokes_*`・`flicks_*`（BS・巻き戻し・引き継ぎ・カーソル・Delete・部分確定・`clear`・`load_pieces`）、fuzzy の座標の単調性（境界に近いほど安い）・座標なしで M1 と同一・打鍵列の不一致時のフォールバック・非 QWERTY の隣接・フリック角度、`server/tests/fuzzy.rs` の `m2`（`reading` つき候補、補正候補の確定と学習（入力中・Space 変換の両方）、`set_layout_geometry` で `q` を `n` の下に置くと `kaqji` が補正される / `m` を遠ざけると補正されない / 12 キーの配置は無視される、境界の座標で補正される・`use_touch_coords=false` で座標を無視、座標なしの `insert_reading` と物理キーが同じ結果、JSON-RPC の受け付け）
- **デバッグビルドの展開予算**: `EXPANSION_BUDGET_MS` を debug では 50ms にした（release 2ms）。debug は約 10 倍遅く、2ms ではテスト（`kamji` の補正）が不安定に落ちたため。release の挙動は変えていない
- **未解決・次の段階**: `distance`（フリックの距離）と `mode` は未使用。文節に割れた入力の文節ごとの打鍵列の対応（今は読みからの復元）。M3（LLM 再採点）。Swift 側（geometry の送信、`touch` / `flick` の送信、`reading` の「もしかして: ○○」表示、iOS の config で `enabled = true`）。`CandidateView` の Swift の `init` に `reading:` が増えたので、`frontend-macos/Tests/.../PreeditAndCandidateTests.swift` と `frontend-ios-spike/Tests/CandidateKindTests.swift` の生成呼び出しに `reading: nil` が必要
  - **後に実装済み（2026-10-22 に実装を確認）**: 上の「Swift 側（geometry の送信、`touch` / `flick` の送信、`reading` の「もしかして: ○○」表示、iOS の config で `enabled = true`）」は `frontend-ios-spike/` に実装されている（`KeyboardModel.updateLayoutGeometry` / `NagiSession.setLayoutGeometry`、`inputText` の touch・`key` の flick を `NagiSession.insert` へ、`CandidateKind.fuzzyHint`、`ProductSession` の `[input.fuzzy] enabled = true`）。コミット d08893f は本リポジトリに存在しない（未確認）。Swift のビルド・テストの CI 確認と実機確認は、この注記の時点では記録がない。

### H-26. キーボード切替時のメモリ一時倍増への対策（LLM の遅延ロード・非表示時の解放・簡易モード注意の猶予）（2026-10-15、要レビュー）

- **事象**: キーボードを開いたまま別アプリへ切り替え、切替先でもキーボードが開いていると、旧拡張インスタンスが生きている間に新インスタンスが LLM をロードしてメモリが一時的に倍になり、オレンジのメモリ警告 → 辞書のみモードになる。他の状況では再現しない。
- **LLM ロードの遅延**: 起動時（コア作り直し時を含む）に `os_proc_available_memory` の空きが `NagiModelRecommendation.canLoadNow(model, availableMB)`（モデルのサイズ + 安全マージン 40MB。純粋関数）に満たなければ、辞書のみで始める（`ProductSession.shouldDeferLLM`）。辞書・拡張本体は既に使用量に含まれるので、`fits`（予算全体の式）のオーバーヘッド 20MB は足さない。マージンは `fits` と同じ定数で整合させた。空きが足りるまで 2 秒おきに確かめ、足りたら読み込む（`KeyboardModel.tryRestoreLLM`）。
  - **`reload_model` を使えない場合がある**: コアは起動時に読み込んだモデルのオブジェクトに対して重みを出し入れする（`Pipeline::unload_model` / `reload_model`）。`no_llm`・メモリ上限不足で起動するとモデルのオブジェクトが無く、`reload_model` では読み込めない。そのため **起動時に待たせた場合はコアを作り直して**（`restartCore`。入力中の文字列があれば入力が終わるまで待つ）読み込み、**一度読み込んだあとに解放しただけ**（メモリ警告・非表示）の場合は `reload_model` で読み込む。コア側の変更（起動時にモデルを持たない状態で始める口）が望ましければ別途判断。
- **非表示時の解放**: `viewWillDisappear` で `focusOut` のあとに `unload_model`（`KeyboardModel.suspendLLM`）。再表示の `focusIn` から、空きメモリを確かめて `reload_model`。旧インスタンスが先に軽くなるので、切替先の読み込みと重ならない。再読み込みは重い（small で約 0.6 秒）ので、短時間の表示・非表示の往復では辞書のみの時間が増える。
- **簡易モード（辞書のみ）の注意**: 辞書のみの状態が **10 秒を超えて**続いた場合のみ出す（`NagiModelRecommendation.shouldShowDictionaryOnlyNotice`）。閉じている間は数えず、再表示から数え直す。切替直後の一時的な辞書のみでは出さない。
- **メモリ警告後の再読み込みの待ち時間**: 従来は 逼迫の保持 60 秒（`memoryPressureHold`）+ コアの自動再読み込みの待ち 30 秒（`MODEL_RELOAD_DELAY`）+ 確認の間隔 15 秒 = 最長約 105 秒。**保持を 10 秒**にし、通常に戻ったら 30 秒を待たず、空きメモリが足りることを確かめて `reload_model`（即時）を送るようにした。確認の間隔は 2 秒。合計 約 10〜12 秒。
  - **理由**: 警告の直接の原因は切替直後の一時的な倍増で、旧インスタンスの解放（遅くとも数秒）で解消する。60 秒 + 30 秒は長すぎる。警告が再発する危険は、保持 10 秒に加えて空きメモリの確認（モデル + 40MB）で抑える。確認が通らなければ待ち続ける。
- **未確認（実機）**: 旧インスタンスのメモリが新インスタンスの `os_proc_available_memory` に反映されるか（プロセスが別なら反映されず、遅延ロードの判定は効かず、非表示時の解放が効く）。拡張の複数インスタンスが同一プロセスか別プロセスかの確認。`viewWillDisappear` が別アプリへの切替で確実に呼ばれるか。診断ログの `unload_model (keyboard hidden)` `reload_model ok=` `llm restored` で追う。


### H-27. キーボードが切り替わる・前の文字が消える（再発）の原因の仮説と対策（2026-10-15、要実機確認）

- **ログから読めた事実**（入力テストタブで入力欄を順にタップしたログ。キー入力は無し）:
  - `set_environment (attach)` のうち、`get_key_table` が続く回は **新しいプロセス**（`KanaKeyTable.fetchAttempted` はプロセスの静的変数で、プロセスごとに 1 回だけ取得する）。続かない回は **同じプロセス内で `UIInputViewController` が作り直された**回。約 80 秒で新しいプロセスが 10 回以上起動していた。
  - 新しい VC・新しいプロセスの前に `focusOut`（`viewWillDisappear`）が一度も出ていない（最後に閉じたときだけ出ている）。**旧 VC は `viewWillDisappear` を受けないまま入れ替わる**。H-26 の「非表示時の解放」はこの経路では働かない。
  - `get_key_table` の後に `focusIn` が無い回がある（VC は作られたが表示されていない。secure 欄で iOS が標準キーボードに差し替えた場合と考えられる）。
- **仮説**: `setupUI` は VC ごとに `ProductSession.make`（LLM 込みのコア）を作る。旧 VC が解放されないうちに新 VC がコアを作ると、同じプロセスで LLM が二重になり（small 70MB × 2 + 辞書で拡張の上限 約 177MB を超える）、メモリ警告を出す前に強制終了される（ログが残らない）。iOS は拡張が落ちると標準キーボードを出す。これが「出てくるキーボードが変わる」。H-26 の遅延ロードは `os_proc_available_memory` で判定するが、作り直しの時点で旧コアがまだ解放されていなければ足りると判定してしまうことがある。
  - 「前の文字が消える」の再発との関係: VC が入れ替わると、ホストの変換中の文字列と `HostCompositionApplier.shownMarked` の対応が崩れうる（ホストが変換中を確定・破棄したのに、拡張は表示中とみなして deleteBackward する）。H-28 で削除を使わない確定に変えた。
  - 入力テストタブの SecureField（`isSecureTextEntry`）は、iOS の仕様で常に標準キーボードになる（他社キーボードは使えない）。この欄を試したあとに標準キーボードのまま戻らないことがあるのは iOS の挙動で、Nagi の不具合とは分けて扱う。
- **対策**:
  - **コア（`KeyboardModel`）をプロセスで 1 つにする**（`KeyboardModel.processShared`）。VC が作り直されたら、既存のモデルを `handOverToNewHost()`（旧ホストの変換中を下線なしで残し、コアは focus_out で状態を初期化）してから新しい VC に付け替える。LLM を読み直さないので、作り直しのたびの読み込み（約 0.6 秒）と二重化が無くなる。
  - 旧 VC の遅れた `viewWillDisappear` / `textDidChange` / メモリ警告は、自分が現在のホスト（`model.host === self`）のときだけ処理する（新しい VC の状態を壊さない）。
  - **診断ログ**: `viewDidLoad instance=#n pid= reuseModel= footprint= available=`、`viewWillAppear` / `viewWillDisappear` / `deinit` の instance 番号、`previous keyboard process pid=… ended while visible`（表示中に終わったプロセスの検出。App Group の `keyboardVisiblePid`）。次の実機ログで、強制終了の有無と、VC の入れ替わりの順序を確定させる。
- **未確認**: 旧 VC がいつ解放されるか（`deinit` のログで確認）、別プロセスへの切替で旧プロセスがいつ終わるか。
- **後の決定**: 「LLM の二重化による強制終了」の仮説は H-31 で裏付けられず、H-33 で撤回した（原因は iOS のキーボード選択）。プロセス内でコアを共有する変更は作り直しの高速化として残す

### H-28. 確定の方式を「変換中の範囲を挿入で置き換える」に変更（2026-10-15、要実機確認）

- **経緯**: H-13（`setMarkedText("")` + `unmarkText` + `insertText`）→ c93768b（`unmarkText` → `deleteBackward`×N → `insertText`）→ 直近（キャレットを末尾へ置き直してから同じ手順）。最後の 2 つは、実機で手前の本文を確定した長さだけ消す。UITextView のテスト（`HostCompositionApplierTests`）では再現しない。「キャレットが変換中の先頭に残る」仮説に基づく修正は効かなかった。
- **判断**: ホストの変換中の範囲の扱いは `textDocumentProxy` 越しでは確かめられず、仮説を当て続けるのは不毛。**失敗しても本文を消さない方式を既定にする**（気配り: 入力を失わない）。
  - `insert`（既定）: `insertText(確定文字列)` のみ。UITextInput の `insertText` は変換中の範囲を置き換えて確定する。ホストが変換中の範囲を見失っていても、最悪は文字の残り（重複）で、手前の本文は消えない。
  - `mark`: `setMarkedText(確定文字列)` → `unmarkText`。
  - `delete`: 従来の手順（比較用）。
  - 設定 `keyboard.commit_method`（詳細モード・診断用）で切り替え、実機で比べる。
- **診断ログ**: `host commit` 行に方式・確定前の文脈（キャレット前の末尾 8 文字・後の先頭 4 文字）を出す。確定 0.3 秒後の照合は一致でも `host verify ok` を出す（従来は不一致の `host MISMATCH` のみ）。

### H-29. 変換中の注目文節の扱い（2026-10-15、ユーザーの指摘への判断、要レビュー）

- **指摘**: 部分変換中に Enter を押すと全体が確定し、使いづらい。
- **判断**: Enter で全体を確定するのは標準 IME と同じなので変えない。代わりに、
  - **候補を選ぶと次の文節へ進む**（最後の文節で選んだときだけ全体を確定）。iOS 標準の日本語キーボードの「候補を選ぶとその文節が決まり、残りの変換が続く」に近い。従来は、どの文節で選んでも全体が確定していた。
  - **候補バーの読み表示で注目文節を目立たせる**（太字・アクセント色。ほかの文節は薄く）。ホストの変換中の表示は `setMarkedText` の文字列のみで文節ごとの装飾ができないため、候補バー側で示す。
- **見送り**: 「注目文節だけをホストへ確定し、残りを変換中のまま続ける」操作は、コアに該当する Action が無い（`TouchAction` に部分確定が無い。Composing の `covers` による部分確定は H-14）。必要ならコアに `commit_focused`（先頭から注目文節までを確定）を足す。Phase 3 の範囲操作 UI と合わせて判断する。

### H-30. フリックの取り消し・長押しの取り消し・クリック音・英字トグル（2026-10-15、要実機確認）

- **フリックの取り消し**: 従来、フリックには取り消しが無かった（`LongPressCancel` は長押しのポップアップだけが対象）。押下位置から横にキー幅の 2 倍、または縦にキーの高さの 2 倍を超えて動かすと取り消し（`FlickCancel`）。ポップアップを半透明にして「取り消し」を出し、離しても入力しない。
- **長押しの取り消し**: 下方向の閾値をキーの高さの 0.5 倍 → 0.3 倍にし、ポップアップの左右の外へ項目 1 つ分を超えて離れた場合も取り消しにした。最下段のキーは下に指を動かす余地が無いため、横で取り消せるようにする。
- **クリック音**: `UIDevice.playInputClick()`（端末の「キーボードのクリック」設定に従う）は実機で鳴らなかった。`keyboard.sound` に `compat`（`AudioServicesPlaySystemSound(1104)`。消音スイッチに従う）を足し、既定にした。触覚の互換モード（`keyboard.haptic_engine = compat`）と同じ方式。
  - 補足: 提供されたログでは `feedback prepare haptic=off ... sound=false` で、触覚・音ともに設定がオフの状態だった。設定画面の値と一致しているかを実機で確かめる。
- **英字トグル**: 自分の操作から 0.5 秒以上たった `textDidChange` で、ホストの文脈が取れない（nil）だけで「外部の変更」とみなして置き換えを打ち切っていた。文脈は非同期で古い・取れないことがあるため、末尾が直前の文字と一致しないと確認できたときだけ打ち切る。原因が別にある可能性が残るので、`toggle key=… continued=… hadState=…` と `toggleState dropped …` のログで確かめる。

### H-31. H-27〜H-30 の実機結果と追加の修正（2026-10-16）

- **確認済み**: 確定で手前の文字が消える不具合は、確定の方式 `insert`（H-28）で解消した（`host verify ok` を確認）。部分確定後の全文確定の重複なし、変換中に候補を選ぶと次の文節へ進む（H-29）、フリックの取り消し（H-30）も期待どおり。触覚・音は互換モードで鳴る。
- **キーボードが切り替わる（未解決、仮説を修正）**: H-27 の診断ログで、新しいプロセスの起動は、ほぼ毎回 `previous keyboard process … ended while visible (no viewWillDisappear)` を伴っていた。ただし起動時の footprint は 4〜5MB、作り直し後も 50〜70MB で、拡張の上限（約 177MB）に遠い。H-27 の「LLM の二重化による強制終了」の仮説は、このログでは裏付けられない。文脈（`set_context tail=`）が二つのアプリの入力欄で交互に変わっていることから、プロセスの終了はアプリの切替に伴う iOS 側の終了（拡張はホストのアプリごとにプロセスが分かれ、旧プロセスには `viewWillDisappear` が届かない）とみるのが自然。
  - 同じプロセスで作り直した VC が、表示されずに（`viewWillAppear` なし）数秒後に次の VC に置き換わる例があった（13:02:31 → 13:02:38）。この間に iOS が標準キーボードを出し、ユーザーが地球儀で Nagi に戻した可能性がある。拡張側からは「どのキーボードが表示されているか」を知る手段が無い。
  - 次の切り分け: 標準キーボードが出た時刻・直前の操作（アプリの切替・入力欄のタップ・地球儀）をメモしてもらい、ログの `viewDidLoad` / `viewWillAppear` と突き合わせる。
- **英字の 12 キーのトグル入力**: 原因を特定した。`flick-alpha` のように `kana` 指定の無い flick キーは、`input.toggle_input` がオンでもタップで常に中央の文字を入れていた（トグルの扱いは `kana` 指定のキーと、レイアウトの `toggle` キーにしか無かった）。flick キーの文字（中央・左・上・右・下）を cycle にして、`toggle` キーと同じ置き換えの仕組みを使う。
- **触覚・音の互換モードで、速く打つと遅れる**: `AudioServicesPlaySystemSound` をメインスレッドで呼んでいたため。専用の直列キューで鳴らし、前の再生が終わる前に来た要求は捨てる（重ねない・入力を待たせない）。
- **フリックの取り消しの表示**: ユーザーの判断で、「取り消し」の表示は出さず、ポップアップの選択を消すだけにする。
- **照合の誤検知**: 確定直後に改行などを applier を通さずに入れると、0.3 秒後の照合が古い期待値で `host MISMATCH` を出していた。applier を通らないホスト操作でも照合を無効にする。
- **実機確認（ビルド 37、2026-10-16）**: 英字フリックのトグル入力、互換モードの触覚・音（速いタップで遅れない）、フリックの静かな取り消しは、いずれも期待どおり。未解決はキーボードが切り替わる件のみ（標準キーボードが出た時刻と直前の操作の情報待ち）。

### H-32. 入力欄の種類に合わせたレイアウトの自動選択と改行キーの表示（2026-10-16、要実機確認）

- **レイアウト**: `textDocumentProxy.keyboardType` が メール・URL・`asciiCapable` なら英字、数字系（`numberPad` など）なら `numbers` で開く（`InputTraitsMapping`）。指定のない欄へ移ったときは、自動で切り替えていた場合だけかなへ戻す。ユーザーが自分で切り替えたレイアウトは保つ。判定は focusIn と入力欄の切替（`documentIdentifier` の変化）で行う。
- **改行キー**: `returnKeyType` に合わせて「検索」「開く」「送信」「完了」「次へ」などを出す（入力中は従来どおり「確定」）。
- 設定 `keyboard.layout_by_field`（docs/ios-settings.md の案）での無効化は未実装（H-34 で追加済み）。
  - 後の決定（2026-10-16）: 実装済み（既定 true。false ならレイアウトの自動選択と改行キーの表示をしない）。

### H-33. 「キーボードが切り替わる」の原因（2026-10-16、ユーザーの観察で判明）

- **原因**: 端末に他社キーボード（azooKey）と Nagi の両方を入れていると、パスワード欄（iOS が標準キーボードを強制する）から通常の欄に戻ったときに、iOS が azooKey を選ぶことがある。どのキーボードに戻すかは iOS が決める（キーボードの一覧の順序・直前の使用状況による）。拡張側から「自分を表示させる」API は無く（`advanceToNextInputMode` は表示中の拡張から次へ送るだけ）、Nagi 側では直せない。
  - H-27 のログで「VC が作られたが表示されない」例があったのは、iOS が Nagi を起動しつつ別のキーボードを表示した場合と整合する。H-27 の「LLM 二重化による強制終了」の仮説は撤回する（プロセス内でコアを共有する変更は、作り直しの高速化として残す）。
- **対処（利用者側）**: 設定 > 一般 > キーボード > キーボード で Nagi を他社キーボードより上に並べる。検証中は azooKey を外すと切り分けやすい。
- **今後**: オンボーディング画面（`App/OnboardingView.swift`）に、一覧の順序についての案内を足すかは、App Store 配布前に判断する。

### H-34. 残り作業の判断（2026-10-16、ユーザーの指示「全部順番に進めて」による）

- **iOS の既定モデルは small**（ユーザー決定）。コードは既に、未選択時は端末の予算で推奨（予算に余裕があれば small）、既定名も small。文書の xsmall の記述を直した。
- **クリック音の `system` 方式（`UIDevice.playInputClick`）を削除**: 実機で鳴らなかったため。選択肢は `off` / `compat`（既定）。保存済みの `system` は正規化で既定に丸められる。
- **`keyboard.layout_by_field` を設定画面に追加**（既定 true）。
- **セットアップ画面に、キーボード一覧の並び順の案内を追加**（H-33）。
- **注目文節まで確定（`TouchAction::CommitFocused`、キーボード用 `Action::CommitFocused`）をコアに追加**: 変換中に先頭から注目文節（複数文節の選択中は選択の末尾）までを確定し、学習に通知する。残りは変換中のまま focus を先頭にして候補を出す。注目文節が最後なら通常の確定と同じ。入力中は通常の確定と同じ。確定した表記は左文脈の末尾に足す（secure では足さない）。iOS では、変換中に候補を選ぶと、その文節までを確定する（H-29 の「次の文節へ進む」を置き換える。iOS 標準の日本語キーボードと同じ動き）。
- **LLM の非表示時の解放（H-26）は現状維持**: 解放を遅らせると、別アプリへの切替で旧プロセスがメモリを持ち続け、H-26 の警告が再発しうる。開いた直後の約 0.6 秒は辞書のみになるが、`reload_model` は裏で読み込むので入力は止まらない。実機で不満が出たら、解放までの猶予（数秒）を設ける案を検討する。
- **App Store 配布前の作業（診断ログ既定 OFF の確認・審査の説明文など）は、ユーザーの指示により今回は扱わない。**
- **実機確認（ビルド 39、2026-10-16）**: 注目文節まで確定・セットアップ画面の案内・クリック音の 2 択は期待どおり。`keyboard.layout_by_field` をオフにすると検索欄の改行キーは「改行」に戻る。**メール欄で英字に切り替わらない**: 入力欄を移っても `documentIdentifier` の変化や `viewWillAppear` が来ない場合に、入力欄の種類を読み直していなかったと推定。`textDidChange` でも keyboardType・returnKeyType を見比べ、変わっていれば反映し直すようにした（要実機確認。診断ログの `input traits keyboardType=…` 行で確認）。
- **診断ログの保持は 64KB（超えたら後半 32KB を残す）で確定**（ユーザー決定）。
- **実機確認（ビルド 40、2026-10-17）**: メール欄（keyboardType=7）・URL 欄（3）で英字に切り替わることを確認（`input traits keyboardType=7 -> alpha`）。検索欄の改行キー表示も期待どおり。入力テストタブの 10 欄のうち、SecureField・`.password`・数字（`.numberPad`）の欄は iOS が標準キーボード（数字は標準の数字キーパッド）を出す。これは iOS の仕様で Nagi 側では変えられない。先頭の通常の欄で標準キーボードが出たのは、直前のパスワード欄などからの戻り先を iOS が選ぶ挙動（H-33）。入力テストタブの説明文をこの実態に合わせた。

### H-35. キーボード拡張の IsASCIICapable を true にする（2026-10-17、実機で判明）

- **事象**: 数字の欄（`.numberPad`）で、azooKey は出るのに Nagi は出ず、標準の数字キーパッドになった。
- **原因**: 拡張の `NSExtensionAttributes.IsASCIICapable` が false だった。iOS は ASCII だけを受け付ける欄（数字キーパッドなど）で、ASCII を入力できると申告していないキーボードを使わない。
- **対策**: `IsASCIICapable: true`（`frontend-ios-spike/project.yml`）。Nagi は英字・数字のレイアウトを持ち、ASCII を入力できる。数字の欄では H-32 の自動選択で数字のレイアウトが開く想定（要実機確認。診断ログの `input traits keyboardType=4 -> numbers`）。
- 補足（2026-10-17、ユーザーの観察）: ほかのアプリの通常のテキスト欄では Nagi が普通に出る。入力テストタブの先頭の通常の欄で標準キーボードが出たのは、直前のパスワード欄などのあとに iOS が戻り先を選ぶ挙動（H-33）で、通常の欄そのものの制約ではない。入力テストタブの数字の欄の説明文も H-35 に合わせて直した。
- 経過（2026-10-18、ユーザーの報告）: `IsASCIICapable: true` のビルドでも、数字の欄では標準と azooKey しか出ない。iOS がキーボードを有効化した時点の拡張属性を保持しており、アップデートだけでは反映されない可能性がある。確認手順は (1) 設定 > 一般 > キーボード > キーボード で Nagi を削除して追加し直す（効かなければ端末を再起動）、(2) 診断ログの `extension attributes`（拡張の起動時に、バンドルに入った `NSExtensionAttributes` を出す）で `IsASCIICapable=1` を確認する。どちらも満たして出ない場合は別の原因を調べる（未解決）。
- 調査（2026-10-21、コードと設定のみ）: project.yml の記法・Info.plist 生成・CI に不備なし（拡張の plist は XcodeGen が毎回生成し、上書き箇所もない）。切り分けのため入力テストタブに `.decimalPad` と `.asciiCapableNumberPad` の欄を追加した。
- **結果（2026-10-21、ビルド 43 の実機・診断ログ）**: `extension attributes IsASCIICapable=1 PrimaryLanguage=ja-JP RequestsOpenAccess=1` が毎回出ており、属性は拡張に反映されている。`.numberPad`（keyboardType=4）・`.decimalPad`（8）・`.asciiCapableNumberPad`（11）で Nagi が数字レイアウト（`-> numbers`）で開くことをログで確認した。メール（7）・URL（3）は `-> alpha`。したがって「`.numberPad` では iOS が第三者キーボードを出さない」という仮説は誤りで、`.numberPad` に Nagi が出ない原因は属性の反映（キーボードの追加時点で登録されるため、追加し直しが要る）だった。
- **自動選択は iOS の挙動で、Nagi 側では直せない**: 入力テストタブで、地球儀キーで切り替えずに Nagi が自動で出たのはワンタイム・URL・小数・検索の欄だけで、通常の 1 行欄や複数行 TextEditor では標準キーボードが出た（一番上の複数行欄は Nagi）。azooKey を外しても、標準の日本語・English を外して Nagi だけにしても変わらなかった。欄の種類（keyboardType）と自動選択に規則性は見えず、iOS が「直前に使ったキーボード」「アプリごとの履歴」「欄の属性」から決めていると見られる（H-33 と同じ種類の問題）。拡張には選択を強制する公開 API が無い。ログの `viewWillAppear` は地球儀キーでの手動切替でも出るため、「Nagi が表示された」ことは自動選択の証拠にならない。拡張が選ばれなかった欄はログに何も残らない。
- 未解明の点: ログに `viewDidLoad` の直後に `viewWillAppear` が出ない箇所がある（拡張は起動したが表示されなかった）。iOS の先読みか、読み込みが間に合わずに標準へ戻った場合かをログから区別できていない。優先度は低く、対応は見送る。

### H-36. キーのアイコンに Iconify を使えるようにする（2026-10-22、ユーザー指示、要実機確認）

- **目的**: レイアウトの `icon` に SF Symbols 以外（Iconify の約 20 万アイコン）を使えるようにする。
- **方針（ユーザー確定）**:
  - **キーボード拡張は通信しない**（既存方針を維持）。取得はホストアプリのレイアウト取り込み時だけで、結果を App Group の `keyboard/assets/icons/`（`<prefix>--<name>.png` と `licenses.json`）に保存し、拡張はローカルファイルを読むだけ。
  - `icon` の書式: `sf:<symbol 名>`（SF Symbols。明示）、`iconify:<prefix>:<name>`（例 `iconify:mdi:home`）、接頭辞なしは従来どおり SF Symbols 名（後方互換）。prefix / name は小文字英数字とハイフン区切り、全体 64 字以内。検査は `config/keyboard/layout.schema.json` と `LayoutLoader.decode`（`KeyboardIconRef.formatError`）の両方。
  - 解決は「使うアイコンだけ 1 件ずつ」。`https://api.iconify.design/<prefix>/<name>.svg?height=96`。取得済み（PNG とそのアイコンセットのライセンスが揃っている）なら再取得しない。
  - 取得失敗時: そのキーに `label` があれば label を表示して取り込み続行（取り込み結果に該当キーを出す）。`label` も無ければ取り込みエラー（キー id とアイコン名を出す）。JSON は書き換えない（ID を保つので、通信できる時に取り込み直せば取得される）。
  - 共有・書き出す JSON には ID のみ（画像は含めない）。受け取り側が取り込み時に再解決する。URL スキームの 1MB 上限は変更しない。
- **SVG → 画像の変換手段: WKWebView（画面に載せない）で SVG を `<img>` として読み、canvas に描いて PNG（高さ 96px。@3x で 32pt）にする**（`App/IconifyResolver.swift`）。拡張は SVG を一切扱わず、`UIImage(data:scale:3)` + テンプレート画像（色は前景色）で描くだけ。
  - 採った理由: (1) Iconify のアイコンセットは path だけでなく stroke・circle / rect / g・transform・円弧を使うものが多く（lucide / tabler は stroke 主体）、自前パーサだと対応範囲の取りこぼしと、Swift をローカルでコンパイルできない中での実装・検証リスクが大きい。WebKit に任せれば SVG の解釈を自作せずに済む。(2) スナップショット API（`takeSnapshot`）は画面階層に載せないと白紙になる既知の癖があるが、canvas へ描いて `toDataURL` で取り出す方式は階層に依存しない。(3) WKWebView の Web コンテンツは別プロセスで、ホストアプリ（取り込み時だけ・1 件ずつ）のメモリへの影響が小さく、拡張のメモリ上限（約 177MB。H-6）には関わらない。(4) `<img>` 経由なので SVG 内のスクリプト・外部参照は実行・読み込みされない。
  - 採らなかった案: SVG path の自前パーサ + CGContext 描画（PDF 保存）。ベクターのまま持てて任意サイズで綺麗になる利点はあるが、上記のとおり対応範囲とリスクが大きい。後から差し替えられるよう、変換は `IconRasterizer` に閉じ込めてある。PDF で持つ案も同様に見送り（PNG @3x で 17pt 程度のキーの表示には十分）。
  - 制約: `currentColor` は黒に置き換えて描き、アルファだけを使う（多色のアイコンセットは単色のシルエットになる）。
- **ライセンス**: `https://api.iconify.design/collection?prefix=<prefix>&info=true` からアイコンセット名・ライセンス名（title / spdx / url）・作者（name / url）を取り、`licenses.json` に保存する。アイコンはライセンス情報を書いた後に保存し、「アイコンがあればライセンスもある」を保つ。ライセンスが読めないアイコンセットは取り込み失敗扱い（表示義務を満たせないため。label があれば上の代替に回る）。一覧はホストアプリの「レイアウト」タブと About > ライセンスの「アイコンの出典」から見られる。意図は CC BY 系の帰属表示義務への対応。
- **取得失敗の文言（2026-10-22）**: HTTP 404 は通信ではなく「名前が無い」ので、取り込み結果の案内を「アイコン名が見つかりません。名前を確認してください」に分けた（取り込み直しても解決しないため）。通信失敗・429・その他は従来どおり「通信できる状態で取り込み直すと取得します」。キーごとに区分して節を分けて出す（`IconifyFailure` / `IconifyFallback.note`）。
- **迷った判断**: (1) 1 回の取り込みで新規取得するアイコンは 64 種類までとした（大量通信の防止。ユーザー指示には無い）。(2) `licenses.json` の取得に失敗したときはそのアイコンセットの全アイコンを失敗扱いにした。(3) 使われなくなったアイコンの掃除はしていない（レイアウト削除時も残る。サイズは 1 件数 KB）。(4) `collection` の応答はトップレベルと `info` の両方の形を受ける。
- **未確認事項**:
  - 実装時（2026-10-22）、開発環境のプロキシが `api.iconify.design` への接続を拒否した（CONNECT 403）ため、**Iconify API の応答（SVG の形式・`height` パラメータの効き方・`collection` の `info` の形・レート制限）は実際には確認できていない**。上記は公開仕様の記憶に基づく。レート制限は不明で、取り込み時に 1 件ずつ・上限 64 件に抑えている。要確認: 429 等が返った場合の挙動（現状は取得失敗として label に代替）。
  - Swift をローカルでコンパイルできないため、WKWebView の読み込み完了待ち・`callAsyncJavaScript` + canvas の PNG 化・テンプレート描画は CI（`ios-test` のユニットテスト、`ios-testflight` のビルド）と実機で確認が要る。単体テストの対象は書式・抽出・ライセンス応答の解釈・検証のみ。
- **サイト側対応（完了、2026-10-22）**: サイトエディタ（`web/`）も `iconify:` / `sf:` の書式検査（`validate.ts`。スキーマ・Swift と同じ規則）とアイコン入力欄に対応した。プレビューはサイトがオンラインなので、ブラウザから `https://api.iconify.design/<prefix>/<name>.svg` を直接読んで表示する（失敗時は label 表示になる旨を出す）。書き出し JSON は ID のみ。
- **2026-10-23 追記（実機で確認済み）**: 上の「未確認事項」の Iconify は、ユーザーの実機で API 応答・SVG → PNG・描画・出典表示まで問題なしを確認した（H-44）。

### H-37. macOS フロントの未実装部分の実装（2026-10-22、ユーザー指示「その順番で進めて」、要実機確認）

順序はユーザー承認済み: (1) 入力モード同期 → (2) `set_environment`・直前テキスト → (3) 通知表示・メニュー → (4) 再変換 → (5) 配布。(5) は鍵・配布先・証明書の決定が要るため保留。

- **入力モード**: Info.plist に英数モード `<bundle-id>.Roman`（`smRoman`、`TISIntendedLanguage = en`、JIS ショートカット 0）を追加した。ひらがなモードの ID は既存の `<bundle-id>.Japanese` のまま（インストール済みの入力ソースを壊さない）。アイコンは同じ角丸の灰色版（`nagi-icon-roman.tiff`）で、「あ / A」の文字アイコンではない（グリフの描画を xtask に入れるのは見送り。差し替えは容易）
  - コアのモードが正（D 節）。応答の `mode` で OS のモードを `selectMode` し、OS 側の選択（`setValue(_:forTag:client:)`）は `perform_action(SetMode)` でコアへ送る。同じモードなら送らない（SetMode は入力中の文字列を確定するため、往復で確定しないように）。activate 時とエンジン起動・再起動後は、OS で選択中の入力ソース（`TISCopyCurrentKeyboardInputSource`）に合わせる
- **set_environment**: `EnvironmentReporter`（電源は IOKit の `IOPSNotificationCreateRunLoopSource`、低電力・熱は `ProcessInfo` の通知、メモリ逼迫は `DispatchSource.makeMemoryPressureSource`）。変化時と `started` のたびに送る。`memory_budget_mb` は macOS では送らない
- **直前テキスト**: preedit が無く、ひらがなモードで、flagsChanged 以外のキーが来たとき（= 入力開始時）と activate 時に、キャレット直前の最大 64 UTF-16 を `attributedSubstring` で読んで `set_context` で送る。取得できないアプリでは `surrounding_text` を送らない（コアは前回の文脈を維持。空文字列を送ると次語候補が消えるため）。キャレットが先頭なら空文字列。secure input は毎回読み直し、secure 中は直前テキストを読まない（CLAUDE.md の停止条件）
- **通知表示**: `notify` の warning / error、エンジン停止（1 回目のみ）、minimal での起動、起動失敗、復元失敗を画面右上の小パネル（`StatusToast`、4 秒、同じ文言は 30 秒間引く）で出す。`info` は出さない。OS の通知センターは許可ダイアログが要るため使わない
- **メニュー**: 開いた時点の `get_status`（プロファイルと理由・LLM の有無・学習件数・ユーザー辞書の語数・設定エラー）を選択不可の行で出し、「設定…」「設定ファイルを再読み込み」「学習データを削除…」（NSAlert で確認）を並べる
- **再変換**: プロトコルに `ProcessKeyResult.reconvert_selection: Option<bool>` を追加した（加算的。Swift 型を再生成）。キーの意味はコアが決め（キーマップ）、選択の取得はフロントがする分担にした
  - 既定の割当: 未入力・ひらがなモードでのかなキー（macOS 標準と同じく、英数モードからは 1 回目でひらがなモード、2 回目で再変換）と変換キー（修飾なし。MS-IME と同じ）。従来、ひらがなモードでのかなキーは何もしない（SetMode の同値）だったので、選択が無ければ挙動は変わらない
  - macOS は選択（最大 200 UTF-16）を `reconvert` に送り、結果のマークテキストで選択範囲を置き換える（`setMarkedText` の `replacementRange`）。初回は逆引き索引の構築（G-29。実データで約 300MB）で時間がかかるため 2 秒まで待ち、超えたら `reset` で取り消して「少し待ってからもう一度」と通知する（要求は順に処理されるので、遅れて変換中になったセッションも戻る）
  - 誤学習の削除は既存の Ctrl+Delete（変換中、コアのキーマップ）で、フロントの追加は不要
- **迷った判断**: (1) 英数モードのアイコンを文字アイコンにしなかった。(2) 変換キーは Direct のみ（Composing / Converting の変換キーは既存どおり）。(3) 再変換の待ち時間 2 秒は実測なし
- **未確認（実機）**: `setValue(_:forTag:client:)` が activate 時・メニュー選択時にどう呼ばれるか、英数/かなキーを OS が先に処理するか（どちらでも同期する作りにした）、`attributedSubstring` の対応アプリ（ブラウザ・ターミナル・Electron）、`setMarkedText` の `replacementRange` で選択範囲が置き換わるか、メモリ逼迫イベントの頻度
- **(5) 配布（2026-10-22 追記、ユーザー決定: Developer ID + 公証 / zip は GitHub Releases・latest.json とインストールスクリプトはサイト / ed25519 鍵は手元で生成して Secrets に登録）**:
  - 公開鍵は `config/update-public-key.txt` を単一ソースにした（core は `include_str!`、macOS は xtask が Info.plist の `NagiUpdatePublicKey` に埋める）。確認先は OS ごと（`https://nagi.shiki-01.com/updates/{os}/latest.json`。従来の `/updates/latest.json` は Windows 版と衝突するため変更）。D 節の `shiki-01.com/nagi/...` は、サイトが `nagi.shiki-01.com` になったためそちらに合わせた
  - `macos-release.yml`（手動実行。入力の version と `core/Cargo.toml` の一致を検査）: 一時キーチェーン → `bundle-macos --llama --sign-identity`（imserver → .app の順、hardened runtime・timestamp、`--deep` なし）→ notarytool → staple → zip → `sign-update` → `gh release create macos-vX.Y.Z` → `web.yml` を起動。`web.yml` は最新の `macos-v*` リリースの `latest.json` を `static/updates/macos/` に取り込む（リポジトリには入れない）
  - アプリ内の更新はフロントの仕事（CLAUDE.md の分離の原則）として Swift で実装した: SHA-256 と ed25519（CryptoKit。xtask で作った既知の値で互換性をテスト）→ `ditto` で展開 → `codesign -R`（実行中のアプリと同じ Team ID の Developer ID）と `spctl` → `replaceItemAt` で差し替え → 入力中の文字列を確定し `shutdown`（学習を保存）して終了。次に使われたとき OS が新しい版を起動する。ad-hoc 署名のビルドは自動更新を断り、インストールスクリプトを案内する
  - インストールスクリプトは `plutil` で JSON を読み、SHA-256・`codesign --verify`・`spctl` を確かめてから置き換え、`--register` する。シェルでは ed25519 を検証しない（macOS 標準の LibreSSL に頼れないため。HTTPS の取得元と公証で代える）
  - **迷った判断**: (1) リリースは LLM 有効版のみ（辞書だけで使う場合も同じバイナリで動くため）。(2) 再起動は「終了して OS に起動し直してもらう」方式で、自前で再起動しない。(3) 辞書・モデルはインストール時に取得しない（辞書は Mozc 由来のデータを再配布せず手元で組む方針（§2.3）で、組むには Rust が要る。別途の検討事項）
  - **未確認**: hardened runtime で LLM（Metal）と imserver の起動が動くか、公証が通るか（Secrets 未登録のため一度も実行していない）、IMK のプロセス終了後の再起動のされ方

### H-38. macOS: 辞書とモデルをボタン一つ・インストール時に自動で入れる（2026-10-22、ユーザー指示、要実機確認）

- **方針**: データは同梱・再配布しない（NOTICE 冒頭、H-37 の (3)）。利用者の Mac が配布元（Mozc の GitHub raw の固定コミット、Hugging Face の固定リビジョン）から直接取得し、SHA-256 を検証して、辞書はその場で組み立てる（約 2 秒）。iOS は IPA に辞書を同梱しているが（`docs/ios-release.md` §4）、macOS はこの方式にした
- **実装**: 取得・検証・組み立てを新しい crate `core/datafetch`（`nagi-datafetch`）にまとめ、xtask（`fetch-dict` / `build-dict` / `fetch-model`）もこれを使う（SHA-256 の表は一か所）。`imserver --install-data --data-dir <dir> [--model small|xsmall|none]` は進捗を標準出力に 1 行 1 JSON（`dict_fetch` / `dict_build` / `model_fetch` / `done` / `error`）で出す。server の既定 feature `data-install` で有効（`bundle-macos` のビルドに入る）。`core/ffi`（iOS）は `default-features = false` なので含まない
- **入口**: (1) `install.sh` がアプリの配置後に自動で実行する（`NAGI_SKIP_DATA=1` で飛ばす。成功したら動作中の NagiIMEApp を止め、次の利用で辞書つきで起動させる）。(2) アプリは辞書（`data/dict/nagi-dict.bin`）が無ければ、起動後の最初の入力時に一度だけダイアログで勧める。(3) 入力メニューの「辞書とモデルをダウンロード…」。アプリからは同梱の imserver をこの引数で起動し、進捗を通知パネルに出し、終わったらエンジンを再起動する
- **既定のモデル**: small（約 74MB。G 節・H-17 の比較で small が上）。辞書の取得は約 70MB
- **あわせて**: web の公開（Cloudflare Pages）を main への push では行わず、手動実行（`macos-release` からの起動を含む）だけにした（ユーザー指示。未完成の install.sh が push で公開されないように）
- **未確認（実機）**: アプリから imserver を子プロセスで実行したときの進捗表示、取得中に入力したときの挙動、hardened runtime 下での imserver のネットワーク接続（エンタイトルメントは不要の想定）

### H-39. Windows フロントの構成と TSF 層の言語（2026-10-22、ユーザー判断「悩んだらお任せ」、実機未確認）

- **TSF 層の実装言語は Rust（`windows` クレート 0.62）**。設計書 §5.3 の第一候補どおり。C++ 薄層は採らない。理由: (1) コアが Rust で、キー正規化・エンジン管理（`nagi-windows`）をそのまま使える（FFI の境界とビルド系が増えない）、(2) `cargo xtask`・CI（windows-latest）だけで完結する、(3) Linux からも `--target x86_64-pc-windows-gnu` で型検査ができ、実機が無くても骨組みの誤りを減らせる。難航した場合に C++ 薄層へ戻す余地は残る（`render` / `engine` が OS 非依存なので、薄層から呼ぶ形に変えやすい）
- **クレート分割と unsafe の隔離**:
  - `frontend-windows/`（`nagi-windows`）: `#![forbid(unsafe_code)]` のまま純ロジック。今回 `session`（SessionStore。macOS の `SessionStore.swift` + `RestoreSessionParams.make` と同じ責務・同じカーソル換算）を追加
  - `frontend-windows-tsf/`（`nagi-windows-tsf`、cdylib + rlib）: COM/TSF。`unsafe_code = "deny"` をクレートに掛け、Win32 を触るモジュール（`dll` / `register` / `text_service` / `edit` / `keys`）だけ `#![allow(unsafe_code)]`。`windows` / `windows-core` は `[target.'cfg(windows)'.dependencies]` のみ（Linux/macOS では空に近いクレートとしてビルドされる）。OS 非依存の `render`（応答 → 編集計画 `EditPlan`）と `engine`（EngineSupervisor + SessionStore + 非同期イベントキューのまとめ）は Linux でテストする
  - core の workspace members に `"../frontend-windows-tsf"` を追加した（コアの crate は変更していない）
- **キー**: `VK_IME_ON`(0x16) → `HIRAGANA`（ひらがなモードへ。Direct・ひらがな中は keymap が再変換に解決する。macOS の「かな」と同じ）、`VK_IME_OFF`(0x1A) → `EISU_TOGGLE`（英数モードへ）、`VK_OEM_BACKTAB`(0xF5) → `HIRAGANA_KATAKANA`（macOS のかなキー 0x68 と同じ扱い）。`ZENKAKU_HANKAKU` のようなトグルにしなかったのは、OS 側の状態との往復で反転しないよう、明示的なモード指定にするため。`VK_OEM_BACKTAB` の実キー割当は資料が乏しく**実機で確認が要る**
- **TSF の流れ**: `OnTestKeyDown` でエンジンの `process_key` を呼び、結果を 1 件キャッシュして `OnKeyDown` で同じ結果を使う（`process_key` は状態を進めるので同じキーで 2 回呼べないため）。反映すべき編集があるのに `consumed=false` の場合も「食った」と返す（返さないと `OnKeyDown` が呼ばれず編集が失われる）。反映は同期の読み書き `ITfEditSession`（確定 → `ITfComposition` の範囲を置換して終了、preedit → 範囲を更新しキャレットを設定）
- **UI スレッドを塞がない対策**: エンジンの起動（`initialize` はモデル読み込みで数秒）は別スレッド。未接続の間のキーは素通し（composition 中は食って文字列を壊さない）。キー処理の要求タイムアウトは 250ms（設計書の通常要求 500ms より短くした。超えたら素通し／エラー扱い。実測なし）。COM のエントリポイントは `catch_unwind` で囲み、パニックでホストアプリを落とさない
- **迷った判断**: (1) エンジンはホストプロセスごとに imserver を起動する（`OnceLock` でプロセス内共有）。設計書は「別プロセス化は必須」だが共有はしていない。メモリ・起動時間の面で、名前付きパイプ + 常駐ブローカーへの変更を**要再検討**。imserver はホスト終了で stdin が EOF になり終了する想定（コア側の既存テストあり） (2) 非同期通知（ライブ変換の差し替え等）は他スレッドで受けてキューに積み、次のキー入力で反映する。即時反映は隠しウィンドウ + `PostMessage` が必要で見送り (3) セッションはホスト内の TextService インスタンスごとに 1 つ（コンテキストごとではない） (4) データは `%APPDATA%\Nagi\data`、imserver は DLL と同じディレクトリ（`--data-dir` のみ渡す。インストーラの仕様は未決定） (5) 修飾キーの離上は `flags_changed` としてエンジンへ送る。押下を食ったキーの離上は食わない
- **検証方法と未検証**: Linux で `cargo check` / `cargo clippy -D warnings --target x86_64-pc-windows-gnu`（リンクはしていない）と、`nagi-windows` / OS 非依存部のテスト。**Windows 実機では何も確認していない**: `regsvr32` での登録、入力方式としての認識、キー入力、composition の表示・確定、アンロード（`DllCanUnloadNow`）、imserver の起動と stdout の扱い、TSF の `windows` クレートの呼び出し規約（`ShiftEnd` によるキャレット位置、`InsertTextAtSelection` の `QUERYONLY` など）
- **未実装（TODO、`frontend-windows-tsf/src/lib.rs` にも記載）**: 候補ウィンドウ、`ITfFnReconversion`（スタブのみ）、表示属性、言語バー・開閉コンパートメント、非同期通知の即時反映、`set_environment` / `set_context` / 通知表示、アイコン、インストーラ・署名・SmartScreen 方針（設計書 §5.3）、CI への組み込み

### H-40. Windows フロント: OS 非依存ロジックの追加（候補ウィンドウ・環境・文脈・表示属性・文言）（2026-10-22、ユーザー指示「裁量に任せる、全部終わるまで進める」、実機未確認）

macOS 版（`CandidatePaging` / `CandidatePanel` / `EnvironmentReporter` / `PreeditStyler` / `StatusMessages` / `reportContext`）と挙動を揃えたロジックを `nagi-windows`（`forbid(unsafe_code)`）に追加した。Win32 の描画・COM・OS 問い合わせは次の層で、ここは trait 注入と純関数だけ。コア・プロトコルは変更していない。

- **`candidates`**: ページング（選択を範囲内に丸め、`page_size` は 1 以上）・行モデル（番号・全体インデックス・出所ラベル）・フッター（「2/5」、1 ページなら無し）・寸法（macOS と同じ式・定数。文字幅は `TextMeasure` で注入）・配置。配置は既定でキャレット行の下、作業領域の下端に収まらなければ上へ反転、どちらも収まらなければ作業領域内に押し込む（上端優先）、水平は作業領域内に丸める。キャレットを含むモニターの作業領域を使う（無ければ先頭 = プライマリ）。キャレットが取れない（ゼロ矩形）ときは呼び出し側が渡すマウス位置で代用（macOS と同じ）。数字キー選択は `index_for_number`（ページ内番号 → 全体インデックス）。**キーの解釈そのものはコアのキーマップ**（macOS と同じ分担）なので、これは将来のマウス・タッチや表示検証用の変換
- **`environment`**: `EnvironmentProbe`（電源・メモリ・プロセッサ周波数）→ `SetEnvironmentParams`。`EnvironmentReporter::poll` は前回送信値から変化したときだけ返し、エンジン起動・再起動のたびに `reset`（新しいエンジンは前回値を知らない）。取得できない項目は省略（コアは前回値を維持）。`memory_budget_mb` は macOS 同様に送らない
  - Windows には macOS のような OS イベントが無いため、次を**推定**にした。閾値はすべて実測なし（要調整）: (1) メモリ逼迫は利用可能量から（5% 未満または 300MB 未満で critical、10% 未満または 800MB 未満で warning）。(2) サーマルは現在周波数 / 上限の比（85% 以上 nominal、60% 以上 fair、40% 以上 serious、未満 critical）。ただしバッテリー節約が有効または不明のときは周波数が下がるのが正常なので**推定せず省略**（誤って軽いプロファイルに落とさないため）。(3) `low_power_mode` は `SystemStatusFlag`（バッテリー節約）。(4) 電池が無い PC（`BatteryFlag` 128）は AC 扱い
- **`context`**: `build_context` は macOS 版と同じ規約。secure なら周辺テキストを送らない（`secure_input: true` は常に伝える）、取得不能なら送らない（空文字列とは区別。空はキャレットが先頭）、直前の最大 64 UTF-16（サロゲートペアの途中で切らない）、空のアプリ ID は省略、`should_report_before_key`（入力開始時・ひらがな・flags_changed 以外）。パスワード欄は TSF の `InputScope` の `IS_PASSWORD`（値 31）で判定する前提の `is_secure_input_scope` を置いた（**定数値は記憶に基づく。実機層で `windows` クレートの定数と一致を確認すること**）
- **`preedit_style`**: `runs`（macOS の `PreeditStyler.runs` と同じ判定: 注目 > 選択 > 種別、保留は別 run）と、`plan`（全文を隙間なく覆い、隣接する同種を結合し、`DisplayAttr` を割り当てる。TSF の `GUID_PROP_ATTRIBUTE` はテキスト全体に必要なため）。属性は macOS と同じ対応（通常 = 細い実線、注目・選択 = 太い実線、確定済み・保留 = 点線、スタイル変換 = 破線）。色は指定しない
  - **迷った判断**: 以前の `frontend-windows-tsf/src/render.rs` の `SpanStyle`（入力中 = 点線、変換済み = 実線、注目 = 太線という MS-IME 風の 3 種）は削除し、この計画に一本化した（`EditPlan::Set.spans` → `attrs`）。macOS と同じ見た目・同じ種別（確定済み・スタイル変換が区別できる）を優先した。MS-IME 風（未変換 = 点線）にしたい場合は `attr_for` の 1 か所で変えられる
- **`status`**: `toast_for(&EngineEvent)`（macOS の文言を踏襲。info は出さず `update_available` のみ例外、停止通知は 1 回目のみ）、`menu_lines`、`mode_indicator`（あ / A）、`mode_name`、`ToastThrottle`（同文言 30 秒間引き。時刻は引数で注入）
- **CI**: `ci.yml` の `on:` は変更していない（workflow_dispatch のみ）。`nagi-windows` / `nagi-windows-tsf` は workspace members なので既存の `core` ジョブ（ubuntu / windows / macos の fmt・clippy・build・test）で 3 OS とも検査される。加えて新ジョブ `windows-tsf`（ubuntu、`rustup target add x86_64-pc-windows-gnu`、`cargo check` と `cargo clippy -D warnings`、`--all-targets`）を追加。COM/TSF 部分は `cfg(windows)` のため通常ジョブでは検査されず、このジョブでだけ型検査される


### H-41. Windows フロント: Win32 / TSF 層の実装（候補ウィンドウ・環境・文脈・表示属性・モード・再変換・通知・アイコン）（2026-10-22、ユーザー指示「裁量に任せる、全部終わるまで進める」、実機未確認）

H-39・H-40 で未実装だった OS 層を `frontend-windows-tsf`（`unsafe` は Win32 を触るモジュールに隔離）に実装した。ロジックは `nagi-windows`（`forbid(unsafe_code)`）に置き、コア・プロトコルは変更していない。`windows` クレートの API 名・シグネチャは `cargo clippy --target x86_64-pc-windows-gnu` で確認した（リンクと実行は未確認）。

- **候補ウィンドウ**（`candidate_window.rs`）: 自前の Win32 ポップアップ（`WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`、`HTTRANSPARENT` でクリックは下へ通す）+ GDI 描画（システム色）。`ITfCandidateListUIElement` で OS に描かせる案は、Win11 のホスト依存が大きく見た目をそろえにくいため採らなかった。`TextMeasure` は GDI（`GetTextExtentPoint32W`）、フォントは Yu Gothic UI（文字の大きさは macOS 版と同じ 15/13/11、`CreateFontW` の高さに DPI 倍率を掛ける）。DPI は「基準位置へウィンドウを動かしてから `GetDpiForWindow`」で得る（Per-Monitor 対応ホストで移動先の DPI、DPI 非対応ホストでは 96 = 座標の仮想化と整合）。`Metrics::scaled(dpi)` / `scale_px` を追加（四捨五入）。作業領域は `EnumDisplayMonitors` + `GetMonitorInfoW`、`primary_first` でプライマリを先頭に（純関数・テスト済み）。キャレット矩形は `ITfContextView::GetTextExt`。**編集セッション内で測る**ため、`apply_plan` の最後に composition（無ければ選択）の範囲を測って `Env.caret` に入れ、候補の表示位置に使う（応答を反映してから候補を出す順序にした。従来は `OnTestKeyDown` で先に候補を更新していた）。取れない（ゼロ矩形・失敗）ときはマウス位置（`anchor_rect`）
- **非同期通知の即時反映**（`message_window.rs`）: スレッドごとの隠しメッセージウィンドウ（`HWND_MESSAGE`）。`EngineHost` はセッションごとに購読（`subscribe(session_id, waker)`）し、イベントを宛先のキューに積んで waker（`PostMessageW`）で UI スレッドを起こす。宛先は `engine::route`（セッション宛て: `update_preedit` / `update_candidates` / `Restored` / `RestoreFailed`。プロセス全体: `Started` / `Crashed` / `Notify` 等は購読中の全セッションへ）。H-39 の「次のキーまで出ない」制限と、従来の共有キューが他セッション宛てのイベントを取り出してしまう問題を解消した。UI スレッドでの反映はキーイベント内なら同期（`TF_ES_SYNC`）、メッセージ経由なら `TF_ES_ASYNCDONTCARE`（文書ロックが空いていれば即時、塞がっていれば後で）。ウィンドウクラスは最後のウィンドウを壊したときに登録解除する（DLL アンロード後に古いプロシージャを指さないため）
- **環境の報告**: `Win32Probe`（`GetSystemPowerStatus` / `GlobalMemoryStatusEx` / `CallNtPowerInformation(ProcessorInformation)`）。**H-40 からの修正**: サーマルの入力には「現在の周波数」ではなく `MhzLimit`（システムが課している上限。コア間の平均）を `current_mhz` として渡す。現在の周波数はアイドルで下がるのが正常で、サーマル制限と区別できず、誤って軽いプロファイルに落とすため。比の閾値（85/60/40%）は H-40 のまま（実測なし）。`EnvironmentReporter` はプロセスで 1 つ（`Mutex`）。送信契機: 30 秒タイマー、`WM_POWERBROADCAST`（`RegisterPowerSettingNotification` の `GUID_ACDC_POWER_SOURCE` / `GUID_POWER_SAVING_STATUS`）、Activate 時、エンジンの `Started` ごと（`reset` してから）。メッセージ専用ウィンドウが `WM_POWERBROADCAST` を受け取れるかは**実機で未確認**（受けられない場合も 30 秒タイマーで追従する）
- **スリープ復帰**: `RegisterSuspendResumeNotification`（`PBT_APMRESUMEAUTOMATIC` / `PBT_APMRESUMESUSPEND`）に加え、タイマーの間隔の飛び（壁時計で間隔の 2 倍超）でも検出する `ResumeDetector`（純関数・テスト済み。同じ復帰は 60 秒間 1 回だけ報告）。検出したら `restart_in_background`（学習は `shutdown` で保存。`restore_session` で preedit を復元）
- **`set_context`**: `context_info.rs`。編集セッション（読み取り・同期）の中で、選択の先頭から手前へ最大 64 UTF-16 を `GetText`、`GUID_PROP_INPUTSCOPE` から `ITfInputScope::GetInputScopes`。`is_secure_input_scope` の定数 31 は **`windows` クレートの `IS_PASSWORD` と一致することをコンパイル時のアサーションで検査**した（一致）。secure なら周辺テキストは読まない。アプリ ID は実行ファイル名（`chrome.exe` 等。プロセスで 1 回取得）。**判断**: 送るのは「文字を生むキー（`text` が空でない）かつ `should_report_before_key`」のときだけにした。毎キーで文書を読むのを避けるため（矢印・ショートカット・修飾キー単独では読まない）。macOS 版は入力開始時に送る点は同じ
- **表示属性**（`display_attr.rs`）: `TextService` に `ITfDisplayAttributeProvider` を実装し、`GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER` を登録。属性は `preedit_style::DISPLAY_ATTRS`（細い実線 / 太い実線 / 点線 / 破線の 4 種）ごとに固定の GUID を 1 つ。色は指定しない。注目文節（太線）の `bAttr` は `TF_ATTR_TARGET_CONVERTED`、それ以外は `TF_ATTR_INPUT`。Activate で `ITfCategoryMgr::RegisterGUID` した識別子を、`SetText` 後に区間ごと（`range_shifts` で区間の範囲を作る）`GUID_PROP_ATTRIBUTE` へ書く。確定・テキストを残した終了では属性を消す（消さないと確定後も下線が残る）。属性の設定に失敗しても入力は続ける。列挙子の終端は `S_FALSE`。**TextService は Activate までエンジンに触れない**ようにした（表示属性の提供のために、アクティブ化されないプロセスでも COM オブジェクトが作られ、imserver が起動するのを避ける）
- **入力モード**（`mode_comp.rs` + `nagi_windows::mode`）: 言語バーのボタン（`ITfLangBarItemButton`）は実装せず、コンパートメント（`GUID_COMPARTMENT_KEYBOARD_OPENCLOSE` / `..._INPUTMODE_CONVERSION`）だけにした。Win8 以降のシェル（タスクトレイの「あ / A」）はこれを読み、入力方式は `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT` で登録済みのため。レガシーの言語バーは非推奨で、実装量が大きい割に得るものが少ない。値の対応: ひらがな = 開・日本語・全角・ローマ字（MS-IME と同じ 0x19）、英数 = **閉**・ローマ字（アプリからは IME オフに見える。MS-IME の半角英数と同様）。逆変換は「閉 → 英数、開 + 日本語ビット → ひらがな、開 + ビット無し → 英数」。コアのモードが正で、応答の `mode` を OS へ書き、OS 側の変更通知（`ITfCompartmentEventSink`）は `perform_action(SetMode)` でコアへ送る。**同じモードなら送らない・書かない**（`ModeSync`。`SetMode` は入力中の文字列を確定するため）。自分で書いている間（2 つの値を順に書く途中）の通知は無視する。Activate 時とエンジン起動・再起動後は、OS が未設定ならひらがなを書き、設定済みで違えばコアへ送る（新しいエンジンはひらがなで始まる前提）
- **再変換**（`reconversion.rs`、`edit::run_reconvert`）: コアの `reconvert_selection` 要求では、編集セッション（同期）の中で選択範囲のテキストを読み（200 UTF-16 超・空は対象外。`nagi_windows::reconvert`）、`reconvert` を最大 2 秒待ち（macOS の `reconvertTimeout` と同じ）、応答の preedit で選択範囲を置き換えて composition にする。待ちきれなければ `reset` して通知（macOS と同文言）。`ITfFunctionProvider` を `ITfSourceSingle::AdviseSingleSink` で登録し、`GetFunction(_, IID_ITfFnReconversion)` で `ITfFnReconversion` を返す。`QueryRange` は渡された範囲を複製して返し常に変換可能とする（範囲の中身は編集セッション無しでは読めないため。空・長すぎる場合は `Reconvert` で何もしない）、`Reconvert` は文書ロックの空き次第（`Deferred`）で同じ処理、`GetReconversion`（候補一覧の提供）は `E_NOTIMPL`。入力中（composition あり）は再変換しない
- **通知（トースト）**（`toast_window.rs`）: 自前の最前面ポップアップ（暗色の角丸・白字・4 秒）をプライマリの作業領域の右下に出す（macOS は右上だが、Windows の通知の慣例に合わせた。`toast::place_toast`）。文言と種別は `status::toast_for`、間引きは `ToastThrottle`（プロセスで 1 つ）。出すのは**フォーカス中のセッションだけ**（各ホストプロセスが imserver を持つため、背面のプロセスの停止通知を出さない）
- **アイコン**（`nagi_windows::icon`）: リソース埋め込み（rc / windres。`embed-resource` 等の追加依存と、クロスビルドでのツール要件）は採らず、**コードで描いた .ico を `DllRegisterServer` が DLL と同じディレクトリに書き出して**、`RegisterProfile` のアイコンパス（番号 0）に渡す。絵柄は藍色の角丸の四角に白い「N」（4×4 超サンプリングのアンチエイリアス。16/24/32/48 px、32bpp DIB）。書き出せなければ DLL のパスを渡す（アイコン無しで登録は続く）。`DllUnregisterServer` で削除する。正式なアイコンに替えるときは `icon.rs` を差し替えるか、インストーラが `nagi.ico` を置く（既存を上書きする）
- **未検証・要確認（実機）**: 上記すべて。特に (1) 候補ウィンドウの位置・DPI・フォーカスを奪わないこと、(2) メッセージ専用ウィンドウでの `WM_POWERBROADCAST`、(3) `GetTextExt` が取れるアプリの範囲、(4) コンパートメントの書き込み順（変換モード → 開閉）で通知の途中状態がシェルに誤って見えないか、(5) `ITfFnReconversion` を実際に呼ぶアプリ（Office 等）での挙動、(6) 英数 = 閉にしたときのアプリ側の挙動（IME オフ扱いで困るアプリがないか）、(7) 非同期の編集セッションと直後のキー入力の順序

### H-42. Windows の配布・インストーラ・署名・更新と SmartScreen 方針（2026-10-22、ユーザー指示「裁量に任せる、全部終わるまで進める」、実機未確認）

Phase 5 の最後の担当。コア（`core/` の各 crate）は変更していない（Phase 5 DoD の「コア側の変更が原則ゼロ」を維持）。変更は xtask・CI・配布スクリプト・フロントの文言のみ。

**配布物と置き場所**
- **配置先は `%ProgramFiles%\Nagi`（管理者権限が要る）**。`%LOCALAPPDATA%` は管理者権限が要らないが、TSF の DLL は UWP / AppContainer など保護されたアプリにも読み込まれ、利用者プロファイル配下は `ALL APPLICATION PACKAGES` の読み取り権限が既定で付かないため読み込めない（見込み。実機で要確認）。Program Files は既定で付く。どのみち `regsvr32` による COM 登録（`HKCR\CLSID`）に管理者権限が要るので、UAC の回数は増えない。データ（辞書・モデル・設定・学習）は従来どおり利用者ごとの `%APPDATA%\Nagi`（H-39 の決定どおり）
- **登録は 64bit の `regsvr32 /s`**（32bit の PowerShell からは `Sysnative` 経由）。x64 のみ。ARM64 Windows と 32bit ホスト用の DLL は作らない（設計書 §7 の対象が x64）。ARM64 は将来、`aarch64-pc-windows-msvc` の DLL を別途用意する必要がある（エミュレーションされた x64 アプリに読ませる `ARM64X`/`ARM64EC` は未検討）
- **zip**: `cargo xtask bundle-windows` が `Nagi-windows-<version>.zip` を作る（最上位に同名ディレクトリ 1 つ）。中身: `nagi_windows_tsf.dll` / `imserver.exe` / `install.ps1` / `uninstall.ps1` / `README.md`（`docs/windows-install.md`）/ `LICENSE-MIT` / `LICENSE-APACHE` / `NOTICE` / `THIRD_PARTY_LICENSES.md` / `VERSION` / `SHA256SUMS`。macOS（`Nagi-macos-<version>.zip`、タグ `macos-vX.Y.Z`）に名前をそろえ、Windows は `Nagi-windows-<version>.zip`・タグ `windows-vX.Y.Z`。ビルドは行わず成果物の収集・署名・チェックサム・zip だけ（ビルドは CI の windows ランナー。収集と zip は OS 非依存で、単体テストは Linux で動く: PE ヘッダ検査・チェックサム・BOM・再現性）。`.ps1` は zip に入れるとき **UTF-8 BOM 付き**にする（BOM が無いと Windows PowerShell 5.1 は日本語を ANSI として読み文字化けする。リポジトリ上は BOM 無し）。zip の更新時刻は固定で、同じ入力から同じバイト列になる
- **`SHA256SUMS`**: 同梱ファイルの `<sha256>  <相対パス>`。`install.ps1` が配置の前に突き合わせる（破損・取り違えの検出。zip ごと差し替えられた場合の改ざん検知は latest.json の SHA-256 と ed25519 の役目）。署名（Authenticode）した後のバイト列で計算する
- `THIRD_PARTY_LICENSES.md` は現状 iOS の依存閉包の一覧（`gen-licenses`）で、Windows の依存（`windows` クレート、`ureq`/`rustls`/`ring`/`webpki-roots` など）は含まない。**Windows 用の一覧の生成は未実装**（`gen_licenses.rs` が `nagi-ffi` と iOS ターゲットに固定。macOS 版は一覧自体を同梱していないので、それよりは進んでいる）

**インストーラ（`frontend-windows/dist/install.ps1` / `uninstall.ps1`。xtask 原則の例外として配布物のスクリプトのみ）**
- 実行は `powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1`。既定の実行ポリシー（Restricted）でも動かすため。ダブルクリック用の `.cmd` は作らない（スクリプトの種類を増やさない。READMEに実行コマンドを書く）
- **昇格は 2 段**: 管理者でなければ UAC を出して、配置と登録（`-Elevated`）だけを子プロセスで昇格実行し、**辞書・モデルの取得は昇格しない親プロセスで行う**（管理者権限で取得すると、別アカウントで昇格した場合に `%APPDATA%` が管理者のものになるため。`APPDATA` は実行している利用者のものを使う）。ただし昇格に別アカウントの資格情報を使うと、`RegisterProfile`（入力方式の登録）が昇格側のアカウントに掛かる可能性がある（READMEに回避手順を記載。要実機確認）
- 更新（上書き）: 使用中の DLL・exe はファイル名の変更ならできるため、古いものを `.old` に退避して新しいものを置く。動作中のエンジンは止めない（止めると入力中の学習が失われる。サインアウトか再起動で新しい版にそろう）。`.old` は次回のインストールとアンインストールで片付ける
- **辞書・モデル**: macOS（H-38）と同じ導線。同梱せず、`imserver.exe --install-data --data-dir %APPDATA%\Nagi\data --model small` が配布元から取得して SHA-256 を検証し、辞書をその場で組み立てる。`-SkipData` / `-Model small|xsmall|none`。失敗しても入力方式の導入自体は成功とし、再実行を案内する。アプリ内の「ダウンロード」ボタンに当たるものは Windows には無い（メニューが無いため。未実装）
- 「アプリと機能」への登録（`HKLM\...\Uninstall\Nagi`。`UninstallString` は同梱の `uninstall.ps1`）。Visual C++ ランタイム（`vcruntime140.dll` / `msvcp140.dll`）が無ければ警告
- **アンインストール**: `regsvr32 /u /s`（DLL が `nagi.ico` も消す）→ ファイル削除。使用中で消せないものは `.old` に名前を変えて `MoveFileEx(DELAY_UNTIL_REBOOT)` で再起動時削除を予約。**ユーザーデータ（`%APPDATA%\Nagi`）は既定で残し**、`-RemoveUserData` のときだけ `yes` の入力で確認して削除する（`-Force` で省略。動作中の imserver を止めてから消す）。学習データだけの削除は README で `learning.tsv` の削除を案内（専用のコマンドは作っていない）
- **ブートストラップ `web/static/install.ps1`**（`irm https://nagi.shiki-01.com/install.ps1 | iex`。macOS の `install.sh` に対応）: `/updates/windows/latest.json` → zip を取得 → SHA-256 検証 → 展開 → 同梱の `install.ps1` を実行。**ASCII のみ**（`irm | iex` は既定のコードページで読むため日本語が文字化けする）。`Invoke-WebRequest` で取得したファイルには Mark of the Web が付かないので SmartScreen に止められない。ed25519 はシェルでは検証しない（macOS と同じ。HTTPS と SHA-256 で代える）。サイトの公開は web.yml の手動実行のときだけ（H-38）

**ビルド・リリース（`.github/workflows/windows-release.yml`、手動実行のみ）**
- `windows-latest` で `x86_64-pc-windows-msvc` を `--target` 明示でビルド。**DLL は CRT を静的リンク**（`-C target-feature=+crt-static`）: あらゆるホストプロセスに読み込まれる DLL が VC++ 再頒布可能パッケージの有無に左右されないようにする。`--target` を明示するのは RUSTFLAGS をビルドスクリプト・proc-macro に掛けないため。**`imserver.exe` は既定（動的 CRT）**: llama.cpp（C++）が絡み、`llama-cpp-sys-2` の `LLAMA_STATIC_CRT=1` との組み合わせを検証できていないため。再頒布可能パッケージを前提にし、`install.ps1` が不足を警告する
- **imserver は既定で feature `llama` あり**（macOS のリリースと同じ判断。辞書だけで使う場合も同じバイナリで動く）。入力 `llama: false` で外せる（libclang / cmake のビルドが通らないときの逃げ道）。`LIBCLANG_PATH` は `C:\Program Files\LLVM\bin`（windows-latest にある前提）。**Windows ランナーでの llama ビルドは Actions 上で一度も実行していない**
- 入力: `llama`（バージョンは `core/Cargo.toml` から自動決定。2026-10-23 に入力 `version` を廃止し、入力と Cargo.toml の不一致で失敗する問題をなくした）/ `llama` / `publish`（false なら artifact だけ）/ `prerelease`（既定 true）。**実機で未検証の間は prerelease** とし、`web.yml` は prerelease の `latest.json` を取り込まない（= サイトの更新確認とインストールコマンドに出ない）。実機で確認できたら `-f prerelease=false`。このため、最初のうちは `irm | iex` が 404 になる（意図した挙動。手動の zip は GitHub Releases から取れる）
- インストーラのスモークテスト（zip を展開 → `install.ps1 -SkipData` → `uninstall.ps1`。regsvr32 の登録と解除を含む）を `continue-on-error` で入れた。ランナー上で TSF の登録が通るかは未確認のため、失敗してもリリースは止めない
- 署名: `sign-update` による ed25519 署名つき `latest.json`（macOS と同じ鍵 `NAGI_UPDATE_SIGNING_KEY`）。Authenticode は Secrets（`WINDOWS_CODESIGN_PFX` / `_PASSWORD`）があるときだけ `bundle-windows --sign-pfx` が `signtool sign /fd SHA256 /tr <TSA> /td SHA256` で DLL と exe に署名して `signtool verify /pa` で確認する（パスワードは環境変数で渡し、コマンドライン引数には出さない）。無ければ署名なしで進む。署名は zip 化の前（チェックサムも署名後の値）
- `ci.yml` に PowerShell の構文検査ジョブ（windows-latest）を追加

**更新検査との整合**
- `update_check::default_check_url()` は `https://nagi.shiki-01.com/updates/{std::env::consts::OS}/latest.json` で、Windows では `/updates/windows/latest.json`。`windows-release.yml` が作る `latest.json`（`download_url` = `.../releases/download/windows-vX.Y.Z/Nagi-windows-X.Y.Z.zip`）と `web.yml` の取り込み先（`static/updates/windows/`）に一致する。**コアの変更は不要**
- 修正したのは Windows フロントの通知文言だけ: `update_available` のトーストが「入力メニューの『アップデートを確認…』から更新できます」だった（Windows には入力メニューが無い）ので、「PowerShell でインストールコマンドをもう一度実行すると更新できます」に変えた
- **アプリ内での自動の置き換え（ダウンロード・検証・差し替え・再起動）は Windows では未実装**（macOS は Swift で実装済み）。通知で知らせるだけで、更新は install コマンドの再実行。実装するなら、`download_verified`（コアにある。SHA-256 + ed25519）で取得 → 展開 → 昇格して配置、の流れになるが、DLL の差し替えに管理者権限が要る点（Program Files）が macOS と違う難所

**SmartScreen / コード署名の方針（設計書 §5.3「Phase 5 で決める」の解消）**

| 案 | 費用・条件 | SmartScreen への効き | 評価 |
| --- | --- | --- | --- |
| 署名なし + 手順書 + `irm \| iex` | 無料 | zip を手動取得するとブラウザ / Defender が警告（回避手順を書く）。`irm \| iex` は印が付かず警告なし | **現状の最適解**（採用） |
| OV / EV の Authenticode 証明書（年額有償） | 証明書の購入と本人・組織確認が要る | 署名すれば「発行元不明」は解消するが、評判（reputation）は配布実績で貯まる。EV でも即時に警告が消えるとは限らない（要確認） | 個人開発では費用対効果が低い |
| Azure Trusted Signing（Artifact Signing 等、クラウド署名） | 月額の低額課金。提供地域・個人 / 組織の本人確認の条件がある（日本からの利用可否は**未調査・要確認**） | 署名の鍵管理が不要で CI から署名できる。評判の扱いは Microsoft が証明書の寿命を短くして管理 | **将来の第一候補**（条件が合えば） |
| MSIX / Microsoft Store | Store 配布は審査と登録料 | Store 経由ならダウンロード時の警告なし | **不可の見込み**: TIP（`HKCR`/`HKLM` へのグローバルな COM 登録と他プロセスへの DLL の読み込み）は MSIX のコンテナと両立しない見込み（要確認） |
| winget / MSI | MSI は署名なしでも警告対象。winget は署名不要でハッシュ検証 | winget 経由の取得は警告が出にくい | zip 内の `.ps1` 方式は winget の installer 種別に合わない。EXE / MSI 化が前提で、署名導入後に再検討 |

- **現状**: 署名なしで配布する。主導線は `irm | iex`（警告なし）、手動の zip は README の手順（zip のブロック解除、または「詳細情報」→「実行」）。ワークフローに Secrets があれば署名するオプション（上記）を入れてある
- **将来**: (1) Azure Trusted Signing が使えれば、`signtool` の `/dlib` 方式でワークフローから署名する。(2) 署名ができたら MSI / EXE インストーラ化と winget への登録。(3) いずれも**一般公開（App Store / 広く配る段階）の前に判断**する。個人利用・テスト配布の間は署名なしを維持する

**迷った判断**
1. 配置先を Program Files にした（管理者権限が要る代わりに、保護されたアプリで読める）
2. 辞書・モデルの取得を、昇格しない親プロセスで行う 2 段構成にした（コードは増えるが、`%APPDATA%` の取り違えを避ける）
3. 動作中の imserver を止めない（更新後の反映はサインアウトか再起動）。macOS は止める。入力中の学習を失わないほうを優先した
4. 実機で未検証の間は prerelease（既定）。macOS のリリースワークフローには無い入力
5. DLL だけ CRT を静的リンク。imserver は動的のまま
6. `.cmd` ラッパー・MSI / EXE は作らない。スクリプトは PowerShell のみ
7. ブートストラップを ASCII のみにした（日本語の案内は zip 内のインストーラが出す）

**未検証（実機 / Actions）**: すべて。特に (1) `regsvr32` の登録と UAC の流れ、`-Elevated` 子プロセスの終了コード、(2) Program Files の DLL が UWP / 保護されたアプリで読めること、(3) 更新（上書き）時の `.old` 退避と再起動後の切り替え、(4) `uninstall.ps1` の再起動時削除、(5) `irm | iex` の動作（PowerShell 5.1 と 7）、(6) ワークフロー全体（windows-latest での llama ビルド、`signtool` の場所、スモークテスト）。PowerShell の構文は Linux の PowerShell 7.5 の構文解析器と `Test-Payload`（`SHA256SUMS` の検証）の動作で確認した範囲のみ

**Phase 5 で残る未実装**: 複数ホストでのエンジン共有（名前付きパイプ + 常駐ブローカー、H-39）、言語バーのボタン、`ITfFnReconversion::GetReconversion`、アプリ内の自動更新（置き換え）、Windows 用の依存ライセンス一覧、ARM64 / 32bit ホスト対応、辞書・モデルのアプリ内ダウンロード導線、Authenticode 署名の実施、winget / MSI。実機での検証すべて（`docs/real-hardware-checklist.md` §8）

### H-43. Windows フロント: GetReconversion・言語バーのボタン・Windows 用ライセンス一覧、エンジン共有（ブローカー）の設計メモ（2026-10-22、ユーザー指示「裁量に任せる」、実機未確認）

H-41・H-42 の残件のうち、実機なしで作れるものを実装し、作らないものは設計メモにした。コア（`core/` の各 crate）とプロトコルは変更していない（変更は xtask・`nagi-windows`・`nagi-windows-tsf`・文書のみ）。

**1. `ITfFnReconversion::GetReconversion`（H-41 では `E_NOTIMPL`）**
- 流れ: フォーカス中のコンテキストで**同期の読み取り編集セッション**を要求して範囲の文字列を読む → `eligible_text`（空・200 UTF-16 超は失敗）→ コアの `reconvert`（待ちは `Reconvert` と同じ 2 秒）→ 応答から候補を作る → **すぐ `reset` でコアの変換中状態を戻す**（候補を選ぶかはアプリ次第で、選ばれるまでコアを変換中に残さない）→ `ITfCandidateList`（`EnumCandidates` / `GetCandidate` / `GetCandidateNum` / `SetResult`）を返す。`SetResult` は `CAND_FINALIZED` のときだけ、範囲を候補の文字列に置き換えてキャレットを末尾に置く（`CAND_SELECTED` / `CAND_CANCELED` は何もしない）
- **候補の作り方（`nagi_windows::reconvert::range_candidates`、テスト済み）**: `ITfFnReconversion` の候補は**範囲全体の代わり**になる文字列だが、コアの候補は注目文節（`preedit.focus`）だけの代わり。そのまま渡すと複数文節の再変換で範囲の残りを失う。そこで先頭に変換結果の全文、続けて「注目文節だけを各候補に替えた全文」を並べる（UTF-16 の区間で差し替え、不正な区間・サロゲートの途中は捨てる。重複と空は除き、最大 30 件）。文節ごとの選び直しは `Reconvert`（composition にする経路）が担当する
- 入力中（composition あり）は失敗を返す。アプリが文書ロックを持つ最中に呼ぶと同期セッションが取れず失敗する（その場合アプリは `Reconvert` 等にフォールバックするはず。**要実機確認**）。選択した候補はコアの学習には送らない（`select_candidate` の対象はコアの変換中状態に限られ、`reset` 後は無い）。再変換結果の学習は未対応
- **迷った判断**: (a) 候補一覧に composition を使う案（`Reconvert` と同じ）は、`GetReconversion` が「候補を返す」契約なので採らなかった。(b) 注目文節だけの候補を返す案は上記の理由で採らなかった。(c) 全文候補のみ（文節差し替えなし）にする案は、候補が 1 件になり機能が弱いので採らなかった

**2. 言語バーのボタン（`langbar.rs`、`ITfLangBarItemButton` + `ITfSource`）**
- 現在のモードを「あ / A」で表示（アイコンは藍色の地に白字を GDI で描いて毎回作る。文字・ツールチップも）。**左クリックでひらがな ⇄ 英数**、右クリック（OS が `InitMenu` を呼ぶ）のメニューは: ひらがな / 英数（現在にチェック）、状態の行（`menu_lines`。選べない）、「設定ファイルを開く…」。スタイルは `TF_LBI_STYLE_BTN_BUTTON | SHOWNINTRAY`
- **モードの変更は `mode_comp` のコンパートメントへ書く**（他のクライアント・トレイの操作と同じ経路。変更通知から `perform_action(SetMode)` でコアへ）。ボタンからコアへ直接送る経路は作らず、「同じモードなら送らない」の一元化（`ModeSync`）を保つ。コアでモードが変わったとき・Activate・エンジン再起動後に `OnUpdate` で描き直させる
- 「設定…」は GUI を作らず（Phase 4 の設定 GUI の範囲）、コアが `get_status` で返す `config_path` の `config.toml` を**メモ帳で開く**（`.toml` の関連付けに依存しないため `notepad.exe` を明示。ファイルが無ければメモ帳が作成を尋ねる）。`config_path` が無ければ項目は選べない（灰色）。ラベルは「設定ファイルを開く…」（設定画面ではないので）
- ロジックは `nagi_windows::status`（`MenuEntry` / `MenuCommand` / `command_id`・`command_from_id` / `menu_entries` / `toggled` / `button_text` / `button_tooltip` / `settings_path`）に置き、COM は ID の変換と描画のみ。H-41 の「言語バーのボタンは実装しない」を覆した。理由: Win8 以降のトレイ表示（コンパートメント）だけでは、メニューから状態の確認・設定の入口が出せず、macOS のメニューバーのメニュー（H-38）に当たるものが無かったため。トレイの「あ / A」は従来どおりコンパートメントで OS が描く（二重に出るかは**要実機確認**）
- **迷った判断**: (a) `BTN_MENU`（左クリックもメニュー）にする案は、1 クリックでの切り替えができなくなるので採らなかった。(b) `GetIcon` は毎回新しい `HICON` を返す（言語バーが破棄する前提。もしそうでなければリークする。**要実機確認**）。(c) メニューを開くたびに `get_status` を同期で呼ぶ（UI スレッドで最大 250ms。開いたときだけなので許容）

**3. Windows 用の依存ライセンス一覧（`cargo xtask gen-licenses`）**
- **別ファイルにした**: `gen-licenses [--platform ios|windows|all]`（既定は all）。iOS は従来どおり `THIRD_PARTY_LICENSES.md`（`project.yml` がアプリに同梱するため名前と内容を変えない。Windows 専用の crate を混ぜない）、Windows は **`THIRD_PARTY_LICENSES_WINDOWS.md`**（104 crates）。`bundle-windows` は後者を zip の中の `THIRD_PARTY_LICENSES.md` として同梱する（`install.ps1` の期待する名前は変えない）。1 ファイルに統合する案は、配布物ごとに実際に取り込まれる依存が違い（iOS に `windows` / `ring` / `rustls` は入らず、Windows に iOS 専用 crate は入らない）、各配布物のライセンス表示が過不足になるため採らなかった
- Windows の依存閉包は `cargo tree -e normal --target x86_64-pc-windows-msvc` を **`nagi-windows-tsf`（DLL）と `nagi-server --features llama`（imserver）の 2 起点の和**として求める（ターゲット未インストールでも `cargo tree` / `cargo metadata` は動くので Linux で生成できる）。proc-macro とローカル crate は除く（従来どおり）。UNKNOWN のライセンスは無かった
- テスト: `cargo tree` 出力の解析・引数組み立て・プラットフォーム定義・描画、`bundle-windows` が Windows 用を同梱すること。**依存を変えたら再生成してコミットする**（CI での差分検査はまだ無い。`check-types` のような検査を足すかは未決）

**4. 複数ホストでのエンジン共有（名前付きパイプ + 常駐ブローカー）: 実装しない。設計メモ**

現状（H-39）はホストプロセスごとに `imserver` を起動する。**実測が無いまま常駐プロセスと IPC を足すと、障害点（権限・寿命・ACL）が増えるだけで利得が不明**なので今回は作らない。判断に必要な実測を先に取る（下記）。

*判断の前提となる現状の問題*
- メモリ: 各 imserver が辞書（mmap で共有されうる）とモデルを読む。モデルは small で数百 MB 規模、`reconvert` の逆引き索引は約 270MB（G-29。`minimal`/`light` は使用後に解放）。ホストが 10 個あれば 10 倍になりうる（辞書は mmap でページ共有されるが Windows では非 mmap 経路。`Dictionary::load` の mmap は非 Windows、H-7）
- 起動時間: ホストを開くたびに `initialize`（モデル読み込み数秒）。その間のキーは素通し（H-39）
- 学習データの整合: 複数の imserver が同じ `learning.tsv` を書く。原子的 flush のため最後に書いた側が勝ち、他ホストの学習を失いうる（現状の既知の弱点。ブローカー化の最大の利点）

*ブローカーの責務（案）*
1. 1 利用者・1 セッション（Windows のログオンセッション）につき `imserver` を 1 つ保持する（`nagi-broker.exe`。子プロセスとして起動し、コア・プロトコルは変更しない）
2. 各ホストの TSF DLL は**名前付きパイプのクライアント**になり、JSON-RPC をそのまま流す。ブローカーは接続ごとに JSON-RPC の `id` を付け替えて（クライアントごとの `id` 空間が衝突するため）imserver へ多重化し、応答を元の接続に返す
3. 非同期通知（`update_preedit` 等）は `session_id` で宛先の接続を引いて配る。プロセス全体の通知（`notify` 等）は全接続へ。セッション ID は現状 `win-<pid>-<n>` でプロセスをまたいで一意なので、そのまま使える
4. `initialize` は最初の接続でだけ imserver に送り、以降の接続には保存した結果を返す（クライアントのバージョン差は、ブローカーが自分のバージョンと一致するか確認する）。`shutdown` は全接続が切れて猶予（例: 10 分）が経ってから
5. `restore_session`: imserver が落ちて再起動したとき、ブローカーが全接続分の最新 preedit を保持して復元する（現在は各ホストの `SessionStore` が保持している。ブローカー側へ移すか、ホストが再接続時に送り直すかを選ぶ）
6. 環境（`set_environment`）は全ホストが送ってくるので、最後の値の重複排除をブローカーでする（現状はホストごとの `EnvironmentReporter`）。`set_context` はセッション単位なのでそのまま転送

*パイプ名・ACL・寿命*
- 名前: `\\.\pipe\LOCAL\Nagi-<ユーザー SID>-<バージョン>`（`LOCAL\` でセッション内に閉じる。バージョンを入れるのは、更新の途中で新旧のホストが混在しても互いに接続しないため）
- ACL: 作成者（その利用者）のみ読み書き（DACL に利用者の SID だけ）。**AppContainer / 保護されたアプリ**から接続させるには `ALL APPLICATION PACKAGES` の許可が別途要る。許可すると任意の UWP アプリが入力内容を受け取れる経路になるため、**保護されたアプリには許可しない**か、許可するなら入力内容が流れることを明記して判断する。`FILE_FLAG_FIRST_PIPE_INSTANCE` と `PIPE_REJECT_REMOTE_CLIENTS` を指定し、サーバーのなりすまし（先に同名パイプを作られる）を防ぐ。クライアント側は接続後に `GetNamedPipeServerProcessId` でブローカーの実行ファイルのパスを検証する
- 起動: ホストの DLL が接続に失敗したら、`CreateProcess` でブローカーを起動して再接続する（排他は名前付きミューテックス。競合した側は待って再接続）。常駐の自動起動（Run キー / スタートアップ）は使わない（遅延起動のみ）
- 寿命: 接続が 0 になってから猶予後に自分で終了。ブローカーのクラッシュ時は、各ホストが現行の指数バックオフと同じ方針でブローカーを再起動し、`restore_session` で復元する。imserver のクラッシュはブローカーが担当（現行の `EngineSupervisor` をブローカー側へ移す）
- 更新: ブローカー実行ファイルが使用中だと置き換えられない（現行の `.old` 退避と同じ運用）。バージョン入りのパイプ名により、新版のホストは新版のブローカーを起動する

*現行方式からの移行手順（段階）*
1. `nagi-windows` の `EngineSupervisor` が使う `EngineLauncher` / 通信路 trait を流用し、「名前付きパイプの通信路」実装を追加（コア不変。既存のテストの流儀で、Linux ではユニックスソケット等の代替で確認）
2. ブローカー（`nagi-broker`）を新規 crate として追加。まずは**ホスト 1 つ + ブローカー**で、現行と同じ挙動になることを実機で確認
3. 設定（レジストリまたは環境変数）で「ブローカーを使う / 使わない」を切り替えられるようにして、既定は使わない。複数ホスト・クラッシュ・スリープ復帰・更新を実機で検証
4. 実測で利得（下記）が確認できたら既定を切り替える。旧方式は `minimal` 相当のフォールバックとして残す（ブローカーに接続できないときは従来どおりプロセス内で起動）

*判断に必要な実測項目（実機）*
- ホストを 1 / 5 / 10 個開いたときの imserver の合計の**ワーキングセット・コミットサイズ**（辞書の共有ページを区別する）と、`reconvert` 後のピーク
- ホスト起動から変換可能になるまでの時間（`initialize` の所要）と、その間に取りこぼすキー数
- 名前付きパイプ経由の `process_key` の往復時間の増分（目標: p95 で +1ms 以内。目標値 `キー入力→ひらがな反映 p95 < 16ms` への影響）
- 複数ホストで同時に学習したときの `learning.tsv` の取りこぼし（現状の弱点の大きさ）
- AppContainer のアプリ（設定アプリ・Store アプリ）からパイプへ接続できるか、ACL の許可の可否
- ブローカーが落ちたとき・サインアウトしたとき・スリープ復帰のときの再接続の所要

**5. 検証**: `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace`（llama なし）と、`cargo check` / `cargo clippy -p nagi-windows-tsf --target x86_64-pc-windows-gnu --all-targets -- -D warnings`（型・シグネチャのみ。リンクと実行は未確認）。`GetReconversion` と言語バーの COM 部分は**実機で一度も動かしていない**。特に (1) `ITfLangBarItemMgr` を `ITfThreadMgr` への `QueryInterface` で得られるか、(2) `BTN_BUTTON` の右クリックで `InitMenu` が呼ばれるか、(3) `GetIcon` の `HICON` の所有権、(4) `GetReconversion` を呼ぶアプリ（Office 等）が文書ロックの外で呼ぶか、`SetResult` の呼び方、(5) トレイの「あ / A」との二重表示

**Phase 5 で最終的に残る未実装**: 複数ホストでのエンジン共有（上記の実測が先）、アプリ内の自動更新（置き換え）、ARM64 / 32bit ホスト対応、辞書・モデルのアプリ内ダウンロード導線、Authenticode 署名の実施、winget / MSI、設定 GUI（Phase 4）、再変換候補の選択結果の学習、候補ウィンドウのマウス操作（v1 非目標）。実機での検証すべて（`docs/real-hardware-checklist.md` §8）

### H-44. iOS 実機確認の結果と、クリック音・触覚の連続再生の課題（2026-10-22〜23、要対応）

- **2026-10-22 実機確認（ユーザー報告）**: `docs/ios-device-checklist.md` §3 の実装済み・未確認の全項目（診断ログ、⌫ 連打、英字シフト、レイアウト各種、設定タブ、スワイプ、テーマ、貼り付け、ホストアプリ、コア連携、学習・ユーザー辞書など）を実機で確認し、問題なし。ビルド番号の記録は無し。
- **2026-10-23 曖昧入力 M1・M2（H-24・H-25）**: QWERTY かなで `kamji` と打つと、「もしかして」に「感じ」「上」「上じ」が出た。「感じ」は期待どおり。「上」「上じ」は余分な候補で、`m` を `i` と読む補正に由来すると思われる。誤補正の多さと順位は調整の余地があり、未判断。σ_noise の実測は、指の位置を保存しない方針のため実施しない。
- **2026-10-23 確認済み**: 絵文字・顔文字（H-21）、D-5（学習データ削除。順番の確認のみで、真の同時書き込みは未検証）、Iconify（H-36。API 応答・SVG → PNG・描画・出典表示を含め問題なし）。H-36 の「未確認」は確認済みに注記した。
- **据え置き**: (1) メモリ閾値（実機で測れないため、自然に起きたときのユーザー報告を待つ）。(2) フルアクセス（触覚）の OFF / ON の比較（未確認）。(3) 他機種（後日ユーザーが実施）。
- **課題（要対応）: 速く打つと、前回のクリック音・触覚が鳴り終わるまで次が鳴らない**（設定どおりには動くが違和感がある。H-30・H-31 の続き）。
  - **原因（コードから読んだ推定）**: `frontend-ios-spike/Keyboard/KeyboardFeedback.swift:23-62` の `CompatFeedbackPlayer` は、種類（haptic / sound）ごとに「再生中」フラグを持つ（:30-31）。再生中に来た要求は捨てる（:37-40）。フラグを下ろすのは `AudioServicesPlaySystemSoundWithCompletion` の完了ハンドラ（:42-45）だけなので、前の音・触覚が鳴り終わるまで次は鳴らない。H-31 で「重ねない」ために入れた仕組みがこれに当たる。`keyDown()`（:110-150）は要求を出すだけで、捨てられる。
  - 触覚は互換モード（`keyboard.haptic_engine = compat`）のときだけ該当する。標準モードは `UIImpactFeedbackGenerator.impactOccurred()`（:126-137）を押下ごとに呼ぶため、この再生中ガードは無い。
  - 未確認: 完了ハンドラが期待どおり毎回呼ばれるか（呼ばれないとフラグが下りたままになる）。実機の再現条件も未確認。
- **決定**: ユーザーの提案を採用する。新しいキーの押下時は、前回の音と触覚の再生を待たず、新しい再生を始める（重ねないことより、入力を待たせないことを優先する）。
  - 方針: 再生中ガードを外す。完了ハンドラで下ろすフラグは世代番号で区別し、古い再生の完了が新しい再生のフラグを下ろさないようにする。前の再生を止める API の有無は未確認のため、止めるかどうかは実装時に確かめる。
  - **実装は未着手**。Swift はローカルでコンパイルできないため、CI（`ios-testflight`）と実機で確認する。

### H-45. 入力中のサジェストの選択操作・Space 変換の候補の拡充・取り残しの修正（2026-10-23、ユーザーの実機フィードバック、要実機確認）

- **背景**: macOS 実機（0.1.2）で、(1) サジェストが消えずに残ることがある、(2) Space 変換の候補が似たものばかり、(3) 短い単語で欲しい変換が出ない、(4) サジェスト表示中に変換へ進む操作が無い（↑↓ は何もせず、Tab は学習・ユーザー辞書の前方一致のみ、Enter はサジェストを選ばない）、の 4 点が出た
- **Space 変換（sentence 経路）**: 文全体の N-best（既定の幅 3）に加えて、**読み全体に対する文節候補**（`segment_candidates`: 学習・ユーザー辞書・システム辞書の同音異義語・ひらがな/カタカナ・Rewriter。出所を併記して重複排除）を後ろに続け、全体で 50 件（`CANDIDATE_LIMIT`）まで。1 件目は従来どおり（N-best の LLM 再採点済み）。短い単語の同音異義語が出きらなかった原因は、N-best の幅しか並べていなかったこと。設定項目は作っていない（品質優先）。**未確認**: 実バックエンドで `segment_candidates` が 1 回増えることの Space→候補表示の p95（目標 150ms）への影響
- **サジェストの選択（Composing）**: Action `SuggestNext` / `SuggestPrev` を追加（キーマップの TOML でも指定可）。サジェスト表示中だけ、↓ と Tab = 次、↑ = 前（先頭から ↑ で選択解除）。選択中の Enter = その候補を確定（`select_composing`: 学習・次語予測も通常の確定と同じ）、Space = 選択を解除して通常の変換、Esc = 選択解除のみ、それ以外のキーは選択を解除して従来どおり。サジェストが無いときの Tab（`Predict`）・↑↓（`NoOp`）は従来どおり。キーマップで上書きしたキーはその上書きが優先
  - 実装は `handle_key` で、サジェスト表示中かつ解決結果が `Predict` / `NoOp` のときだけ読み替える形（`resolve` は従来どおり）。Shift+Tab / Shift+↓ も `SuggestNext` になる（Shift+Tab を `SuggestPrev` にする案は見送り）
  - **プロトコル**: `CandidatesView.unselected: Option<bool>` を追加（加算的。`Some(true)` = 候補は出すが選択行なし）。従来は `selected` が 0 のとき「未選択」と「先頭を選択」を区別できず、最初の ↓ で表示が変わらなかった。macOS と Windows の候補ウィンドウは未選択のとき強調しない。iOS は無視する（候補バーの表示は変えない）
- **取り残し（コア）**: 同じ読みでも `left_context` が変わったら再計算する（`ComposingCands` に文脈を持たせた）。Composing 以外の状態では `finish` で `composing_cands` を捨てる（Esc・確定・Reset などで Direct に戻って同じ読みを入れ直しても古い候補が出ない）。`set_context` で Composing 中に文脈が変わったら、サジェストを捨てて非表示の差分を返し、サーバーが `update_candidates` を push して再計算を予約する。Composing を抜けるすべての経路で同期応答が非表示の差分を返すことを回帰テストにした
- **取り残し（macOS フロント）**: 入力中の文字列が空になったらコアの応答に関わらず候補パネルを閉じる。入力中の文字列が無いときに届いた `update_candidates` は表示しない。候補パネルは全入力欄で共有なので、出した入力欄のセッションが閉じる・破棄されるときだけ閉じる（別の入力欄の候補を、遅れて来た deactivate で消さない）
- **迷った判断**: (1) Tab を「次のサジェストを選ぶ」にしたため、サジェスト表示中の前方一致（`Predict`）は使えなくなる（サジェストが出ていないときは従来どおり）。前方一致を使いたい場合は、キーマップで別のキーを `Predict` に割り当てる。(2) 選択中の Enter は確定（Space は変換）とした。矢印だけで変換に進む案は採らなかった

### H-46. macOS: 確定のあとに古い非同期更新が反映されて、確定した文字が複製される問題の修正（2026-10-23、ユーザーの実機報告、要実機確認）

- **症状**: 変換を確定したあと、確定した文字と同じ文字が後ろに入力中の文字として残る（次に打つと消える）。再現性が低い。サジェストの取り残しも同じ原因と考えられる
- **原因**: `update_preedit` / `update_candidates`（ライブ変換・サジェストの遅れて届く結果）は、受信スレッドで `RevisionGate` により古さを確かめたあと、`DispatchQueue.main.async` でメインスレッドへ渡される。一方、キー処理（Enter 等）はメインスレッドで同期応答を待っている。確定の少し前に計算された更新が、同期応答の記録より先に受信されると、ゲートを通ったあとで、メインでは確定の反映が先に走り、そのあとで古い更新が反映される（確定した文字が入力中の文字として復活する）。反映する時点では古さを確かめていなかった
- **修正**: 入力欄ごとに反映済みの revision を持ち（`AppliedRevision`）、反映する直前に、反映済みの同期応答以前の非同期更新を捨てる。プリエディットと候補が同じ revision で続けて届くことがある（コアが同時に送る）ので、非同期どうしは同じ値を許す。エンジンの再起動で 0 に戻す
- **未確認（実機）**: 再現性が低いため、直ったかは使いながらの確認になる。キー処理が応答待ちで時間切れになった場合（`keyTimeout` 0.6 秒）に遅れて届く同期応答は、別の経路なのでこの修正の対象外
- **入力メニューの状態表示を設定ウィンドウへ移した（2026-10-23、ユーザー指示）**: 入力メニュー（H-37）の先頭に出していた「プロファイル: …（理由）」「変換: …」「学習: …」「ユーザー辞書: …」は、横に長くてメニューの邪魔になるため、設定ウィンドウの先頭の「状態」セクションに移した（開くたびに `get_status` を読み直す）。メニューの先頭に出る見出しの「Nagi」は IMKit が付けるもので、そのまま。メニューを開くときに同期で `get_status` を呼ばなくなったので、開くのも速くなる

### H-47. 実機フィードバックの調査（電卓・アイコン・標準の英字・Meltype）と、次の機能の計画（2026-10-23、ユーザー指示）

- **標準の英字（ABC）が削除できない件**: `NagiIMEApp --diagnose`（実機 macOS 27.0.1）で、`com.shiki.inputmethod.Nagi.Roman` は種別「入力モード」で **ASCII 入力可能: はい**。「Nagi が英字と判定されていない」説は成り立たない。macOS が英字の**キーボード配列**を最低 1 つ残す仕様（コミュニティの情報。Apple の公式説明は未確認）と推測する。Nagi 側の変更は不要と判断。診断の「有効」「名前」「バンドル」は値を読めなかった（結論に影響しない）
- **電卓候補が出ない件（調査済み。実装は H-48 以降に記録）**: 原因は 2 つ。(1) `[convert] calculator` の既定が false。(2) `1+2` は `Literal` で、`engine.rs` の `start_converting` / `ensure_candidates` が `Literal` では Rewriter（日付・漢数字・全角/半角・電卓）を呼ばない（1 回目の Space で窓も開かない）。したがって電卓だけでなく、数字・記号だけの入力では Rewriter の候補が全く出ていなかった
- **Fn の言語切替でアイコンが白い四角・透明な件（原因の見立て、確度: 中）**: `.app` に `CFBundleIconFile`（.icns）が無い。Mozc / Squirrel は `CFBundleIconFile` に .icns を置く。対応として、xtask で `nagi.icns`（青緑の角丸四角に白い「凪」。PNG 埋め込みの 9 種別、約 80KB）を書き出し、`CFBundleIconFile` と `tsInputMethodIconFileKey` に指定した。`TISIconIsTemplate`（ルート）は true のまま。**白いままなら外す（2 段階）**。icns の種別とサイズの対応は記憶に基づく（確認できる資料に接続できなかった）。実機で要確認。確認時は、再インストール後に `killall TextInputMenuAgent` とログアウト（必要なら入力ソースの再追加）
- **Meltype（GPL-3.0。https://github.com/yksr-melt/Meltype）の調査**: 日英の自動判定を中核とする Windows 向けの日本語入力（Mac・Linux はプレビュー）。変換エンジンは Microsoft IME / Mozc / azooKey で、LLM は持たない。GitHub の API に接続できず、ソースの中身・最終コミット日・リリースは未確認（README・docs・THIRD-PARTY-NOTICES.md の記述による）。Nagi が優れる点: LLM による変換、環境プロファイル、範囲操作・再変換、キーマップ、iOS、ライセンス。Meltype が優れる点: 日英の自動判定、Windows の完成度と利用者規模、アプリごとの設定、周辺機能。**取り入れる点は `docs/plan-next-features.md` に計画として組み込んだ**（アプリの種類ごとの既定モードと素通し、他 IME の辞書の取り込み、日英の自動判定、英語の綴りの「もしかして」）。**コードは取り込まない**（GPL-3.0 と MIT OR Apache-2.0 の非互換。仕様を文章にして独立に実装する。ユーザー了承済み: 「コードは参考にしても、自分で考えて同じことができるならそれでよい」）
- **計画（実装は未着手）**: `docs/plan-auto-english.md`（日英の自動判定。raw のキー列を保持して、日英の区間を格子上の最小コストで切り分ける。段階 1: 候補に出すだけ → 段階 2: 先頭を ASCII → 段階 3: 入力中に切り替え。設定: off / 慎重 / 標準 / 積極的）、`docs/plan-loanword-translation.md`（カタカナ語の翻訳候補。JMdict の外来語の原語（`lsource`）を、同梱せず取得して索引にする。英語だけでなく、原語の言語を単語ごとに出し分けられる。任意の言語への翻訳は範囲外。**データの構造は記憶に基づくので、実物で最初に確認する**）、`docs/plan-next-features.md`（全体の順序と、アプリごとの既定モード・辞書の取り込み・英語の綴りの「もしかして」の仕様）

### H-48. 候補の拡充: 電卓の既定 ON・数字記号だけの入力でも Space で候補・記号の変換・末尾の形式候補（2026-10-23、ユーザー要望、要実機確認）

- **電卓**: `convert.calculator` の既定を **true** にした（`config/default.toml` と `ConvertSection` の `Default`）。自前の再帰下降パーサー（`core/rewriter/src/calc.rs`）で、`+ - * / × ÷` と全角版、括弧、優先順位、末尾の `=`／`＝` に対応。結果は整数は整数、小数は有効桁 10 で末尾の 0 を落とす。0 除算・絶対値 1e15 以上・長すぎる式・深すぎる括弧・先頭の符号・演算子のない入力・べき乗は候補を出さない（`10/3` は `3.333333333`）
- **Literal（数字・英字・記号だけの入力）でも Space で候補を出す**: `ConversionBackend::literal_candidates(text, limit)`（既定実装は空。`Pipeline` は Rewriter だけを返し、辞書・LLM・学習は通さない）。1 回目の Space から窓が開く（候補が表記 1 件だけなら従来どおり開かない）。混在入力では Literal 文節にフォーカスしたとき同様。先頭は常に元の表記。以前は Literal では Rewriter（日付・漢数字・全角/半角・半角カナ・電卓）が一切出ていなかった（H-47 の原因 2）
- **記号の変換**（`core/rewriter/src/symbols.rs`）: 読みから（`かっこ` → `「」` `『』` `（）` `【】` …、`まる`・`てん`・`さんかく`・`ほし`・`やじるし` など約 50 項目）と、1 文字の記号から（`(` → `（` `「` `『` `【` …、約 30 文字）。出所は Rewriter。**ローマ字入力の `(` は Japanese の `（` になる**（`config/romaji/standard.tsv`）ので、全角の記号と `。、「」・ー〜` も同じ表で引く（Space 2 回で `（` → `(` → `「` → …）
- **末尾の形式候補**: Japanese 文節の候補の末尾に、ひらがな・カタカナ（出所 Kana）・全角英数・半角英数（出所 Rewriter）の順で並べる（既出の表記は出所だけ併記）。Literal は全角・半角を末尾。件数の上限は主要部分を `CANDIDATE_LIMIT - 4` で切ってから末尾を足す。文全体 N-best 経路でも同様。全角英数・半角英数の元のローマ字は、`Composer::strokes()` の打鍵列を打ち直して文字ごとの打鍵を割り当てる（`RawMap`）。かな直接入力・フリックなど対応が取れない文節では出さない。Composing 中のサジェストには足さない
- **直した既存テスト**: 候補の末尾が増えたための期待値（`api.rs` の sentence 経路 2 件、`50_candidates.yaml` の先頭 Space・循環・↑↓）、電卓の既定 ON（`rewriter.rs`、`settings.rs`）、有効桁 10 の仕様変更（`10/3`・`1/3` の桁数）
- **未対応・要判断**: 形式候補を選んで確定すると、既存のカタカナ候補と同じく読み → 表記が学習される（例: きょう → `ｋｙｏｕ`）。学習から除外するかは要判断。`docs/ime-design-doc.md` に電卓の記述は見当たらなかった。実バックエンド（LLM）での遅延の計測は未実施


### H-49. アプリの種類ごとの既定モード・記号で始まる語の素通し・数式の `-`（2026-10-23、`docs/plan-next-features.md` A、要実機確認）

- **設定 `[apps]`**（`core/config`、`config/default.toml`、設定 GUI の「アプリごとの設定」）: `code_mode`（既定 `alphanumeric`）/ `chat_mode` / `general_mode`（既定 `keep`。`hiragana` / `alphanumeric` / `keep`）、`remember_mode`（既定 true）、`passthrough`（既定 true）、`passthrough_chars`（既定 `"/$@!:#"`）、`[apps.classes]`（利用者の分類表。パターン = `code` | `chat` | `general`）。同梱の分類表は `nagi_config::DEFAULT_APP_CLASSES`（macOS の bundle id と Windows の実行ファイル名。`com.jetbrains.*` など）。照合は大文字小文字を区別せず、末尾 `*` は前方一致。優先順位は「利用者の表 > 同梱の表」、それぞれ「完全一致 > 前方一致（長いパターン先）」。分類表は GUI には出さない（`config.toml` で編集）
- **focus_in**: 新しい Action は作らず、`Engine::focus_in` がセッションのアプリ識別子（直前の `set_context` で伝わったもの）からモードを決め、変わるなら入力中の文字列を確定してからモードを切り替え、応答の `mode` で返す。優先順位は「アプリごとの記憶 > 種類ごとの既定 > 変えない」。`app_id` が無い（`set_context` が先に来ていない）ときは変えない
- **モードの記憶**: 利用者のモード切替（`SetMode` / `ToggleMode`。`process_key` と `perform_action` 経由）をエンジンのメモリに保持する（キーは小文字のアプリ識別子）。**再起動で消える**（永続化は未実装）。`focus_in` による自動切替は記憶しない
- **素通し**: Hiragana モードで未入力（Direct・Composer が空）のとき、対象記号を打つと `Composer::begin_literal_run` で Literal を始め、以降の ASCII は Shift+英字と同じ英字継続（literal_mode）で Literal になる。その語の入力中の Space は変換ではなく**語の確定（`CommitAndPassThrough`）**で、スペース自体はアプリに渡る。Enter は確定、Esc は取り消し。英数モード・入力中の途中・`passthrough = false` では働かない。素通しにできるのは ASCII の記号だけ（`passthrough_chars` の英数字・非 ASCII は無視）。タッチ入力の `insert_reading` も同じ経路を通る
- **数式の `-`**: 直前（保留なし）が Literal の数字のときの `-` は ASCII `-` のまま（`1-2`・`(1-2)`・`10-3-2`）。それ以外（`ka-` → かー、先頭の `-`、`1ka-` → 1かー）の長音の規則は従来どおり。`merge_math_pieces`（直前の変更。コード内のコメントが「H-49」を指すのはこの件）で `(1-2)` は 1 つの Literal 文節にまとまり、電卓候補が出る
- **迷った判断**: (1) 分類表の優先順位の解決は設定側（`AppsSection::rules`）で行い、エンジンには優先順に並べた規則だけを渡す（エンジンは設定クレートに依存しない）。(2) 素通しの先頭記号は文字列（`"/$@!:#"`）にした（設定 GUI の Text で編集できるため）。(3) 素通しの Space を「確定してスペースを通す」にした（`/help` のあとのスペースが日本語変換の開始になる不都合を避けるため。スペースで全角英数などの形式候補は出なくなる）
- **未確認・未対応**: macOS では `activateServer` が `focus_in` の応答を反映した直後に `adoptSelectedMode`（OS で選ばれている入力ソースへコアを合わせ直す）を呼ぶ。`syncMode` が OS 側の入力ソースを同期的に切り替えない場合、コアのモードが元に戻り、**切り替えた結果がアプリごとの記憶として記録される**可能性がある（フロントは未変更）。実機で確認し、必要なら `adoptSelectedMode` を `focus_in` の応答にモードが含まれたときは省く。Windows・iOS で `set_context` が `focus_in` より先に送られているかは未確認。記憶の永続化は未実装

### H-50. 他の IME のユーザー辞書の取り込み（2026-10-23、`docs/plan-next-features.md` B、要実機確認）

- **変換器**: `core/dict/src/import.rs`（`import_user_dict(bytes, hint, existing)`）。形式は先頭の行と構造から自動判定する: `<?xml`/`<plist` → macOS plist、`!Microsoft IME` → MS-IME、`!!ATOK` → ATOK、それ以外は 3 列以上の行が多ければ Mozc TSV、2 列なら 2 列テキスト。ヒントがあれば判定より優先する。結果は `UserEntry` の列で、サーバーが Mozc TSV として書き出す（正の形式は変えない）
- **文字コード**: BOM つきの UTF-8 / UTF-16 LE・BE、BOM なしの UTF-16（NUL が片側の位置にだけ並ぶもの）、UTF-8、それ以外は Shift_JIS（`encoding_rs`。依存はロックに既にあったもので、新規の取得は無い）。ATOK の文字コード判定はこの範囲で足りる想定
- **品詞**: Mozc の品詞名はそのまま。他は部分一致で写す（人名・姓・地名・組織・固有名詞・副詞・接続詞・感動詞・連体詞・形容詞・名詞サ変・顔文字・記号）。動詞は「ラ行五段」などの活用が分かれば Mozc の `動詞ラ行五段` に、一段・サ変・カ変も同様、分からなければ `動詞`（Nagi のオーバーレイは `動詞` で始まる名前を動詞として扱う）。形容動詞・接頭/接尾・短縮よみ・その他は `名詞`。スニペット（品詞 `スニペット`）は Mozc TSV の取り込みでだけ有効
- **読み**: カタカナはひらがなに直し、ひらがな・長音・中黒・ASCII 英数字とハイフン以外（漢字など）を含む、または空なら「読みが不正」でスキップ。macOS plist は `shortcut` を読み、`phrase` を単語にする（品詞は名詞）。バイナリ plist は未対応（エラー）
- **重複**: 読みと単語が同じなら、既存の登録（ユーザー辞書ディレクトリ内の全ファイル）を優先し、ファイル内の重複は最初を優先する。スキップは理由ごと（`missing_columns` / `invalid_reading` / `empty_surface` / `duplicate_in_file` / `duplicate_existing`）に件数と最初の行番号を返す
- **JSON-RPC `import_user_dict`**（加算）: params `{data_base64, format?}`、result `{format, total, imported, skipped, skipped_reasons[]}`。内容は**バイト列そのものを base64** で渡す（JSON に UTF-16 などを載せられないため、文字コードの判定はコアが行う）。取り込んだ語はユーザー辞書ディレクトリの `imported.tsv` に追記し（既存は書き換えない）、`reload_config` と同じ経路で即座に反映する。ユーザー辞書の保存先が無い（`--user-dir` なしのテスト用サーバー等）ときは `ENGINE_FAILURE`、内容が空・形式不明・base64 不正は `INVALID_PARAMS`
- **macOS**: 設定ウィンドウに「ユーザー辞書」のセクションを足し、「取り込み…」（`NSOpenPanel` → `import_user_dict` → 件数とスキップ理由を表示）を置いた。計画書の「ユーザ辞書を編集…」は現状の macOS フロントに存在しないため、隣ではなく独立したセクションにした
- **迷った判断**: (1) 追記先を専用の `imported.tsv` にした（利用者が手で編集するファイルを書き換えないため。重複排除のため読み込みは `load_dir` 全体を見る）。(2) 2 列しか無い行は Mozc/MS-IME/ATOK でも品詞 `名詞` として受け入れた（1 列だけの行は列不足）。(3) 読みに ASCII 英数字を許した（macOS の shortcut は英字のことが多いため）。ただしローマ字入力から変換される読みは常にひらがななので、英字の読みの語はほぼ引けない
- **未確認**: 各 IME の書き出しの実物での確認は未実施（自作の小サンプルのテストのみ）。特に MS-IME / ATOK の品詞名の表記、ATOK の文字コード、macOS plist の実物。Swift は未コンパイル（CI の結果で確認）。`docs/ime-design-doc.md` §3.2 のメソッド一覧には未追記


### H-51. 英語の自動判定 段階 1（Space の候補に ASCII を足す）の実装と、閾値の根拠（2026-10-23、`docs/plan-auto-english.md`）

- **構成**: 新クレート `core/english`（`nagi-english`。`features.rs` 特徴量 / `lexicon.rs` fst 語彙 / `judge.rs` 判定）。エンジンは `ConversionBackend::english_level(raw, reading) -> Level` を呼ぶだけ（既定実装は語彙なしの劣化動作）。`Pipeline` が語彙と「日本語として引けるか」（システム辞書・ユーザー辞書・スニペット・学習）を渡して上書きする。コアの JSON-RPC・protocol の型は変えていない（`check-types` 差分なし）
- **特徴量**: 打鍵列（小文字 a-z のみ。数字・記号・大文字を含むものは対象外＝`/ $ @` の素通し H-49 や Shift 英字の Literal と衝突しない）を標準のローマ字テーブルで変換し、かなにならず素通しになった文字数（`Emit::Raw`）・その割合・連続の最大長を数える。`pt` `sc` のような日本語に無い子音連続はテーブルに当たらず素通しとして数えられるので、別個の子音表は持たない
- **語彙**: SCOWL 2020.12.07（`cargo xtask fetch-english` → `data/english/words.fst`）。取得元 SourceForge、**アーカイブ SHA-256 `5587667caa20c4891390c2d42dbb4d5c4c3f41bee77af1457ece3ba23fb859cc`**（実際に取得して検証済み。2,569,810 bytes）。`final/{english,american,british}-words.{10,20,35,40,50,55,60,70}` と `english-upper.{同}`（月・曜日・言語名など）を小文字にして取り込む（ASCII 小文字のみ・2 文字以上。アポストロフィ・アクセント付きは捨てる）。サイズ 80 以上（珍語）と `english-proper-names`（日本語の人名と衝突しやすい）は使わない。頻度の段階はそのファイルのサイズ番号（小さいほど一般的）。結果は **125,427 語、fst 635,653 bytes、`words.fst` の SHA-256 `feea3b4520db8889f809e95572b965ec15236a1000e2f6c2351e79a0329491b6`**。`Lexicon::load` は非 Windows で mmap（辞書と同じ方式。更新は一時ファイル → rename）。アーカイブの展開は datafetch 内の最小の tar 読み（`flate2` を足しただけ。ロックには既存）。ライセンスは SCOWL 本体の Copyright（許諾表示を残せば再配布・利用自由）で、同梱せず取得する方針どおり
- **判定スコア（`Judge::judge(raw, japanese_known)`）**: 3 文字未満・英小文字以外は 0。(1) 有効なローマ字（素通し 0）: 語彙にあり、日本語として引けず、4 文字以上（3 文字は段階 20 以下の最も一般的な語のみ）なら Mid（0.6。段階 41 以上は 0.45＝Low）、それ以外は 0（**日本語優先**。`sense` `banana` など）。(2) 素通しが残る: 語彙にあれば 0.9（High）、語彙の 2 語をつないだ複合語（`frontend`）は 0.85（High）、どちらでもなければ `0.2 + 0.9 × 素通し割合`（素通しが 1 文字だけなら 0.3 止まり＝打ち間違い扱い）で、語彙があるときは High に届かないよう 0.79 で頭打ち。段階: High ≥ 0.8 / Mid ≥ 0.5 / Low ≥ 0.3
- **段階 1 の見せ方**: High → Space の候補の **2 番目**（先頭の次）、Mid → 主要な候補の少し後ろ（**4 番目**。主要候補が少なければその末尾）、Low 以下 → 従来どおり末尾の「形式の変換」（半角英数）。**先頭は動かさない（自動では変えない）**。`off` ならこの並べ替えをしない。(a) 日本語の文節として変換される入力（`house` → ほうせ。文全体 N-best／先頭文節の両方）は、末尾にあった ASCII を前へ動かす。(b) ローマ字にならなかった英字が混ざる入力（`typescript` → `tyぺscりpt`。素通しの英字は Literal、かなは Japanese の Piece が交互に並び、そのままでは文節がばらばらになる）は、全体が Mid 以上のときだけ、曖昧入力（H-24）と同じく **入力全体を 1 つの文節（先頭は打った通りの読み）** にして候補窓を開き、そこに ASCII を置く。判定が弱ければ従来どおりの変換。→ 英語らしい入力に限り、Space の 1 回目の見え方が「文節ごとの変換」から「打った通りの全体」に変わる（崩れた表示を変換するよりまし、という判断）
- **評価コーパス**: `bench/auto-english/corpus.tsv`（自作 475 行: 日本語 `ja` 251 = 単語 + 文、英語 `en` 169 = 一般語 + 技術用語、日英混在 `mixed` 30、曖昧 `amb` 25。列: ラベル・打鍵列・「読みが辞書で引ける」か）。`sense` `text` `term` `made` `node` `tempura`（英語でもあるヘボン式の外来語）などは `amb` にして率に含めず、分布だけを見る。ハーネスは `core/english/tests/corpus.rs`（`--nocapture` で表）と、実辞書で測る `core/server/tests/auto_english.rs`（`data/` が無ければ省略）
- **実測（誤判定 = ja + mixed の 281 件のうち Mid 以上と判定された割合 / 取りこぼし = en の 169 件のうち Mid 以上（High）に届かない割合）**

  | 構成 | 誤判定 Mid+ | 誤判定 High | 取りこぼし Mid+ | 取りこぼし High |
  | --- | --- | --- | --- | --- |
  | 語彙なし（劣化動作） | 0.00%（0） | 0.00%（0） | 59.2%（100） | 96.4%（163） |
  | 自作の小さな語彙（419 語、技術用語なし。テスト用） | 0.00%（0） | 0.00%（0） | 24.9%（42） | 47.9%（81） |
  | SCOWL（辞書の「引ける」は列の値） | 0.00%（0） | 0.00%（0） | 7.7%（13） | 24.9%（42） |
  | **SCOWL + Mozc 辞書（実機に近い構成）** | **0.00%（0/281）** | **0.00%（0/281）** | **9.5%（16/169）** | **24.9%（42/169）** |

  実機構成の取りこぼし 16 件: `the` `button` `token`（Mozc の辞書で「てぇ」「ぶっとん」「とけん」が引けるため日本語優先。意図どおり）、`java` `kotlin` `ubuntu` `kubernetes` `redis` `nginx` `vue` `json` `rebase` `runtime` `tokio` `serde` `tokenizer`（語彙に無い技術用語。計画どおり**ユーザー辞書・学習で補う前提**。複合語の分割で `frontend` `backend` `github` などは拾えた）。High に届かない 42 件は、有効なローマ字になる英単語（`house` `there` `other` など）が Mid（4 番目）止まりなのによる
- **閾値の根拠**: 誤判定が日本語入力の邪魔になるのが最悪なので、**誤判定 0 を優先**して決めた。(1) 語彙にある語が有効なローマ字のとき High にしないのは、`house`→ほうせ のような日本語としても成り立つ列を先頭近くに出さないため。(2) 語彙に無い語の頭打ち（0.79）は、語彙があるのに載っていない列は技術用語・固有名詞・活用形・打ち間違いのいずれもありうるため。(3) 素通し 1 文字は 0.3 止まり（`kyouhaxtenki` のような打ち間違いを英語にしない）。(4) 日本語として引ける読みは常に日本語優先（Mid の条件に入れた）。(5) 語彙が無い劣化動作は素通し割合のみで、有効なローマ字の英単語は拾えない（取りこぼし 59%）代わりに、日本語を英語と判定しない。**コーパスは小さく（日本語 281 件）、誤判定 0 は「1% 未満」の根拠までで、0 の保証ではない**。テストは誤判定 High=0・Mid 以上 1% 以下、取りこぼし（劣化 65% / 自作語彙 30% / SCOWL 12% 以下）を固定している
- **設定 `[input.auto_english] mode = off|conservative|standard|aggressive`**（既定 `conservative`）。`config/default.toml`（日本語コメント）、`core/config`（`AutoEnglishMode`、既知キーの入れ子検査）、設定 GUI の記述子（`input.auto_english.mode`、Simple、選択肢 4 つ）に追加。**`standard`/`aggressive` は段階 2・3 の予約で、現状は `conservative` と同じ動作**（記述子の選択肢の文言・TOML のコメント・列挙体のドキュメントに明記）。アプリごとの上書き・例外語リストは未実装（計画書 §6 の 4 の後半。段階 2 と一緒に）
- **迷った判断**: (1) 判定の置き場所は、エンジンに語彙を持たせず `ConversionBackend::english_level` に寄せた（辞書・学習・ユーザー辞書が `Pipeline` にあり、「日本語として引ける」を同じ場所で判断できる。エンジンのテストは既定実装＝劣化動作で動く）。 (2) 候補の出所に新しい値（English）を足さなかった（protocol の変更と Swift の再生成を避け、既存の `Rewriter` のまま）。フロントのアイコンで区別したくなったら別途。 (3) `english-upper` は取り込み、`english-proper-names` は取り込まない（曜日・月・言語名は技術文書で多いが、人名の列は日本語のローマ字と衝突する）。 (4) 技術用語を同梱の自作リストで補う案は、評価が自己採点になるうえ「ユーザー辞書／学習で補う」指示と外れるため採らなかった
- **未実装・未確認**: 学習による補正（英語にした語・日本語に戻した語の記憶。計画書 §6 の 5）、ユーザー辞書の ASCII 語を語彙に加えること、アプリごとの上書き、段階 2・3。判定のレイテンシは未計測（1 回の判定は打鍵列の長さに比例する変換と fst 引き 数回で、Space を押したときの 1 度だけ呼ぶ）。実機（macOS・iOS・Windows）での使い勝手は未確認。（追記: `imserver --install-data` に語彙の取得を組み込み済み。進捗は `english_fetch` / `english_build`、取得済みで SHA-256 一致かつ `words.fst` があれば飛ばす。`web/static/install.sh` は `--install-data` を呼ぶので自動で入る。Swift の `DataInstallProgress.parse` は未知の段階を無視するので変更なし。`nagi-datafetch::install_english`。）iOS は語彙を `data/english/words.fst` に置けば読むが、取り込みの UI は無い（語彙なしの劣化動作になる）。英語の綴りの「もしかして」（`plan-next-features.md` C）は未着手

### H-52. カタカナ語の原語候補（JMdict の lsource）と、任意データのアプリ内導入（2026-10-23、`docs/plan-loanword-translation.md`）

- **データの実物確認**: EDRDG（ftp.edrdg.org / www.edrdg.org）はこの環境から取得できなかった。代わりに、PyPI の `jamdict-data` 1.5（JMdict 1.08 由来の SQLite。2020 年ごろの版）の `SenseSource` 表で構造を確認した（lsource 5,182 件、65 言語: eng 1,874 / fre 938 / ger 503 / ita 401 / lat 240 / chi 221 …、`lstype` は `''` と `part`、`wasei` は `y`、綴りが空の lsource がある）。**XML の生ファイルは未確認**で、XML の形は JMdict の DTD の記憶どおり（`<lsource xml:lang="ger" ls_type="part" ls_wasei="y">綴り</lsource>`、`xml:lang` 省略時 eng、属性の順序不定、空要素あり）と仮定した。パーサは自作の小さな XML と、上の SQLite から組み立てた JMdict 風 XML（gz）で試した
- **構成**: 新クレート `core/loanword`（`nagi-loanword`）。`normalize`（正規化・骨格・編集距離）/ `jmdict`（XML から語の入力を取り出す）/ `index`（fst の `Set` 1 つ。mmap、非 Windows）/ `archive`（gz の JMdict → 索引ファイル）/ `rewriter`（`nagi_rewriter::Rewriter` 実装）。取得は `nagi-datafetch::loanword`。`cargo xtask fetch-loanword [--from <JMdict_e.gz>]`。索引の大きさは、上のデータで 285,827 bytes（3,644 語）
- **索引**: キー `正規形 \0 順位 和製英語 言語 \0 綴り`（順位昇順に並ぶ）と、逆引き `\x01 骨格 \0 正規形`。順位は ke_pri / re_pri（`*1` = 0、`*2` = 1、nfNN、無印 = 3）×2 + 部分借用（`ls_type=part`）。1 つの正規形に最大 8 件。**1 つの語義に同じ言語の綴りが複数ある lsource は複合語（hot + dog）として捨てる**（言語が違うものは別言語の綴りとして残す: por/spa）。綴りが空・仮名を含む・48 文字超は捨てる
- **正規化**: ひらがな → カタカナ、中黒・空白除去、ヴ系 → バ行、小書き母音 → 大書き（ウィ → ウイ、ファ → フア、ティ → テイ）、末尾の長音符を除く（除いた残りが 3 文字以上のときだけ）。**語中の長音符は残す**（ビール / ビル、ボール / ボウルが衝突するため）。「ティ と チ」「ファ と ハ」の同一視は、別の語を取り違えるので採らなかった。骨格 = 長音符・促音・撥音を除き、オ段+ウ / エ段+イ の長音を除き、ヂ・ヅを統合
- **ゆるい一致**: **索引に語（和製英語としてのものも含む）が無いときだけ**、骨格の編集距離 1（置換・挿入・削除・隣接入れ替え）以内の語の原語を、正確な一致より後ろに出す。骨格 **6 文字以上**・**先頭の 1 文字が同じ**ときに限る（4〜5 文字を許すとフォルダー → forme、バイオリン → aïoli、ファイル → hwaiting のような誤りが実データで出たため、評価で絞った）。読み（ひらがな → カタカナ）からの引き当ては正確な一致のみ
- **出力**: 和製英語（`ls_wasei=y`）は出さない。複合語は出さない（索引に無い連続は引かない）。多義は順位順に最大 3 件、綴りの重複は除く。大文字小文字は辞書の綴りのまま
- **エンジンへの組み込み**: `Pipeline::segment_candidates` の Rewriter 群（記号などの Rewriter の後ろ・絵文字の前）に足す。引く語は、そのときの候補（学習・ユーザー辞書・LLM・システム辞書）のうちカタカナだけの表記と、読みをカタカナにしたもの。出所は `Rewriter`（protocol・Swift の型は変えていない。`cargo xtask check-types` 差分なし）。「形式の変換」（ひらがな・カタカナ・全角/半角）は従来どおりエンジンが末尾に足すので、原語はそれより前に並ぶ。`Pipeline::set_loanword_index`、サーバー起動時に `<data_dir>/loanword/loanword.fst` があれば読む（無ければ原語が出ないだけ）
- **設定**: `[convert.loanword] enabled`（既定 true）。`config/default.toml`、`core/config`（`LoanwordSection`、入れ子の既知キー検査）、設定 GUI の記述子 `convert.loanword.enabled`（詳細モード、表記の候補）
- **評価**（`bench/loanword/eval.tsv` 自作 237 語: 原語が出るべき `pos` 197 / 和製英語 20 / 外来語でない `neg` 20。ハーネス `core/loanword/tests/eval.rs`、`--nocapture` で表示。索引は `data/loanword/loanword.fst`、無ければ省略）。**索引は上の 2020 年版（lsource の充足度が低い）から作ったもの**で、現行の JMdict での数値ではない:

  | 指標 | 値 |
  | --- | --- |
  | 取りこぼし（pos で期待する原語が出ない） | 81.2%（160/197） |
  | うち誤訳（候補は出たが期待と違う） | 1.0%（2/197: `バス` → Bass（独。bus は索引に無い）、`テンプラ` → tempero（葡。期待 tempura は自作の誤り）） |
  | 先頭一致（pos で先頭が期待どおり） | 17.8%（35/197） |
  | 誤出力（和製英語・外来語でない語に何か出る） | 2.5%（1/40: `マンション` → mansion。JMdict が和製英語の印を付けていない） |

  取りこぼしの大半（149 件）は、2020 年版のデータでその語の lsource が空要素（綴りなし）または無いもの（computer、チーズ、ファン、ガス …）で、**出力が空**だった。つまり実行結果は「データが無ければ出さない」であり、誤った綴りは出していない。現行の JMdict は gairaigo の綴りの補完が進んでいるはずだが、**未確認**。評価語のうち自分の期待が誤りだったもの（ラーメン = lāmiàn、ヨーグルト = Yoghurt、テーマ = Thema の重複、カステラ = pão de Castella）は、結果を見て期待側を直した（システムの出力に合わせて調整したので、取りこぼしの数字は期待を直す前より良く見える可能性がある。直したのはこの 4 語のみ）
- **取得元と SHA-256**: 当初の EDRDG `JMdict_e.gz` は日々中身が変わり固定できないため、下の追記のとおり jmdict-simplified の版つきリリース資産に切り替えて固定した（この項の以前の「未固定で取得を拒否する」仕様は廃止）
- **任意データの導入導線**: (1) `imserver --install-data` に英語の語彙（H-51 の SCOWL、SHA 固定済み）と原語データを追加（進捗 JSON: `english_fetch` / `english_build` / `loanword_fetch` / `loanword_build`、失敗は `warning`）。`web/static/install.sh` はこれを呼ぶので自動で入る。(2) macOS: `DataInstallProgress` に同名の種別、完了時に警告があればまとめて表示、設定の「状態」に未導入の案内（`StatusMessages.dataLines`）。(3) iOS: ffi に `nagi_optional_data_sources`（取得元の JSON）と `nagi_build_optional_index`（取得済みアーカイブから索引を組み立て。ネットワーク無し）を追加（ffi は ureq/TLS を含めない方針のまま）。取得は本体アプリの Swift（`OptionalDataInstaller`、URLSession → SHA-256 検証 → App Group の `data/english`・`data/loanword` に配置 → FFI で索引を組み立て）。ホームの「追加データ」セクションに状態とボタン。拡張はコアが `data_dir` から読む（英語・原語とも mmap）。SCOWL の組み立てと JMdict の組み立ては、ネットワーク非依存の部分を `nagi_english::scowl` / `nagi_loanword::archive` に移した（datafetch は取得と進捗通知）
- **未確認**: Swift（macOS・iOS とも）はローカルでコンパイルしていない（CI で確認）。iOS の実機での取得・拡張での反映・メモリは未確認。JMdict の現行版での数値、XML の生ファイルでのパース、`ls_wasei` の現行の付き方は未確認。複合語の分割、言語の選択は作っていない（計画どおり）

#### H-52 追記: JMdict の固定（jmdict-simplified）と JSON 化、gloss からの推定（2026-10-23）

- **固定した資産**: scriptin/jmdict-simplified のリリース tag `3.6.2+20261019180022`（JMdict の日付 2026-10-19）、資産 `jmdict-eng-3.6.2+20261019180022.json.zip`（約 11.5MB）、**SHA-256 `ef3ea475127ce2a85c60baa3ca5a8f7e33b836e02c4fad6633b3dd72d2b362c9`**、URL `https://github.com/scriptin/jmdict-simplified/releases/download/3.6.2%2B20261019180022/jmdict-eng-3.6.2%2B20261019180022.json.zip`。固定値は `nagi_loanword::archive`（`JMDICT_TAG/FILE/URL/SHA256`）。値は GitHub Actions `jmdict-pin` の実行で得たもの（この開発環境からは github.com の資産に届かないため、取得・SHA-256 の再計算は未実施）。zip 内は `jmdict-eng-3.6.2.json`（約 118MB、218,930 語）1 ファイル。ライセンス（EDRDG: CC BY-SA 4.0）と jmdict-simplified の出典は NOTICE に記載
- **実データの構造（Actions で確認）**: `words[].kana[]{text, common}`、`words[].kanji[]{text, common}`、`words[].sense[]{languageSource[]{lang, full, wasei, text(null 可)}, gloss[]}`。languageSource は 6,341 件、うち text あり 4,350・wasei=true 2,061・full=false 81。抽出（`jmdict.rs`）はこの JSON に合わせた。XML 版は削除した
- **メモリ**: 118MB の JSON を丸ごと読まず、`serde_json` のストリーム（`words` 配列を `DeserializeSeed` で 1 語ずつ）で処理し、zip も展開しながら読む。保持するのは索引の組み立て用の集合だけ。ローカルでは 2020 年版から作った 50MB の JSON で、取得処理全体のピーク RSS が約 54MB（cargo を含む）、5 秒（debug ビルド）。実物（118MB）の値は Actions `loanword-eval` で測る。iOS は拡張ではなくホストアプリで走る前提
- **lsource の無い語への対策（設計見直し）**: 実データでは computer・チーズ・bus・guard などに languageSource の綴りが無い。そこで **綴りの無い語義**（languageSource が空、または text が null で wasei が付いていない）に限り、gloss が **1 語の英単語（小文字の ASCII 3〜20 文字）** で、**カタカナ表記と子音の骨格が一致**するときだけ「推定の原語」として低順位（順位 100 以上）で採る（`phonetic.rs`）。骨格は、子音を調音の近いものにまとめる（k/g/c、s/z/j、t/d、n/m、l/r、b/p/v、f/h。母音・半母音・促音・長音は無視。英語側は tion→shn などの綴りの揺れと、語末・子音前の r の脱落を扱う）粗い比較で、一致は完全一致のみ・骨格 2 文字以上。他の gloss（複数語・大文字・音が違う語）は出さない。gloss は重要な順なので、1 語義につき音が合う最初の 1 つだけ。擬音語・擬態語（ワクワク、前半と後半が同じ語）と `ly` で終わる gloss は除く。lsource 由来の綴りが常に優先（順位 0〜3）。**言語は「推定」で、languageSource に lang があればそれ（text が null の場合）、無ければ eng とみなす**
- **順位**: `common` フラグを使う（lsource 由来: `(common でない)×2 + (full でない)`、推定: 100 + (common でない)）
- **残る誤り**: JMdict が和製英語の印（wasei）を付けていない語（マンション → mansion など）は出る。gloss 推定は音が合うだけの別語（mail → male、mouse → mouth など。語義が別なら別々に出る）を出しうる
- **ローカルでの評価（実データではない）**: 実データが無いので、2020 年版（`jamdict-data`）から同じ JSON の形に組み立てた 50MB の JSON（`SenseGloss` を gloss に）で測った。索引 967,829 bytes・14,778 語。`bench/loanword/eval.tsv`（237 語）: **取りこぼし 24.4%（48/197）、誤訳 4.1%（8/197。うち正しい別綴り programme・anime・snorkel・etiquette・tiramisu 等と私の期待値の誤り tempero が 6 件、実際の誤りは mail → male、mouse → mouth の 2 件）、先頭一致 74.1%、誤出力 10.0%（4/40: マンション・シール・クーラー（評価語として不適切。JMdict が和製英語の印を付けていない英語由来語）と カタカナ → katakana）**。gloss 推定を入れる前（lsource のみ）は取りこぼし 81.2%。ハーネスの上限は誤訳 10%・誤出力 20%（データに依存するので回帰の目安）
- **実データ評価（Actions 実行後に追記）**: `.github/workflows/loanword-eval.yml`（手動実行）。取得・索引化（SHA-256 検証）、所要時間・ピークメモリ（`/usr/bin/time -v`）、索引サイズ、MISS/LEAK 一覧（先頭 80 件）、末尾の `pos …` の 1 行（取りこぼし・誤訳・先頭一致・誤出力）が出る
- **実データ評価（現行 3.6.2、Actions `loanword-eval`、コミット 6fd9630）**: 索引 **1,108,778 bytes**。`bench/loanword/eval.tsv`（237 語）: **取りこぼし 23.9%（47/197）、誤訳 3.0%（6/197）、先頭一致 74.1%（146/197）、誤出力 12.5%（5/40）**。取得・組み立て 32.7 秒、ピーク RSS 504,864 KB（下の注意）。傾向: (a) MISS は、gloss が 1 語でない語（`ice cream`、`roller coaster`、`personal computer` 型）、骨格が合わない語（母音の脱落・綴りと発音の差が大きい語。例: メニュー menu、デザイン design、イメージ image）、そもそも languageSource も推定できる gloss も無い語に大別される。(b) 誤出力（LEAK）は、JMdict が和製英語の印（wasei）を付けていない英語由来語・和製英語（マンション → mansion、シール → seal、クーラー → cooler など）と、日本語のローマ字化が gloss に載る語（カタカナ → katakana）。これらは印が無い限り機械的に除けない
- **ピークメモリの調査（2026-10-23）**: 上の 504MB は `cargo xtask`（開発ビルドの再コンパイルを含む）経由の計測で、コンパイラのメモリが混ざっていた。ビルド済みのバイナリを直接測ると、50MB の JSON（2020 年版から作成）で **17MB**、同じ語を 3 倍にした約 150MB の JSON でも **17MB で一定**（メモリは入力の大きさに比例しない）。zip エントリは `Vec` に展開せず `BufReader` 経由で `serde_json` が 1 語ずつ読み、保持するのは索引の組み立て用集合（正規形 + 原語のキー、骨格の逆引き、正規形ごとの件数）だけ。iOS ホストアプリの目標 150MB に対して十分余裕があるので、中間表現の縮小は行わなかった。workflow はビルド済みバイナリを直接測るように直した（再実行すれば実際のピークが出る）
- **評価語の見直し（2026-10-23）**: JMdict が wasei を付けていない英語由来語を「和製英語」として期待していた不適切な評価語（マンション・シール・クーラー・ワンピース）と、日本語のローマ字化の例（カタカナ）を `wasei` / `neg` から外し、集計に含めない `info` 種別に移した（出力は表示する）。`neg` に ヤマモト を足した。見直し後は 誤出力 0.0%（0/36）。上の実データ評価の誤出力 12.5% は見直し前の評価語での値

### H-53. iOS の数字・記号のキーをコアの入力中の文字列に通す（2026-10-23、要実機確認）

- **決定（ユーザー）**: 「数字・記号のキーも入力中の文字列（コアの composing）に入れる。設定でオフ可。既定オン」。それまで数字・記号のレイアウトは `hostInsert` で半角のままホストへ直接入れていたので、コアの候補（電卓の答え `merge_math_pieces`、全角/半角、記号の装飾。H-48/H-49）が出なかった。
- **実装**: `KeyboardModel.inputText` / `toggle`（`flick` の文字・トグルも `inputText` / `toggle` を通る）で、`NumberCompose.shouldCompose`（`Keyboard/TouchLogic.swift`）が true のとき `insertReading`（コアの `insert_reading`）へ送る。条件は、設定 `input.compose_numbers`（既定 true）がオン、レイアウトが `number` / `symbol`、パスワード欄（`hostIsSecure`）でない、日本語入力の流れ（かなのレイアウトから切り替えて来た、または入力中の文字列がある）。英字のレイアウトは従来どおり直接入力。コアは `Literal` として扱うので、Space / 候補バーで候補が出て、何も選ばずに Enter や次のかな入力をすれば半角のまま確定する。Backspace・Space・Enter はかなモードと同じ経路（コアが消費しなければホストへ）。トグルの置き換えはかなモードと同じくコアの backspace を `replaceCount` 回送ってから入れ直す。
- **日本語入力の流れ**（`NumberCompose.nextContext`、`KeyboardModel.japaneseContext`）: かなは true、英字・絵文字・カスタムは false、数字・記号はかなから来たか、数字・記号の間の切替なら引き継ぐ。入力欄の種類による自動選択（`keyboardType` が数字・電話番号など）で数字のレイアウトへ変わったときは false（電話番号欄などで候補を出さない）。
- **パスワード欄**: `hostIsSecure` のときは常に直接入力（学習・候補・コアへの送信を避ける。既存のかな入力のパスワード欄の扱いと同じ方針）。
- **設定の置き場**: ユーザー指示は「core/config の iOS セクション」だったが、`ios.toggle_timeout_ms` に当たるものは実在せず、iOS 固有の設定は `NagiIOSSettings`（App Group の `ios-settings.json`、キー `input.toggle_timeout_ms` など）にある。それに合わせて `input.compose_numbers`（`isSimple: false`）として足し、設定画面は記述子から自動で描く。`config/default.toml` とコアの `Config` は変えていない（コアの変更なし）。
- **迷った判断**: (1) 設定キーの名前は `input.compose_numbers`（`[ios] compose_numbers` ではなく既存の iOS 設定の命名に合わせた）。 (2) 数字・記号レイアウトでの Shift は従来どおり直接入力側でしか使わない（コア経路では無視）。 (3) `custom` モードのレイアウトは対象外。
- **テスト**: `NumberCompose` の判定（`TouchLogicTests`）と設定の既定・型違い（`IOSSettingsTests`）。Swift はローカルでコンパイルできないので CI で確認する。
- **未確認（実機）**: 数字・記号のレイアウトで入力した文字が下線付きの入力中になること、候補バー・Space・Enter・Backspace の挙動、`=` での電卓の答え、未確定のままレイアウトを切り替えたときの確定、トグル（cycle）入力の置き換え、全角記号候補の選択後の見た目。電話番号欄など数字欄の自動選択で直接入力のままであること。

### H-54. Windows: トレイの入力インジケーターに出すため言語バー項目の GUID を `GUID_LBI_INPUTMODE` にし、診断ログ tsf.log を追加（2026-10-23、ユーザー指示、実機未確認）

- **症状**: Windows 実機で Nagi を選んでも、タスクバーのトレイに入力モードのアイコンが出ず、右クリックの設定メニューも出せない。
- **仮説と対応**: Windows 8 以降（特に 11）のシェルの入力インジケーター（トレイの「あ / A」）は、言語バー項目の `guidItem` が `GUID_LBI_INPUTMODE`（{2C77A81E-41CC-4178-A3A7-5F8A987568E6}）のものだけを表示する（Mozc / MS-IME 系の実装も同様。独自 GUID の項目はレガシー言語バーにしか出ない）。`langbar.rs` の `guidItem` を独自の `GUID_NAGI_LANGBAR_BUTTON` から windows クレートの `GUID_LBI_INPUTMODE`（値は一致を確認）に変え、独自 GUID の定数は削除した（H-43 の「トレイの『あ / A』はコンパートメントから OS が描く」という前提は、これで不足と判断）。
- **点検した他の条件**（コード上の確認。いずれも変更なしか小変更）: `dwStyle` = `BTN_BUTTON | SHOWNINTRAY`（Mozc と同じ。右クリックのメニューは `InitMenu`）/ `clsidService` = CLSID / `szDescription` あり / `AddItem` は `ActivateEx`、`RemoveItem` は `Deactivate`（`LangBar` の Drop）/ `ITfSource::AdviseSink` は `ITfLangBarItemSink` のみ受ける / モード変化で `OnUpdate(ICON|TEXT|TOOLTIP)` / `TIPCAP_SYSTRAYSUPPORT`・`IMMERSIVESUPPORT`・`UIELEMENTENABLED` は登録済み / コンパートメント（`OPENCLOSE`・`INPUTMODE_CONVERSION`）はスレッドマネージャーのもので `mode_comp` が読み書き。**変更**: `GetIcon` の大きさをシステム DPI の小アイコン（`GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem())`）にした。
- **Activate とエンジンの独立**: `Core::new` はエンジンを別スレッドで起動するだけで、接続失敗は `Activate` を失敗させない。言語バーの登録（`LangBar::new`）は接続を待たない。`LangBar::new` が失敗しても `Activate` は続く（従来どおり入力はできる）が、握りつぶしていた原因を HRESULT として tsf.log に残すようにした。`Activate` 中の失敗（`ITfKeystrokeMgr` の取得・`AdviseKeyEventSink`・コンパートメント）も同様に記録する。
- **診断ログ**: `%LOCALAPPDATA%\Nagi\logs\tsf.log`。純関数（行の整形・UTC 時刻・ローテーション判断・イベントの文言）は `nagi_windows::diag`、追記だけの I/O は `nagi_windows_tsf::tsf_log`（追記モードで 1 行 1 回の書き込み。256KB 超で `tsf.log.1` へ 1 世代。書けなければ無視）。**入力内容は書かない**: キー・テキスト・preedit は渡さず、`update_preedit` / `update_candidates` は記録しない、`notify` は本文を書かずレベルとコードのみ、サーバーの RPC エラーの本文と不正な応答の本文も書かない。記録するのは DLL の Activate / Deactivate、エンジンの場所（`exe` / `data_dir` の存在）、`EngineEvent`（Started / Crashed / LaunchFailed の種別 = spawn 失敗・initialize タイムアウト・initialize 失敗 / EnteredMinimalMode / Restored / RestoreFailed）、`start()` の失敗（起動直後に通信路が閉じた場合は `LaunchFailed` にならないのでここで拾う）、言語バーの登録・解除の HRESULT。supervisor のロジックは変えていない（`set_on_event` のイベント購読で記録）。エンジン起動失敗の根本原因は別担当が調査中。
- **迷った判断**: (1) 番号は指示の「H-44（次番号を確認）」が既に使われていたため H-54。 (2) ログの時刻は UTC（ローカル時刻の取得に追加の API を使わないため）。 (3) `LOCALAPPDATA` が無ければログなし（`%TEMP%` 等へは逃がさない）。 (4) `GUID_NAGI_LANGBAR_BUTTON` は削除（同じ DLL で二つの項目を出さない。旧 GUID の項目は Deactivate で外れるので残骸は出ない）。
- **未確認（実機）**: トレイに「あ / A」が出ること、左クリックの切り替え、右クリックのメニュー、`dwStyle` で足りるか（出ない場合に `TF_LBI_STYLE_TEXTCOLORICON` などを試す）、複数プロセスからの追記。手順は `docs/real-hardware-checklist.md` §8.5d、`docs/windows-verification.md`。

### H-55. 数式・記号の入力中に候補バーへ電卓・全角・記号の候補を出す（2026-10-23、要実機確認）

- 症状: iOS の数字・記号で `1+2` を打つと下線だけで、候補バーが空のまま（TestFlight 64d0f9b）。
- 原因（コア）: 入力中の候補（composing の suggest）は日本語の読みだけが対象で、Literal だけの preedit は候補が作られなかった。Space / 変換キー（`perform_action` の Space・StartConvert）は、コアのテストでは Literal でも 1 回目から候補ウィンドウが開く（`1+2` → `1+2`・`3`・`１＋２`、`(1+2)*3` → `9`、`1+2=` → 答え）。Swift 側（候補バーの表示・`classifyCandidates`・Space/変換キーの送信）に Literal を弾く処理は見つからなかった。実機で Space 後も出ないなら別原因（下記ログで切り分ける）。
- 修正: preedit 全体が 1 つの Literal の式（数字と記号を含む。`merge_math_pieces` で全角括弧もまとめる）または記号 1 文字のとき、入力中の候補に `literal_candidates`（Rewriter のみ。電卓 → 記号の装飾 → 全角/半角）を出す。辞書・LLM は通さないので軽い。`1`・`123`・`abc` のような数字だけ・英字だけは打つたびに出ると邪魔なので対象外（既存のゴールデンは変えていない）。入力中の候補の選択は学習に通さない。

### H-56. Windows: imserver 起動失敗（5 回失敗 → 辞書のみ）の原因仮説と、stderr 非継承・panic 回避・終了コード記録（2026-10-23、ユーザー指示、実機未確認）

- **症状**: Windows 実機で「imserver の起動に 5 回失敗したので辞書のみで起動します」。5 回が約 3 秒で揃う（バックオフ 200ms から倍々で 5 回 = 即時終了型と整合）。Linux では再現しない。
- **仮説 A（有力）**: DLL が imserver を `stderr(Stdio::inherit())` で起動していた（`process.rs`）。GUI ホストの stderr が無効・壊れたハンドルだと、imserver の最初の `eprintln!`（`main.rs` の起動メモ・プロファイル行）が panic（`failed printing to stderr`）して終了コード 101 で即死する。Linux で `/dev/full` を stderr にすると同じ panic を再現できた（修正前は `core/server/tests/broken_stderr.rs` が失敗、修正後は通る）。なお std は無効ハンドル（`ERROR_INVALID_HANDLE`）の書き込みは成功扱いにするので、実機の症状が出る条件は「有効だが書けないハンドル」（閉じたパイプ等）に限られる可能性があり、**仮説 A が実機の原因かは未確認**。
- **仮説 B**: LLM モデルの読み込み側で落ちる（llama.cpp の DLL/メモリ）。ただし imserver は `async_model_load` でモデル読み込みを別スレッドにしており、読み込み失敗は辞書のみで継続する設計なので、`initialize` の前に落ちる理由にはなりにくい（ネイティブのクラッシュなら別）。切り分けは imserver-<pid>.log・tsf.log の終了コードと PowerShell 手順（`docs/windows-verification.md`）で行う。
- **修正（フロント）**: (1) 子の stderr を**継承しない**。`ProcessEngineConfiguration.stderr_log`（`StderrLog { dir, host_pid }`。TSF 層が `%LOCALAPPDATA%\Nagi\logs` と `std::process::id()` を注入）で `imserver-<ホストpid>.log` へ追記し、開けなければ `Stdio::null()`。起動のたびに `[launch] imserver launch minimal=…` の区切り行。512KB 超なら作り直す。ホストごとに 1 回、`imserver-<数字>.log` だけを対象に 7 日超と 21 件目以降（現在のファイルを含め 20 件）を掃除。純ロジックは `nagi_windows::engine_log`。 (2) `Transport::exit_report()`（既定 `None`）を追加し、`ProcessTransport` が stdout の EOF 後（こちらから終了させた場合を除く）に `try_wait` で終了コードと生存時間を取り、`on_close` の**前**に記録する。 (3) `EngineEvent` に `ProcessExited(FailureRecord)`（`initialize` の前後・minimal か・終了コード・生存時間・起動エラー種別。`Crashed` の直前）、`MinimalSwitchReport { failures }`（minimal 切り替え時に、通常起動での連続失敗の一覧。安定稼働でリセットされるカウンタと同じ寿命）、`Diagnostic(ClientDiagnostic)` を追加。`tsf.log` には `diag::describe_event` が文言にする（終了コードは 10 進と 16 進）。minimal 切り替えの条件（`stable_uptime` でのリセット含む）は変えていない。 (4) JSON として読めない行（stdout への混入）を黙って捨てず、**長さ・先頭バイトの種別（open-brace / alpha / non-ascii など）・累計件数だけ**を `Diagnostic` で通知（1, 2, 4, 8, … 件目のみ。ログを溢れさせない）。内容は記録しない。
- **修正（コア。Phase 5「コア変更ほぼゼロ」の例外）**: `core/server` に `stderr_log`（`elog!` / `write_line`）を追加し、`imserver` の起動経路と `setup.rs` / `pipeline.rs` / `install_data.rs` の `eprintln!` を置き換えた。stderr への書き込み失敗は無視する（`let _ = write…`）。出力される文言・stdout（JSON-RPC 専用）は一切変えておらず、他のフロント（macOS・iOS の `nagi-ffi` は同じ関数を呼ぶが、書き込めるときの出力は同一）の挙動は変わらない。例外とした理由: 原因が imserver 側の `eprintln!` の panic であり、フロントだけでは「壊れた stderr を渡さない」しか防げず、他のホスト（ターミナル起動・サービス・将来のブローカー）でも同じ事故が起きうるため。`nagi-eval` などのツール類の `eprintln!` は対象外。
- **入力内容を stderr に出していないことの確認**: `core/server/src` の stderr 出力（`main.rs` / `setup.rs` / `pipeline.rs` / `install_data.rs`）は、起動メモ（モデル/辞書/設定の読み込み結果・パス・エラー文）、プロファイルとその理由、モデル読み込み完了/失敗、キャリブレーションの所要時間、データ取得の進捗のみで、キー・preedit・確定文字列・周辺テキストは出力しない（`lib.rs` の方針どおり）。したがって stderr ファイルの保存は CLAUDE.md の「ログに入力内容を残さない」に反しない。ただしパス（ユーザー名を含みうる）と設定ファイルのエラー行は含まれうる。
- **迷った判断**: (1) 終了コードの受け渡しは trait の `on_close` を変えず、`Transport::exit_report()` を既定 `None` の追加メソッドにした（既存のモック・実装を変えない）。 (2) `Crashed` / `EnteredMinimalMode` の形は変えず、詳細は新イベントにした（`status.rs`・TSF の既存テストへの影響を避けるため。トーストは出さない）。 (3) EOF 後の終了待ちは最大 1 秒（stdout だけ閉じて生きている子はコード不明のまま）。 (4) ログは tsf.log と同じディレクトリ（ユーザー別の `%LOCALAPPDATA%`。他ユーザーから読めない）。 (5) 統合テスト `broken_stderr.rs` は Linux のみ（`/dev/full`）で、`--no-llm` のため通常の `cargo test` で動く。
- **未確認（実機）**: 実機の原因が仮説 A か（修正後に起動失敗が消えるか）、`imserver-<pid>.log` が作られ追記されること、Windows での終了コード（NTSTATUS の符号付き表記）の取得、`CREATE_NO_WINDOW` と stderr ファイルの併用、掃除。手順は `docs/real-hardware-checklist.md` §8.5e、`docs/windows-verification.md`。

### H-57. iOS: レイアウト切替時の確定を設定にする／入力中の候補バーに英語（ASCII）の候補を出す（2026-10-23、要実機確認）

#### 件 1: 切替時に入力中の文字列を保つ設定（iOS のみ、コアの変更なし）

- **症状（ユーザー実機報告）**: かな → 数字・記号へ切り替えた瞬間に入力中の文字列が確定され、`1+2` のような式が分断される。英字との切替も同様。
- **原因**: `KeyboardModel.changeLayout` が、切替の前に必ず `perform(.commit)` を送っていた（H-53 の「未確定のままレイアウトを切り替えたときの確定」）。
- **設定**（`NagiIOSSettings`、詳細モード、設定画面は記述子から自動）: `input.keep_preedit_on_symbols`（かな ⇄ 数字・記号。既定 **true**）、`input.keep_preedit_on_english`（かな ⇄ 英字。既定 **false**＝従来どおり確定）。
- **判定**（`TouchLogic.swift` の純関数 `LayoutSwitchKeep.shouldKeep` / `EnglishCompose.shouldCompose`。`TouchLogicTests` / `IOSSettingsTests`）: 入力中の文字列があり、パスワード欄でなく、(a) かな・数字・記号の間（かな同士は対象外）は `keep_preedit_on_symbols`、(b) 英字とそれらの間は `keep_preedit_on_english`。数字・記号が関わる切替は、切替先で続けられるよう `input.compose_numbers` がオンのときだけ保つ（オフなら確定）。絵文字・カスタムへの切替は常に確定。
- **切替先での入力**: 数字・記号は H-53 の経路（`hasPreedit` なら compose）でそのまま同じ composing に続く。英字は、`keep_preedit_on_english` がオンで入力中の文字列があるときだけ、英字キー（Shift 適用後の文字）とトグルも `insertReading` でコアへ送る（Literal として入る）。入力中の文字列が無ければ従来どおり `hostInsert`。`japaneseContext` の更新（`NumberCompose.nextContext`）は変えていない。
- **迷った判断**: 英字 → 数字・記号のように英字で入力を続けた後の切替は、数字・記号側の条件（`compose_numbers`）と英字側の設定の両方が通るときに保つ（`hasPreedit` なので数字・記号側はそのまま compose する）。
- **未確認（実機・CI）**: Swift はローカルでコンパイルしていない。切替後の下線の見た目、英字レイアウトでの Shift・Backspace・Space・Enter、トグルの置き換え。

#### 件 2: 「こmぷてr」（computer）が英語の候補にならない

- **原因 1（語彙ファイルの場所・読み込みは問題なし）**: 拡張が読む場所は `<App Group>/data/english/words.fst`（`SharedStore.dataDir()` → `core` の `load_english_lexicon`）で、ホストアプリの取り込み先（`OptionalData.indexURL` = `containerURL/data/<dir>/<index_file>`、`dir`=`english`・`index_file`=`words.fst` は ffi の `nagi_optional_data_sources` が `LEXICON_FILE` から返す）と一致する。語彙はホストの「追加データ」で入れるまで存在せず、未導入だと**特徴量だけの劣化動作**になる。そのとき `computer` は素通し 2 文字（m・r）/ 8 文字で 0.43（Low）となり、**設計上 Mid に届かない**（語彙があれば tier ヒットで 0.9 = High）。実機で効かなかった可能性の大半は「語彙未導入（または導入後にキーボードを開き直していない。コアは起動時に 1 度だけ読む）」。これは設計上の限界で、コードでは直していない（テスト `without_a_lexicon_computer_is_not_judged_english` で固定）。
- **原因 2（コア）**: H-51 の段階 1 は Space の候補窓だけで、入力中の候補バー（composing の suggest）には出ていなかった。さらに入力中の候補は「保留中のローマ字が無い」ことが条件（`candidate_reading`）で、`computer` は末尾の `r` が保留のため、打ち終わりの時点で候補バー自体が出なかった（曖昧入力が無効の既定では、`こmぷて` でも日本語の読みとして作れない）。
- **修正**（`core/engine`、macOS・Windows の入力中サジェストにも同じく効く）: 英語らしい打鍵列の候補を入力中の候補に足す。(1) `english_typing`: 実際の打鍵列 + 保留（小文字の ASCII 3 文字以上）を打ち直して現在の入力と一致するときだけ対象（かな直接・フリック・数字・Shift 入力は対象外）。(2) 日本語の読みとして候補を作れず、ローマ字にならなかった英字を含む入力（`english_only_reading`）は、打った通りの文字列を読みとして候補を作る。キー入力の経路（`composing_candidate_reading`）では判定せず、判定はデバウンス後のバックグラウンド（`compute_items_for`）で行う。(3) 判定（`english_level`。日本語として引ける読みは除かれる）が **High なら 2 番目**、**Mid なら主要な候補の後ろ（ひらがな・カタカナの前。ただし先頭にはしない）**に打鍵列の ASCII を足す。Low 以下は足さない。英語と判定されなければ、日本語の読みを作れない入力は従来どおり候補なし。先頭は動かさない。(4) その候補を選ぶと ASCII を確定する。読みにローマ字にならなかった英字を含む候補は学習に通さない（`こmぷてr` → computer を学習しない。曖昧入力の補正候補は従来どおり補正後の読みで学習）。
- **ゴールデン／既存テストの変更**: `engine/tests/touch.rs` の `literal_expression_shows_rewriter_candidates_while_composing` は、英字だけ `abc`（→ あbc）を「候補の読みなし」としていたが、`abc` は英語らしい（語彙なしの特徴量でも High）ので、`composing_candidate_reading` が `Some` になり、候補は `あbc`・`abc` になる、に変えた。状態機械のゴールデン（YAML）は変えていない。
- **テスト**: `server/tests/composing_english.rs`（iOS と同じ呼び出し = `insert_reading` の `text` を 1 文字ずつ。語彙は `core/english/tests/fixtures/words.txt`）。`computer` で 1 番目 `こmぷてr`・2 番目 `computer`、選択で `computer` を確定、`kyouhagoogle...` など日本語の入力の全接頭辞で ASCII 候補が出ない、語彙なしでは出ない。
- **誤判定の見込み**: 判定は H-51 と同じ（誤判定 0 を優先した閾値）。入力途中の状態（接頭辞）は評価コーパスで測っていない（日本語の接頭辞で ASCII が出ないことを上のテストの数件で確認したのみ）。`comp` のように語彙にない子音連続の途中は Mid になりうる（候補の後ろに出るだけで、先頭は動かさない）。
- **未確認（実機）**: 語彙導入後の iOS で `computer` の候補バー表示、導入直後に拡張が語彙を読むタイミング（キーボードを開き直す必要）、入力途中の見た目のちらつき。

### H-58. 入力中の候補（サジェスト）を、読みの接頭辞の候補まで拡充する（iOS の候補バー。2026-10-23、ユーザー要望、要実機確認）

- **要望**: 候補の数が文章全体で固定の少ない数（`COMPOSING_CANDIDATES_LIMIT` = 12）で、普通のキーボードアプリのようには出ない。読み全体を基準に予測したあと、読みの末尾を 1 文字ずつ削った接頭辞で部分予測を繰り返し、最後は 1 文字の読みの漢字まで候補に出す。候補バーはスクロールでよい。
- **並び**（`Engine::append_prefix_items`）: (1) 読み全体の候補（従来どおり。文全体の最良・先頭文節の候補・曖昧入力・ひらがな・カタカナ。英語・原語はその中）、(2) 接頭辞を長い順（len-1 … 2）に、各接頭辞の候補、(3) 末尾は 1 文字の読みの候補（漢字）。同じ (表記, `covers`) は 1 度だけ。ひらがな・カタカナだけの形式の候補は接頭辞では出さない（読み全体のものだけ）。接頭辞 1 つあたりの件数は長さで変える（2 文字以下 6 件・3〜4 文字 4 件・5 文字以上 2 件。辞書が長い接頭辞で文の途中の切り方違いの似た候補を並べるため）。1 文字の読みは 20 件。
- **上限**: 総数は設定 `convert.suggest_max`（既定 **120**、12〜500、`config/default.toml`）。途中の接頭辞で埋まっても 1 文字の候補が押し出されないよう、先に枠を取る。長さに応じて増える（実辞書で 9 文字 48 件・19 文字 68 件・36 文字 64 件。従来は最大 12 件）。設定 GUI の記述子（`settings.rs`）には足していない（`config.toml` のみ）。
- **軽さ**: 接頭辞の候補は新しいバックエンド境界 `ConversionBackend::prefix_candidates`（既定は `segment_candidates`）で引く。`Pipeline` は LLM 推論と曖昧入力の補正を使わず、学習・ユーザー辞書・システム辞書・Rewriter のみ（`segment_candidates_inner(light)`）。再計算は従来どおりデバウンス後のバックグラウンド（`refresh_composing_candidates`）で、同期の応答（`insert_reading` / `process_key`）には載らない。キー入力 → ひらがな反映の経路は変えていない（`composing_candidate_reading` は読みの判定だけ）。
- **能力の宣言**: `initialize` の `capabilities.partial_suggestions`（`Option<bool>`、加算的）が `true` のときだけ接頭辞の候補を出す（`Engine::set_partial_suggestions`。設定の再読み込みでは変わらない）。省略・`false`（macOS・Windows）は従来どおり。iOS（`NagiSession` の `makeClientCapabilities`）だけが宣言する。Swift の型は `cargo xtask gen-types` で更新。
- **部分候補の選択**: 新しい項目は足さず、既存の `CandidateView.covers`（先頭から何文字の読みを置き換えるか。Unicode スカラー数。ひらがなは UTF-16 でも同じ）をそのまま使う。部分変換（H-14）の機構（`select_composing`）が、その接頭辞の surface だけを確定し、残りの読みを入力中のまま残す。学習（その接頭辞の読み → surface）・`last_commit`（部分確定では取消の対象外）・`revision` の扱いは H-14 のまま。選ぶと `maybe_schedule_composing_candidates` が残りの読みの候補を作り直す。iOS は `selectCandidate` が既にコアの `select_candidate` を呼ぶので、その経路のまま（Swift の変更は能力の宣言と、候補バーの `LazyHStack`・入れ替え時に先頭へ戻す `id` のみ）。
- **要望との差**: 「消費する読みの長さ」を UTF-16 の別項目で足す案だったが、`covers` が同じ意味で既にあり（macOS / Windows は `covers` を無視して従来どおり描画、`select_candidate` はコアが部分確定する）、重複を避けて再利用した。macOS / Windows に接頭辞の候補を出さないのは、候補ウィンドウに 100 件以上が並ぶのを避けるためと、宣言のないフロントの挙動を変えないため。
- **ゴールデンへの影響**: なし（YAML・既存テストの期待値は変更なし）。
- **既存テストの修正（H-57 の影響）**: `server/tests/fuzzy.rs` の `typo_input_without_fuzzy_has_no_composing_candidates` は、H-57 で `kamji`（英語らしい打鍵列）の読みが候補の対象になったためこの変更の前から失敗していた。曖昧入力が無効なら補正の候補（感じ・出所 Fuzzy）が出ないこと、に主張を直した。
- **速度**（`server/tests/suggest_latency.rs`、release、実辞書、LLM なし、7 回の中央値）: 9 文字 8.0 → 8.2 ms（10 → 48 件）、19 文字 8.0 → 8.9 ms（10 → 68 件）、36 文字 7.7 → 9.9 ms（4 → 64 件）。接頭辞の分の増加は 0.2〜2 ms 程度で、元の再計算（LLM ありなら LLM の分が加わる）に比べて小さい。数字は `cargo test --release -p nagi-server --test suggest_latency -- --nocapture` で出る（辞書が無ければ省略）。
- **テスト**: `engine/tests/prefix_suggest.rs`（全体 → 接頭辞 → 1 文字の順・長さで増える・宣言なしでは出ない・上限と 1 文字の枠・接頭辞の選択で残りが入力中に残る・全体の選択は全部確定・1 文字の読みは接頭辞なし）、`server/tests/suggest_prefix.rs`（JSON-RPC で宣言あり / なし / false、`select_candidate` で `今日` が確定して `はいいてんき` が残る、`insert_reading` の同期応答に載らない）、`config`（`suggest_max` の既定と範囲）、iOS の XCTest（`candidateBarIdentity`・`makeClientCapabilities`・covers つきの振り分け）。
- **未確認（実機）**: iOS の候補バーで 100 件前後を `LazyHStack` で並べたときのスクロールの滑らかさ・メモリ（拡張の上限 H-6）、入れ替え時に先頭へ戻す挙動、接頭辞の候補を選んだ直後の下線（残りの読み）の見た目、LLM ありのときの総所要時間。Swift はローカルでコンパイルしていない（CI で確認）。長い接頭辞の候補は辞書の切り方違いの似た候補が残る（品質の調整は実機の感触で）。

### H-59. カタカナ語の原語（`computer`）を入力中の候補バーに確実に出す（2026-10-23、ユーザー要望、要実機確認）

- **要望**: `こんぴゅーたー` を打ったとき、候補バーに `コンピューター`、`computer` が出るように。H-52 の原語候補は Space / 変換の候補窓（`segment_candidates` の Rewriter 群）には出るが、入力中の候補バーでは「先頭文節の候補を 12 件で打ち切る」過程に依存していて、辞書の候補が多い語や複数の文節に割れた読みでは押し出される・そもそも引かれないことがあった。
- **修正**: 新しいバックエンド境界 `ConversionBackend::loanword_candidates(surfaces, reading)`（既定は空）を足し、入力中の候補（`compute_items_for`）で、**読み全体を置き換える候補**のうちカタカナだけの表記と、読みをカタカナにしたものの原語を引いて、読み全体のカタカナの候補の**直後**（無ければ読み全体の候補の末尾）に出所 `Rewriter` で足す（`Engine::insert_loanword_items`）。すでにある表記は重ねない。`Pipeline` は H-52 と同じ `LoanwordRewriter::candidates`（和製英語は出さない・多義は最大 3 件・ゆるい一致は骨格 6 文字以上）を使い、`convert.loanword.enabled` が false・データが無ければ空。上限（12 件）の打ち切りの後に足すので押し出されない。件 A の接頭辞の候補（`prefix_candidates` → Rewriter 群）にも同じ索引が効く（`コンピュータ` の接頭辞の下にも `computer` が出る。実データで確認）。
- **ゲートなし**: 原語は macOS / Windows の入力中の候補にも出る（`capabilities.partial_suggestions` には依存しない。全体を置き換える候補で、最大 3 件）。
- **データの場所**: iOS の拡張が読むのは `<App Group>/data/loanword/loanword.fst`（コアの `load_loanword_index`）で、ホストアプリの取り込み先（`OptionalData.indexURL` = `dataDir/<dir>/<index_file>`、`dir` = `loanword`・`index_file` = `loanword.fst` は ffi の `nagi_optional_data_sources` が `LOANWORD_INDEX_FILE` から返す）と一致することをコードで確認した。コアは起動時に 1 度だけ読むので、導入直後はキーボードを開き直す必要がある（英語の語彙と同じ。H-57）。データが無ければ原語が出ないだけで壊れない（テスト `without_loanword_data_*`）。
- **テスト**: `engine/tests/prefix_suggest.rs`（Mock の原語表で `コンピューター` の直後に `computer`・データ無しなら出ない・`computer` を選ぶと ASCII を確定）、`server/tests/suggest_prefix.rs`（`IndexBuilder` で作った自作の小さな索引を `Pipeline::set_loanword_index` で渡し、`insert_reading` の 1 文字ずつ入力で候補バーに出る / 索引なしでは出ない）。実データ（`data/loanword/loanword.fst`、H-52 の現行版）で `こんぴゅーたー` → `コンピューター` `computer` `こんぴゅーたー` …を `suggest_latency` の出力で確認。
- **未確認（実機）**: iOS で原語データを導入したあとの候補バー表示、導入直後に拡張が索引を読むタイミング。学習: `computer` を選ぶと読み → `computer` が学習される（H-48 の形式候補と同じ扱い。除外するかは要判断）。

### H-60. 記号の読みの拡充と、入力中の候補バーへの記号の表示（2026-10-23、ユーザー要望、要実機確認）

- **要望**: 記号を読みから変換できる内容を増やす。`かっこ` と打った時点（Space の前の入力中の候補バー）で、装飾された括弧（`「」` `『』` `【】` `〔〕` `〈〉` `《》` `（）` `［］` `｛｝` `＜＞` など）も出す。
- **読みの表**（`core/rewriter/src/symbols.rs` の `READING_GROUPS`）: 1 行 = 1 グループ（同じ候補を引く読みの別名）にし、**367 グループ・626 読み**（従来は約 58）。括弧（かっこ／かぎかっこ／にじゅうかぎかっこ／まるかっこ／かくかっこ／すみつきかっこ／きっこうかっこ／なみかっこ／やまかっこ／おおかっこ／つめかっこ、開き・閉じ＝`かっこひらき`・`かっことじ` など）、約物・句読点（てん／てんてんてん／ちょうおん／なみせん／だっしゅ／ぎもんふ／かんたんふ ほか）、図形（まる／しろまる／くろまる／さんかく／しかく／ひしがた／ほし ほか）、矢印（やじるし／みぎ／ひだり／うえ／した／ななめ／みぎうえ ほか）、数学（かける／わる／たす／ひく／いこーる／ふとうごう／むげん／るーと／しぐま／せきぶん ほか）、ギリシャ文字（主要なもの）、単位・通貨（ど／ぱーせんと／ぱーみる／ぷらいむ／えん／ぽんど／どる／ゆーろ／めーとる／きろ／せんち／きろぐらむ ほか、`㎏㎞㎝㎡℃％‰`）、丸数字（まるいち〜まるにじゅう、くろまるいち〜じゅう、かっこいち〜じゅう）、ローマ数字（ろーますうじ、ろーまいち〜じゅうに）、囲み文字・元号、よく使う記号（あっと／しゃーぷ／あんど／すらっしゅ／ばっくすらっしゅ／あすたりすく／ちるだ／ぱいぷ／あんだーばー ほか）、天気・星座・キー記号・罫線など。重複検査と「どの読みでも候補が出る」検査をテストで固定（`readings_are_unique_and_every_reading_has_candidates`）。
- **出典・方針**: 表は**自作**（Unicode の文字名と日本語の一般的な呼び方）。Mozc の `symbol.tsv`（BSD-3-Clause、`raw.githubusercontent.com/google/mozc/master/src/data/symbol/symbol.tsv`。この環境で取得でき、2116 行・SHA-256 `bc68b032a195f203dccba6f95667424199ba9b185fee2be64eeaa959253c097a`）は**読みの取りこぼしを調べる参考にしただけ**で、データは取り込んでも同梱してもいない。取得して取り込む方式（`nagi-datafetch`）にしなかった理由: iOS の拡張はホストアプリの取り込み（Swift 側の追加）が無いとデータを読めず、今回の要望（iOS 実機）に届かないため。将来、全 2000 行規模へ広げるなら datafetch + ビルドの方式が候補（その場合は BSD の帰属表示が必要）。
- **入力中の候補バー**（`core/engine/src/engine.rs` の `insert_symbol_items`）: 新しいバックエンド境界 `ConversionBackend::symbol_candidates(reading)`（既定は空。`Pipeline` は Rewriter チェーンの `SymbolRewriter` だけを引く）。読み全体が記号の読みに一致するとき、記号を**読み全体の最良の変換（先頭の読み全体の候補）の直後**に最大 16 件挟む（辞書の通常候補 `括弧`・`格好` は先頭か、記号の後ろに残る。すでに同じ表記があれば前へ動かして出所に Rewriter を併記）。出所は Rewriter。接頭辞（`partial_suggestions` を宣言した iOS のみ）は、各接頭辞（長さ 2 以上）の辞書候補の先頭の次に記号を**最大 3 件**挟む（`かっこいい` の接頭辞 `かっこ` → `「」` `『』` `（）`）。1 文字の読みの接頭辞には出さない。再計算は従来どおりデバウンス後のバックグラウンドで、同期の応答（`insert_reading` / `process_key`）には載らない（テストで確認）。macOS・Windows でも読み全体の記号は候補に出る（原語と同じくゲートなし）。
- **既存の挙動**: ユーザーが全角の `（` などを打った場合（Literal／記号 1 文字）の入力中の候補（H-55）は変更なし。Space で開く候補窓（`segment_candidates`）の Rewriter 候補は従来どおり末尾（表が増えた分だけ記号が増える）。ゴールデン YAML・既存テストの期待値は変更なし。
- **迷った判断**: (1) 位置は「最良の変換の直後」。`みぎ`・`うえ`・`ほし` など普通の語でもある読みでは、記号が 2 番目に出る（雑音になるなら、記号だけの読みと普通の語の読みで位置を分ける案がある）。(2) ユーザーが挙げた `ながさ` は 長さ という普通の語のため登録せず、長音は `ちょうおん`・`のばしぼう` で引く。(3) `かつ`・`または`・`ならば`・`すべて`・`かい` など普通の語と区別が付かない数学・ギリシャ文字の読みは入れていない。(4) 非 BMP の絵文字は記号の表に入れず、絵文字は従来の `EmojiRewriter` の担当にした（設定で止められるため）。
- **テスト**: `rewriter` の表の検査と代表の読み、`engine/tests/prefix_suggest.rs`（Mock: `かっ` では出ず `かっこ` で 2 番目から、接頭辞は 3 件・宣言なしでは出ない）、`server/tests/suggest_prefix.rs`（`insert_reading` の 1 文字ずつ入力で `かっこ` の各括弧、辞書の `括弧`・`格好`・`カッコいい`・`恰好` が残る、同期の応答に載らない、接頭辞の `「」` を選ぶと `いい` が残る、`（` の候補は維持）。
- **未確認（実機）**: iOS の候補バーでの見え方・件数の感触、普通の語の読みで記号が 2 番目に出る煩わしさ。LLM ありの実バックエンドでの順序（Mock と辞書のみのテスト）。

### H-61. 最下段のフリックの案内を view の枠内に収める（iOS。2026-10-24、ユーザー要望、要実機確認）

- **症状**: 最下段のキーを下へフリックすると、下方向のセル（例: 「`」の下の文字）が、キーボードの view の下端（その下のシステムのフッター = 地球儀・マイクの帯）で切れて見えない。
- **原因**: キーボード拡張の view は拡張の外（システムのフッター）に描けない。案内のセルはキーの下に 1 キー分（高さ + 2pt）出すが、最下段のキーの下の余白は 6pt しかなく、残りが view の外へ出ていた。左端・右端のキーの左右のセルも同様に外へ出うる。
- **対策**（`Keyboard/TouchLogic.swift` の `PopupClamp`、`Keyboard/NagiKeyboardView.swift`）: 案内のセル全体（中央 + 出す方向）の矩形が view の範囲からはみ出す分だけ、全体を内側へ動かす。セル同士の相対位置は変えないので重ならず、選択の判定（指の移動方向）にも影響しない。最下段では、下のセルがキー自身の位置に、中央のセルが 1 段上の行に出る。view の範囲は、ルートの座標空間（`NagiKeyboardSpace`）とルートの大きさ（環境値 `nagiKeyboardSize`）から、キーの座標系で求める。大きさが未測定（0）の間は動かさない。
- **採らなかった案**: 最下段の下に余白を足して枠内で描く案（キーボード全体が 1 キー分高くなり、全ユーザーの見た目が変わる）。
- **背景**: `Color(UIColor.systemGray6)` の塗りをやめ、システムのキーボードの背景（フッターの帯と同じ素材）をそのまま見せる。候補バーの帯・キーの塗りは従来どおり。
- **テスト**: `FeedbackAndPopupLogicTests`（はみ出す方向ごとの移動量・枠より大きい場合・未測定・最下段/左端/中央のキー）。
- **未確認（実機）**: 最下段のキーのフリックの見え方（中央のセルが上の行に出る配置が自然か）、背景を塗らないことでライト/ダーク・テーマ上書き時（`overrideUserInterfaceStyle`）にフッターと色が揃うか。

### H-62. 候補バーに、入力中でなく候補が無いときのツールバーを出す（iOS。2026-10-24、ユーザー要望、要実機確認）

- **要望**: 入力中でない・候補が無いとき、候補バーの場所にツールバー（キーボードを閉じる・貼り付けほか）を出す。項目と並び順を設定でき、後から項目を足せる構造にする。
- **構造**: `Shared/ToolbarAction.swift`（純粋。`ToolbarAction` enum = 項目の名前・表示名・SF Symbols、`ToolbarLogic` = 保存名からの復元・出す判定・貼り付けの可視判定）。実行は `KeyboardModel.perform(toolbar:)` が既存のキー操作・機能（`perform(action:)`・`arrow`・`pasteFromClipboard`・`deleteWordBackward`・`switchLayout`・`nextKeyboard`）を呼ぶだけ。項目を足すには enum に case を足し、`perform(toolbar:)` に 1 行足す。
- **項目**: キーボードを閉じる / 貼り付け / カーソルを左・右へ / 改行 / 半角スペース / 単語を削除 / 絵文字（絵文字のレイアウトへ）/ 次のキーボード（地球儀）。
- **設定** `toolbar.items`（詳細モード。既定 `[dismiss, paste, cursorLeft, cursorRight]`）: 設定ファイル（`ios-settings.json`）に文字列の配列で持つ。新しい設定の型 `.list`（選択肢の部分集合を順序つきで）を `NagiIOSSettingDef.Kind` に足し、設定画面は並べ替え・削除・追加の編集画面（`NagiListSettingEditor`）に入る行で出す。知らない名前・重複は読むときに捨て、型違いは既定、空配列は「何も出さない」という明示の指定。既存の `candbar.paste_button` は「ツールバーに貼り付けを出す」の意味でそのまま残し、オフなら項目に入れていても出さない。
- **出す条件**: 入力中でなく（`hasPreedit` が偽）、候補が無く、メッセージ（辞書が無いなど）も無いとき。確定直後の次語予測は候補なので、出ている間は候補バーになる（従来は貼り付けボタンが次語予測の左に並んでいた。ここは変わる）。バーの高さは 44pt のまま。パスワード欄での貼り付けの扱いは従来と同じ（表示判定は `hasStrings` のみ、中身はタップ時だけ読む。フルアクセスが無い・許可されなければ入らず、ボタンも隠れる）。
- **未対応（迷った判断）**: 取り消し（undo）/ やり直し・全選択・設定を開く（ホストアプリへ）は入れていない。`UITextDocumentProxy` に undo・選択の API が無く（キーボード拡張からは選択範囲を作れない）、拡張からホストアプリを開く正規の API も無い（レスポンダ連鎖で `UIApplication` を探す非公式の手は審査・将来の OS で壊れうる）ため。API で実現できる範囲だけに絞った。
- **テスト**: `Tests/ToolbarActionTests.swift`（既定の項目・保存/読込で並びを保つ・不正値・空配列・出す判定・貼り付けの可視）。
- **未確認（実機）**: ツールバーのボタンの見た目・幅（5 個以上並べたとき）、次語予測が出るたびにツールバーが候補バーに入れ替わる体感、絵文字のレイアウトへの切替。
