//! `Pipeline::segment_candidates` に Rewriter（全角/半角変換）の候補が
//! `CandidateSource::Rewriter` として出所付きで含まれることの統合テスト。

use nagi_engine::ConversionBackend;
use nagi_learn::{LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_protocol::view::CandidateSource;
use nagi_server::{Pipeline, Runtime, Tunables};
use std::time::Duration;

mod common;
use common::{FakeModel, dict};

fn backend(model: FakeModel) -> Pipeline {
    let runtime = Runtime::new(
        SystemInfo {
            total_memory_mb: 16384,
            cpu_cores: 8,
            has_gpu: false,
        },
        true,
        Tunables {
            sync_budget: Duration::from_millis(1000),
            ..Tunables::default()
        },
        Learner::new(LearnConfig::default()),
        None,
    );
    Pipeline::new(Some(dict()), Some(Box::new(model)), runtime)
}

#[test]
fn halfwidth_ascii_reading_gets_fullwidth_rewriter_candidate() {
    let b = backend(FakeModel::new(&[]));
    let c = b.segment_candidates("abc123", "", 10);
    let hit = c
        .iter()
        .find(|cand| cand.surface == "ａｂｃ１２３")
        .expect("全角変換候補が含まれるはず");
    assert!(
        hit.sources.contains(&CandidateSource::Rewriter),
        "出所は Rewriter であるはず: {:?}",
        hit.sources
    );
}
