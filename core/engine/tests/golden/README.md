# 状態機械ゴールデンテスト

`*.yaml` の各ケースが「キー列 → 状態」を表す。実行は `core/engine/tests/golden.rs`。

```yaml
- name: 一意なケース名
  backend: kana            # kana（既定）| mock
  mock:                    # backend: mock のとき
    sentences:             # 文の変換結果: 読み全体 → 文節ごとの (読み, 候補一覧。先頭が変換結果)
      "きょうはいいてんき":
        - {reading: "きょうは", candidates: ["今日は", "京は", "きょうは"]}
        - {reading: "いいてんき", candidates: ["いい天気"]}
    segments:              # 任意: 文節単独の候補
      "いいてんき": ["いい天気", "良い天気"]
  config:                  # 任意
    shift_letter: literal_continue   # | single_upper
    punctuation: "、。"              # | "，．" | "，。"
    page_size: 9
    mode: hiragana                   # | alphanumeric（初期モード）
    romaji_carryover: true           # | false（Backspace でのローマ字の引き継ぎ。H-23）
  session: s1              # 任意
  steps:
    - key: "k"             # `Ctrl+Shift+Right` 形式。または {text: "😀"}
      expect:              # 書いたものだけ比較する
        consumed: true
        commit: null       # null = 確定なし。"文字列" = その確定
        delete_before: null   # null = 応答に含まれない（確定取消なし）。数値 = 削除すべき UTF-16 長
        preedit: "k"       # PreeditView.text（保留ローマ字を含む）
        pending: "k"       # 保留ローマ字（無ければ ""）
        segments: ["か"]   # セグメントの surface 列
        readings: ["か"]
        kinds: ["japanese"]   # japanese | literal | fixed | styled
        cursor: 1          # PreeditView.cursor（UTF-16）
        focus: 0           # null = 注目文節なし
        state: composing   # direct | composing | converting
        mode: hiragana     # 現在の入力モード
        candidates: ["今日は", "京は"]   # 候補ウィンドウの surface。[] は非表示
        candidates_visible: true
        selected: 1
        result_mode: alphanumeric   # このステップの応答に mode が含まれる（null = 含まれない）
        result_preedit_present: true   # 応答に preedit（差分）が含まれるか
        result_candidates_present: false
        revision_increased: true       # 応答の revision が前のステップより増えたか
    - action: focus_out    # focus_in | focus_out | reset | restore
    - action: restore
      restore:
        segments: [{reading: "かな", surface: "かな", kind: japanese}]
        cursor: 2
        focus: 0
        converting: false
```

`consumed` / `commit` / `result_*` / `revision_increased` は「そのステップの応答」、それ以外は「そのステップ後の状態」を検査する。
