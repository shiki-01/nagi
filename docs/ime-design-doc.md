# Nagi 企画・設計書

- プロジェクト名: `Nagi`（コアは `nagi-core`、各フロントは `nagi-macos` / `nagi-windows`）
- 版: v0.1 (draft)
- 対象プラットフォーム: macOS(先行) / Windows(後続) / Linux(将来検討)

---

## 1. 概要

ローカルLLM（GPT-2級の小型モデル）によるニューラルかな漢字変換を中核とする日本語IMEを、
**コアエンジン（プラットフォーム非依存）** と **フロントエンド（OS組み込み層）** に分離して開発する。

先行事例の位置づけ:

| プロジェクト | 参考にする点 |
| --- | --- |
| karukan (togatoga) | コア/フロント分離、JSON-RPC over stdio、チャンク式ライブ変換、adaptive戦略、学習キャッシュ設計 |
| azooKey / Zenzai | 投機的デコーディング、いい感じ変換（スタイル変換）、プロファイル注入 |
| rakukan | Windows TSF層の構成（azooKey-Windows参照）、literal保護、設定ホットリロード |
| riq0h fork | 部分変換（Shift+矢印）、ローマ字再結合条件の緩和 |
| Mozc | 辞書フォーマット(TSV)、キーマップの粒度、Rewriterの発想 |

本プロジェクトの差別化点は「**範囲操作を第一級市民としたIR設計**」と
「**範囲×LLMアクション（スタイル変換）のプラグイン化**」の2点。
既存実装はいずれも範囲操作が後付け（またはフォークのみ）であり、ここを最初から設計に含める。

## 2. 目標 / 非目標

### 目標

1. macOS / Windows の両方で同一コアが動作する
2. ライブ変換（Space不要のリアルタイム変換）
3. Surrounding Text + プロファイルを加味した文脈推論
4. 変換中・確定後テキストの**範囲指定**と、範囲に対するアクション実行
   （再変換 / 部分変換 / スタイル変換）
5. 学習の自動登録（確定候補・選択順位・文脈を記録）と誤学習の削除UI
6. ユーザー辞書（Mozc TSV互換）+ スニペット展開
7. キーバインドの完全カスタム（TOML定義、コア側で解決）
8. 実用レイテンシ（後述のパフォーマンス目標）

### 非目標（当面やらない）

- かな入力レイアウト（ローマ字入力のみ）
- 手書き・音声入力
- クラウド変換（すべてローカル完結。プライバシー方針でもある）
- iOS / Android
- 候補ウィンドウのマウス操作（v1では非対応で可）

## 3. 全体アーキテクチャ

```
┌────────────────────────────┐                     ┌──────────────────────────────┐
│ Frontend (OS組み込み層)      │   JSON-RPC 2.0      │ Core Engine (Rust)            │
│                            │   (改行区切り,       │                              │
│ macOS: IMKit + Swift       │    stdin/stdout)    │ ・状態機械 / IR               │
│ Windows: TSF + Rust/C++    │ ◀─────────────────▶ │ ・ローマ字変換                │
│                            │                     │ ・候補生成パイプライン         │
│ 責務:                       │                     │ ・llama.cpp推論 (GGUF)       │
│ ・OSキーイベント→keysym変換  │                     │ ・辞書 / 学習 / Rewriter      │
│ ・preedit/候補ウィンドウ描画 │                     │ ・キーバインド解決             │
│ ・範囲ハイライト描画          │                     │ ・スタイル変換プラグイン       │
│ ・子プロセス管理/自動再起動  │                     │                              │
└────────────────────────────┘                     └──────────────────────────────┘
```

### 3.1 分離の原則

- **コアに置くもの**: 状態機械、キーバインド解決、変換・学習・辞書のすべてのロジック
- **フロントに置くもの**: OSイベントの正規化、描画、プロセスライフサイクル管理**のみ**
- フロントは「keysym+modifierを送り、描画指示を受け取る」だけのthin clientとする。
  これによりカスタムキーバインド・範囲操作・スタイル変換が全OSで一度に実装される。

### 3.2 プロトコル

- JSON-RPC 2.0 / 改行区切り / stdin-stdoutパイプ（karukan方式）
- Rust側に正のスキーマ定義を置き、Swift側は型ミラーを持つ
- 主要メソッド（案）:

| method | 方向 | 内容 |
| --- | --- | --- |
| `process_key` | F→C | keysym, modifiers, 押下/離上 |
| `set_context` | F→C | Surrounding Text、アプリ識別子、secure input フラグ |
| `commit_text` | C→F | 確定文字列 |
| `update_preedit` | C→F | セグメント配列（属性付き。§4.1） |
| `update_candidates` | C→F | 候補リスト + アノテーション + 選択index |
| `update_mode` | C→F | 入力モード（ひらがな/英数/カタカナ）表示用 |
| `reconvert` | F→C | 確定済みテキストの再変換要求（OS側の再変換APIから） |
| `reload_config` | F→C | 設定ホットリロード |

### 3.3 プロセス管理

- エンジンはフロントの子プロセス。クラッシュ時は指数バックオフで自動再起動
- スリープ復帰時に再起動（macOSがスリープ中にパイプを破棄する事象への対処）
- 再起動時、学習キャッシュはファイルから復元（揮発状態は捨てて良い設計にする）

### 3.4 リポジトリ構成（案）

```
nagi/
├── core/            # Rust workspace
│   ├── engine/      # 状態機械・IR・パイプライン
│   ├── romaji/      # ローマ字変換（テーブル外部定義）
│   ├── infer/       # llama.cpp binding・投機的デコーディング
│   ├── dict/        # trie辞書・Mozc TSV・ユーザー辞書
│   ├── learn/       # 学習キャッシュ
│   ├── rewriter/    # 決定的変換（日付・記号・計算 等）
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

## 4. コア設計

### 4.1 IR: プリエディットのセグメント構造（最重要）

範囲操作を後付けにしないため、プリエディットは最初からセグメント列として持つ。

```rust
struct Preedit {
    segments: Vec<Segment>,
    cursor: CursorPos,            // segment index + 文字offset
    selection: Option<Range>,     // 範囲指定（segment跨ぎ可）
}

struct Segment {
    reading: String,              // ひらがな読み（正）
    surface: String,              // 現在の表示（変換結果 or 読みそのまま）
    kind: SegmentKind,            // Japanese | Literal(英数・記号・数字) | Fixed(部分確定済み)
    candidates: Option<CandidateSet>, // 遅延生成
    chunk_id: u32,                // ライブ変換の再変換単位
}
```

設計上の不変条件:

- `reading` が常に真実であり、`surface` はその射影。範囲操作・再変換・スタイル変換は
  すべて「readingの部分列を選んで surface を再計算する」操作として統一的に定義する
- `Literal` セグメントは変換パイプラインを素通しする（literal保護）
- 部分確定は該当セグメントを `Fixed` にするだけで表現できる

### 4.2 状態機械

```
Direct ──文字キー──▶ Composing ──変換開始──▶ Converting
  ▲                    │  ▲                    │
  └──────確定/ESC──────┘  └──ESC(候補破棄)──────┘
                       │
                       └─範囲操作キー─▶ RangeSelecting ─アクション─▶ Converting(範囲のみ)
                                                        └─スタイルキー─▶ StyleAction
```

状態: `Direct` / `Composing`（ライブ変換はここで動く） / `Converting` /
`RangeSelecting` / `StyleAction`（LLM実行中。キャンセル可能な非同期状態）

- すべての状態遷移は `Action` enum（§4.8）で駆動され、キーとの対応はキーマップが解決する
- 状態機械はOSに一切依存しない純粋なRustコードとし、ヘッドレステスト対象とする

### 4.3 変換パイプライン

```
key → romaji変換 → Preedit更新 → 候補生成 → ランキング → preedit/候補通知
```

候補生成の優先順位（karukan準拠 + 拡張）:

1. 学習キャッシュ（完全一致・前方一致）
2. ユーザー辞書 / スニペット
3. LLM推論（文脈・プロファイル込み）
4. システム辞書（スコア順）
5. ひらがな / カタカナ
6. Rewriter（半角カナ・全角英数・日付・数字表記・簡易計算・記号バリアント）

候補には出所アノテーション（📝/👤/🤖/📚 等）を付与してフロントに渡す。

### 4.4 ライブ変換

- `composing_chunk_len`（既定30文字）でreadingをチャンク分割し、編集されたチャンクのみ再変換
- チャンク境界は「記号・数字の連続」「Fixedセグメント」で強制分割
- 1キーあたりのレイテンシ上限をチャンク長で担保する（karukan方式）
- デバウンス: キーリピート中は推論をスキップし、最後の状態でのみ推論

### 4.5 推論と投機的デコーディング

- llama.cpp (GGUF) をFFIで利用。`n_threads` / GPUオフロードは設定可能
- **投機的デコーディング（Zenzai方式）**: システム辞書 + Viterbi（2-gram級）を
  ドラフトモデルとして候補文字列を生成し、LLMは検証・修正に使う。
  これにより入力長に対する線形な生成コストを大幅に削減する
- **adaptive戦略（karukan方式）**: `max_latency_ms` 超過でメインモデル→軽量モデルに自動切替。
  `strategy = adaptive | light | main` を設定で選択可能
- 文脈: `set_context` で受けた Surrounding Text（最大N文字）+ ユーザープロファイル
  （固有名詞・文体設定をpromptに注入）を条件付けに使う
- アプリ別プリセット: アプリ識別子ごとに文体傾向（ターミナル=変換控えめ 等）を切替可能

### 4.6 学習

- 記録内容: 読み / 確定surface / 選択された候補の順位 / 出所 / 直前文脈ハッシュ / timestamp
- スコア: recency（最終使用時刻の減衰）重視 + 頻度補正。`max_entries` で上限、LRU削除
- 保存: TSV1ファイル。IME切替・フォーカス喪失時にflush（commit毎には書かない）
- **誤学習削除**: 候補ウィンドウで学習候補に対し `Ctrl+Delete`（キーマップで変更可）で個別削除
- **学習停止条件**: secure inputフィールド / シークレットモード（トグルキー）/
  パスワードマネージャ系アプリ識別子。停止中はSurrounding Text取得も止める

### 4.7 辞書

- システム辞書: double-array trie バイナリ。ビルドツールをCLIとして同梱
- ユーザー辞書: ディレクトリ内のファイルを全読み込み。Mozc TSV / 自前バイナリを自動判定
- スニペット: ユーザー辞書の特殊品詞として実装（読み→定型文、複数行可）
- インポート/エクスポート: Mozc TSVを正とする

### 4.8 キーバインド

- コア側で解決。`Action` enum（例）:
  `Insert(char)` / `StartConvert` / `NextCand` / `PrevCand` / `SelectCand(n)` /
  `Commit` / `CommitPartial` / `Cancel` / `MoveCursor(dir)` /
  `ExtendSelection(dir)` / `ShrinkSelection(dir)` / `ConvertSelection` /
  `StyleAction(id)` / `ToggleLive` / `ToggleKatakana` / `ToggleSecret` /
  `DeleteLearning` / `Reconvert` ...
- 定義はTOML:

```toml
[keymap.converting]
"Shift+Right" = "ExtendSelection(right)"
"Shift+Left"  = "ExtendSelection(left)"
"Ctrl+K"      = "ToggleKatakana"
"Ctrl+T"      = "StyleAction(formalize)"
```

- モード（Direct/Composing/Converting/RangeSelecting）ごとにマップを持つ
- 既定プリセット: `default` / `macos-like` / `mozc-like` を同梱

### 4.9 範囲操作

- `Shift+←/→` で選択範囲を伸縮（RangeSelecting状態へ）。`←/→` で選択位置の移動
- 範囲に対するアクション:
  - `Space`: 範囲のみ再変換（候補は範囲のreadingに対して生成）
  - `StyleAction(id)`: スタイル変換（§4.10）
  - `Ctrl+U/I/O` 等: ひらがな/カタカナ/半角化（Rewriter直行）
- 確定済みテキストの**再変換**: OSの再変換API（IMKit / TSF `ITfFnReconversion`）から
  選択テキスト+周辺を受け取り、Composingへ復帰させる

### 4.10 スタイル変換プラグイン

範囲テキスト（または確定直前の全文）に対しLLMで書き換えを行う。azooKey「いい感じ変換」の一般化。

```toml
[[style]]
id = "formalize"
name = "敬語化"
prompt = "次の文を敬体(です・ます調)に書き換えよ。出力は書き換え後の文のみ: {text}"
keybind = "Ctrl+T"           # keymapからも参照可
model = "main"               # main | light | 専用GGUFパス

[[style]]
id = "dearu"
name = "である調"
prompt = "次の文を常体(である調)に書き換えよ。出力のみ: {text}"
```

- 実行は非同期（StyleAction状態）。ESCでキャンセルし元テキストへ復帰
- 結果はプリエディット置換として反映し、Undo可能（元readingを保持しているため）
- v2以降: 変換候補として複数案を出す / 外部コマンド実行のフックも検討

### 4.11 Rewriter

決定的変換の最終段。プラグイン的にRewriterを追加できるtrait設計にする。

- 同梱: 全半角変換、カタカナ/半角カナ、日付（きょう/あした→複数表記）、
  数字表記（漢数字/桁区切り）、簡易計算（`3*4=`）、記号バリアント
- typo耐性: ローマ字再結合条件の緩和（1打鍵抜け・挿入への復帰）はromajiモジュール側で対応

## 5. フロントエンド設計

### 5.1 共通責務

- OSキーイベント → 共通keysym表現（XKB keysym準拠）への変換テーブル
- preedit描画（セグメント属性: 通常/選択範囲/Fixed を下線スタイルで区別）
- 候補ウィンドウ（数字キー・矢印選択。マウス非対応で可）
- モードインジケータ（メニューバー/タスクトレイ + 可能ならカーソル付近）
- エンジンプロセス管理（起動・監視・再起動・ログ収集）

### 5.2 macOS（先行実装）

- IMKit（IMKServer / IMKInputController）+ Swift
- NSEvent → keysym変換（karukan `KeyCodeMap.swift` が参考実装）
- JIS かな/英数キー、右⌘単独タップ（US配列向け）をモード切替に割当
- Secure Input検知: `IsSecureEventInputEnabled` 相当で学習停止フラグを送る
- 既知の罠（karukan READMEより）: ログイン再認識、TextInputMenuAgentのキャッシュ、
  スリープ時のパイプ破棄 → 設計に織込み済み

### 5.3 Windows（後続実装）

- TSF (Text Services Framework)。実装言語はRust（windowsクレート）を第一候補、
  難航時はC++薄層 + Rustコアの構成に切替
- 参考: azooKey-Windows / rakukan のTSF層
- エンジン別プロセス化は必須（IME DLLはホストアプリ内で動くため、
  クラッシュ・メモリをホストから隔離する。rakukanが同構成で安定化した実績あり）
- 再変換: `ITfFnReconversion` 実装

### 5.4 Linux（将来）

- fcitx5 addon。コアがkeysymベースなので追加コストは小さい見込み

## 6. 設定

- パス: XDG / `~/Library/Application Support/com.shiki.nagi/` / `%APPDATA%\Nagi\`
- `config.toml`（抜粋。全項目は `config/default.toml` に日本語コメント付きで管理）:

```toml
[conversion]
live_conversion = true
composing_chunk_len = 30
strategy = "adaptive"        # adaptive | light | main
num_candidates = 9
n_threads = 0                # 0 = auto
model = "..."                # モデルID or GGUFパス
light_model = "..."
use_context = true
max_context_length = 20
max_latency_ms = 100

[learning]
enabled = true
max_entries = 10000

[profile]
persona = ""                 # 文体・固有名詞のヒント（prompt注入）

[keymap]
preset = "default"           # + 個別上書き
```

- ホットリロード: ファイル監視 or フロントの`reload_config`で即時反映
  （モデル変更は次回変換開始時に遅延ロード）

## 7. パフォーマンス目標

| 指標 | 目標 |
| --- | --- |
| キー入力→preedit反映（ライブ変換ON） | p95 < 50ms |
| Space変換→候補表示 | p95 < 150ms |
| スタイル変換（1文） | < 1.5s（非同期・キャンセル可なので体感を担保） |
| アイドルメモリ | light戦略で < 500MB 目安 |
| コールドスタート | < 2s（モデルはmmap + 遅延ロード） |

計測はエンジン内にレイテンシメトリクスを常設し、ログ + ベンチCLIで確認する。

## 8. セキュリティ・プライバシー

- 全処理ローカル完結。ネットワークアクセスはモデル/辞書の初回ダウンロードのみ
- secure input / シークレットモードで学習・文脈取得・ログを停止
- 学習ファイルはユーザー領域のみ。ワンコマンドで全削除可能にする
- ログに入力内容を残さない（デバッグビルドのみopt-inで許可）

## 9. テスト戦略

- **状態機械ゴールデンテスト**: キー列 → (preedit, commit) の期待値をYAMLで大量定義
- **JSON-RPC統合テスト**: サーバーバイナリにイベント列を流すヘッドレステスト
- **変換精度ベンチ**: 読み→正解表記のコーパスで一致率を計測（AJIMEE-Bench形式を参考）。
  学習・辞書・モデル変更時の回帰検出に使う
- **レイテンシベンチ**: チャンク長・戦略別のp50/p95を CI で記録
- フロントは手動E2Eチェックリスト（アプリ互換: ブラウザ/ターミナル/エディタ/Office系）

## 10. リスク

| リスク | 影響 | 対策 |
| --- | --- | --- |
| 変換モデルの調達（zenz系・jinen系モデルのライセンスが自プロジェクトで利用可能か要確認） | 高 | 先にライセンス調査。不可なら自前でGPT-2小型を蒸留/学習（コスト大なのでMVPでは既存モデル前提を崩さない） |
| Windows TSFの実装難度 | 高 | macOS先行でコアを固める。azooKey-Windows/rakukanのコードを精読してから着手 |
| 投機的デコーディングの実装コスト | 中 | MVPでは素朴推論 + adaptive + チャンクで凌ぎ、最適化フェーズで導入 |
| IMKit/TSF固有の互換性問題（アプリごとの挙動差） | 中 | 互換チェックリスト運用。preedit非対応アプリ向けのフォールバック（下線なし直接挿入） |
| 学習の暴走・誤学習 | 中 | 削除UI・上限・減衰を最初から実装 |
| 開発リソース（個人・並行プロジェクト多数） | 高 | フェーズごとに「使える状態」を維持。各Phase単体で日常利用可能なDoDを置く |

## 11. MVPとロードマップ

方針: **各フェーズ完了時点で自分が日常利用できる**ことをDoD（完了条件）に含める。
ドッグフーディングが最大のテストであり、学習データ・不満点の収集源になる。

### Phase 0: 技術検証（〜2週間目安）

- llama.cpp GGUF推論をRustから叩き、読み→表記の変換が成立することを確認
- 候補モデルのライセンス・品質・速度の比較表を作る
- IMKitのスケルトン（「a」を打ったら「あ」がpreeditに出る）を動かす
- DoD: 変換1回のE2E（キー→推論→表示）がプロトタイプで動く

### Phase 1: MVP（macOS・一括変換）

含む:
- 状態機械（Direct/Composing/Converting）+ IR（セグメント構造は**この時点で**導入）
- ローマ字変換（テーブル外部定義）/ literal保護
- 一括変換: LLM + システム辞書 + ひらがな/カタカナ候補
- 基本キー操作（karukan相当: Space/Enter/ESC/矢印/数字選択/Ctrl+K）
- JSON-RPCプロトコル + プロセス管理（自動再起動・スリープ対応）
- config.toml（strategy/light含む最小セット）
- ヘッドレステスト基盤

含まない: ライブ変換 / 学習 / ユーザー辞書 / 範囲操作 / スタイル変換 / Windows

DoD: macOSの主要アプリで日本語入力の常用に耐える（標準IMEからの一時乗換が可能）

### Phase 2: ライブ変換 + 学習 + 辞書

- チャンク式ライブ変換 + Surrounding Text文脈
- 学習キャッシュ（自動登録・予測変換・削除UI・secure input停止）
- ユーザー辞書（Mozc TSV）+ スニペット
- 候補出所アノテーション
- DoD: ライブ変換ONで1週間常用してレイテンシ目標p95を満たす

### Phase 3: 範囲操作

- RangeSelecting状態 / Shift+矢印での伸縮 / 範囲のみ再変換 / 部分確定
- 確定済みテキストの再変換（IMKit経由）
- カスタムキーバインド（TOML + プリセット + ホットリロード）
- DoD: 誤変換をESCで打ち直す頻度が体感で減る（範囲再変換で直せる）

### Phase 4: スタイル変換 + Rewriter拡張

- StyleActionプラグイン（敬語化/である調/カタカナ英語化 等のプリセット同梱）
- Rewriter: 日付・数字表記・簡易計算
- プロファイル注入（persona設定）
- DoD: レポート執筆（である調）とチャットで文体プリセットを使い分けられる

### Phase 5: Windows対応

- TSFフロントエンド + keysym変換テーブル + 再変換（ITfFnReconversion）
- インストーラ / 設定パスのWindows対応
- DoD: kispc側で常用可能。コア側の変更が原則ゼロであること（分離設計の検証）

### Phase 6: パフォーマンス最適化

- 投機的デコーディング（辞書+Viterbiドラフト → LLM検証）
- レイテンシ計測基盤の常設化、モデル量子化の比較、GPUオフロード検証
- DoD: p95目標の再達成 + light戦略時のメモリ目標達成

### Phase 7以降（バックログ）

- Linux (fcitx5) / スタイル変換の複数候補提示 / 辞書の半自動拡張 /
  アプリ別プロファイル / 候補ウィンドウのリッチ化（注釈・プレビュー）

---

## 付録A: 最初に決めること

1. ~~プロジェクト名~~ → `Nagi` に確定（bundle id: `com.shiki.nagi`、リポジトリ名: `nagi`）
2. 使用モデル（ライセンス調査の結果で確定。Phase 0の成果物）
3. ローマ字テーブルの初期セット（標準 + AZIK対応をv1に含めるか）
4. ライセンス（OSS公開前提ならMIT/Apache-2.0。同梱辞書・モデルのライセンス互換に注意）

## 付録B: 参考資料

- karukan: コア分離・プロトコル・ライブ変換チャンク・adaptive戦略の実装参考
- Zenzai論文（NLP2025）+ 解説記事: 投機的デコーディングの理論と実装
- azooKey-Desktop / azooKey-Windows: IMKit・TSF層の実装参考
- rakukan: Windowsでの別プロセス化・設定ホットリロード
- riq0h fork: 部分変換・ローマ字再結合緩和の実装参考
- Mozc: 辞書TSV・キーマップ設計・Rewriter
