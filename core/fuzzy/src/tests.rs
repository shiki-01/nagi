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
    let c = fuzzy().corrections("かmじ", None, &index(), &params());
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
    let c = fuzzy().corrections("かjに", None, &index(), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops.len(), 1);
    assert_eq!(hit.ops[0].kind, EditKind::Transpose);
    assert!((hit.cost - 2.0).abs() < 1e-3);
}

#[test]
fn omission_adds_a_char() {
    // kanj → kanji（末尾の i の打ち忘れ）
    let c = fuzzy().corrections("かんj", None, &index(), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops[0].kind, EditKind::Delete);
    assert_eq!(hit.ops[0].intended, Some('i'));
    assert!((hit.cost - 3.0).abs() < 1e-3);
}

#[test]
fn insertion_removes_duplicate_or_adjacent() {
    // kaanji: a の重複
    let c = fuzzy().corrections("かあんじ", None, &index(), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops[0].kind, EditKind::Insert);
    assert!((hit.cost - 3.0).abs() < 1e-3);
    // kanjui: u は i に隣接 → 取り除いて kanji
    let c = fuzzy().corrections(
        "かんじうい",
        None,
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
        None,
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
    assert!(f.corrections("かmじ", None, &index(), &low).is_empty());
    let gentle = Params {
        max_cost: 3.0,
        ..params()
    };
    assert!(readings(&f.corrections("かmじ", None, &index(), &gentle)).contains(&"かんじ"));
    // 置換 2 回 (4.0 以上) は normal (4.5) で通り gentle では出ない: kamji の m→n と j→? 別語へ
    // 「かんじゅ」: kamji の m→n と i→u の 2 置換（2.0 + 2.2）
    let two = f.corrections("かmじ", None, &index(), &params());
    let both = two.iter().find(|c| c.reading == "かんじゅ").map(|c| c.cost);
    assert!(both.is_some_and(|c| c > 4.0 && c <= 4.5), "{two:?}");
    assert!(
        !readings(&f.corrections("かmじ", None, &index(), &gentle)).contains(&"かんじゅ"),
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
    assert!(!readings(&f.corrections("かmじ", None, &index(), &no_sub)).contains(&"かんじ"));
    let none = Params {
        kinds: EditKinds::parse(""),
        ..params()
    };
    assert!(f.corrections("かmじ", None, &index(), &none).is_empty());
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
    let c = fuzzy().corrections("かmじ", None, &spy, &params());
    assert_eq!(readings(&c), ["かんじ"], "辞書に無い読みは出ない");
    assert!(asked.get() > 0);
    // 実際に探索したノード数の上限（max_nodes=1 ではルートだけで打ち切られ、何も出ない）
    let one = Params {
        max_nodes: 1,
        ..params()
    };
    assert!(fuzzy().corrections("かmじ", None, &spy, &one).is_empty());
}

#[test]
fn max_results_limits() {
    let words: Vec<String> = ('a'..='z').map(|c| format!("かん{c}")).collect();
    let idx = WordSet::new(words.iter().map(String::as_str).chain(["かん"]));
    let c = fuzzy().corrections(
        "かんj",
        None,
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
    let c = f.corrections("かんし", None, &WordSet::new(["かんじ"]), &params());
    let hit = c.iter().find(|c| c.reading == "かんじ").expect("かんじ");
    assert_eq!(hit.ops[0].kind, EditKind::Voicing);
    assert!((hit.cost - 1.5).abs() < 1e-3, "{}", hit.cost);
    // 同じ行内の隣: かん + う(上) を い(左) と誤フリック。かんい のコストは行の隣 (2.5) か
    // ローマ字の隣接 (u-i) の安い方
    let c = f.corrections("かんう", None, &WordSet::new(["かんい"]), &params());
    assert!(readings(&c).contains(&"かんい"), "{c:?}");
    // 濁点の付け過ぎ・半濁点: かんば → かんは
    let c = f.corrections("かんば", None, &WordSet::new(["かんは"]), &params());
    assert_eq!(c[0].reading, "かんは");
    assert_eq!(c[0].ops[0].kind, EditKind::Voicing);
    // 小書き: がつこう → がっこう
    let c = f.corrections("がつこう", None, &WordSet::new(["がっこう"]), &params());
    assert!(
        c.iter()
            .any(|c| c.reading == "がっこう" && c.ops[0].kind == EditKind::Voicing),
        "{c:?}"
    );
}

#[test]
fn row_cost_applies_for_kana_only() {
    // ローマ字側の補正で届かない行内の隣（う→え は x の距離が遠い）でも、かな側が拾う
    let c = fuzzy().corrections("かんあ", None, &WordSet::new(["かんお"]), &params());
    // あ(中央) → お(下) は 2.5
    let hit = c.iter().find(|c| c.reading == "かんお").expect("{c:?}");
    assert!(hit.cost <= 3.0 + 1e-3);
}

#[test]
fn length_and_charset_limits() {
    let f = fuzzy();
    // 短すぎる・長すぎる・かな以外を含む読みは展開しない
    assert!(f.corrections("かm", None, &index(), &params()).is_empty());
    assert!(
        f.corrections("カンジ", None, &index(), &params())
            .is_empty()
    );
    assert!(
        f.corrections("かん字", None, &index(), &params())
            .is_empty()
    );
    let long = "か".repeat(MAX_READING_CHARS + 1);
    assert!(f.corrections(&long, None, &index(), &params()).is_empty());
}

#[test]
fn clean_reading_is_not_listed_and_deterministic() {
    let f = fuzzy();
    let c = f.corrections("かんじ", None, &index(), &params());
    assert!(!readings(&c).contains(&"かんじ"), "自分自身は含めない");
    let a = f.corrections("かmじ", None, &index(), &params());
    let b = f.corrections("かmじ", None, &index(), &params());
    assert_eq!(a, b);
}

fn keys(s: &str, touches: &[(usize, (f32, f32))]) -> Vec<Stroke> {
    let mut v: Vec<Stroke> = s.chars().map(Stroke::key).collect();
    for &(i, t) in touches {
        v[i].touch = Some(t);
    }
    v
}

fn cost_of(c: &[Correction], reading: &str) -> Option<f32> {
    c.iter().find(|c| c.reading == reading).map(|c| c.cost)
}

#[test]
fn touch_near_the_boundary_makes_substitution_cheaper() {
    let f = fuzzy();
    // kamji: m の左端（n との境界）を押した / 中央を押した / 右端を押した
    let edge = keys("kamji", &[(2, (0.02, 0.5))]);
    let center = keys("kamji", &[(2, (0.5, 0.5))]);
    let far = keys("kamji", &[(2, (0.98, 0.5))]);
    let at = |s: &[Stroke]| f.corrections("かmじ", Some(s), &index(), &params());
    let (e, c, r) = (at(&edge), at(&center), at(&far));
    let ce = cost_of(&e, "かんじ").expect("境界なら補正される");
    // 中央を押したのは本当に m の可能性が高い: 上限を超えて補正されない（あっても境界より高い）
    assert!(cost_of(&c, "かんじ").is_none_or(|x| x > ce));
    assert!(cost_of(&r, "かんじ").is_none_or(|x| x > ce));
    // 離散コストより境界は安い
    let discrete = f.corrections("かmじ", None, &index(), &params());
    assert!(ce < cost_of(&discrete, "かんじ").unwrap());
}

#[test]
fn touch_is_ignored_when_disabled_and_matches_discrete() {
    let f = fuzzy();
    let strokes = keys("kamji", &[(2, (0.02, 0.5))]);
    let off = Params {
        use_touch: false,
        ..params()
    };
    let with = f.corrections("かmじ", Some(&strokes), &index(), &off);
    // 座標なしの打鍵列・読みからの復元と、結果が一致する（後方互換）
    let plain = keys("kamji", &[]);
    let a = f.corrections("かmじ", Some(&plain), &index(), &params());
    let b = f.corrections("かmじ", None, &index(), &params());
    assert_eq!(with, b);
    assert_eq!(a, b);
}

#[test]
fn actual_strokes_are_preferred_and_mismatch_falls_back() {
    let f = fuzzy();
    // 実際は "si"（し）と打った。復元は最短の si で同じだが、"shi" と打った場合は h が打鍵列にある
    let words = WordSet::new(["しんぶん"]);
    let shi = keys("shinbun", &[]);
    assert!(f.strokes_reproduce(&shi, "しんぶん"));
    let c = f.corrections("しんぶん", Some(&shi), &words, &params());
    assert!(c.is_empty(), "辞書語と一致する読みは補正の対象外: {c:?}");
    // h の隣の j を誤って打った: 実際の綴り shi の h を j に取り違え、という補正は
    // 打鍵列があるときだけ作れる（復元は si）
    let typed = keys("sjinbun", &[]);
    let c = f.corrections("sじんぶん", Some(&typed), &words, &params());
    assert!(readings(&c).contains(&"しんぶん"), "{c:?}");
    // 打鍵列が読みと合わなければ読みから復元する
    let wrong = keys("zzz", &[]);
    assert!(!f.strokes_reproduce(&wrong, "かmじ"));
    assert_eq!(
        f.corrections("かmじ", Some(&wrong), &index(), &params()),
        f.corrections("かmじ", None, &index(), &params())
    );
}

#[test]
fn custom_layout_changes_neighbors() {
    // 3 キーだけの配置（a b c が縦に並ぶ）。b の隣は a と c、a の隣は b だけ
    let layout = KeyLayout {
        keys: ['a', 'b', 'c']
            .iter()
            .enumerate()
            .map(|(i, &c)| Key {
                id: c.to_string(),
                label: c,
                x: 0.0,
                y: i as f32 * 60.0,
                w: 40.0,
                h: 50.0,
            })
            .collect(),
    };
    let f = Fuzzy::new(&layout, kana_table(), RomajiTable::standard());
    let nb = |c| -> Vec<char> { f.adjacency().neighbors(c).iter().map(|n| n.label).collect() };
    assert_eq!(nb('b'), ['a', 'c']);
    assert_eq!(nb('a'), ['b']);
}

#[test]
fn flick_near_a_direction_boundary_is_cheaper() {
    let f = fuzzy();
    let words = WordSet::new(["かんう"]);
    let plain = f.corrections("かんい", None, &words, &params());
    let base = cost_of(&plain, "かんう").expect("い→う の補正");
    let flicks = |deg: f32| vec![None, None, Some((deg, 0.8))];
    let near = f.corrections_with("かんい", None, Some(&flicks(140.0)), &words, &params());
    let mid = f.corrections_with("かんい", None, Some(&flicks(180.0)), &words, &params());
    assert!(cost_of(&near, "かんう").unwrap() < base);
    assert_eq!(cost_of(&mid, "かんう").unwrap(), base);
    // 長さが合わないフリック情報は無視
    let bad = f.corrections_with(
        "かんい",
        None,
        Some(&[Some((140.0, 0.8))]),
        &words,
        &params(),
    );
    assert_eq!(cost_of(&bad, "かんう").unwrap(), base);
    // use_touch=false ならフリックも無視
    let off = Params {
        use_touch: false,
        ..params()
    };
    let r = f.corrections_with("かんい", None, Some(&flicks(140.0)), &words, &off);
    assert_eq!(cost_of(&r, "かんう").unwrap(), base);
}
