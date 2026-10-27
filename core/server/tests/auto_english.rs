//! 英語の自動判定（`Pipeline::english_level`）: 日本語として引ける読みの優先・語彙なしの劣化動作・
//! 評価コーパス（実データがあるときだけ）での誤判定率と取りこぼし率

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_engine::ConversionBackend;
use nagi_english::{Level, Lexicon, LexiconBuilder};
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_romaji::{Composer, RomajiTable, ShiftLetterMode};
use nagi_server::{Pipeline, Runtime, Tunables};
use std::path::PathBuf;
use std::sync::Arc;

const EXTRA: &str = "\
ばなな\t1\t1\t3000\tバナナ
";

fn runtime() -> Arc<Runtime> {
    Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        false,
        Tunables::from_config(&Config::default()),
        Learner::new(LearnConfig::default()),
        None,
    )
}

fn lexicon(words: &[(&str, u8)]) -> Arc<Lexicon> {
    let mut b = LexiconBuilder::new();
    for (w, t) in words {
        b.add(w, *t);
    }
    Arc::new(Lexicon::from_vec(b.build().unwrap()).unwrap())
}

/// 打鍵列を日本語として読んだかな（保留の n は ん）
fn reading_of(raw: &str) -> String {
    let mut c = Composer::new(RomajiTable::standard(), ShiftLetterMode::LiteralContinue);
    for ch in raw.chars() {
        c.insert_char(ch);
    }
    c.flush_pending();
    c.text()
}

fn pipeline(lex: Option<Arc<Lexicon>>) -> Pipeline {
    let mut p = Pipeline::new(
        Some(dict_with_extra(EXTRA)),
        Some(Box::new(FakeModel::new(&[]))),
        runtime(),
    );
    p.set_english_lexicon(lex);
    p
}

fn level(p: &Pipeline, raw: &str) -> Level {
    p.english_level(raw, &reading_of(raw))
}

#[test]
fn readings_known_to_the_dictionary_stay_japanese() {
    let p = pipeline(Some(lexicon(&[
        ("banana", 20),
        ("data", 10),
        ("typescript", 70),
    ])));
    assert!(p.has_english_lexicon());
    // 辞書で引ける（ばなな）→ 語彙にあっても日本語を優先
    assert_eq!(level(&p, "banana"), Level::None);
    // 辞書で引けない有効なローマ字 → 中程度
    assert_eq!(level(&p, "data"), Level::Mid);
    // 日本語に無い子音連続を含む語彙の語 → 高い
    assert_eq!(level(&p, "typescript"), Level::High);
    // 語彙に無い日本語
    assert_eq!(level(&p, "kyouha"), Level::None);
}

#[test]
fn works_without_a_lexicon_using_features_only() {
    let p = pipeline(None);
    assert!(!p.has_english_lexicon());
    assert_eq!(level(&p, "banana"), Level::None);
    assert_eq!(level(&p, "data"), Level::None);
    assert!(level(&p, "typescript") >= Level::Mid);
    assert_eq!(level(&p, "kyouha"), Level::None);
}

#[test]
fn user_dictionary_and_learning_make_a_reading_known() {
    // 学習で「だた」が引けるようになると、`data` は日本語として尤もらしい
    let rt = runtime();
    let mut p = Pipeline::new(
        Some(dict_with_extra(EXTRA)),
        Some(Box::new(FakeModel::new(&[]))),
        rt.clone(),
    );
    p.set_english_lexicon(Some(lexicon(&[("data", 10)])));
    assert_eq!(level(&p, "data"), Level::Mid);
    rt.learn_record("だた", "ダタ", nagi_learn::context_hash(""));
    assert_eq!(level(&p, "data"), Level::None);
}

#[test]
fn spellings_use_the_lexicon_and_keep_japanese_first() {
    use nagi_engine::ConversionBackend;
    let p = pipeline(Some(lexicon(&[
        ("typescript", 70),
        ("world", 30),
        ("banana", 20),
    ])));
    let spell = |raw: &str| p.english_spellings(raw, &reading_of(raw), 3);
    assert_eq!(spell("typscript"), ["typescript"]);
    // QWERTY で隣のキー（o → p）の打ち間違い
    assert_eq!(spell("wprld"), ["world"]);
    // 語彙にある語には出さない
    assert!(spell("typescript").is_empty());
    // 有効なローマ字（ばなか）は日本語を優先し、小さい誤り（隣のキー・入れ替え）だけ
    assert!(spell("banaka").is_empty());
    assert_eq!(spell("banama"), ["banana"]);
    assert!(
        pipeline(None)
            .english_spellings("typscript", "", 3)
            .is_empty()
    );
}

/// 実データ（`data/dict/nagi-dict.bin` と `data/english/words.fst`）で評価コーパスを測る。無ければ省略
#[test]
fn corpus_with_real_data_if_available() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (dict_path, lex_path) = (
        root.join("data/dict/nagi-dict.bin"),
        root.join("data/english/words.fst"),
    );
    if !dict_path.exists() || !lex_path.exists() {
        eprintln!("実データが無いので省略（cargo xtask fetch-dict / build-dict / fetch-english）");
        return;
    }
    let dict = Arc::new(nagi_dict::Dictionary::load(&dict_path).unwrap());
    let mut p = Pipeline::new(Some(dict), None, runtime());
    p.set_english_lexicon(Some(Arc::new(Lexicon::load(&lex_path).unwrap())));
    let text = std::fs::read_to_string(root.join("bench/auto-english/corpus.tsv")).unwrap();
    let (mut neg, mut fp_mid, mut fp_high) = (0, Vec::new(), Vec::new());
    let (mut pos, mut miss_mid, mut miss_high) = (0, Vec::new(), Vec::new());
    for l in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let c: Vec<&str> = l.split('\t').collect();
        let lv = level(&p, c[1]);
        match c[0] {
            "ja" | "mixed" => {
                neg += 1;
                if lv >= Level::Mid {
                    fp_mid.push(c[1].to_string());
                }
                if lv >= Level::High {
                    fp_high.push(c[1].to_string());
                }
            }
            "en" => {
                pos += 1;
                if lv < Level::Mid {
                    miss_mid.push(c[1].to_string());
                }
                if lv < Level::High {
                    miss_high.push(c[1].to_string());
                }
            }
            _ => {}
        }
    }
    println!(
        "[実辞書+SCOWL] 誤判定 Mid+ {}/{neg} High {}/{neg} / 取りこぼし Mid+ {}/{pos} High {}/{pos}",
        fp_mid.len(),
        fp_high.len(),
        miss_mid.len(),
        miss_high.len()
    );
    println!("  誤判定(Mid+): {fp_mid:?}\n  取りこぼし(Mid+): {miss_mid:?}");
    assert!(fp_high.is_empty(), "{fp_high:?}");
    assert!(fp_mid.len() * 100 <= neg, "{fp_mid:?}");
    assert!(miss_mid.len() * 100 <= pos * 12, "{miss_mid:?}");

    // 通しの確認: 実バックエンドで、Space の 2 番目に英字が出る（先頭は打った通りのまま）
    let mut e = nagi_engine::Engine::new(Arc::new(p), nagi_engine::EngineConfig::default());
    for c in "typescript".chars() {
        e.process_key("s", &nagi_engine::parse_key(&c.to_string()).unwrap());
    }
    let r = e.process_key("s", &nagi_engine::parse_key("Space").unwrap());
    let items: Vec<String> = r
        .candidates
        .expect("候補ウィンドウ")
        .items
        .into_iter()
        .map(|i| i.surface)
        .collect();
    println!("  typescript の候補: {items:?}");
    assert_eq!(
        items.iter().position(|i| i == "typescript"),
        Some(1),
        "{items:?}"
    );
}

/// 綴りの「もしかして」を実データ（`data/english/words.fst`。辞書 `data/dict/nagi-dict.bin` もあれば使う）で
/// 測る: 打ち間違いのコーパスの上位 3 件の再現率、日本語・日英混在の入力に出てしまう件数、1 回の時間。
/// 語彙が無ければ省略
#[test]
fn spellings_with_real_data_if_available() {
    use nagi_engine::ConversionBackend;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lex_path = root.join("data/english/words.fst");
    if !lex_path.exists() {
        eprintln!("実データが無いので省略（cargo xtask fetch-english）");
        return;
    }
    let dict_path = root.join("data/dict/nagi-dict.bin");
    let dict = dict_path
        .exists()
        .then(|| Arc::new(nagi_dict::Dictionary::load(&dict_path).unwrap()));
    let mut p = Pipeline::new(dict, None, runtime());
    p.set_english_lexicon(Some(Arc::new(Lexicon::load(&lex_path).unwrap())));

    let typos = std::fs::read_to_string(root.join("bench/auto-english/typos.tsv")).unwrap();
    let (mut total, mut top1, mut top3, mut missed) = (0, 0, 0, Vec::new());
    let mut slowest = std::time::Duration::ZERO;
    for l in typos
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (typo, want) = l.split_once('\t').unwrap();
        total += 1;
        let start = std::time::Instant::now();
        let got = p.english_spellings(typo, &reading_of(typo), 3);
        slowest = slowest.max(start.elapsed());
        match got.iter().position(|w| w == want) {
            Some(0) => {
                top1 += 1;
                top3 += 1;
            }
            Some(_) => top3 += 1,
            None => missed.push(format!("{typo}→{want} {got:?}")),
        }
    }
    let corpus = std::fs::read_to_string(root.join("bench/auto-english/corpus.tsv")).unwrap();
    let (mut neg, mut noisy) = (0, Vec::new());
    for l in corpus
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let c: Vec<&str> = l.split('\t').collect();
        if c[0] == "ja" || c[0] == "mixed" {
            neg += 1;
            let got = p.english_spellings(c[1], &reading_of(c[1]), 3);
            if !got.is_empty() {
                noisy.push(format!("{} {got:?}", c[1]));
            }
        }
    }
    println!(
        "[SCOWL{}] 綴り: 1 位 {top1}/{total}・3 位以内 {top3}/{total} / 日本語・混在に出た {}/{neg} / 最長 {slowest:?}",
        if dict_path.exists() { "+辞書" } else { "" },
        noisy.len()
    );
    println!("  取りこぼし: {missed:?}\n  日本語・混在に出た: {noisy:?}");
    // 実測（docs/decisions.md H-70）から余裕を持たせた下限
    assert!(top3 * 100 >= total * 60, "{missed:?}");
    assert!(noisy.len() * 100 <= neg * 10, "{noisy:?}");
}
