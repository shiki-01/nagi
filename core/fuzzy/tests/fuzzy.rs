//! `nagi-fuzzy` の結合テスト（モック oracle と、メモリ上で構築した辞書）。

use nagi_dict::{Dictionary, DictionaryBuilder};
use nagi_fuzzy::{
    FuzzyConfig, KeyAlternatives, PrefixOracle, ReadingHypothesis, distance_cost, expand,
    expand_typed, expand_with_stats,
};
use nagi_romaji::RomajiTable;
use std::collections::HashSet;

struct Mock(HashSet<String>);

impl Mock {
    fn new(words: &[&str]) -> Self {
        Mock(words.iter().map(|s| s.to_string()).collect())
    }
}

impl PrefixOracle for Mock {
    fn has_prefix(&self, p: &str) -> bool {
        self.0.iter().any(|w| w.starts_with(p))
    }
    fn contains_word(&self, w: &str) -> bool {
        self.0.contains(w)
    }
}

fn readings(h: &[ReadingHypothesis]) -> Vec<&str> {
    h.iter().map(|h| h.reading.as_str()).collect()
}

fn run(typed: &str, words: &[&str], cfg: &FuzzyConfig) -> Vec<ReadingHypothesis> {
    expand_typed(typed, &RomajiTable::standard(), &Mock::new(words), cfg)
}

#[test]
fn insertion_nihn() {
    let h = run("nihn", &["にほん"], &FuzzyConfig::default());
    assert_eq!(h[0].reading, "にhん");
    assert_eq!(h[0].cost, 0.0);
    assert_eq!(h[1].reading, "にほん");
    assert_eq!(h[1].edits.insertions, 1);
    assert_eq!(h[1].cost, 3.0);
}

#[test]
fn deletion_kanizi() {
    let h = run("kanizi", &["かんじ"], &FuzzyConfig::default());
    assert!(readings(&h).contains(&"かんじ"), "{h:?}");
    let x = h.iter().find(|h| h.reading == "かんじ").unwrap();
    assert_eq!(x.edits.deletions, 1);
}

#[test]
fn deletion_double_strike() {
    // kk → k（二重打鍵ミス）: 「かな」を kkana と打った
    let h = run("kkana", &["かな"], &FuzzyConfig::default());
    assert_eq!(h[0].reading, "っかな");
    assert!(readings(&h).contains(&"かな"), "{h:?}");
}

#[test]
fn transposition_at_ta_and_hsi_shi() {
    let h = run("at", &["た"], &FuzzyConfig::default());
    assert_eq!(readings(&h), vec!["あt", "た"]);
    assert_eq!(h[1].cost, 2.0);
    assert_eq!(h[1].edits.transpositions, 1);

    let h = run("hsi", &["し"], &FuzzyConfig::default());
    assert!(readings(&h).contains(&"し"), "{h:?}");
}

#[test]
fn confusion_l_r() {
    let h = run("lamen", &["らめん"], &FuzzyConfig::default());
    let x = h.iter().find(|h| h.reading == "らめん").expect("l→r");
    assert_eq!(x.edits.confusions, 1);
    assert_eq!(x.cost, 2.5);

    let h = run("ramen", &["ぁめん"], &FuzzyConfig::default());
    assert!(readings(&h).contains(&"ぁめん"), "r→l: {h:?}");

    let cfg = FuzzyConfig {
        enable_confusions: false,
        ..Default::default()
    };
    let h = run("lamen", &["らめん"], &cfg);
    assert_eq!(h.len(), 1);
}

#[test]
fn first_hypothesis_always_kept() {
    let h = run("xyzw", &[], &FuzzyConfig::default());
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].cost, 0.0);
    // 辞書にない語でも、打たれたとおりの読みが先頭に残る
    let h = run("nihon", &["かな"], &FuzzyConfig::default());
    assert_eq!(h[0].reading, "にほん");
    let cfg = FuzzyConfig {
        max_cost: 0.0,
        ..Default::default()
    };
    let h = run("nihn", &["にほん"], &cfg);
    assert_eq!(h.len(), 1);
}

#[test]
fn max_edits_and_window_boundaries() {
    let cfg = FuzzyConfig {
        max_edits: 0,
        ..Default::default()
    };
    assert_eq!(run("nihn", &["にほん"], &cfg).len(), 1);

    // 2 編集が必要な入力: nihn → にほん（挿入）と、kanizi の削除
    let words = ["にほん", "かんじ"];
    let two = run("nihnkanizi", &words, &FuzzyConfig::default());
    assert!(readings(&two).contains(&"にほんかんじ"), "{two:?}");
    let one = FuzzyConfig {
        max_edits: 1,
        long_input_len: 100,
        ..Default::default()
    };
    let h = run("nihnkanizi", &words, &one);
    assert!(!readings(&h).contains(&"にほんかんじ"), "{h:?}");

    // window: 欠落は末尾から数えて window 打鍵以内のみ
    let w = |window| FuzzyConfig {
        window,
        ..Default::default()
    };
    let words = ["にほん", "か"];
    assert!(readings(&run("nihnka", &words, &w(3))).contains(&"にほんか"));
    assert!(!readings(&run("nihnka", &words, &w(2))).contains(&"にほんか"));
}

#[test]
fn max_cost_cutoff() {
    let cfg = FuzzyConfig {
        max_cost: 2.9,
        ..Default::default()
    };
    // 入れ替え(2.0)は残り、欠落補完(3.0)は落ちる
    assert_eq!(run("at", &["た"], &cfg).len(), 2);
    assert_eq!(run("nihn", &["にほん"], &cfg).len(), 1);
}

#[test]
fn sorted_unique_and_limited() {
    let words = ["か", "た", "あt", "きt"];
    let cfg = FuzzyConfig {
        max_hypotheses: 10,
        ..Default::default()
    };
    let h = run("kat", &words, &cfg);
    let r = readings(&h);
    let set: HashSet<_> = r.iter().collect();
    assert_eq!(set.len(), r.len());
    assert!(h.windows(2).all(|w| w[0].cost <= w[1].cost));
    let cfg = FuzzyConfig {
        max_hypotheses: 2,
        ..Default::default()
    };
    assert!(run("kat", &words, &cfg).len() <= 2);
}

#[test]
fn distance_to_cost() {
    let cfg = FuzzyConfig::default();
    assert_eq!(distance_cost(0.0, &cfg), 0.0);
    let c = distance_cost(0.35, &cfg);
    assert!((c - 0.5).abs() < 1e-5);
    assert!(distance_cost(0.6, &cfg) > distance_cost(0.3, &cfg));

    let a = KeyAlternatives::from_distances(
        'k',
        &[('l', 0.3), ('j', 1.5), ('o', 0.9), ('l', 0.5)],
        &cfg,
    );
    let items = a.items();
    assert_eq!(items[0].ch, 'k');
    assert_eq!(items[0].cost, 0.0);
    // j は max_distance 超過で除外、l は小さい距離を採用、コスト昇順
    let chars: Vec<char> = items.iter().map(|i| i.ch).collect();
    assert_eq!(chars, vec!['k', 'l', 'o']);
    assert!((items[1].cost - distance_cost(0.3, &cfg)).abs() < 1e-6);
    assert!(items[1].cost < items[2].cost);

    // 打たれたキー自身の距離が与えられたら相対コスト
    let b = KeyAlternatives::from_distances('k', &[('k', 0.2), ('l', 0.3)], &cfg);
    let rel = distance_cost(0.3, &cfg) - distance_cost(0.2, &cfg);
    assert!((b.items()[1].cost - rel).abs() < 1e-6);
}

#[test]
fn touch_alternative_substitution() {
    let cfg = FuzzyConfig::default();
    let keys = vec![
        KeyAlternatives::from_distances('k', &[('l', 0.3)], &cfg),
        KeyAlternatives::exact('a'),
    ];
    let h = expand(
        &keys,
        &RomajiTable::standard(),
        &Mock::new(&["か", "ぁ"]),
        &cfg,
    );
    assert_eq!(readings(&h), vec!["か", "ぁ"]);
    assert_eq!(h[1].edits.substitutions, 1);
    assert!((h[1].cost - distance_cost(0.3, &cfg)).abs() < 1e-6);
}

#[test]
fn empty_and_long_inputs() {
    let h = run("", &["か"], &FuzzyConfig::default());
    assert_eq!(readings(&h), vec![""]);

    let typed = "ka".repeat(50);
    let cfg = FuzzyConfig::default();
    let keys: Vec<_> = typed.chars().map(KeyAlternatives::exact).collect();
    let (h, stats) = expand_with_stats(&keys, &RomajiTable::standard(), &Mock::new(&["か"]), &cfg);
    assert_eq!(h[0].reading, "か".repeat(50));
    assert!(stats.states_expanded <= cfg.beam * keys.len());
}

// --- メモリ上で構築した辞書との結合 ---

struct DictOracle(Dictionary);

impl PrefixOracle for DictOracle {
    fn has_prefix(&self, p: &str) -> bool {
        self.0.has_prefix(p)
    }
    fn contains_word(&self, w: &str) -> bool {
        self.0.contains_reading(w)
    }
}

fn small_dict(readings: &[&str]) -> Dictionary {
    let mut b = DictionaryBuilder::new();
    b.set_pos_defs(vec!["名詞".to_string()]);
    b.set_connection(1, vec![0]);
    for r in readings {
        b.add_entry(r, r, 0, 0, 0, 100);
    }
    b.build().expect("辞書の構築")
}

#[test]
fn with_real_dictionary() {
    let oracle = DictOracle(small_dict(&["にほん", "にほんご", "かんじ"]));
    let table = RomajiTable::standard();
    let h = expand_typed("nihn", &table, &oracle, &FuzzyConfig::default());
    assert_eq!(readings(&h), vec!["にhん", "にほん"]);
    let h = expand_typed("kanizi", &table, &oracle, &FuzzyConfig::default());
    assert!(readings(&h).contains(&"かんじ"), "{h:?}");
}

/// 窓 8・ビーム 8 の所要時間を出力する（アサートなし）。`cargo test -p nagi-fuzzy --release -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_expand() {
    let table = RomajiTable::standard();
    let oracle = Mock::new(&[
        "にほん",
        "にほんご",
        "かんじ",
        "を",
        "いれる",
        "きょう",
        "は",
    ]);
    let cfg = FuzzyConfig::default();
    let keys: Vec<_> = "kyouhanihnnogokanizi"
        .chars()
        .map(KeyAlternatives::exact)
        .collect();
    let iters = 200;
    let t = std::time::Instant::now();
    for _ in 0..iters {
        std::hint::black_box(expand(&keys, &table, &oracle, &cfg));
    }
    println!("expand: {:?} / iter", t.elapsed() / iters);
}

#[test]
fn kana_keys_with_alternatives() {
    // フリックの「か」行: く を狙って け に判定された、のような代替（ひらがな 1 文字 + 距離）
    let cfg = FuzzyConfig::default();
    let table = RomajiTable::standard();
    let keys = vec![
        KeyAlternatives::exact('あ'),
        KeyAlternatives::from_distances('け', &[('く', 0.35)], &cfg),
    ];
    let h = expand(&keys, &table, &Mock::new(&["あく"]), &cfg);
    assert_eq!(readings(&h), vec!["あけ", "あく"], "{h:?}");
    assert!((h[1].cost - 0.5).abs() < 1e-4);
}

#[test]
fn kana_keys_flush_pending_romaji_like_the_composer() {
    // ローマ字の n を打ってからひらがなを直接挿入すると、n は「ん」に確定してから挿入される
    let cfg = FuzzyConfig::default();
    let table = RomajiTable::standard();
    let keys = vec![KeyAlternatives::exact('n'), KeyAlternatives::exact('か')];
    let h = expand(&keys, &table, &Mock::new(&["んか"]), &cfg);
    assert_eq!(h[0].reading, "んか");

    // 混在したキー列でもローマ字側の編集が効く（欠落補完: nihn → にほん の後ろに kana）
    let keys: Vec<KeyAlternatives> = "nihn"
        .chars()
        .map(KeyAlternatives::exact)
        .chain([KeyAlternatives::exact('の')])
        .collect();
    let h = expand(&keys, &table, &Mock::new(&["にほん", "の"]), &cfg);
    assert_eq!(h[0].reading, "にhんの");
    assert!(readings(&h).contains(&"にほんの"), "{h:?}");
}

#[test]
fn insertion_is_not_applied_between_kana_keys() {
    // かなだけのキー列（フリック）では、ローマ字の欠落補完は意味を持たない
    let cfg = FuzzyConfig::default();
    let table = RomajiTable::standard();
    let wide = FuzzyConfig {
        beam: 64,
        ..cfg.clone()
    };
    let words = Mock::new(&["かんじ", "かあんじ", "かんあじ", "かんじあ", "あかんじ"]);
    let kana: Vec<KeyAlternatives> = "かんじ".chars().map(KeyAlternatives::exact).collect();
    let (h, stats) = expand_with_stats(&kana, &table, &words, &wide);
    assert_eq!(readings(&h), vec!["かんじ"], "{h:?}");
    // 探索も広がらない（打たれたとおりの 1 状態ずつ）
    assert_eq!(stats.states_expanded, 3, "{stats:?}");

    // ローマ字キーに隣接する位置では従来どおり補完される（nihn の末尾の後ろ・かなキーとの境界）
    let words = Mock::new(&["にほんの", "かんあ"]);
    let mixed: Vec<KeyAlternatives> = "nihn"
        .chars()
        .map(KeyAlternatives::exact)
        .chain([KeyAlternatives::exact('の')])
        .collect();
    let h = expand(&mixed, &table, &words, &cfg);
    assert!(readings(&h).contains(&"にほんの"), "{h:?}");
    // かな + ローマ字: かなの直後（ローマ字キーの直前）での補完
    let mixed: Vec<KeyAlternatives> = [KeyAlternatives::exact('か')]
        .into_iter()
        .chain("si".chars().map(KeyAlternatives::exact))
        .collect();
    // 子音の欠落補完の候補（保留だけが違う状態）でビームが埋まらないよう、ビームを広げて確かめる
    let wide = FuzzyConfig {
        beam: 64,
        ..cfg.clone()
    };
    let h = expand(&mixed, &table, &Mock::new(&["かあし"]), &wide);
    assert_eq!(h[0].reading, "かし");
    assert!(readings(&h).contains(&"かあし"), "{h:?}");
}
