//! 変換バックエンド: engine が変換を依頼する相手（辞書 + Viterbi、LLM など）の抽象。
//!
//! 候補の供給元を差し替えられるように、状態機械は [`ConversionBackend`] だけを見る。
//! 辞書との接続は `nagi-server` 側の実装が行う。

use nagi_protocol::view::CandidateSource;
use std::collections::HashMap;

/// [`ConversionBackend::convert_sentence`] が返す 1 文節
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSegment {
    /// ひらがなの読み
    pub reading: String,
    /// 変換結果の表記（最良の 1 案）
    pub surface: String,
    /// 出所
    pub sources: Vec<CandidateSource>,
}

/// 文全体のN-best候補の1つ（[`ConversionBackend::sentence_candidates`] 用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSentence {
    /// 文節に区切られたセグメント列
    pub segments: Vec<BackendSegment>,
}

/// 1 文節分の候補
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCandidate {
    /// 表記
    pub surface: String,
    /// 出所
    pub sources: Vec<CandidateSource>,
}

/// 変換バックエンド
pub trait ConversionBackend: Send + Sync {
    /// バックエンド名（診断用。入力内容は含めない）
    fn name(&self) -> &str;

    /// ひらがなだけから成る読みを文節に区切り、最良の変換を返す。
    ///
    /// `left_context` は直前の確定済み・Literal の表記（推論の文脈用。辞書だけなら無視してよい）。
    /// 返す文節の reading を連結すると入力の reading に一致すること。
    fn convert_sentence(&self, reading: &str, left_context: &str) -> Vec<BackendSegment>;

    /// 文節 1 つ分の候補（良い順、最大 `limit` 件）。ひらがな・カタカナの補完は engine 側で行う
    fn segment_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendCandidate>;

    /// 予測変換の候補（学習キャッシュ・ユーザー辞書の前方一致。良い順、最大 `limit` 件）。
    /// `prefix` は入力中の読み（ひらがな）。既定実装は非対応（空を返す）
    fn predict(&self, prefix: &str, left_context: &str, limit: usize) -> Vec<BackendCandidate> {
        let _ = (prefix, left_context, limit);
        Vec::new()
    }

    /// 確定済みの表記（surface）から読みを推定し、文節に区切る（`reconvert` 用）。
    /// 辞書を持たないバックエンドは非対応でよい（既定実装は非対応。空を返す）
    fn reconvert(&self, surface: &str) -> Vec<BackendSegment> {
        let _ = surface;
        Vec::new()
    }

    /// 文全体のN-best（`conversion.initial_view = "sentence"` 用。上位 `limit` 件、コスト昇順)。
    /// 辞書を持たないバックエンドは非対応でよい（既定実装は非対応。空を返す。呼び出し側は
    /// 通常の `convert_sentence`（1-best）にフォールバックする）
    fn sentence_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendSentence> {
        let _ = (reading, left_context, limit);
        Vec::new()
    }
}

/// 辞書もモデルも使わないバックエンド（読みをそのまま返す）
#[derive(Debug, Clone, Copy, Default)]
pub struct KanaBackend;

impl ConversionBackend for KanaBackend {
    fn name(&self) -> &str {
        "kana"
    }

    fn convert_sentence(&self, reading: &str, _left_context: &str) -> Vec<BackendSegment> {
        vec![BackendSegment {
            reading: reading.to_string(),
            surface: reading.to_string(),
            sources: vec![CandidateSource::Kana],
        }]
    }

    fn segment_candidates(&self, _: &str, _: &str, _: usize) -> Vec<BackendCandidate> {
        Vec::new()
    }
}

/// テスト用に変換結果を登録できるバックエンド
#[derive(Debug, Clone, Default)]
pub struct MockBackend {
    sentences: HashMap<String, Vec<(String, Vec<String>)>>,
    segments: HashMap<String, Vec<String>>,
    predictions: HashMap<String, Vec<String>>,
}

impl MockBackend {
    /// 空のバックエンド
    pub fn new() -> Self {
        Self::default()
    }

    /// 文の変換結果を登録する: 読み全体 → 文節ごとの (読み, 候補一覧)。`convert_sentence` は
    /// 各文節の先頭候補を表記として返す
    pub fn with_sentence(mut self, reading: &str, segments: Vec<(&str, Vec<&str>)>) -> Self {
        self.sentences.insert(
            reading.to_string(),
            segments
                .into_iter()
                .map(|(r, c)| (r.to_string(), c.into_iter().map(str::to_string).collect()))
                .collect(),
        );
        self
    }

    /// 文節単独の候補を登録する（`with_sentence` の文節より優先）
    pub fn with_segments(mut self, reading: &str, candidates: Vec<&str>) -> Self {
        self.segments.insert(
            reading.to_string(),
            candidates.into_iter().map(str::to_string).collect(),
        );
        self
    }

    /// 予測変換の候補を登録する（読みの前方一致で使われる。テスト用）
    pub fn with_predictions(mut self, prefix: &str, candidates: Vec<&str>) -> Self {
        self.predictions.insert(
            prefix.to_string(),
            candidates.into_iter().map(str::to_string).collect(),
        );
        self
    }
}

impl ConversionBackend for MockBackend {
    fn name(&self) -> &str {
        "mock"
    }

    fn convert_sentence(&self, reading: &str, left_context: &str) -> Vec<BackendSegment> {
        match self.sentences.get(reading) {
            Some(segments) => segments
                .iter()
                .map(|(r, c)| BackendSegment {
                    reading: r.clone(),
                    surface: c.first().cloned().unwrap_or_else(|| r.clone()),
                    sources: vec![CandidateSource::SystemDict],
                })
                .collect(),
            None => KanaBackend.convert_sentence(reading, left_context),
        }
    }

    fn segment_candidates(
        &self,
        reading: &str,
        _left_context: &str,
        limit: usize,
    ) -> Vec<BackendCandidate> {
        let found = self.segments.get(reading).cloned().or_else(|| {
            self.sentences
                .values()
                .flatten()
                .find(|(r, _)| r == reading)
                .map(|(_, c)| c.clone())
        });
        found
            .unwrap_or_default()
            .into_iter()
            .take(limit)
            .map(|surface| BackendCandidate {
                surface,
                sources: vec![CandidateSource::SystemDict],
            })
            .collect()
    }

    fn predict(&self, prefix: &str, _left_context: &str, limit: usize) -> Vec<BackendCandidate> {
        self.predictions
            .get(prefix)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(limit)
            .map(|surface| BackendCandidate {
                surface,
                sources: vec![CandidateSource::Learned],
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kana_backend_returns_reading() {
        let b = KanaBackend;
        let segs = b.convert_sentence("きょう", "");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].surface, "きょう");
        assert!(b.segment_candidates("きょう", "", 5).is_empty());
    }

    #[test]
    fn mock_backend_sentence_and_segments() {
        let b = MockBackend::new()
            .with_sentence(
                "きょうは",
                vec![("きょう", vec!["今日", "京"]), ("は", vec!["は"])],
            )
            .with_segments("いい", vec!["良い", "いい"]);
        let segs = b.convert_sentence("きょうは", "");
        assert_eq!(
            segs.iter().map(|s| s.surface.as_str()).collect::<Vec<_>>(),
            ["今日", "は"]
        );
        // 未登録は読みそのまま
        assert_eq!(b.convert_sentence("あ", "")[0].surface, "あ");
        // 文の文節からの候補、単独登録、limit、未登録
        let c = b.segment_candidates("きょう", "", 5);
        assert_eq!(
            c.iter().map(|x| x.surface.as_str()).collect::<Vec<_>>(),
            ["今日", "京"]
        );
        assert_eq!(b.segment_candidates("いい", "", 1).len(), 1);
        assert!(b.segment_candidates("ぬ", "", 5).is_empty());
    }
}
