//! カタカナ語の原語候補（`Pipeline::segment_candidates`）: Rewriter の位置・設定でのオフ・データなしで壊れない

mod common;

use common::{FakeModel, dict_with_extra};
use nagi_config::Config;
use nagi_engine::ConversionBackend;
use nagi_learn::{LearnConfig, Learner};
use nagi_loanword::{Entry, IndexBuilder, LoanwordIndex};
use nagi_profile::SystemInfo;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::sync::Arc;

const EXTRA: &str = "\
こんぴゅーたー\t1\t1\t3000\tコンピューター
あるばいと\t1\t1\t3000\tアルバイト
";

fn runtime(config: &Config) -> Arc<Runtime> {
    Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        false,
        Tunables::from_config(config),
        Learner::new(LearnConfig::default()),
        None,
    )
}

fn index() -> Arc<LoanwordIndex> {
    let mut b = IndexBuilder::new();
    for (h, lang, sp, wasei) in [
        ("コンピューター", "eng", "computer", false),
        ("アルバイト", "ger", "Arbeit", false),
    ] {
        b.add(&Entry {
            headword: h.into(),
            lang: lang.into(),
            spelling: sp.into(),
            wasei,
            rank: 0,
        });
    }
    Arc::new(LoanwordIndex::from_vec(b.build().unwrap()).unwrap())
}

fn pipeline(config: &Config, with_index: bool) -> Pipeline {
    let mut p = Pipeline::new(
        Some(dict_with_extra(EXTRA)),
        Some(Box::new(FakeModel::new(&[]))),
        runtime(config),
    );
    if with_index {
        p.set_loanword_index(Some(index()));
    }
    p
}

fn surfaces(p: &Pipeline, reading: &str) -> Vec<String> {
    p.segment_candidates(reading, "", 30)
        .into_iter()
        .map(|c| c.surface)
        .collect()
}

#[test]
fn original_word_follows_dictionary_candidates() {
    let p = pipeline(&Config::default(), true);
    let s = surfaces(&p, "こんぴゅーたー");
    let kata = s.iter().position(|x| x == "コンピューター").unwrap();
    let orig = s.iter().position(|x| x == "computer").unwrap();
    assert!(kata < orig, "{s:?}");
    assert_eq!(
        surfaces(&p, "あるばいと")
            .iter()
            .filter(|x| *x == "Arbeit")
            .count(),
        1
    );
}

#[test]
fn disabled_by_config_or_missing_data() {
    let mut off = Config::default();
    off.convert.loanword.enabled = false;
    assert!(!surfaces(&pipeline(&off, true), "こんぴゅーたー").contains(&"computer".to_string()));
    // データが無いときは、原語が出ないだけで他の候補は変わらない
    let s = surfaces(&pipeline(&Config::default(), false), "こんぴゅーたー");
    assert!(s.contains(&"コンピューター".to_string()));
    assert!(!s.contains(&"computer".to_string()));
}

#[test]
fn unrelated_readings_are_unaffected() {
    let p = pipeline(&Config::default(), true);
    assert!(
        !surfaces(&p, "きょう")
            .iter()
            .any(|x| x.is_ascii() && x.len() > 3 && x.chars().all(char::is_alphabetic))
    );
}
