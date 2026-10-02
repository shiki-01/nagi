# Nagi 企画・設計書

- プロジェクト名: `Nagi`（コアは `nagi-core`、各フロントは `nagi-macos` / `nagi-windows`）
- 版: v0.2 (draft) — 2026-09-27 改訂。決定の経緯・理由は `decisions.md` を参照
- 対象プラットフォーム: macOS(先行) / Windows(後続) / Linux(将来検討)

### v0.2 の主な変更点

- 基本方針に「品質優先」「環境適応」「気配り」を明記（§2.1）
- 既存資産の利用方針を追加（§2.3）
- IR に reading/surface のアライメントと文節粒度を定義（§4.1, §4.2）
- 文全体変換 + 文節変換を Phase 1 に含め、範囲操作のキー割当を一般的なIMEに合わせて再定義（§4.4, §4.10）
- プロトコルを「同期応答で差分を返す」方式に再設計（§3.2）
- 環境プロファイル（ハードウェアに応じた精度/軽さの自動調整）を追加（§4.7）
- 辞書のみで動く `minimal` 経路を Phase 1 に前倒し（モデル無しでも常に動く）
- 配布・アップデート方式を追加（§5.2, §8）。当初は Apple Developer Program 非加入前提だったが、2026-10-02 に加入済み前提へ変更（`decisions.md` I 節）

---

## 1. 概要

ローカルLLM（GPT-2級の小型モデル）によるニューラルかな漢字変換を中核とする日本語IMEを、
**コアエンジン（プラットフォーム非依存）** と **フロントエンド（OS組み込み層）** に分離して開発する。

先行事例の位置づけ:

| プロジェクト | ライセンス | 参考にする点 |
| --- | --- | --- |
| karukan (togatoga) | MIT OR Apache-2.0（Mozc由来データは BSD-3） | コア/フロント分離、JSON-RPC over stdio、チャンク式ライブ変換、adaptive戦略、学習キャッシュ設計 |
| azooKey / Zenzai | MIT（モデル zenz 系は CC-BY-SA-4.0） | 投機的デコーディング、いい感じ変換（スタイル変換）、プロファイル注入 |
| rakukan | 要確認 | Windows TSF層の構成、literal保護、設定ホットリロード |
| riq0h fork | 要確認 | 部分変換（Shift+矢印）、ローマ字再結合条件の緩和 |
| Mozc | BSD-3 | 辞書・連接コスト、キーマップの粒度、Rewriter、文節区切り操作の挙動 |

本プロジェクトの差別化点は「**範囲操作を第一級市民としたIR設計**」と
「**範囲×LLMアクション（スタイル変換）のプラグイン化**」の2点。
既存実装はいずれも範囲操作が後付け（またはフォークのみ）であり、ここを最初から設計に含める。

## 2. 方針・目標 / 非目標

### 2.1 基本方針

1. **品質優先**: 開発コストがかかっても良いものを作る。暫定実装で済ませる箇所は、後から置き換えられる境界（trait・プロトコル）を先に切っておく
2. **環境適応**: 同じ Nagi が、高性能機では精度重視に、非力な機械・バッテリー駆動時には軽量に動く。ユーザーが何も設定しなくても妥当な状態で動き、望めば細かく調整できる（§4.7）
3. **気配り**: 入力を失わない、キー入力を待たせない、標準IMEと同じ操作が同じ結果になる、状態が常に見える、を全体の品質基準とする（§4.14 に一覧）
4. **オープン**: コードは OSS（`MIT OR Apache-2.0`）。外部資産は無料・再配布制約の小さいものを優先し、同梱せず別途取得する形でコードのライセンスから切り離す

### 2.2 目標

1. macOS / Windows の両方で同一コアが動作する
2. ライブ変換（Space不要のリアルタイム変換）
3. Surrounding Text + プロファイルを加味した文脈推論
4. 変換中・確定後テキストの**範囲指定**と、範囲に対するアクション実行（再変換 / 部分変換 / スタイル変換）
5. 学習の自動登録（確定候補・選択順位・文脈を記録）と誤学習の削除UI
6. ユーザー辞書（Mozc TSV互換）+ スニペット展開
7. キーバインドの完全カスタム（TOML定義、コア側で解決）
8. 実用レイテンシ（§7）と、ハードウェアに応じた精度/軽さの自動調整（§4.7）
9. モデルが無くても辞書だけで常に日本語入力できること（`minimal` 経路）

### 2.3 既存資産の利用方針

「全部自作」と「既存IMEをベースに改造」の中間を取る。**コード（ロジック）は自作、データ資産は既存のものを利用、アルゴリズムは既存実装を読んで参考にする**。

| 対象 | 方針 | 理由 |
| --- | --- | --- |
| 状態機械 / IR / パイプライン / プロトコル | 自作 | 差別化点そのもの。既存実装の IR はセグメント構造を前提にしておらず、改造は作り直しと同等になる |
| ローマ字変換・Rewriter・キーマップ解決 | 自作（Mozc・karukan の挙動を仕様として参照） | 小さく、挙動を細部まで制御したい部分 |
| システム辞書・連接コスト | 既存データを利用（Mozc OSS / SudachiDict 等を Phase 0 で比較） | 語彙とコストの品質は個人で再現できない |
| 変換モデル | 既存モデルを利用（zenz 系等を Phase 0 で比較）。将来は自前 fine-tune も検討 | 学習コストが大きい。同梱せず別途ダウンロードにしてライセンスを分離 |
| llama.cpp | 既存バインディング crate を利用し trait で隠蔽 | 推論ランタイムの自作は品質に寄与しない |
| macOS / Windows フロントの定型部 | 自作（karukan / azooKey-Desktop / rakukan を精読して罠を回避） | 薄いため自作コストは小さい。OSの罠の知見は流用する |

- MIT / Apache-2.0 / BSD-3 のコードを部分的に移植する場合は、ファイル先頭と `THIRD_PARTY_LICENSES` に出典とライセンスを記載する
- Mozc 本体（C++・Bazel）や azooKey の変換器（Swift）を丸ごと組み込むことはしない（言語・ビルド系が Rust コア + Windows 展開と合わないため）

### 2.4 非目標（当面やらない）

- かな入力レイアウト（ローマ字入力のみ）
- 手書き・音声入力
- クラウド変換（すべてローカル完結。プライバシー方針でもある）
- iOS / Android の実装（v1 では対象外。ただし将来の対応を前提とし、コアは `mobile-design.md` §3 の制約を守って開発する）
- 候補ウィンドウのマウス操作（v1では非対応で可）

## 3. 全体アーキテクチャ

```
┌────────────────────────────┐                     ┌──────────────────────────────┐
│ Frontend (OS組み込み層)      │   JSON-RPC 2.0      │ Core Engine (Rust)            │
│                            │   (改行区切り,       │                              │
│ macOS: IMKit + Swift       │    stdin/stdout)    │ ・状態機械 / IR / アライメント │
│ Windows: TSF + Rust/C++    │ ◀─────────────────▶ │ ・ローマ字変換                │
│                            │                     │ ・候補生成パイプライン         │
│ 責務:                       │                     │ ・llama.cpp推論 (GGUF)       │
│ ・OSキーイベント→keysym変換  │                     │ ・辞書 / 学習 / Rewriter      │
│ ・preedit/候補ウィンドウ描画 │                     │ ・キーバインド解決             │
│ ・範囲ハイライト描画          │                     │ ・環境プロファイル             │
│ ・子プロセス管理/自動再起動  │                     │ ・スタイル変換プラグイン       │
│ ・アップデート確認           │                     │                              │
└────────────────────────────┘                     └──────────────────────────────┘
```

### 3.1 分離の原則

- **コアに置くもの**: 状態機械、キーバインド解決、変換・学習・辞書・環境適応のすべてのロジック
- **フロントに置くもの**: OSイベントの正規化、描画、プロセスライフサイクル管理、アップデート**のみ**
- フロントは「キーイベントを送り、描画差分を受け取る」だけのthin clientとする。
  これによりカスタムキーバインド・範囲操作・スタイル変換が全OSで一度に実装される

### 3.2 プロトコル

- JSON-RPC 2.0 / 改行区切り / stdin-stdoutパイプ
- 正のスキーマは Rust 側（`core/protocol/`）。Swift の型は `typeshare` で生成してコミットし、CI で再生成差分がないことを検査する
- 文字位置（cursor / selection / 属性範囲）は **UTF-16 コード単位**。コア内部表現との変換はコアが行う

#### 同期・非同期の原則

- `process_key` は同期リクエスト。応答で描画差分をまとめて返す（順序と原子性を保証）:
  `{consumed, commit?, preedit?, candidates?, mode?, revision}`
- 非同期通知（C→F）はライブ変換の推論結果・スタイル変換結果・設定エラー通知に限る。
  通知は `revision` を持ち、フロントは現在より古い revision の通知を破棄する
- 推論待ちは `sync_budget_ms`（既定150）まで同期応答内で待ち、超過時はひらがなで応答して後から通知する。
  **キー入力をブロックする処理はこの予算内に限る**

#### メソッド

| method | 種別 | 内容 |
| --- | --- | --- |
| `initialize` | F→C 要求 | プロトコルバージョン・capability・OS情報の交換 |
| `process_key` | F→C 要求 | `{session_id, keysym, keycode, text, modifiers, kind: down\|flags_changed}` |
| `focus_in` / `focus_out` | F→C 要求 | セッションのフォーカス変化。`focus_out` 時は表示中の surface を確定して返す |
| `set_context` | F→C 通知 | Surrounding Text、アプリ識別子、secure input フラグ |
| `select_candidate` | F→C 要求 | 候補ウィンドウ外からの選択（将来のマウス対応用に予約） |
| `reconvert` | F→C 要求 | 確定済みテキストの再変換要求（OS側の再変換APIから） |
| `restore_session` | F→C 要求 | エンジン再起動後、フロントが保持していた preedit を復元（§3.3） |
| `reset` | F→C 要求 | 未確定状態の破棄 |
| `set_environment` | F→C 通知 | 電源（バッテリー・低電力モード）・サーマル・メモリ逼迫の報告。コアが環境プロファイルを自動で切り替える（§4.7）。省略した項目は前回の値を維持 |
| `get_status` | F→C 要求 | 現在のプロファイルとその理由・LLM/学習の有効状態・学習件数・ユーザー辞書語数・設定ファイルパス・設定エラーの有無（メニュー表示用） |
| `clear_learning` | F→C 要求 | 学習データの全削除（プライバシー要件: ワンコマンドで全削除） |
| `reload_config` | F→C 要求 | 設定ホットリロード |
| `get_settings` | F→C 要求 | GUI 設定画面向けの設定項目の記述子（キー・グループ・表示名・説明・型と範囲・現在値・既定値・明示指定の有無・再起動要否）を返す（スキーマ駆動。`SettingsSnapshot`） |
| `update_settings` | F→C 要求 | `{changes: [{key, value}]}`。コアが型・範囲・選択肢を検証し、通ったものだけ `config.toml` にコメントを保って原子的に書き込み、ホットリロードで反映する。`value` が `null` なら明示指定を削除して既定に戻す。結果は `{applied, errors, snapshot}`（不正な項目だけ `errors` に入り、他は適用される） |
| `ping` / `shutdown` | F→C 要求 | 死活監視 / 終了 |
| `update_preedit` / `update_candidates` | C→F 通知 | 非同期推論結果の反映（revision付き） |
| `notify` | C→F 通知 | 設定エラー・モデルDL進捗・警告等のユーザー向け通知 |

- キーイベントは押下と修飾キー変化のみ（離上は送らない）。右⌘単独タップ等は `flags_changed` 列からコアが判定
- `keysym` は XKB keysym 数値。`text` は OS の配列が生成した文字（Dvorak 等の対応用）。JIS 英数/かなキーは XKB の `Eisu_toggle` / `Hiragana_Katakana` 相当
- エラーは JSON-RPC 標準コード + `-32000` 台のアプリ固有コード

### 3.3 プロセス管理

- エンジンはフロントの子プロセス（フロント1プロセスにつき1つ）。クライアントごとに `session_id` を持つ
- クラッシュ時は指数バックオフで自動再起動。スリープ復帰時も再起動（macOSがスリープ中にパイプを破棄する事象への対処）
- 応答が 500ms 無い場合はエンジン異常とみなす。Direct 状態ならキーは素通し、それ以外は消費する
- **入力を失わない**: フロントは直近の preedit（reading 列 + セグメント境界）を保持し、再起動後に `restore_session` で復元する。復元に失敗した場合のみ、表示中の文字列をそのまま確定する
- 学習キャッシュはファイルから復元。それ以外の揮発状態は捨てて良い設計にする
- 再起動回数が閾値を超えたら通知し、`minimal` プロファイルで起動する（モデル起因のクラッシュループ回避）

### 3.4 リポジトリ構成

Phase 1 は必要な crate のみで始め、責務が育ったら分割する。

```
nagi/
├── core/                 # Rust workspace
│   ├── engine/           # 状態機械・IR・アライメント・パイプライン・環境プロファイル  [Phase 1]
│   ├── romaji/           # ローマ字変換（テーブル外部定義）                          [Phase 1]
│   ├── dict/             # 辞書・連接コスト・Viterbi・ユーザー辞書                     [Phase 1]
│   ├── infer/            # llama.cpp binding（trait越し）・モデルアダプタ              [Phase 1]
│   ├── protocol/         # JSON-RPCスキーマ（正）                                      [Phase 1]
│   ├── server/           # imserverバイナリ                                          [Phase 1]
│   ├── learn/ rewriter/ keymap/ style/   # 各Phaseで engine から分離
│   └── xtask/            # 開発用タスク（OS非依存。bash/ps1 を書かない）
├── frontend-macos/       # Swift + IMKit
├── frontend-windows/     # TSF
├── config/               # default.toml / romaji tables / keymap presets / style presets
├── bench/                # 精度・レイテンシベンチ
└── docs/
```

## 4. コア設計

### 4.1 IR: プリエディットのセグメント構造（最重要）

用語: **reading** = ひらがなの読み（例 `とうきょうにいく`）、**surface** = 表示・確定される表記（例 `東京に行く`）。

```rust
struct Preedit {
    segments: Vec<Segment>,
    cursor: CursorPos,            // segment index + reading内offset
    focus: Option<usize>,         // Converting時の注目文節
    selection: Option<Range>,     // 2文節以上にまたがる範囲指定（RangeSelecting）
    revision: u64,
}

struct Segment {
    reading: String,              // ひらがな読み（正）
    surface: String,              // 現在の表示（変換結果 or 読みそのまま）
    kind: SegmentKind,            // Japanese | Literal | Fixed | Styled
    candidates: Option<CandidateSet>, // 遅延生成
    chunk_id: u32,                // ライブ変換の再変換単位
}
```

設計上の不変条件:

- `reading` が常に真実であり、`surface` はその射影。範囲操作・再変換は
  すべて「readingの部分列を選んで surface を再計算する」操作として統一的に定義する
- 1 Segment = 1 文節相当。位置の内部表現は reading 基準、プロトコルへは surface 上の UTF-16 offset で出す
- `Literal`（英数・記号・数字）は **surface を変更しない**だけであり、推論の入力（文脈）には含める
- 部分確定は該当セグメントを `Fixed` にするだけで表現できる
- `Styled` はスタイル変換結果（reading を持たない）。元のセグメント列を保持し Undo 可能。再変換の対象外

### 4.2 アライメント

LLM は surface しか返さないため、reading との対応（どの漢字がどの読みか）をコア側で求める。

1. surface 中のひらがな（送り仮名・助詞）を reading 上の固定アンカーとして一致させる
2. アンカー間の漢字列に、システム辞書の読みを用いた DP で reading の部分列を割り当てる
3. 割り当て不能区間は 1 Segment にまとめる（それ以上分割しない）
4. 文節区切りは辞書 + 連接コストの Viterbi 結果を初期値とし、アライメント結果と整合させる

- 候補の供給元が結果文字列しか返さない場合でも、この処理で IR が成立する
- 実現可能性と精度は **Phase 0 で検証**する（Phase 1 の前提となるため）

### 4.3 状態機械

```
Direct ──文字キー──▶ Composing ──Space──▶ Converting ──Ctrl+Shift+←/→──▶ RangeSelecting
  ▲                    │  ▲                 │   ▲                              │
  └────確定/ESC────────┘  └─ESC/BS(取消)────┘   └──────────ESC──────────────────┤
                                                                                 └─スタイルキー─▶ StyleAction
```

状態: `Direct` / `Composing`（ライブ変換はここで動く） / `Converting` /
`RangeSelecting` / `StyleAction`（LLM実行中。キャンセル可能な非同期状態）

- すべての状態遷移は `Action` enum（§4.9）で駆動され、キーとの対応はキーマップが解決する
- 状態機械はOSに一切依存しない純粋なRustコードとし、ヘッドレステスト対象とする
- 完全な遷移表（状態 × Action）はゴールデンテスト YAML と同時に Phase 1 冒頭で作成する

主要な個別挙動:

| 状態 | 入力 | 挙動 |
| --- | --- | --- |
| Composing | Enter | 表示中の surface を確定（ライブ変換OFFなら reading） |
| Composing | ESC | preedit 全破棄（`composing.esc_behavior` で変更可） |
| Converting | ESC / Backspace | Composing に戻る（変換取消） |
| Converting | 文字キー | 現在の変換結果を確定し、新しい入力を開始 |
| 任意 | Shift+英字 | 大文字を Literal として挿入し、変換/確定/ESC まで後続の小文字も英字で継続（`Nagi` と打てる）。`input.shift_letter = literal_continue \| single_upper` |
| Composing/Converting | Ctrl+K | 対象をカタカナ化（単発操作） |
| 任意 | 英数/かなキー | 入力モード切替 |
| 確定直後 | Ctrl+Backspace（既定） | 確定取消: 直前の確定を未確定状態へ戻す（アプリが対応する場合） |

### 4.4 変換単位: 文全体変換と文節変換

- Space で変換開始時、文全体の最良候補を文節に区切って表示する
- 候補ウィンドウの初期表示は `conversion.initial_view`:
  - `sentence`（既定）: 文全体の N-best
  - `segment`: 先頭文節の候補（MS-IME / Mozc と同じ）
- `←/→`: 注目文節の移動（候補は注目文節のものに切替）
- `Shift+←/→`: 注目文節の区切りを reading 1文字単位で伸縮し、当該文節と後続を再変換
- `Ctrl+Shift+←/→`: 複数文節にまたがる選択（`RangeSelecting`）
- いずれもキーマップで変更可能

### 4.5 変換パイプライン

```
key → romaji変換 → Preedit更新 → 候補生成 → アライメント → ランキング → 差分通知
```

候補生成の供給元（既定の優先順。`[ranking]` で順序・重みを上書き可能）:

1. 学習キャッシュ（完全一致・前方一致）
2. ユーザー辞書 / スニペット
3. LLM推論（文脈・プロファイル込み）
4. システム辞書（Viterbi）
5. ひらがな / カタカナ
6. Rewriter（半角カナ・全角英数・日付・数字表記・簡易計算・記号バリアント）

- N-best（上位N案の生成）:
  - 文全体: LLM のビームサーチ（幅 `conversion.beam_width`、既定3。プロファイルで変動）
  - 文節: 辞書で列挙 → 上位数件を LLM で文脈込み再採点
- 出所の異なるスコアは共通尺度に正規化して統合。同一表記は1候補にまとめ、出所アノテーションは複数保持
- アノテーションはプロトコル上 enum（`learned` / `user_dict` / `llm` / `system_dict` / `rewriter` …）。アイコンはフロントが割り当てる
- 任意の再ランク器（reranker）を差し込める trait を用意する（§4.8 の Laya 等の評価用）

### 4.6 ライブ変換

- `composing_chunk_len`（既定30文字）でreadingをチャンク分割し、編集されたチャンクのみ再変換
- 強制分割は「記号の連続」「Fixed境界」のみ。**数字の直後では分割しない**（`3じ→3時` のため）
- 直前チャンクの surface を最大 `max_context_length` 文字（Unicodeスカラー単位）左文脈として渡す
- デバウンス 30ms。キーリピート中は推論をスキップし、最後の状態でのみ推論
- 推論結果が遅れて届いても、表示中のひらがな→変換結果の差し替えでカーソル位置と選択を保持する

### 4.7 環境プロファイル（精度と軽さの自動調整）

同じ Nagi が機械とその時の状態に合わせて振る舞いを変える。

| プロファイル | 想定 | 内容 |
| --- | --- | --- |
| `quality` | 高性能機・電源接続 | 大きめのモデル、広いビーム、長い文脈、reranker 有効 |
| `balanced`（既定候補） | 一般的な機械 | 標準モデル、ビーム3、文脈20 |
| `light` | 非力な機械・バッテリー駆動 | 軽量モデル、ビーム1〜2、ライブ変換のチャンク短縮 |
| `minimal` | モデル未取得・障害時 | LLMを使わず辞書 + Viterbi のみ。常に動く最後の砦 |

- **初回起動時の自動判定**: メモリ量・CPUコア数・GPU（Metal 等）を検出し、短いキャリブレーション推論（数百ms）で実測速度を測ってプロファイルを選ぶ
- **実行時の自動切替**（`profile.auto_switch = true`）:
  - バッテリー駆動 / 低電力モード → 一段軽く
  - サーマル状態の悪化 / メモリ逼迫 → 一段軽く（必要ならモデル解放）
  - 直近のレイテンシ p95 が目標超過 → 一段軽く（karukan の adaptive を一般化）
  - 条件が解消されればヒステリシス付きで戻す
- 個々のパラメータ（モデル・ビーム幅・文脈長・スレッド数・GPUオフロード）は設定で固定でき、固定値は自動調整より優先
- 現在のプロファイルと切替理由はメニューから確認できる（「なぜ今変換が弱いのか」が分かる）

### 4.8 推論

- llama.cpp (GGUF) を既存バインディング crate 経由で利用（バージョン固定、`infer` の trait で隠蔽）
- **モデルアダプタ**: モデルごとのプロンプト形式（区切りトークン・文脈欄・プロファイル欄）を吸収。プロファイル注入はアダプタが対応を宣言した欄にのみ行い、非対応モデルでは無視して警告する
- 変換モデル候補: zenz 系（GPT-2 系・約95M・CC-BY-SA-4.0・Q5で約74MB）ほか。Phase 0 で比較して確定
- **Reranker（任意）**: Laya（`convaiinnovations/laya`、Apache-2.0、ModernBERT/mmBERT 系の非生成型「判断」モデル。有料API Jev の互換クローン）は、候補の中から文脈に合うものを選ぶ・入力が英語か日本語かを判定する用途なら適合し得る。ただし
  - かな漢字変換そのもの（生成）はできない
  - 日本語の精度が未確認、多言語版で約322M・重みは safetensors（GGUF ではない）で別ランタイムが必要
  - → コア経路には入れず、Phase 0 で「reranker / 英日判定」として評価し、効果があれば `quality` プロファイル限定の任意機能とする
- 投機的デコーディング（Zenzai方式: 辞書+Viterbi をドラフト、LLM で検証）は Phase 6

### 4.9 キーバインド

- コア側で解決。`Action` enum（例）:
  `Insert(char)` / `StartConvert` / `NextCand` / `PrevCand` / `SelectCand(n)` /
  `Commit` / `CommitPartial` / `Cancel` / `MoveCursor(dir)` / `MoveFocus(dir)` /
  `ResizeSegment(dir)` / `ExtendSelection(dir)` / `ConvertSelection` /
  `StyleAction(id)` / `ToggleLive` / `ToKatakana` / `ToggleSecret` /
  `DeleteLearning` / `Reconvert` / `UndoCommit` ...
- 定義はTOML:

```toml
[keymap.common]
"Eisu"  = "SetMode(alphanumeric)"
"Kana"  = "SetMode(hiragana)"

[keymap.converting]
"Right"            = "MoveFocus(right)"
"Shift+Right"      = "ResizeSegment(right)"
"Ctrl+Shift+Right" = "ExtendSelection(right)"
"Ctrl+K"           = "ToKatakana"
"Ctrl+T"           = "StyleAction(formalize)"
```

- キー名は `Ctrl+Shift+Right` 形式（修飾順不問）、Action は `Name` / `Name(arg)`
- `[keymap.common]` を全モードが継承。プリセット < ユーザー個別定義
- 不正な行はその行のみ無視してフロントへ通知
- 既定プリセット: `default` / `macos-like` / `mozc-like` / `ms-ime-like` を同梱（乗り換え時の違和感を減らす）

### 4.10 範囲操作

- 範囲に対するアクション:
  - `Space`: 範囲のみ再変換（候補は範囲の reading に対して生成）
  - `StyleAction(id)`: スタイル変換（§4.12）
  - `Ctrl+U/I/O` 等: ひらがな/カタカナ/半角化（Rewriter直行）
- 確定済みテキストの**再変換**: OSの再変換API（IMKit / TSF `ITfFnReconversion`）から
  選択テキスト+周辺を受け取り Composing へ復帰。reading は辞書の逆引きで推定し、不能区間は LLM で補う

### 4.11 学習

- 記録: `reading, surface, context_hash, count, last_used(unix秒), source`
- マッチは reading の完全一致・前方一致。`context_hash` は一致時の加点のみ（フィルタにしない）
- スコア: `ln(1+count)` × 最終使用からの指数減衰（半減期14日）。パラメータ設定可。`max_entries` 超過は低スコアから削除
- 保存: TSV（1行目スキーマバージョン、タブ・改行はバックスラッシュエスケープ）。
  IME切替・フォーカス喪失時 + 5分毎に flush。一時ファイル→rename で原子的に書き、ファイルロックを取る
- **誤学習削除**: 候補ウィンドウで学習候補に `Ctrl+Delete`（変更可）
- **学習停止条件**: secure input / シークレットモード / パスワードマネージャ系アプリ。停止中は Surrounding Text 取得も止める
- 予測変換（Phase 2）: 一般的なスマホ・PC IME と同様。reading `predict.min_chars`（既定1）文字以上で表示、Tab/↓で選択。供給元は学習キャッシュ・ユーザー辞書の前方一致（LLMによる続き生成は後続）

### 4.12 スタイル変換プラグイン

範囲テキスト（または確定直前の全文）に対しLLMで書き換えを行う。azooKey「いい感じ変換」の一般化。

- 変換モデル（GPT-2級）は指示に従えないため、**別の指示追従モデル**を使う。任意ダウンロード、使用時ロード・`style.unload_after_sec` で解放
- アイドルメモリ目標（§7）は変換モデルのみが対象

```toml
[[style]]
id = "formalize"
name = "敬語化"
prompt = "次の文を敬体(です・ます調)に書き換えよ。出力は書き換え後の文のみ: {text}"
keybind = "Ctrl+T"
model = "style"              # style | 専用GGUFパス
```

- 実行は非同期（StyleAction状態）。ESCでキャンセルし元テキストへ復帰。進行中はフロントにインジケータ表示
- 結果は `Styled` セグメントとして反映し、Undo可能
- v2以降: 変換候補として複数案 / 外部コマンド実行フック

### 4.13 辞書と Rewriter

- システム辞書: 既存データ（Mozc OSS / SudachiDict 等を Phase 0 比較）から double-array trie バイナリ + 連接コスト表を生成。ビルドツールは `xtask` / CLI として同梱
- 辞書 + Viterbi は Phase 1 から使う（`minimal` 経路・文節区切り・アライメント・文節候補）
- ユーザー辞書: ディレクトリ内の Mozc TSV を全読み込み。インポート/エクスポートは Mozc TSV を正とし、MS-IME / Google 日本語入力形式からのインポートも用意する
- スニペット: ユーザー辞書の特殊品詞（読み→定型文、複数行可）
- Rewriter: 決定的変換の最終段。trait でプラグイン追加可能。全半角、カタカナ/半角カナ、日付、数字表記、簡易計算、記号バリアント
- typo耐性: ローマ字再結合条件の緩和（1打鍵抜け・挿入への復帰）は romaji 側で対応

### 4.14 気配りチェックリスト（全フェーズ共通の品質基準）

各機能の完了時にこの一覧で確認する。

- **入力を失わない**: エンジン再起動時の preedit 復元（§3.3）、学習の原子的保存、確定取消
- **待たせない**: キー入力は常に即座にひらがなを返す。推論は予算内のみ同期、超過分は後から差し替え
- **標準と揃える**: Enter/ESC/BS/矢印/Shift+矢印の挙動を macOS 標準 IME・MS-IME・Mozc と照合し、差異は意図したものだけにする。乗り換え用キーマッププリセット・辞書インポート
- **状態が見える**: 入力モード（メニューバー + 切替時にカーソル付近へ短時間表示）、環境プロファイル、モデルDL進捗、スタイル変換中表示、学習停止中表示
- **説明できる**: 候補の出所アノテーション、プロファイル切替理由、設定エラーの行番号付き通知
- **壊れても動く**: モデル無し/障害時は `minimal` で入力継続。設定が壊れても旧設定で継続
- **環境に馴染む**: ダークモード、フォントサイズ、マルチモニタ・画面端での候補ウィンドウ位置、VoiceOver/ナレーターでの候補読み上げ
- **表記の好み**: 句読点（、。/ ，．）、スペース幅、記号の全半角、数字の全半角などを設定可能
- **診断できる**: `nagi doctor`（設定検証・モデル/辞書整合・レイテンシ簡易計測）。ログに入力内容を含めない

## 5. フロントエンド設計

### 5.1 共通責務

- OSキーイベント → `{keysym, keycode, text, modifiers, kind}` への正規化
- preedit描画（セグメント属性: 通常/注目文節/選択範囲/Fixed/Styled を下線スタイル等で区別）
- 候補ウィンドウ（独自実装。数字キー・矢印選択、アノテーション表示）
- モードインジケータ（メニューバー/タスクトレイ + 切替時にカーソル付近）
- エンジンプロセス管理（起動・監視・再起動・preedit保持と復元・ログ収集）
- アップデート確認（§8）

### 5.2 macOS（先行実装）

- IMKit（IMKServer / IMKInputController）+ Swift
- 入力モード: 状態の正はコアの内部モード。OS の入力モード（ひらがな/英数）へ同期してメニューバーに表示（Mozc と同方式）
- 候補ウィンドウ: 独自 NSPanel（IMKCandidates は使わない）
- JIS 英数/かなキー、右⌘単独タップ（US配列向け）をモード切替に割当
- Secure Input検知で学習停止フラグを送る
- preedit 属性の各アプリでの反映範囲は Phase 0 で実機確認
- 既知の罠（karukan README等より）: ログイン再認識、TextInputMenuAgentのキャッシュ、スリープ時のパイプ破棄 → 設計に織込み済み
- 配布（Apple Developer Program 加入済み。2026-10-02 に非加入前提から変更、`decisions.md` I 節）:
  - Developer ID Application 証明書で署名（Hardened Runtime 有効）→ 公証（`notarytool`）→ staple。Mac App Store には出さない（エンジンの子プロセス起動・`~/Library/Input Methods` への配置・サンドボックス制約のため）
  - 主導線: 署名・公証済みの `.pkg`（または zip）を `shiki-01.com` と GitHub Releases で配布。Gatekeeper の警告は出ない
  - 補助: `curl -fsSL https://shiki-01.com/nagi/install.sh | sh`（CLI・自動化向け）。アンインストールスクリプトも同梱
  - アップデートは自前の ed25519 署名検証を継続（Sparkle は採用しない）。公証・Developer ID 署名は OS 側の信頼、ed25519 は更新経路の完全性という別目的で併用する
  - 署名・公証は CI の secrets（証明書・App Store Connect API キー）で行う。ローカル開発は従来どおり ad-hoc 署名でよい

### 5.3 Windows（後続実装）

- TSF。実装言語は Rust（windowsクレート）を第一候補、難航時は C++薄層 + Rustコア
- 参考: azooKey-Windows / rakukan のTSF層
- エンジン別プロセス化は必須（IME DLLはホストアプリ内で動くため、クラッシュ・メモリをホストから隔離）
- 再変換: `ITfFnReconversion` 実装
- 署名なし配布時の SmartScreen 警告への対応方針は Phase 5 で決める

### 5.4 Linux（将来）

- fcitx5 addon。コアがkeysymベースなので追加コストは小さい見込み

### 5.5 iOS / Android（将来）

- 詳細は `mobile-design.md`。コアは子プロセスではなくライブラリ（FFI）として組み込み、JSON-RPC スキーマは共通のまま使う
- デスクトップ開発中から守る制約（コアのライブラリ完結・パス注入・メモリを意識した辞書形式・キー以外の入力経路）は同文書 §3

## 6. 設定

- パス: XDG / `~/Library/Application Support/com.shiki.nagi/` / `%APPDATA%\Nagi\`
- `config.toml`（抜粋。全項目は `config/default.toml` に日本語コメント付きで管理）:

```toml
[profile]
mode = "auto"                # auto | quality | balanced | light | minimal
auto_switch = true           # バッテリー・温度・レイテンシで自動切替
persona = ""                 # 文体・固有名詞のヒント（対応モデルのみ注入）

[conversion]
live_conversion = true
initial_view = "sentence"    # sentence | segment
composing_chunk_len = 30
beam_width = 3               # 未指定時はプロファイルに従う
num_candidates = 9
n_threads = 0                # 0 = auto
model = "..."                # モデルID or GGUFパス（未指定時はプロファイルに従う）
use_context = true
max_context_length = 20      # 文字数
max_latency_ms = 100
sync_budget_ms = 150

[input]
shift_letter = "literal_continue"   # literal_continue | single_upper
punctuation = "、。"                  # 、。 | ，． | ，。

[ranking]
source_order = ["learned", "user_dict", "llm", "system_dict", "kana", "rewriter"]

[learning]
enabled = true
max_entries = 10000

[keymap]
preset = "default"           # + 個別上書き

[update]
auto_check = true
```

- 未指定の項目はプロファイルが決め、明示指定は常にプロファイルより優先
- ホットリロード: ファイル監視 or `reload_config`。読込失敗時は旧設定を維持し、行番号付きでフロントへ通知
- モデル変更は次回変換開始時に遅延ロード
- v1 の設定手段はファイル編集 + メニューからのプロファイル/モード切替。設定GUIは Phase 4 以降

## 7. パフォーマンス目標

基準機: Apple Silicon M1 / メモリ 8GB / `balanced` プロファイル。量子化レベルは Phase 0 で確定。

| 指標 | 目標 |
| --- | --- |
| キー入力→preedit反映（ひらがな） | p95 < 16ms（推論を待たない） |
| キー入力→ライブ変換結果反映 | p95 < 50ms |
| Space変換→候補表示 | p95 < 150ms |
| 文節伸縮→再変換表示 | p95 < 100ms |
| スタイル変換（1文） | < 1.5s（非同期・キャンセル可） |
| アイドルメモリ | `light` で < 500MB（変換モデルのみ） |
| コールドスタート | < 2s（モデルはmmap + 遅延ロード。`minimal` は < 300ms） |

- 対応OS: macOS 13 以降（Apple Silicon。Intel Mac はベストエフォート）/ Windows 10 22H2 以降・11（x64）
- 計測はエンジン内にレイテンシメトリクスを常設し、ログ + ベンチCLIで確認。プロファイル別に記録

## 8. セキュリティ・プライバシー・配布

- 全処理ローカル完結。ネットワークアクセスは以下のみ:
  - モデル/辞書のダウンロード（初回・プロファイル変更時・ユーザー操作時）
  - アップデート確認（起動時 + 任意タイミング。`update.auto_check` で無効化可）
- ダウンロード元は `shiki-01.com`（モデル等は取得元ミラーまたは原配布元）。全ファイル SHA-256 検証、配布物は ed25519 署名で検証（公開鍵はアプリに埋め込み）
- secure input / シークレットモードで学習・文脈取得・ログを停止
- 学習ファイルはユーザー領域のみ。ワンコマンドで全削除可能
- ログに入力内容を残さない（デバッグビルドのみopt-inで許可）
- ライセンス: コードは `MIT OR Apache-2.0`。モデル・辞書は同梱せず別途取得し各ライセンスに従う。表示義務は `NOTICE` と「このアプリについて」で満たす

## 9. テスト戦略

- **状態機械ゴールデンテスト**: キー列 → (preedit, commit) の期待値をYAMLで大量定義。遷移表と1対1で対応させる
- **標準IME挙動の照合テスト**: 同一キー列に対する macOS 標準 IME / MS-IME / Mozc の挙動を記録し、差異を明示的に管理
- **アライメントテスト**: (reading, surface) → 文節対応の期待値。絵文字・結合文字・サロゲートペアを含む
- **JSON-RPC統合テスト**: サーバーバイナリにイベント列を流すヘッドレステスト。再起動・`restore_session` を含む
- **変換精度ベンチ**: 公開ベンチ（ライセンスが許すもの）+ 自前評価セット（ドッグフーディング中の誤変換をローカル・opt-in で収集し、手で正解付け）。プロファイル別に計測
- **レイテンシベンチ**: チャンク長・プロファイル別の p50/p95 を CI で記録
- フロントは手動E2Eチェックリスト（アプリ互換: ブラウザ/ターミナル/エディタ/Office系/チャット系）

## 10. 開発基盤

- macOS / Windows のどちらでもコアを開発・テスト可能にする。補助タスクは `cargo xtask`（OS非依存）
- Rust edition 2024、`rust-toolchain.toml` で stable の版を固定
- CI: GitHub Actions。ubuntu（コアテスト）/ macos（コア + Swiftビルド）/ windows（コア）。clippy・rustfmt 必須、SwiftLint 併用
- モデル・辞書はリポジトリに入れない（取得スクリプト + SHA-256 検証）
- ローマ字テーブル: Mozc 互換 TSV（`入力\t出力\t保留`）。v1 は標準テーブル、AZIK は追加テーブルとして後続

## 11. リスク

| リスク | 影響 | 対策 |
| --- | --- | --- |
| 変換モデルのライセンス・品質 | 高 | Phase 0 で比較。同梱せず別途取得でライセンス分離。将来は自前 fine-tune も選択肢 |
| アライメント（§4.2）の精度不足 | 高 | Phase 0 で検証。不足時は文節単位選択に限定し、辞書 Viterbi の区切りを優先 |
| Windows TSFの実装難度 | 高 | macOS先行でコアを固める。azooKey-Windows/rakukanのコードを精読してから着手 |
| 公証・署名運用の失敗（証明書期限切れ、公証拒否、Hardened Runtime の権限不足） | 中 | CI で署名・公証・staple を自動化し、`spctl --assess` で検証。証明書の期限を管理。Windows の署名は別途 Phase 5 で決める |
| Phase 1 の範囲拡大（文節変換を含む） | 中 | 辞書 Viterbi を先に作り、LLM 無しで文節操作を完成させてから LLM を載せる順で進める |
| 投機的デコーディングの実装コスト | 中 | Phase 6 まで素朴推論 + プロファイル + チャンクで対応 |
| IMKit/TSF固有の互換性問題 | 中 | 互換チェックリスト運用。preedit非対応アプリ向けフォールバック |
| 学習の暴走・誤学習 | 中 | 削除UI・上限・減衰を最初から実装 |
| 開発リソース（個人・並行プロジェクト多数） | 高 | フェーズごとに「使える状態」を維持（DoD） |
| 将来のモバイル対応でコア改修が必要になる | 中 | `mobile-design.md` §3 の制約をデスクトップ開発中から守る |

## 12. MVPとロードマップ

方針: **各フェーズ完了時点で自分が日常利用できる**ことを DoD（Definition of Done = 完了条件）に含める。
ドッグフーディングが最大のテストであり、学習データ・不満点の収集源になる。

### Phase 0: 技術検証（〜3週間目安）

- llama.cpp GGUF推論をRustから叩き、読み→表記の変換が成立することを確認
- 変換モデル・システム辞書のライセンス・品質・速度の比較表（§2.3 の基準）
- アライメント（§4.2）のプロトタイプと精度確認
- Laya の reranker / 英日判定としての評価（日本語精度・速度・メモリ）
- 基準機・非力機でのキャリブレーション推論の実測（§4.7 の判定材料）
- IMKitのスケルトン（「a」を打ったら「あ」がpreeditに出る）と、主要アプリでの preedit 属性反映の確認
- DoD: 変換1回のE2E（キー→推論→表示）がプロトタイプで動き、上記比較表・検証結果が `decisions.md` に記録されている

### Phase 1: MVP（macOS・文全体変換 + 文節変換）

含む:
- 状態機械（Direct/Composing/Converting）+ IR（セグメント構造・アライメントは**この時点で**導入）
- ローマ字変換（テーブル外部定義）/ literal保護 / Shift+英字
- 辞書 + Viterbi（`minimal` 経路）→ その上に LLM + ひらがな/カタカナ候補
- 文全体の N-best と文節変換（注目移動・区切り伸縮）
- 基本キー操作（Space/Enter/ESC/BS/矢印/Shift+矢印/数字選択/Ctrl+K）
- JSON-RPCプロトコル + プロセス管理（自動再起動・スリープ対応・preedit復元）
- 環境プロファイルの自動判定（実行時自動切替は Phase 2）
- config.toml（最小セット）/ インストールスクリプト / アップデート確認
- ヘッドレステスト基盤（ゴールデン・統合）

含まない: ライブ変換 / 学習 / ユーザー辞書 / 複数文節の範囲選択 / スタイル変換 / Windows

DoD: 以下のアプリで日本語入力の常用に耐える（標準IMEからの一時乗換が可能）
Safari・Chrome・VS Code・ターミナル・メモ・Slack/Discord・Microsoft Word（macOS）

### Phase 2: ライブ変換 + 学習 + 辞書

- チャンク式ライブ変換 + Surrounding Text文脈
- 環境プロファイルの実行時自動切替
- 学習キャッシュ（自動登録・予測変換・削除UI・secure input停止）
- ユーザー辞書（Mozc TSV + 他IME形式インポート）+ スニペット
- 候補出所アノテーション
- DoD: ライブ変換ONで1週間常用してレイテンシ目標p95を満たす

### Phase 3: 範囲操作

- RangeSelecting状態 / 範囲のみ再変換 / 部分確定 / 確定取消
- 確定済みテキストの再変換（IMKit経由）
- カスタムキーバインド（TOML + プリセット + ホットリロード）
- DoD: 誤変換をESCで打ち直す頻度が体感で減る（範囲再変換で直せる）

### Phase 4: スタイル変換 + Rewriter拡張 + 設定GUI

- StyleActionプラグイン（敬語化/である調/カタカナ英語化 等のプリセット同梱）+ スタイル用モデルの任意DL
- Rewriter: 日付・数字表記・簡易計算
- プロファイル注入（persona設定）
- 設定GUI（プロファイル・キーマップ・辞書管理・学習削除）
- DoD: レポート執筆（である調）とチャットで文体プリセットを使い分けられる

### Phase 5: Windows対応

- TSFフロントエンド + キー正規化 + 再変換（ITfFnReconversion）
- インストーラ / 設定パスのWindows対応
- DoD: kispc側で常用可能。コア側の変更が原則ゼロであること（分離設計の検証）

### Phase 6: パフォーマンス最適化

- 投機的デコーディング（辞書+Viterbiドラフト → LLM検証）
- モデル量子化の比較、GPUオフロード検証、プロファイル閾値の再調整
- DoD: 全プロファイルで p95 目標の再達成 + `light` 時のメモリ目標達成

### Phase 7以降（バックログ）

- Linux (fcitx5) / スタイル変換の複数候補提示 / 辞書の半自動拡張 /
  アプリ別プロファイル / 候補ウィンドウのリッチ化（注釈・プレビュー）/
  変換モデルの自前 fine-tune / Laya 等による英日自動判定（評価結果次第で前倒し）
- モバイル（Android 先行 → iOS）: Phase M0〜M4（`mobile-design.md` §7）

---

## 付録A: 決定状況

1. ~~プロジェクト名~~ → `Nagi`（IME の bundle id: `com.shiki.inputmethod.Nagi`（→ decisions.md G-37）、設定/データの識別子: `com.shiki.nagi`、リポジトリ名: `nagi`）
2. 使用モデル → Phase 0 の成果物（基準: 無料・再配布制約が小さい → 品質 → 速度/サイズ）
3. ~~ローマ字テーブル~~ → Mozc 互換 TSV、v1 は標準のみ、AZIK は後続
4. ~~ライセンス~~ → `MIT OR Apache-2.0`、外部資産は別途取得
5. システム辞書 → Phase 0 の成果物（基準は 2 と同じ）

## 付録B: 参考資料

- karukan: コア分離・プロトコル・ライブ変換チャンク・adaptive戦略
- Zenzai論文（NLP2025）+ 解説記事: 投機的デコーディングの理論と実装
- azooKey-Desktop / azooKey-Windows: IMKit・TSF層
- rakukan: Windowsでの別プロセス化・設定ホットリロード
- riq0h fork: 部分変換・ローマ字再結合緩和
- Mozc: 辞書・キーマップ設計・Rewriter・文節操作
- Laya (`convaiinnovations/laya`): 判断型モデル。reranker / 英日判定の評価対象
