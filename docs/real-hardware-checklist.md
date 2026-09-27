# 実機（Windows / macOS）テストチェックリスト

このセッション（クラウドのLinuxサンドボックス）で実装した機能は、実データ（Mozc辞書・zenzモデル）も
実OSも無い環境で開発した。GitHub Actions は ubuntu/windows/macos の3OSでビルド・テストしているが、
`--features llama`（実LLM推論）は既定でCIに含めていないため、以下は実機でしか確認できない。

各項目の実装根拠・設計判断は `docs/decisions.md` の該当G番号を参照。

## 0. 準備

```sh
git clone https://github.com/shiki-01/nagi.git
cd nagi/core
cargo xtask fetch-dict          # Mozc OSS辞書を取得（SHA-256検証あり）
cargo xtask build-dict          # data/dict/nagi-dict.bin を生成
```

LLM（ビームサーチ・reconvertの品質確認等）も試す場合、追加で:

```sh
cargo xtask fetch-model         # zenz-v3.1-small を取得（SHA-256検証あり）
```

`--features llama` でビルドするには cmake と LLVM/clang（`LIBCLANG_PATH`）が必要（Windowsは
LLVM本体のインストールが必要な場合がある。macOSは `xcode-select --install` で足りるはず）。

## 1. ビルド・テストの基本確認（実機で初めて走らせる意味がある）

```sh
cargo fmt --all --check
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CIはubuntu/windows/macosの3OSで既に通っているはずだが、**実データ（`data/dict/nagi-dict.bin`）が
無い環境で走るCIでは、実データ依存のテストは自動でskipされている**（`dictionary_backed_conversion_end_to_end`
等、テスト内で `data_dir()` が `None` なら早期return）。実機で辞書を取得した状態で同じコマンドを
実行すると、これらのテストが初めて実際に走る。

LLMも含めて確認する場合:

```sh
cargo test -p nagi-infer -p nagi-server --features llama
```

## 2. ライブ変換（G-25・G-26・G-27）

`imserver` は標準入出力でJSON-RPCを受け付ける。手元で対話的に試す一番簡単な方法は、
別ターミナルで `imserver` を起動し、標準入力に手でJSON-RPC行を打つか、簡単なスクリプトで送る:

```sh
cargo run -p nagi-server --bin imserver -- --data-dir ../data --user-dir /tmp/nagi-test
```

（`--user-dir` を指定して実ユーザーの学習データ・設定を汚さないこと。LLMも試すなら
`cargo run --release -p nagi-server --features llama --bin imserver -- --data-dir ../data --user-dir /tmp/nagi-test`）

確認したいこと:

1. `initialize` → `process_key` でかな2文字以上を打った後、**何も送らずに**約228ms
   （既定の `live_conversion_delay_ms`）待つと、`update_preedit` 通知が自発的に出力されること。
   1文字だけでは出ないこと（既定 `live_conversion_min_key_length = 2`）。
2. ライブ変換の変換結果が、同じ読みをSpaceで変換した結果と比べて違和感が無いか（実データでの
   変換品質。辞書のみ/LLM併用の両方で確認できると良い）。
3. 高速に連続してキーを打ったとき（1文字ごとにデバウンススレッドが1本立つ設計）、CPU負荷・
   メモリが問題にならないか（タスクマネージャー/アクティビティモニタで `imserver` プロセスを見る）。
4. 60文字を超える長い読みを一度に打ったとき、ライブ変換の反映が遅く感じないか（`composing_chunk_len`
   でチャンク分割しているが、「編集されたチャンクのみ再変換」という最適化はまだ無いため、
   長文では毎回チャンク全体を再変換している）。

## 3. ビームサーチ（文全体N-best、G-30）— `--features llama` 推奨

`conversion.initial_view = "sentence"`（既定）で、同音異義語を含む文を打ってSpaceを1回押す:

1. 候補窓がその場で開き、複数の文全体の変換結果が並ぶこと（`conversion.initial_view = "segment"`
   に設定すると従来どおり候補窓が出ないことも確認できると良い）。
2. **1番目の候補が、LLM併用時の通常のSpace変換（1-best）と同じ表記になっていること**
   （辞書コスト順ではなくLLM再採点済みの結果を1番目に揃える設計にした。ここが辞書コスト順に
   戻っていたら品質の後退なので要報告）。
3. 2番目以降の候補（辞書コスト順、LLM再採点なし）の並びが不自然に感じられないか。
4. 候補を選んだ状態で矢印キー（MoveFocus）を押すと、その候補の文節分割に展開されて複数文節の
   通常編集に戻ること。

## 4. 確定取消 Ctrl+Backspace（G-28）

**フロント（macOS/Windows）が無いため、ホストアプリのテキストを実際に削除する動作はまだ無い。**
`imserver` に直接JSON-RPCを送って、エンジンの機構だけを確認する:

1. かな入力→Enterで確定
2. 直後に `Ctrl+Backspace`（`keysym` は BackSpace=0xff08、`modifiers.ctrl=true`）を送る
3. 応答に `delete_before`（削除すべきUTF-16長）が入り、`preedit` が確定前の状態に戻ること
4. 確定後に他のキー（何でも良い）を1回挟んでから `Ctrl+Backspace` を送ると、何も起きず
   素通し（`consumed: false`）になること

## 5. reconvert（G-29）

`reconvert` RPCに `{"session_id": "s", "text": "<確定済みの表記>"}` を送る:

1. 一般的な単語・文で、辞書の逆引きによる読みの推定がどの程度使えるか（辞書のみ、LLM補完なし）
2. 辞書に無い表記（固有名詞・当て字等）を渡したときにクラッシュしないこと（`reading` が
   正しくなくても編集を続けられればOK）
3. **`reconvert` を初めて呼んだ直後のメモリ増加量**（逆引き索引を辞書全体から構築する。
   実データ・語数約120万での見積もりは数十MB程度だが未実測）。タスクマネージャー/
   アクティビティモニタで `imserver` のメモリを、`reconvert` を呼ぶ前後で比較してほしい。
   `light`/`minimal` プロファイルのメモリ目標（アイドル時500MB目安）に影響しないか。

## 6. アップデート署名検証（G-31）

`UPDATE_PUBLIC_KEY_HEX`（`core/server/src/update_check.rs`）は実装確認用の使い捨て鍵。
実際の配布フローはまだ無い（`nagi.shiki-01.com` のマニフェスト配信も未整備）ため、実機で
試せるのは暗号ロジック自体の単体テストのみ:

```sh
cargo test -p nagi-server update_check
```

ed25519の実装（`ed25519-dalek`）はOS依存のCライブラリを使わない純Rustのはずなので、Win/Macでも
問題なく通るはずだが、実際に確認してほしい。実際の配布を始める際は、新しい鍵ペアを生成し直す
必要がある（このリポジトリの `UPDATE_PUBLIC_KEY_HEX` は差し替える前提のプレースホルダー）。

## 7. その他

- 標準IME（macOS標準／MS-IME／Mozc）との挙動照合（ゴールデンテストの期待値は設計書から書いたもので、
  標準IMEとの実際の照合はまだ行っていない）
- レイテンシベンチ（p95）: 設計書の目標値（キー入力→ひらがな反映 p95 < 16ms 等）を実機で計測できると良い
- `cargo xtask fetch-model` でモデルを取得した状態で `cargo run --release -p nagi-server --features llama --bin nagi-eval -- --data-dir ../data` を実行すると、精度・レイテンシの計測結果が出る（G-11の数値は評価環境（クラウドサンドボックス）でのもので、実機での再計測が無い）
