//! 辞書（`nagi-dict`）を engine の変換バックエンドとして使う `minimal` 経路の実装。

use nagi_dict::Dictionary;
use nagi_engine::{BackendCandidate, BackendSegment, ConversionBackend};
use nagi_protocol::view::CandidateSource;
use std::sync::Arc;

/// 辞書 + Viterbi だけで変換するバックエンド（モデル無し・障害時の最後の砦）
pub struct DictBackend {
    dict: Arc<Dictionary>,
}

impl DictBackend {
    /// 読み込み済みの辞書から作る
    pub fn new(dict: Arc<Dictionary>) -> Self {
        DictBackend { dict }
    }
}

fn sources_for(surface: &str, reading: &str) -> Vec<CandidateSource> {
    if surface == reading {
        vec![CandidateSource::Kana]
    } else {
        vec![CandidateSource::SystemDict]
    }
}

impl ConversionBackend for DictBackend {
    fn name(&self) -> &str {
        "dict"
    }

    fn convert_sentence(&self, reading: &str, _left_context: &str) -> Vec<BackendSegment> {
        self.dict
            .convert(reading)
            .into_iter()
            .map(|s| BackendSegment {
                sources: sources_for(&s.surface, &s.reading),
                reading: s.reading,
                surface: s.surface,
            })
            .collect()
    }

    fn segment_candidates(
        &self,
        reading: &str,
        _left_context: &str,
        limit: usize,
    ) -> Vec<BackendCandidate> {
        self.dict
            .segment_candidates(reading, limit)
            .into_iter()
            .map(|c| BackendCandidate {
                sources: sources_for(&c.surface, reading),
                surface: c.surface,
            })
            .collect()
    }
}
