//! 実データ（`cargo xtask fetch-dict && cargo xtask build-dict` で生成した辞書）での変換テスト。
//! 辞書が無い環境では何もせず終了する。

use nagi_dict::Dictionary;
use std::path::Path;
use std::time::Instant;

fn load() -> Option<Dictionary> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dict/nagi-dict.bin");
    if !path.exists() {
        eprintln!("skip: {} が無い（cargo xtask build-dict）", path.display());
        return None;
    }
    let t = Instant::now();
    let d = Dictionary::load(&path).expect("辞書の読み込み");
    eprintln!("load: {:?}, entries={}", t.elapsed(), d.entry_count());
    Some(d)
}

fn joined(d: &Dictionary, reading: &str) -> String {
    d.convert(reading)
        .iter()
        .map(|s| s.surface.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

/// 言語的に正しいことを確認済みの文（文節区切りの `|` も含めて固定）
#[test]
fn known_good_sentences() {
    let Some(d) = load() else { return };
    let cases = [
        ("きょうはいいてんきです", "今日は|いい天気です"),
        ("わたしのなまえはなかのです", "私の|名前は|中野です"),
        ("とうきょうにいく", "東京に|行く"),
        ("すしをたべたい", "寿司を|食べたい"),
        ("でんしゃがおくれた", "電車が|遅れた"),
        ("しゅくだいをおわらせる", "宿題を|終わらせる"),
        (
            "しんかんせんでおおさかへいきます",
            "新幹線で|大阪へ|行きます",
        ),
        ("たのしいじかんをすごした", "楽しい時間を|過ごした"),
    ];
    for (reading, expected) in cases {
        assert_eq!(joined(&d, reading), expected, "{reading}");
    }
}

/// 精度の目安の確認（誤変換を含むので assert せず出力を残す）
#[test]
fn report_other_sentences() {
    let Some(d) = load() else { return };
    for s in [
        "にほんごをにゅうりょくする",
        "かんじへんかんのてすと",
        "これはほんです",
        "あしたがっこうにいきます",
        "かれはきのうびょういんにいったそうです",
        "あめがふっているのでかさをもっていく",
        "ぼくはまいにちにほんごをべんきょうしています",
        "いまなんじですか",
    ] {
        eprintln!("{s} -> {}", joined(&d, s));
    }
}

#[test]
fn segment_candidates_on_real_data() {
    let Some(d) = load() else { return };
    let c = d.segment_candidates("かんじ", 10);
    let surf: Vec<_> = c.iter().map(|x| x.surface.as_str()).collect();
    assert!(
        surf.contains(&"漢字") && surf.contains(&"感じ") && surf.contains(&"幹事"),
        "{surf:?}"
    );
    let c = d.segment_candidates("きょうは", 10);
    assert_eq!(c[0].surface, "今日は");
    let unique: std::collections::HashSet<_> = c.iter().map(|x| &x.surface).collect();
    assert_eq!(unique.len(), c.len());
}

#[test]
fn every_kana_string_is_convertible_and_fast() {
    let Some(d) = load() else { return };
    // 辞書に無い並びでも必ず経路が存在し、読みが保存される
    let reading = "ぬぬぬぬぬぬぬぬぬぬぬぬぬぬぬぬぬぬぬぬ、。「」ー";
    let segs = d.convert(reading);
    assert_eq!(
        segs.iter().map(|s| s.reading.as_str()).collect::<String>(),
        reading
    );
    let long = "きょうはいいてんきなのででんしゃにのっておおさかへいくつもりですがあしたはあめがふるそうです"
        .repeat(2);
    let t = Instant::now();
    let segs = d.convert(&long);
    let elapsed = t.elapsed();
    eprintln!(
        "long ({} chars) converted in {:?}",
        long.chars().count(),
        elapsed
    );
    assert_eq!(
        segs.iter().map(|s| s.reading.as_str()).collect::<String>(),
        long
    );
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "{elapsed:?}"
    );
}

/// 強制アライメント: 辞書自身の変換結果を表記として与えたとき、文節区切りまで一致する
#[test]
fn align_reproduces_dictionary_conversion() {
    let Some(d) = load() else { return };
    for reading in [
        "きょうはいいてんきです",
        "わたしのなまえはなかのです",
        "とうきょうにいく",
        "すしをたべたい",
        "しんかんせんでおおさかへいきます",
        "たのしいじかんをすごした",
        "にほんごをにゅうりょくする",
    ] {
        let converted = d.convert(reading);
        let surface: String = converted.iter().map(|s| s.surface.as_str()).collect();
        let aligned = d.align(reading, &surface).expect("アライメント");
        assert_eq!(aligned, converted, "{reading} / {surface}");
    }
}

/// LLM が返しそうな、辞書の最良経路とは違う表記のアライメント（出力を確認できるようにする）
#[test]
fn align_other_surfaces() {
    let Some(d) = load() else { return };
    for (reading, surface) in [
        ("かんじへんかんのてすと", "漢字変換のテスト"),
        ("かれはきのうびょういんにいった", "彼は昨日病院に行った"),
        ("ぼくはがっこうにいく", "僕は学校に行く"),
        ("しゅうかつのめんせつ", "就活の面接"),
        ("なかのです", "中野です"),
        ("すずきさんがきた", "鈴木さんが来た"),
        ("らーめんをたべる", "ラーメンを食べる"),
    ] {
        let v = d
            .align(reading, surface)
            .unwrap_or_else(|| panic!("{reading} / {surface}"));
        assert_eq!(
            v.iter().map(|s| s.reading.as_str()).collect::<String>(),
            reading
        );
        assert_eq!(
            v.iter().map(|s| s.surface.as_str()).collect::<String>(),
            surface
        );
        eprintln!(
            "{reading} / {surface} -> {}",
            v.iter()
                .map(|s| format!("{}({})", s.surface, s.reading))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
}

/// 実辞書で mmap 版とヒープ版の変換結果が一致する（feature `mmap`。辞書が無ければ skip）
#[cfg(feature = "mmap")]
#[test]
fn mmap_matches_heap_on_real_dictionary() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dict/nagi-dict.bin");
    if !path.exists() {
        eprintln!("skip: {} が無い（cargo xtask build-dict）", path.display());
        return;
    }
    let heap = Dictionary::load(&path).expect("辞書の読み込み");
    let mapped = Dictionary::load_mmap(&path).expect("mmap");
    assert_eq!(mapped.entry_count(), heap.entry_count());
    assert_eq!(mapped.reading_count(), heap.reading_count());
    for text in [
        "きょうはいいてんきです",
        "わたしのなまえはなかのです",
        "とうきょうにいく",
        "にほんごにゅうりょくのへんかんせいど",
        "あしたのかいぎのしりょうをじゅんびしないと",
    ] {
        assert_eq!(mapped.convert(text), heap.convert(text), "{text}");
        assert_eq!(
            mapped.segment_candidates(text, 8),
            heap.segment_candidates(text, 8)
        );
        assert_eq!(
            mapped.common_prefix_lookup(text),
            heap.common_prefix_lookup(text)
        );
        assert_eq!(mapped.has_prefix(text), heap.has_prefix(text));
    }
}

#[cfg(all(feature = "mmap", target_os = "linux"))]
fn status_kb(key: &str) -> usize {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l[key.len()..].split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

/// ロード時のヒープ（RssAnon）の比較。Linux・実辞書が必要:
/// `cargo test --release -p nagi-dict --features mmap --test real_data -- --ignored --nocapture`
#[cfg(all(feature = "mmap", target_os = "linux"))]
#[test]
#[ignore = "実辞書と /proc が必要。メモリ計測（実行は単独で: --test-threads=1）"]
fn mmap_keeps_heap_small_on_real_dictionary() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/dict/nagi-dict.bin");
    if !path.exists() {
        eprintln!("skip: {} が無い（cargo xtask build-dict）", path.display());
        return;
    }
    let anon0 = status_kb("RssAnon:");
    let mapped = Dictionary::load_mmap(&path).expect("mmap");
    for text in ["きょうはいいてんきです", "とうきょうにいく"] {
        let _ = mapped.convert(text);
    }
    let mmap_delta = status_kb("RssAnon:").saturating_sub(anon0);
    eprintln!(
        "mmap: RssAnon +{mmap_delta} kB, RssFile {} kB",
        status_kb("RssFile:")
    );
    let anon1 = status_kb("RssAnon:");
    let heap = Dictionary::load(&path).expect("辞書の読み込み");
    let heap_delta = status_kb("RssAnon:").saturating_sub(anon1);
    eprintln!("heap: RssAnon +{heap_delta} kB");
    assert!(
        mmap_delta < 5_000,
        "mmap 版のヒープが大きい: {mmap_delta} kB"
    );
    assert!(
        heap_delta > 40_000,
        "ヒープ版の計測値が小さすぎる: {heap_delta} kB"
    );
    drop(heap);
}
