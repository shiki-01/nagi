use super::*;
use std::cell::Cell;

fn kana_table() -> KanaTable {
    KanaTable::new(
        &[
            ['あ', 'い', 'う', 'え', 'お'],
            ['か', 'き', 'く', 'け', 'こ'],
            ['は', 'ひ', 'ふ', 'へ', 'ほ'],
        ],
        &[
            vec!['か', 'が'],
            vec!['し', 'じ'],
            vec!['は', 'ば', 'ぱ'],
            vec!['つ', 'っ', 'づ'],
        ],
    )
}

fn fuzzy() -> Fuzzy {
    Fuzzy::qwerty(kana_table())
}

fn index() -> WordSet {
    WordSet::new([
        "かんじ",
        "かん",
        "じ",
        "きょう",
        "しんぶん",
        "てんき",
        "かんじゅ",
        "かんい",
        "かんは",
        "かんば",
        "がっこう",
        "ありがとう",
    ])
}

fn params() -> Params {
    Params {
        deadline: None,
        ..Params::default()
    }
}

fn readings(c: &[Correction]) -> Vec<&str> {
    c.iter().map(|c| c.reading.as_str()).collect()
}

#[test]
fn substitute_adjacent_key() {
    // kamji: m の隣の n を打つつもりだった
    let c = fuzzy().corrections("かmじ", &index(), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops.len(), 1);
    assert_eq!(hit.ops[0].kind, EditKind::Substitute);
    assert_eq!(
        (hit.ops[0].typed, hit.ops[0].intended),
        (Some('m'), Some('n'))
    );
    assert!((hit.cost - 2.0).abs() < 1e-3, "{}", hit.cost);
}

#[test]
fn transpose_adjacent_chars() {
    // kajni → kanji
    let c = fuzzy().corrections("かjに", &index(), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops.len(), 1);
    assert_eq!(hit.ops[0].kind, EditKind::Transpose);
    assert!((hit.cost - 2.0).abs() < 1e-3);
}

#[test]
fn omission_adds_a_char() {
    // kanj → kanji（末尾の i の打ち忘れ）
    let c = fuzzy().corrections("かんj", &index(), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops[0].kind, EditKind::Delete);
    assert_eq!(hit.ops[0].intended, Some('i'));
    assert!((hit.cost - 3.0).abs() < 1e-3);
}

#[test]
fn insertion_removes_duplicate_or_adjacent() {
    // kaanji: a の重複
    let c = fuzzy().corrections("かあんじ", &index(), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops[0].kind, EditKind::Insert);
    assert!((hit.cost - 3.0).abs() < 1e-3);
    // kanjui: u は i に隣接 → 取り除いて kanji
    let c = fuzzy().corrections(
        "かんじうい",
        &WordSet::new(["かんじ", "かんじい"]),
        &params(),
    );
    assert!(readings(&c).contains(&"かんじい"), "{c:?}");
}

#[test]
fn insertion_only_for_duplicate_or_adjacent() {
    // p は前後 (a, n) のどちらとも隣接せず重複でもない → 削除の補正は作らない
    let c = fuzzy().corrections(
        "かpんじ",
        &index(),
        &Params {
            kinds: EditKinds::parse("insert"),
            ..params()
        },
    );
    assert!(c.is_empty(), "{c:?}");
}

#[test]
fn cost_limit_and_strength() {
    let f = fuzzy();
    // 置換 1 回 (2.0) は gentle (3.0) でも通るが、max_cost 1.9 では出ない
    let low = Params {
        max_cost: 1.9,
        ..params()
    };
    assert!(f.corrections("かmじ", &index(), &low).is_empty());
    let gentle = Params {
        max_cost: 3.0,
        ..params()
    };
    assert!(readings(&f.corrections("かmじ", &index(), &gentle)).contains(&"かんじ"));
    // 置換 2 回 (4.0 以上) は normal (4.5) で通り gentle では出ない: kamji の m→n と j→? 別語へ
    // 「かんじゅ」: kamji の m→n と i→u の 2 置換（2.0 + 2.2）
    let two = f.corrections("かmじ", &index(), &params());
    let both = two.iter().find(|c| c.reading == "かんじゅ").map(|c| c.cost);
    assert!(both.is_some_and(|c| c > 4.0 && c <= 4.5), "{two:?}");
    assert!(
        !readings(&f.corrections("かmじ", &index(), &gentle)).contains(&"かんじゅ"),
        "gentle では 2 操作は出ない"
    );
    // 結果は常に上限以内でコスト昇順
    for c in &two {
        assert!(c.cost <= 4.5 + 1e-3);
    }
    assert!(two.windows(2).all(|w| w[0].cost <= w[1].cost));
}

#[test]
fn kinds_switch_off() {
    let f = fuzzy();
    let no_sub = Params {
        kinds: EditKinds::parse("delete,insert,transpose,voicing"),
        ..params()
    };
    assert!(!readings(&f.corrections("かmじ", &index(), &no_sub)).contains(&"かんじ"));
    let none = Params {
        kinds: EditKinds::parse(""),
        ..params()
    };
    assert!(f.corrections("かmじ", &index(), &none).is_empty());
}

#[test]
fn prunes_prefixes_without_dictionary_entry() {
    // 辞書の接頭辞でない枝は作らない: 問い合わせの中に、辞書に接頭辞が無いものが残らない
    struct Spy<'a> {
        inner: WordSet,
        asked: &'a Cell<usize>,
    }
    impl ReadingIndex for Spy<'_> {
        fn word_prefix_lens(&self, text: &str) -> Vec<usize> {
            self.asked.set(self.asked.get() + 1);
            self.inner.word_prefix_lens(text)
        }
        fn has_reading_prefix(&self, prefix: &str) -> bool {
            self.asked.set(self.asked.get() + 1);
            self.inner.has_reading_prefix(prefix)
        }
    }
    let asked = Cell::new(0);
    let spy = Spy {
        inner: WordSet::new(["かんじ"]),
        asked: &asked,
    };
    let c = fuzzy().corrections("かmじ", &spy, &params());
    assert_eq!(readings(&c), ["かんじ"], "辞書に無い読みは出ない");
    assert!(asked.get() > 0);
    // 実際に探索したノード数の上限（max_nodes=1 ではルートだけで打ち切られ、何も出ない）
    let one = Params {
        max_nodes: 1,
        ..params()
    };
    assert!(fuzzy().corrections("かmじ", &spy, &one).is_empty());
}

#[test]
fn max_results_limits() {
    let words: Vec<String> = ('a'..='z').map(|c| format!("かん{c}")).collect();
    let idx = WordSet::new(words.iter().map(String::as_str).chain(["かん"]));
    let c = fuzzy().corrections(
        "かんj",
        &idx,
        &Params {
            max_results: 2,
            ..params()
        },
    );
    assert!(c.len() <= 2);
}

#[test]
fn kana_voicing_and_row_neighbors() {
    let f = fuzzy();
    // かんし → かんじ（濁点の付け忘れ）
    let c = f.corrections("かんし", &WordSet::new(["かんじ"]), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops[0].kind, EditKind::Voicing);
    assert!((hit.cost - 1.5).abs() < 1e-3, "{}", hit.cost);
    // 同じ行内の隣: かん + う(上) を い(左) と誤フリック。かんい のコストは行の隣 (2.5) か
    // ローマ字の隣接 (u-i) の安い方
    let c = f.corrections("かんう", &WordSet::new(["かんい"]), &params());
    assert!(readings(&c).contains(&"かんい"), "{c:?}");
    // 濁点の付け過ぎ・半濁点: かんば → かんは
    let c = f.corrections("かんば", &WordSet::new(["かんは"]), &params());
    assert_eq!(c[0].reading, "かんは");
    assert_eq!(c[0].ops[0].kind, EditKind::Voicing);
    // 小書き: がつこう → がっこう
    let c = f.corrections("がつこう", &WordSet::new(["がっこう"]), &params());
    assert!(
        c.iter()
            .any(|c| c.reading == "がっこう" && c.ops[0].kind == EditKind::Voicing),
        "{c:?}"
    );
}

#[test]
fn row_cost_applies_for_kana_only() {
    // ローマ字側の補正で届かない行内の隣（う→え は x の距離が遠い）でも、かな側が拾う
    let c = fuzzy().corrections("かんあ", &WordSet::new(["かんお"]), &params());
    // あ(中央) → お(下) は 2.5
    let hit = c.iter().find(|c| c.reading == "かんお").expect("{c:?}");
    assert!(hit.cost <= 3.0 + 1e-3);
}

#[test]
fn length_and_charset_limits() {
    let f = fuzzy();
    // 短すぎる・長すぎる・かな以外を含む読みは展開しない
    assert!(f.corrections("かm", &index(), &params()).is_empty());
    assert!(f.corrections("カンジ", &index(), &params()).is_empty());
    assert!(f.corrections("かん字", &index(), &params()).is_empty());
    let long = "か".repeat(MAX_READING_CHARS + 1);
    assert!(f.corrections(&long, &index(), &params()).is_empty());
}

#[test]
fn clean_reading_is_not_listed_and_deterministic() {
    let f = fuzzy();
    let c = f.corrections("かんじ", &index(), &params());
    assert!(!readings(&c).contains(&"かんじ"), "自分自身は含めない");
    let a = f.corrections("かmじ", &index(), &params());
    let b = f.corrections("かmじ", &index(), &params());
    assert_eq!(a, b);
}
