# iOS 設定項目一覧（v0 draft）

`docs/ios-backlog.md` 要望 1「設定項目の洗い出しと実装」の項目表。シンプルモード / 詳細モードの二段構成を前提に、
項目・型・既定値・反映先・実装状況を列挙する。実装の進行に合わせて本書を更新する。

## 1. 方針

- **シンプルモード**: 少数の主要項目のみ。すべて安全な既定値を持ち、触らなくても実用になること。モード列 `S` の項目だけを出す
- **詳細モード**: シンプルの項目に加え、`D` の項目をすべて出す。各項目は「既定に戻す」操作を持つ（コアの `update_settings` で値に `null` を送ると明示指定が消える）
- **キー名の共通化**: デスクトップの `config/default.toml` と同じ意味の項目は **同じキー名**（`conversion.live_conversion` など）を使い、`config.toml` に書く。iOS 固有の項目は別グループ（`flick.*` `keyboard.*` `candbar.*` `privacy.*` `data.*` `diag.*` `exp.*` `ios.*`）とし、`config.toml` には書かない
- **安全側の既定**: 入力を失わない / キー入力を待たせない / メモリ上限を超えない（設計書 §4.14、`mobile-design.md` §4.3・§5.2）
- **未指定の扱い**: プロファイルが決める項目（`conversion.beam_width` `conversion.model` `conversion.max_context_length`）は「自動」を選択肢に持ち、自動のときは `config.toml` から項目を消す
- モード列の値: `S` = シンプル + 詳細に表示 / `D` = 詳細のみ表示 / `-` = UI に出さない（内部・実験向け。`exp.show_hidden` で表示）

### 表の凡例

- **反映先**
  - `core:<key>` … コアの `config.toml` のキー。拡張側に反映する経路は §3
  - `kbd` … キーボード UI のみ（拡張が App Group の設定ファイルを読む。コアは関与しない）
  - `app` … ホストアプリのみ
  - `kbd+core` … 両方に効く（ホストアプリが両方を書く）
- **状況**
  - `既存(GUI)` … コアの設定記述子（`core/config/src/settings.rs` の `definitions()`）に載っており、`get_settings` / `update_settings` でそのまま編集できる
  - `既存(toml)` … `Config` に実装済みだが記述子に未掲載。記述子への追加が必要（コア側の小変更）
  - `新規` … 新しく実装する。`新規(core)` はコアの変更を伴い、`新規(iOS)` はフロントのみ
- 既定値の「自動」は、プロファイルまたは実行環境が決めることを指す

## 2. 項目表

### 2.1 入力（`input` / `flick` / `touch`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `input.mode` | 入力方式 | choice: `flick`（12 キー）/ `qwerty_kana`（ローマ字）/ `qwerty_alpha` | `flick` | S | kbd | 新規(iOS) | 起動時に開く既定レイアウト。実際の割当は `keyboard.layout_*` |
| `input.toggle_input` | トグル入力（同じキーの連続タップで次の文字） | bool | false | S | kbd+core | 既存(iOS) | 現状 `KeyboardModel` のコンストラクタ引数 `toggleInput`。タップを `toggle` ジェスチャでコアへ送る。false のときタップは先頭の文字 |
| `input.toggle_timeout_ms` | トグル確定までの時間 | int 200–3000, step 50 | 1000 | D | kbd+core | iOS 側で完結（既存(iOS)） | コアには時間の概念が無く、キーボード拡張が判定する。設定画面には未登録で、`ios-settings.json` に値があればそれを使う（無ければ 1000ms。`IOSSettings.swift`）。コアの項目にはしない |
| `input.compose_numbers` | 数字・記号のキーも入力中の文字列（コアの composing）に入れる | bool | true | D | kbd | 実装済み(iOS)・実機未確認（H-53） | かなから数字・記号のレイアウトへ切り替えた状態、または入力中のときだけ、`insert_reading` でコアへ送る（Literal。Space で電卓・全角・記号の候補）。何も選ばず確定すれば半角のまま。英字のレイアウトは対象外（直接入力）。パスワード欄は常に直接入力。false なら従来どおり直接入力。iOS 側で完結し、コアの設定にはしない |
| `input.keep_preedit_on_symbols` | かな ⇄ 数字・記号の切替で入力中の文字列を保つ | bool | true | D | kbd | 実装済み(iOS)・実機未確認（H-56） | オフなら切替時に確定（従来）。`input.compose_numbers` がオフのときも確定（切替先で続けられないため）。パスワード欄は常に確定。`1+2` のような式を分断しない |
| `input.keep_preedit_on_english` | かな ⇄ 英字の切替で入力中の文字列を保つ | bool | false | D | kbd | 実装済み(iOS)・実機未確認（H-56） | オンなら切替時に確定せず、英字のキー（Shift 含む）もコアへ送って同じ composing に続ける（Literal）。入力中の文字列が無いときは従来どおり直接入力。パスワード欄は常に従来動作 |
| `input.dakuten_cycle` | 濁点・小書き循環キーの順序 | choice: `small_first`（つ→っ→づ）/ `dakuten_first`（つ→づ→っ） | `small_first` | D | core:input.dakuten_cycle | 既存(GUI)（platforms=ios） | 従来の固定表の動作を既定にするため既定値と名前を変更（違いは「つ」「う」のみ）。`get_key_table` の `variants` にも反映 |
| `input.dakuten_key_enabled` | 濁点キーを出す | bool | true | D | kbd | 新規(iOS) | OFF にすると、濁点はフリック・長押しのみ |
| `input.shift_letter` | Shift + 英字の動作 | choice: `literal_continue` / `single_upper` | `literal_continue` | D | core:input.shift_letter | 既存(GUI) | QWERTY のみ有効 |
| `input.romaji_carryover` | 消したローマ字を引き継ぐ | bool | true | D | core:input.romaji_carryover | 実装済み(core)（platforms=both） | QWERTY（ローマ字）入力のみ有効。Backspace で残った未変換のローマ字を保留に戻す（H-23） |
| `input.fuzzy.enabled` | タイプミスの補正候補（曖昧入力） | bool | false | S | core:input.fuzzy.enabled | 実装済み(core)（platforms=both。simple）。iOS は ProductSession の生成 config で `[input.fuzzy] enabled = true` を書く（H-25。Swift 側も実装済み: `Shared/ProductSession.swift`） | キー配置から考えられる打ち間違いを「もしかして」候補として変換候補に混ぜる（出所 `fuzzy`）。入力中の読みは打った通りのまま（自動置換しない）。M1 は隣接キー + 辞書のみ（H-24）。QWERTY（ローマ字）は隣接キー・打ち忘れ・余計な打鍵・転置、12 キーは濁点・小書きの付け忘れと同じ行内の隣の文字。Swift 側の追従は設定 UI の汎用フォームのみ（出所 `fuzzy` の候補は既存の候補バーにそのまま並ぶ） |
| `input.fuzzy.strength` | 補正の強さ | choice: `gentle` / `normal` / `aggressive` | `normal` | S | core:input.fuzzy.strength | 実装済み(core)（platforms=both。simple） | 累積コスト上限 3.0 / 4.5 / 6.5。強めは探索が 2ms で打ち切られやすく、評価では正解率が下がった（H-24） |
| `input.fuzzy.max_candidates` | 補正候補の最大数 | int 1–5, step 1 | 3 | D | core:input.fuzzy.max_candidates | 実装済み(core)（platforms=both） | 候補に混ぜる補正候補の上限 |
| `input.fuzzy.kinds` | 補正する打ち間違いの種類 | text（カンマ区切り） | `substitute,delete,insert,transpose,voicing` | D | core:input.fuzzy.kinds | 実装済み(core)（platforms=both） | 隣接キー置換 / 打ち忘れ / 余計な打鍵 / 打ち順の入れ替え / 濁点・小書き。汎用フォームでは自由入力のテキスト |
| `input.fuzzy.use_touch_coords` | タッチ位置を補正に使う | bool | true | D | core:input.fuzzy.use_touch_coords | 実装済み(core)（platforms=ios）。Swift 側の座標送信も実装済み（`insert_reading` の touch / flick、`set_layout_geometry`。H-25） | QWERTY のキー内のタッチ位置・12 キーのフリック角度を補正コストに使う（H-25）。Swift は `insert_reading` に `touch` / `flick`（`InsertReadingTouchParams`）を足し、`set_layout_geometry` でキー矩形を送る |
| `input.punctuation` | 句読点の種類 | choice: `、。` / `，．` / `，。` | `、。` | S | core:input.punctuation | 既存(GUI) | 12 キーの「、。?!」キーの先頭にも反映 |
| `input.auto_commit_ms` | 一定時間入力が無ければ自動確定 | int 0–10000（0 = 無効）, step 500 | 0 | D | kbd+core | 新規(core) | 入力を失わない原則との兼ね合いで既定は無効 |
| `input.auto_space_alpha` | 英単語の後に自動でスペース | bool | false | D | core:input.auto_space_alpha | 新規(core) | 英字入力の予測（backlog 3 章）と同時に検討 |
| `input.auto_capitalize` | 文頭を自動で大文字（英字） | bool | true | D | kbd | 新規(iOS) | ホストの `autocapitalizationType` に従う。ホストが無効を指定した場合はそちらを優先 |
| `input.double_space_period` | スペース 2 回で句点 | bool | false | D | kbd | 新規(iOS) | OS 標準の挙動と揃える項目 |
| `input.auto_close_bracket` | 括弧の自動閉じ | bool | false | D | kbd | 新規(iOS) | 閉じ括弧の間にカーソルを置く |
| `input.long_vowel_assist` | 「ー」「っ」の補助（連続入力） | bool | false | D | core:input.long_vowel_assist | 新規(core) | backlog 3 章。精度検証後に昇格 |
| `flick.sensitivity` | フリック感度（判定距離の倍率） | float 0.5–2.0, step 0.1 | 1.0 | S | kbd | 既存(iOS) | 値が大きいほど短い移動で反応。`flick.threshold_pt` を倍率で調整 |
| `flick.threshold_pt` | フリック判定距離 | float 4–40 pt, step 1 | 14 | D | kbd | 既存(iOS) | キー幅に対する比ではなく絶対距離。小さい端末は自動で縮める |
| `flick.popup` | フリック方向のポップアップ表示 | bool | true | S | kbd | 既存(iOS) | OFF は視認性が下がる代わりに描画が軽い |
| `flick.popup_size` | ポップアップの大きさ | choice: `small` / `medium` / `large` | `medium` | D | kbd | 新規(iOS) | |
| `flick.diagonal` | 斜めフリックを認識 | bool | false | D | kbd | 新規(iOS) | 4 方向 + 中央の仕様（`ios-keyboard-layout.md`）と整合。ON で 8 方向を最寄りの 4 方向に丸める |
| `flick.long_press_ms` | 長押しと判定する時間 | int 150–1000, step 50 | 350 | D | kbd | 既存(iOS・保存のみ) | 長押しポップアップ（レイアウト仕様 `longPress`）の起点 |
| `flick.cancel_on_return` | 元の位置に戻したらキャンセル | bool | true | D | kbd | 新規(iOS) | |
| `touch.tap_slop_pt` | タップと判定する許容移動量 | float 2–20 pt | 8 | D | kbd | 新規(iOS) | |
| `touch.key_repeat_ms` | ⌫・カーソル長押し連打の間隔 | int 30–300, step 10 | 80 | D | kbd | 既存(iOS) | 初回遅延は `touch.key_repeat_delay_ms` |
| `touch.key_repeat_delay_ms` | 長押し連打を始めるまでの時間 | int 200–1000, step 50 | 400 | D | kbd | 既存(iOS) | |
| `touch.space_swipe` | スペースキーのスワイプでカーソル移動 | bool | true | D | kbd | 実装済み(iOS)・確認済み（2026-10-22） | スペースを押したまま左右に動かすと移動モード（離してもスペースは入らない。タップは通常のスペース）。入力中は文節/カーソル移動の Action、未入力はホストの `adjustTextPosition`。旧 ID `touch.space_swipe_cursor` |
| `touch.space_swipe_step_pt` | カーソル移動 1 文字あたりの距離 | float 4–40 pt, step 1 | 14 | D | kbd | 実装済み(iOS)・確認済み（2026-10-22） | ドラッグ距離に比例。最初の 1 文字もこの距離で入る |
| `touch.backspace_swipe` | ⌫の左スワイプで単語単位の削除 | bool | false | D | kbd | 実装済み(iOS)・確認済み（2026-10-22） | 単語境界は文字種の連なり（漢字/ひらがな/カタカナ/英数）でフロントが判定（コアは関与しない暫定）。入力中は無効。押した瞬間の 1 文字削除は先に済む。旧 ID `touch.backspace_swipe_word` |
| `touch.arrow_flick_cursor` | 左右フリックでカーソル移動 | bool | true | D | kbd | 新規(iOS) | 12 キーの「←」「→」キー |

### 2.2 変換（`conversion` / `profile` / `predict` / `ranking`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `conversion.live_conversion` | ライブ変換 | bool | true（iOS の実効も true。生成 config に指定なし＝コア既定） | S | core:conversion.live_conversion | 既存(GUI) | デスクトップ・iOS とも既定は true。iOS の生成 config（`ProductSession.swift`）は `live_conversion` を書いていないため、実効はコア既定の true。**iOS の既定を false にする場合は、生成 config に `live_conversion = false` を明示する必要がある**（電池・発熱の計測に基づく判断は未決。§5） |
| `conversion.live_conversion_delay_ms` | ライブ変換の待ち時間 | int 0–5000, step 10 | 228 | D | core:conversion.live_conversion_delay_ms | 既存(GUI) | |
| `conversion.live_conversion_min_key_length` | ライブ変換を始める最小の読みの長さ | int 1–20 | 2 | D | core:conversion.live_conversion_min_key_length | 既存(GUI) | |
| `conversion.composing_chunk_len` | ライブ変換のチャンク長 | int 1–200 | 30 | D | core:conversion.composing_chunk_len | 既存(GUI) | 小さいほど軽いが文脈が切れる |
| `conversion.initial_view` | 変換開始時の表示 | choice: `sentence` / `segment` | `sentence` | D | core:conversion.initial_view | 既存(GUI) | |
| `conversion.num_candidates` | 展開した候補一覧の 1 ページの候補数 | int 1–50 | 9 | D | core:conversion.num_candidates | 既存(GUI) | 候補バーに出す件数は `candbar.visible_count` |
| `conversion.use_llm` | LLM による変換 | bool | true（LLM が使える端末のみ。使えなければ自動で false 相当） | S | core:conversion.use_llm | 既存(GUI) | メモリ上限・低電力モードで自動降格（`profile.auto_switch`）。OFF で辞書のみ |
| `conversion.model` | 変換モデル | choice: `auto` / 取得済みモデル ID | `auto` | D | core:conversion.model | 既存(GUI) | 次回起動時に反映（コアの仕様）。拡張の再起動が要る旨を UI に表示。選択肢は `data.models` から生成 |
| `conversion.beam_width` | ビーム幅 | int 1–16 または自動 | 自動 | D | core:conversion.beam_width | 既存(GUI) | 大きいほど遅く、メモリも増える |
| `conversion.n_threads` | 推論スレッド数 | int 0–8（0 = 自動） | 0 | D | core:conversion.n_threads | 既存(GUI) | 次回起動時に反映。拡張の発熱に影響するため上限は端末のコア数 |
| `conversion.sync_budget_ms` | Space 変換の待ち時間（LLM の同期待ち） | int 10–5000, step 10 | 150（デスクトップ）。iOS の生成 config は 30 | D | core:conversion.sync_budget_ms | 既存(GUI) | 超過するとひらがなのまま応答し、結果は後で差し替え。iOS は生成 config（`ProductSession.swift`）で 30ms（H-10）。デスクトップの既定 150 とは別の値 |
| `conversion.max_latency_ms` | 推論の最大レイテンシ（タイムアウト） | int 10–10000 | 100 | - | core:conversion.max_latency_ms | 既存(GUI) | 通常は触らせない |
| `conversion.use_context` | 前後の文脈を使う | bool | true | D | core:conversion.use_context | 既存(GUI) | ホストの文字列を読むため、パスワード欄では自動で無効。プライバシー注記を UI に出す |
| `conversion.max_context_length` | 文脈の最大文字数 | int 1–200 または自動 | 自動 | D | core:conversion.max_context_length | 既存(GUI) | 小さいほどメモリ・時間が減る |
| `profile.mode` | プロファイル | choice: `auto` / `quality` / `balanced` / `light` / `minimal` | `auto` | S | core:profile.mode | 既存(GUI) | シンプルモードでは「自動 / 軽量 / 辞書のみ」に絞って表示。iOS の `auto` は `light` から開始し、実測に応じて昇格（`mobile-design.md` §4.3） |
| `profile.auto_switch` | 環境に応じて自動で切り替え | bool | true | D | core:profile.auto_switch | 既存(GUI) | 低電力モード・温度・メモリ警告での降格。OFF にするとメモリ警告時も昇降格しない点に注意（拡張が落ちる恐れ。警告を出す） |
| `profile.persona` | ペルソナ | text, 200 文字以内 | 空 | D | core:profile.persona | 既存(GUI) | スタイル変換（将来）で使用。端末内のみ |
| `ios.memory_limit_mb` | 拡張のメモリ上限の上書き | int 0–2048（0 = 端末の実測値を使う） | 0 | - | kbd | 新規(iOS) | `nagi_server_new_with_budget` の `memory_budget_mb` に渡す。通常は `os_proc_available_memory()` の実測。実験用 |
| `ios.llm_min_free_mb` | LLM を読み込む最小の空きメモリ | int 100–1500 | 実測で決定 | - | kbd | 新規(iOS) | これを下回ると辞書のみで起動。安全側のしきい値 |
| `predict.enabled` | 予測変換 | bool | true | S | core:predict.enabled | 既存(GUI) | 学習キャッシュ・ユーザー辞書の前方一致。LLM による次語予測は別（`predict.next_word`） |
| `predict.min_chars` | 予測を始める文字数 | int 1–20 | 1 | D | core:predict.min_chars | 既存(GUI) | |
| `predict.next_word` | 確定直後の次語予測（学習した直前の語から） | bool | false | D | core:predict.next_word | 既存(GUI) | LLM は回さず、学習した「直前の確定語 → 次の確定語」のバイグラム（`learning-next.tsv`）の上位を返す軽量版。学習が有効なときだけ記録・表示する。LLM 版は将来 `ConversionBackend::predict_next` の裏に追加 |
| `predict.next_word_count` | 次語予測の候補数 | int 1–10 | 5 | D | core:predict.next_word_count | 既存(GUI) | 確定直後の候補バーに出す最大数 |
| `ranking.source_order` | 候補の出所の優先順 | 配列（`learned` / `user_dict` / `llm` / `system_dict` / `kana` / `rewriter` の並べ替え） | デスクトップと同じ | D | core:ranking.source_order | 既存(toml) | 並べ替え UI が必要。記述子の型に「順序つき配列」が無いので追加が要る |
| `convert.half_width_alnum` | 数字・英字を半角で確定 | choice: `half` / `full` / `follow_input` | `follow_input` | D | core:convert.half_width_alnum | 新規(core) | 全角/半角の相互変換（backlog 3 章）の基本設定 |
| `convert.date_time` | 日付・時刻の候補（「きょう」「いま」） | bool | true | D | core:convert.date_time | 新規(core) | Rewriter の機能として実装 |
| `convert.calculator` | 電卓候補（「1+2=」） | bool | true | D | core:convert.calculator | 新規(core) | |
| `rewriter.emoji` | 読みからの絵文字候補（「わらう」→😄） | bool | true | D | core:rewriter.emoji | 実装済み(core) | Rewriter の機能。表は `config/emoji.tsv`（自作・バイナリ同梱）。候補の末尾側（出所 `rewriter`）。ユーザー辞書・変換結果が先。入力中の候補は先頭文節の読みが表に完全一致するときだけ |
| `rewriter.kaomoji` | 読みからの顔文字候補（「えがお」→(^_^)） | bool | true | D | core:rewriter.kaomoji | 実装済み(core) | 同上。旧 ID `convert.emoji_candidates` を 2 項目に分けた |
| `convert.emoji_skin_tone` | 絵文字の肌色 | choice: `none` / `1`–`5` | `none` | D | kbd | 新規(iOS) | 絵文字キーボードと候補に適用 |

### 2.3 学習（`learning`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `learning.enabled` | 学習する | bool | true | S | core:learning.enabled | 既存(GUI) | OFF にしても既存の学習データは残る（削除は別操作） |
| `learning.max_entries` | 学習データの最大件数 | int 100–1,000,000 | iOS: 10000（暫定） | D | core:learning.max_entries | 既存(GUI) | ファイルサイズ・読み込みメモリに直結。iOS では上限を低めに検討 |
| `learning.strength` | 学習の強さ（確定を順位へ反映する度合い） | float 0.0–2.0 | `1.0` | D | core:learning.strength | 既存(GUI) | コアの記述子（`core/config/src/settings.rs`）に掲載済み。範囲外は検証で拒否される。weak / normal / strong の 3 段階案は取り下げた |
| `learning.forget_days` | 未使用の学習を忘れるまでの日数 | int 0–3650（0 = 忘れない） | 0 | D | core:learning.forget_days | 新規(core) | 忘却速度（backlog 3 章） |
| `learning.disabled_apps` | 学習を無効にするアプリ | 文字列配列（bundle id） | パスワードマネージャの標準リスト | - | core:learning.disabled_apps | 既存(toml) | **iOS ではキーボード拡張からホストアプリの bundle id を取れない**ため機能しない。代わりにキーのタイプ（下記 `privacy.secure_field_off`）で判定する |
| `learning.delete_candidate` | 候補の長押しで学習から削除 | bool | true | D | kbd+core | 新規(iOS) | コアの `DeleteLearning` アクション相当を候補バーから呼ぶ |
| `learning.clear` | 学習データの全削除 | 操作（確認ダイアログ付き） | - | S | app→core | 既存(iOS) | コアの `clear_learning` を呼ぶ。拡張が動いていない場合はホストアプリが `learning.tsv` を直接削除する経路が必要 |
| `learning.export` | 学習データの書き出し | 操作 | - | D | app | 新規(iOS) | TSV を共有シートへ。内容は入力履歴そのものなので、共有先の選択に注意を出す |

### 2.4 ユーザー辞書・スニペット（`userdict`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `userdict.enabled` | ユーザー辞書を使う | bool | true | D | core:userdict.enabled | 既存(GUI) | 語を変換・予測に使うかどうか。登録内容と件数表示は保持 |
| `userdict.entries` | 単語の登録・編集・削除 | 操作（読み・語・品詞・コメント） | - | S | app→core（`user_dict/` のファイル） | 既存(core) | ファイルは Mozc TSV 互換。ホストアプリが App Group の `user_dict/` に書く |
| `userdict.import` | 取り込み（Mozc TSV） | 操作 | - | D | app | 既存(core) | 形式エラーは行番号つきで表示 |
| `userdict.export` | 書き出し（Mozc TSV） | 操作 | - | D | app | 既存(core) | |
| `userdict.register_from_selection` | 選択テキストから登録（共有シート / ショートカット） | 操作 | - | D | app | 新規(iOS) | 共有拡張またはショートカットアプリ連携。段階 3 以降 |
| `snippet.enabled` | スニペット（短い読みで定型文） | bool | true | D | core:snippet.enabled | 既存(GUI) | 候補の出所は `user_dict` の一種として扱う |
| `snippet.entries` | スニペットの登録・編集 | 操作（読み・本文・改行可） | - | D | app→core | 新規(core) | 本文は 256 文字以内（レイアウト仕様の `shortcut` と同じ上限を暫定採用） |
| `userdict.max_entries` | 登録の上限 | int 100–100000 | 10000 | - | core:userdict.max_entries | 新規(core) | 読み込みメモリの保護 |

### 2.5 キーボード（`keyboard` / `candbar` / `ios`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `keyboard.layout_kana` | かなモードのレイアウト | choice: 組み込み（`flick-kana` / `qwerty-kana`）+ ユーザー定義の id | `flick-kana` | S | kbd | 既存(iOS) | レイアウト仕様 `ios-keyboard-layout.md`。不正なレイアウトは既定に戻す |
| `keyboard.layout_alpha` | 英字モードのレイアウト | choice: `flick-alpha` / `qwerty-alpha` / ユーザー定義 | `qwerty-alpha` | S | kbd | 既存(iOS) | |
| `keyboard.layout_number` | 数字モードのレイアウト | choice: `numbers` / ユーザー定義 | `numbers` | D | kbd | 新規(iOS) | |
| `keyboard.layout_symbol` | 記号モードのレイアウト | choice: `symbols` / ユーザー定義 | `symbols` | D | kbd | 新規(iOS) | |
| `keyboard.layout_emoji` | 絵文字モードのレイアウト | choice: `emoji` / ユーザー定義 | `emoji` | D | kbd | 新規(iOS) | |
| `keyboard.layout_by_field` | 入力欄の種類で自動切替（URL・メール・数字） | bool | true | D | kbd | 実装済み(iOS)・確認済み（ビルド 39・40。2026-10-16〜17、H-34） | `keyboardType` に従う。数字パッド欄では数字配列。自動切替は `Keyboard/InputTraits.swift`（メール・URL・`asciiCapable` は英字、数字系は `numbers`、指定のない欄に移ると自動で切り替えていた場合だけかなへ戻す）。改行キーの表示も `returnKeyType` に合わせる。OFF ではレイアウトを自動で選ばず、改行キーもレイアウトのラベルのまま（OFF にする前に自動で切り替えていた場合は、次の focusIn・入力欄の切替でかなへ戻す。`KeyboardModel.applyInputTraits()`） |
| `keyboard.remember_last_mode` | 最後に使ったモードで起動 | bool | true | D | kbd | 新規(iOS) | OFF の場合は `input.mode` で起動 |
| `keyboard.height` | キーボードの高さ | choice: `compact` / `standard` / `tall` | `standard` | S | kbd | 既存(iOS) | 内部的には下記の倍率 |
| `keyboard.height_scale` | 高さの倍率 | float 0.8–1.3, step 0.05 | 1.0 | D | kbd | 既存(iOS) | 縦・横画面で別々に持つ（`keyboard.height_scale_landscape`） |
| `keyboard.height_scale_landscape` | 横画面の高さの倍率 | float 0.7–1.2, step 0.05 | 1.0 | D | kbd | 新規(iOS) | |
| `keyboard.one_handed` | 片手モード | choice: `off` / `left` / `right` | `off` | S | kbd | 既存(iOS) | キーボードを寄せる。切替ボタンをキー上に出す |
| `keyboard.one_handed_width` | 片手モードのときの幅の比 | float 0.6–0.9, step 0.05 | 0.8 | D | kbd | 既存(iOS) | |
| `keyboard.key_spacing_pt` | キーの余白 | float 0–8 pt, step 0.5 | 4 | D | kbd | 既存(iOS) | |
| `keyboard.key_corner_pt` | キーの角丸 | float 0–16 pt, step 1 | 6 | D | kbd | 既存(iOS) | |
| `keyboard.key_label_size` | キー文字の大きさ | float 0.8–1.4（倍率） | 1.0 | D | kbd | 新規(iOS) | Dynamic Type との関係は `a11y.*` で整理 |
| `keyboard.show_globe` | 地球儀キーを出す | bool | 自動（OS の `needsInputModeSwitchKey`） | D | kbd | 新規(iOS) | 他のキーボードへ切り替えられない端末では常に出す |
| `keyboard.haptic` | キー入力の触覚 | choice: `off` / `light` / `medium` / `heavy` | `light` | S | kbd | 既存(iOS)・確認済み（2026-10-22） | 触覚の発生にフルアクセスが要るかは、`ios-device-checklist.md` §2 設定タブの確認項目（「フルアクセスなし・ありの両方で振動するか」。2026-10-22 問題なし）。結論（要否）は文書に記録がない（`UIFeedbackGenerator` は拡張では制限あり） |
| `keyboard.sound` | キー入力の音 | choice: `off` / `compat`（表示名「クリック音」） | `compat` | S | kbd | 既存(iOS)・確認済み（2026-10-22） | `compat` はシステムのキーボードのクリック音（`AudioServicesPlaySystemSound(1104)`）を直接鳴らす（端末の「キーボードのクリック」設定には従わず、消音スイッチに従う）。以前あった `system`（`UIDevice.playInputClick`。端末の「キーボードのクリック」設定に従う）は実機で鳴らなかったので廃止した。保存済みの `system` は読むときに既定の `compat` に丸める。独自音源は持たない。**既定は `compat`**（実装 `IOSSettings.swift` の `defaultValue`） |
| `keyboard.haptic_engine` | 触覚の方式（互換モード） | choice: `standard` / `compat` | `standard` | D | kbd | 実装済み(iOS・要実機確認) | `standard` は `UIImpactFeedbackGenerator`、`compat` は「標準」が働かない端末向けで、システムサウンド（Peek など）の振動で知らせる（`AudioServicesPlaySystemSound`）。`keyboard.haptic` が `off` のときは働かない |
| `keyboard.commit_method` | 確定の方式（診断用） | choice: `insert` / `mark` / `delete` | `insert` | D | kbd | 実装済み(iOS)・確認済み（2026-10-22。既定 `insert` は H-31 でも確認。`mark` / `delete` の確認記録は文書にない） | 変換中の文字列がある状態での確定の手順（`HostCompositionApplier.CommitMethod`）。`insert` は `insertText` だけで変換中の範囲を置き換える（失敗しても手前の本文を消さない）。`mark` は確定文字列を `setMarkedText` してから `unmarkText`。`delete` は旧方式（`unmarkText` → `deleteBackward`×N → `insertText`。実機で手前の本文を消すことがあった）。不具合の切り分け用 |
| `keyboard.sound_volume` | 音量 | float 0–1 | 0.5 | - | kbd | 新規(iOS) | システムサウンド（`compat`）は OS が音量を決めるため、独自音源を持つまでは不要 |
| `keyboard.theme` | テーマ | choice: `system` / `light` / `dark`（ユーザー定義は未実装） | `system` | S | kbd | 実装済み(iOS)・確認済み（2026-10-22） | 拡張のビューの `overrideUserInterfaceStyle` で上書き |
| `keyboard.accent` | アクセントカラー | choice: `blue` / `green` / `orange` / `pink` / `gray` | `blue` | S | kbd | 実装済み(iOS)・確認済み（2026-10-22） | 確定キー・選択中の候補・シフト等の色。任意色（sRGB 16 進）は未実装。旧 ID `keyboard.accent_color` |
| `keyboard.key_color_set` | キーの配色 | choice: `standard` / `high_contrast` / `custom` | `standard` | D | kbd | 新規(iOS) | `custom` 時は色ごとの指定を JSON で持つ |
| `keyboard.background_image` | 背景画像 | 画像（ホストアプリで選択） | なし | D | kbd | 新規(iOS) | **拡張のメモリを消費する**ため縮小して保存（長辺 1024px 以内）し、拡張では必要時のみ読む |
| `keyboard.show_key_preview` | 押したキーのプレビュー（QWERTY） | bool | true | D | kbd | 新規(iOS) | |
| `keyboard.capslock_double_tap` | シフト 2 回でキャップスロック | bool | true | D | kbd | 新規(iOS) | |
| `keyboard.number_row` | QWERTY に数字行を常時表示 | bool | false | D | kbd | 新規(iOS) | |
| `keyboard.long_press_symbols` | QWERTY の長押しで記号 | bool | true | D | kbd | 新規(iOS) | |
| `keyboard.custom_layouts` | カスタムレイアウトの管理（取り込み・削除・割り当て） | 操作 | - | D | app | 既存(iOS) | App Group の `keyboard/layouts/<id>.json`。仕様は `ios-keyboard-layout.md`。並べ替えは未実装（`LayoutManagerView.swift` に無い） |
| `keyboard.keymap_preset` | キー操作プリセット | choice: `default` | `default` | - | core:keymap.preset | 既存(toml) | タッチ UI ではボタンと Action の対応に使う。現時点では選択肢が 1 つなので UI に出さない |
| `candbar.visible_count` | 候補バーに並べる候補数 | int 3–20 | 9 | D | kbd+core | 新規(iOS) | コアに要求する候補数でもある（`conversion.num_candidates` とは別に持つか、同じにするかは要決定） |
| `candbar.expand_button` | 展開ボタン（全候補一覧）を出す | bool | true | D | kbd | 新規(iOS) | |
| `candbar.expand_columns` | 展開時の列数 | choice: `auto` / `3` / `4` / `5` | `auto` | D | kbd | 新規(iOS) | |
| `candbar.font_size` | 候補の文字の大きさ | float 0.8–1.4（倍率） | 1.0 | D | kbd | 新規(iOS) | |
| `candbar.show_source_icon` | 候補の出所アイコン（学習 / ユーザー辞書 / LLM 等） | bool | false | D | kbd | 新規(iOS) | 出所はプロトコル上の enum（アイコンはフロント割当） |
| `candbar.show_preedit` | 候補バー左に入力中の文字列を表示 | bool | true | D | kbd | 新規(iOS) | 現状は常時表示。ホストが marked text を描かない場面の備え |
| `candbar.show_segments` | 文節列（注目文節の表示）を出す | bool | true | D | kbd | 新規(iOS) | iOS は marked text に属性を付けられないため主手段（`mobile-design.md` §5.1）。OFF は非推奨 |
| `candbar.auto_hide` | 入力が無いときの候補バーの扱い | choice: `always` / `hide_when_empty` | `always` | D | kbd | 新規(iOS) | 非表示にすると高さが変わるため既定は常時表示 |
| `candbar.tap_commit` | 候補のタップで即確定 | bool | true | D | kbd | 新規(iOS) | OFF なら 1 回目で選択、2 回目で確定 |
| `candbar.paste_button` | 「貼り付け」ボタンを候補バーの左端に出す | bool | true | D | kbd | 実装済み(iOS)・確認済み（2026-10-22） | 表示判定は `UIPasteboard.hasStrings` のみ（中身は読まない）、入力中は非表示。中身はタップ時だけ読む（iOS 16 の貼り付け許可が出うる）。旧 ID `candbar.clipboard_paste`（既定は旧 false → true）。`privacy.clipboard_access` とは独立 |

### 2.6 プライバシー・フルアクセス（`privacy`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `privacy.secure_field_off` | パスワード欄・機密欄で学習・文脈取得・ログを停止 | bool | true | S | kbd+core | 部分実装（固定 ON。設定項目は未実装） | `isSecureTextEntry` と `textContentType`（パスワード・ワンタイムコード）で判定し、学習・文脈取得（`set_context`）を止める（`KeyboardViewController.swift` の `hostIsSecure`）。診断ログは押したキーを残さない（`send <secure>`）。設定項目として ON/OFF できるかは §5 の未決事項 |
| `privacy.no_learning_fields` | 特定の入力欄の種類（メール・URL・数字）では学習しない | multi-choice: `email` / `url` / `number` / `phone` | `email`, `url`, `number`, `phone` | D | kbd+core | 新規(iOS) | |
| `privacy.full_access_status` | フルアクセスの状態表示と案内 | 表示（操作: 設定アプリを開く） | - | S | app | 新規(iOS) | 用途の説明（データ参照のみ・外部送信なし）を併記。未許可時は辞書・モデルが読めず縮退する |
| `privacy.no_network` | ネットワークを使わない（更新確認・取得を無効化） | bool | false | D | app+core:update.auto_check | 新規(iOS) | ON でも手動取得は可能。拡張は元々ネットワークを使わない |
| `privacy.stats_local` | 使用統計を端末内に記録 | bool | false | D | app | 新規(iOS) | 打鍵数・変換精度の推移。**送信しない**。記録内容に入力文字列を含めない（件数・時間のみ） |
| `privacy.stats_clear` | 使用統計の削除 | 操作 | - | D | app | 新規(iOS) | |
| `privacy.clear_all` | すべてのデータを削除（学習・ユーザー辞書・統計・設定） | 操作（二重確認） | - | D | app | 新規(iOS) | 辞書・モデルは別（`data.delete_*`）。学習データの全削除はワンコマンドで行える原則（CLAUDE.md） |
| `privacy.clipboard_access` | クリップボードを読む機能を許可 | bool | false | D | kbd | 新規(iOS) | `candbar.clipboard_paste` と履歴タブの前提 |

### 2.7 データ（辞書・モデルの取得・更新）（`data` / `update`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `data.dict_status` | システム辞書の状態表示（版・サイズ・取得日） | 表示 | - | S | app | 新規(iOS) | 保存先は App Group の `dict/`（`DataPaths`） |
| `data.dict_fetch` | 辞書の取得・更新 | 操作 | - | S | app | 取り込み（取得は手動） | ホストアプリがバンドル同梱の辞書を App Group にコピーする（取り込み）。取得（ダウンロード）は手動。現状の iOS 側に SHA-256 検証は無い（デスクトップの取得タスクは検証あり） |
| `data.models` | 取得済みモデルの一覧 | 表示（名前・量子化・サイズ・メモリ目安） | - | D | app | 新規(iOS) | `conversion.model` の選択肢の元 |
| `data.model_fetch` | モデルの取得・更新 | 操作 | - | S | app | 取り込み（取得は手動） | iOS の既定は small（`DataPaths.swift` の `defaultModelFileName`）。未選択時は端末の予算で自動選択（余裕があれば small）、Picker での明示選択を優先（`ProductSession.swift` の `preferredModelPath`。`decisions.md` H-22(c)）。ライセンス表示を併記 |
| `data.download_wifi_only` | Wi-Fi のみでダウンロード | bool | true | S | app | 新規(iOS) | 容量が大きいためモバイル回線は既定で使わない |
| `data.download_confirm_mb` | この容量以上は確認を出す | int 10–2000 | 100 | D | app | 新規(iOS) | |
| `data.delete_dict` | 辞書の削除 | 操作 | - | D | app | 新規(iOS) | 容量確保用。削除後は読みそのまま返す縮退動作 |
| `data.delete_model` | モデルの削除 | 操作 | - | D | app | 新規(iOS) | |
| `data.storage_usage` | 使用容量の表示 | 表示（辞書・モデル・学習・画像） | - | D | app | 新規(iOS) | |
| `update.auto_check` | 辞書・モデル・アプリの更新を自動確認 | bool | true | S | core:update.auto_check | 既存(GUI) | iOS では「アプリの更新」は App Store に任せ、辞書・モデルの更新確認の意味に読み替える。通知のみでダウンロードはしない |
| `update.check_url` | 更新確認マニフェストの URL | text | 既定 | - | core:update.check_url | 既存(toml) | UI に出さない |
| `data.backup_export` | 設定のバックアップ（JSON 書き出し） | 操作 | - | D | app | 新規(iOS) | 設定のみ。学習データ・ユーザー辞書は別操作。iCloud 同期は対象外（プライバシー方針の確認が済むまで） |
| `data.backup_import` | 設定の復元 | 操作 | - | D | app | 新規(iOS) | 検証に通らない項目は無視し、行（キー）単位で結果を表示 |
| `data.reset_settings` | 設定を初期化 | 操作 | - | S | app | 新規(iOS) | データ（辞書・学習）は残す |

### 2.8 アクセシビリティ（`a11y`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `a11y.voiceover_labels` | VoiceOver 用のラベルを付与 | bool | true（固定で ON でも可） | - | kbd | 新規(iOS) | 項目としては出さず常に有効にする方針 |
| `a11y.dynamic_type` | Dynamic Type に追従 | bool | true | D | kbd | 新規(iOS) | `keyboard.key_label_size` と乗算 |
| `a11y.high_contrast` | ハイコントラスト | choice: `system` / `on` / `off` | `system` | D | kbd | 新規(iOS) | OS の「コントラストを上げる」に追従 |
| `a11y.reduce_motion` | アニメーションを減らす | choice: `system` / `on` / `off` | `system` | D | kbd | 新規(iOS) | |
| `a11y.haptic_notifications` | 状態変化を触覚で知らせる（変換開始・確定失敗など） | bool | false | D | kbd | 新規(iOS) | |

### 2.9 診断（`diag`）

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `diag.status` | 状態表示（プロファイル・LLM の有無・メモリ・辞書の有無） | 表示 | - | S | app | 既存(iOS) | 実機計測画面の簡略版。入力内容は含めない |
| `diag.latency_view` | 応答時間の表示（直近 N 回の p50/p95） | 表示 | - | D | app | 既存(iOS) | 現状はホストアプリの実機計測（H 節）。常用の画面に整理する |
| `diag.memory_view` | 拡張のメモリ使用量の表示 | 表示 | - | D | app | 既存(iOS) | `MemoryProbe` 相当 |
| `diag.log_enabled` | 診断ログを記録 | bool | TestFlight・開発用ビルドは true、App Store 配布は false | D | kbd+core | 既存(iOS) | TestFlight 配布期間中は既定 ON で入力内容を含める（パスワード欄は伏せる）。App Store 配布版は既定 OFF・内容を含めない（`decisions.md` H-22(a)、CLAUDE.md の例外。判定は `DiagLog.isTestBuild`） |
| `diag.log_level` | ログのレベル | choice: `error` / `warn` / `info` / `debug` | `warn` | D | kbd+core | 新規(iOS) | `debug` はデバッグビルドのみ |
| `diag.log_export` | ログの書き出し | 操作 | - | D | app | 新規(iOS) | 共有シート。書き出し前に内容の種類を表示 |
| `diag.log_clear` | ログの削除 | 操作 | - | D | app | 新規(iOS) | |
| `diag.log_max_kb` | ログの最大サイズ | int 64–4096 | 64 | - | kbd | 新規(iOS) | 設定項目は未実装。現状は固定で、64KB を超えたら後半 32KB を残して切り詰める（`DiagLog.swift` の `maxBytes` / `keepBytes`） |
| `diag.report_crash_flag` | 前回の異常終了（メモリ超過など）を検出して案内 | bool | true | D | kbd+app | 新規(iOS) | 拡張が起動中フラグを書き、正常終了で消す方式。検出時は `profile` の降格を提案 |

### 2.10 実験的機能（`exp`）

実験的機能は既定 OFF で、詳細モードの末尾にまとめる。未完成の機能は `exp.show_hidden` が ON のときだけ並べる。

| ID | 項目 | 型・範囲 | 既定 | モード | 反映先 | 状況 | 備考 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `exp.show_hidden` | 隠し項目（モード列 `-`）を表示 | bool | false | D | app | 新規(iOS) | 誤操作防止のため確認ダイアログ付き |
| `exp.style_convert` | スタイル変換（丁寧語・口語・要約・誤字修正） | bool | false | D | core:exp.style_convert | 新規(core) | LLM。デスクトップの Phase 4 と共通。メモリ・時間の制約が大きく、実機で可否を確認してから |
| `exp.reconvert` | 確定済みテキストの再変換 | bool | false | D | core | 新規(core) | コアの `reconvert` は未実装（`decisions.md`）。iOS は選択範囲の取得に制約あり |
| `exp.clipboard_history` | クリップボード履歴・定型文タブ | bool | false | D | kbd | 新規(iOS) | `privacy.clipboard_access` が前提。履歴の保存はメモリ上のみ（永続化しない）を既定とする |
| `exp.speculative_decode` | 投機的デコーディング | bool | false | - | core | 新規(core) | デスクトップ Phase 6 |
| `exp.cursor_trackpad` | スペース長押しでトラックパッド化 | bool | false | D | kbd | 新規(iOS) | `touch.space_swipe_cursor` の拡張版（縦移動も含む） |
| `exp.llm_reranker` | 候補の LLM 再採点を強める | bool | false | - | core | 新規(core) | Laya の評価結果次第 |
| `exp.shortcuts_integration` | ショートカットアプリ連携（辞書登録など） | bool | false | D | app | 新規(iOS) | App Intents |

## 3. 保存先と同期

### 3.1 保存先

| 種別 | 保存先 | 書く側 | 読む側 |
| --- | --- | --- | --- |
| コア設定（`core:` の項目） | App Group コンテナの `user/config.toml`（`<App Group>/user/config.toml`。`--user-dir` 直下。デスクトップと同じ形式。`ProductSession.prepareUserDir`） | ホストアプリ（`update_settings` 相当）。拡張が動作中なら拡張内のコアが書く | コア（拡張プロセス内） |
| iOS 固有設定（`kbd` / `app` の項目） | App Group コンテナの `ios-settings.json`（`schema` 番号つき。未知のキーは保持） | ホストアプリ | キーボード拡張、ホストアプリ |
| キーボードレイアウト | App Group の `keyboard/layouts/<id>.json` | ホストアプリ | キーボード拡張 |
| 学習・ユーザー辞書 | App Group の `learning.tsv` / `user_dict/` | コア（拡張内）、ホストアプリ（登録・削除） | コア |
| 背景画像など | App Group の `keyboard/assets/` | ホストアプリ | キーボード拡張 |

- ホストアプリと拡張で同じファイルを同時に書かないようにする。`config.toml` は **コアの `update_settings` 経由に統一**（コメントと未知のキーを保つ原子的な書き込みが実装済み）。拡張が動いていないときはホストアプリが同じ `nagi-config` のロジックで書く（Rust 側の関数を FFI で公開するか、コアを一時的に起動する。要決定）
- `ios-settings.json` は「ホストアプリのみが書く」ことを原則とし、一時的な状態（最後のモードなど）は別ファイル `ios-state.json` に拡張が書く。書き込みは原子的（一時ファイル → rename）
- `UserDefaults(suiteName:)`（App Group）は使わない案を第一候補とする。理由は、`config.toml` と同じくファイルで持つほうが検証・バックアップ・書き出しを共通化できるため。ただし変更通知が必要なら Darwin 通知（下記）で補う
- **実装の現状**: 表示モード（`ui.mode`）・キーボードのレイアウト割り当て・診断ログの ON/OFF・使うモデルの選択は、App Group の UserDefaults に置いている（小さな状態で、ホストと拡張が同じファイルを書かないため）。`ios-settings.json` は `kbd` / `app` の値（`IOSSettings.swift`）に使う。上の「第一候補」の記述は、この実装と食い違っている（方針の見直し対象）

### 3.2 反映の経路

1. ホストアプリで設定を変更 → 検証（範囲・選択肢）→ ファイルへ原子的に書き込み
2. ホストアプリが Darwin 通知（`CFNotificationCenterGetDarwinNotifyCenter`）を発行
3. キーボード拡張が起動中なら通知を受け、(a) `ios-settings.json` を再読み込みして UI に反映、(b) コアへ `reload_config` を送る。拡張が停止中なら次回起動時に読む
4. コアは `reload_config` で `config.toml` を再読み込みする（コアは要求処理時に更新時刻を見て最短 1 秒間隔で自動再読み込みもする）。**反映されるもの**: `input.*` `conversion.num_candidates` `sync_budget_ms` `max_context_length` `use_context` `profile.*` `ranking` `learning.*`。**次回起動時に反映**: `conversion.model` `n_threads`（`decisions.md` の再読み込みの仕様）。後者は、設定画面に「キーボードを開き直すと反映」と表示する
5. 読み込みに失敗した場合は旧設定を維持し、行番号つきの通知を出す（設計書 §6）。ホストアプリは書き込み前に検証済みだが、手編集やバックアップ復元で壊れた場合に備える
6. `get_settings` が壊れた `config.toml` では既定値と `error` を返す仕様のため、ホストアプリは「設定ファイルが壊れています」の表示と、修復（初期化）の選択肢を出す

- シンプル ⇄ 詳細の切替は UI のみの状態で、App Group の UserDefaults（キー `ui.mode`。`SettingsView.swift`）に保存する。`ios-settings.json` には入れない（実装どおり）。切替で設定値は変わらない。詳細モードで変更した値は、シンプルモードに戻しても保持する
- プロファイルによる自動値（自動のとき）と、ユーザーの明示指定（`explicit`）を区別して表示する（コアの `SettingState` が既に持つ）

### 3.3 メモリ・安全面の制約

- 拡張が読む設定は小さいファイルのみ（背景画像を除く）。起動時に同期で読んで 1 ms 程度に収める
- `conversion.use_llm` `conversion.model` `conversion.beam_width` `keyboard.background_image` など、メモリを増やす項目には、変更時に「拡張のメモリ上限に近づく」警告を出す。LLM を読み込む前に、実測の空きメモリ（`os_proc_available_memory`）を見て不足なら辞書のみで起動する
- 設定の不正値は無視して既定にフォールバックする（入力を失わない）

## 4. 実装の進め方（段階）

1. **基盤（コア側の小変更 + 共通部）**
   - `settings.rs` の記述子に既存(toml)の項目を追加: `live_conversion_min_key_length` `model` `n_threads` `max_context_length` `max_latency_ms` `ranking.source_order`（型に「順序つき配列」を追加）
   - 記述子に **モード（simple / detail）とプラットフォーム（desktop / ios）の属性**を追加し、iOS 用の既定値（`live_conversion` など）の上書きを持たせるか、iOS 用の初期 `config.toml` を用意する（要決定）
   - ホストアプリの設定画面の骨組み: `get_settings` の記述子から汎用フォームを描く（macOS の設定画面と同じ方式）。モード切替（`ui.mode`）
   - `ios-settings.json` の読み書きと Darwin 通知による反映
2. **シンプルモード（モード列 S）**: 入力方式・トグル入力・フリック感度・句読点・ライブ変換・LLM の使用・プロファイル・予測・学習の有効/全削除・レイアウト割当・高さ・片手・触覚・テーマ・辞書/モデルの取得・フルアクセスの案内・診断の状態表示。実機（TestFlight）で確認
3. **詳細モード（キーボード UI 側）**: フリック・タッチの細部、キー外観、候補バーの細部、アクセシビリティ、診断ログ
4. **詳細モード（コア側の新規項目）**: `input.dakuten_cycle` `learning.strength` `learning.forget_days` `predict.next_word` `convert.*` `userdict.enabled` `snippet.*`。デスクトップ共通の項目として `config/default.toml` と記述子に追加し、macOS の設定画面にも出す
5. **データ管理**: ユーザー辞書・スニペットの編集、バックアップ/復元、使用統計、使用容量
6. **レイアウト連携**: `keyboard.custom_layouts`（`ios-keyboard-layout.md` の段階 4 と同時）
7. **実験的機能**: 実機で可否を確認したものから `exp.*` に追加し、安定したら昇格

各段階の完了時に、本書の「状況」列を更新する（`新規` → `既存`）。デスクトップとキー名を共有する項目を追加した場合は、`config/default.toml` のコメントと、`defaults_match_config_default_and_default_toml` テストの対象に含める。

## 5. 未決事項

- iOS の `conversion.live_conversion` の既定値（電池・発熱・応答時間の実測後）
- `conversion.num_candidates` と `candbar.visible_count` を分けるか
- ホストアプリが `config.toml` を書く経路（FFI で `nagi-config` の関数を公開 / コアを起動して `update_settings`）
- `privacy.secure_field_off` を設定にするか固定にするか
- `input.toggle_timeout_ms` `touch.backspace_swipe_word` などコア側に概念がない項目の置き場（トグルの確定タイミングの扱い）
- 触覚（`UIFeedbackGenerator`）・クリック音がフルアクセス無しで使えるか（実機確認）
- iCloud によるバックアップ同期の可否（プライバシー方針との整合）
