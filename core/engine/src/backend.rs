//! 変換バックエンド: engine が変換を依頼する相手（辞書 + Viterbi、LLM など）の抽象。
//!
//! 候補の供給元を差し替えられるように、状態機械は [`ConversionBackend`] だけを見る。
//! 辞書との接続は `nagi-server` 側の実装が行う。

use nagi_fuzzy::{PrefixOracle, ReadingHypothesis};
use nagi_protocol::view::CandidateSource;
use std::collections::HashMap;
use std::sync::Arc;

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

/// [`ConversionBackend::sentence_candidates`] が返す、文全体の N-best の 1 つの切り方。
/// 文節列。reading を連結すると入力の reading に一致すること（`convert_sentence` と同じ制約）
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

/// [`ConversionBackend::convert_hypotheses`] が返す、1 つの読み仮説の文全体変換（曖昧入力）
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredSentence {
    /// 変換結果の文節列。reading を連結すると、その仮説の reading に一致すること
    pub segments: Vec<BackendSegment>,
    /// 変換コスト（小さいほど良い。単位は nat 相当で、[`ReadingQuality::conv_cost`] と同じ尺度。
    /// 入力コスト（`ReadingHypothesis::cost`）と足し合わせて順位を決める）
    pub conv_cost: f32,
    /// 渡した `hyps` の何番目の仮説か（0 始まり）
    pub hypothesis_index: usize,
}

/// 読みの変換品質（[`ConversionBackend::quality`]）。曖昧入力で補正仮説を走らせるか判定するのに使う
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReadingQuality {
    /// 辞書に載らない読みの割合（0.0〜1.0）
    pub uncovered: f32,
    /// 変換コスト（[`ScoredSentence::conv_cost`] と同じ尺度。小さいほど良い）
    pub conv_cost: f32,
}

/// バックエンドの非同期処理が完了したことの通知。`convert_sentence` に渡された引数と同じ値を持つ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// ひらがなの読み
    pub reading: String,
    /// 左文脈
    pub left_context: String,
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

    /// 文全体の N-best（別の切り方・変換の候補列。辞書ベースの k-best、実LLMのビームサーチなど
    /// 供給元はバックエンド次第）。良い順、最大 `limit` 件。
    ///
    /// 候補ウィンドウの `sentence` モード（設計書 §4.4・`conversion.initial_view = sentence`）用。
    /// 既定実装は非対応で空を返す（呼び出し側は `convert_sentence`（1-best）のみのフォールバック
    /// として扱う。2 件未満なら sentence モードには入らない）
    fn sentence_candidates(
        &self,
        reading: &str,
        left_context: &str,
        limit: usize,
    ) -> Vec<BackendSentence> {
        let _ = (reading, left_context, limit);
        Vec::new()
    }

    /// 曖昧入力（補正候補）の枝刈りに使う辞書の照会。辞書を持つバックエンドだけ `Some` を返す
    /// （既定は `None`。`None` のバックエンドでは補正候補は出ない）
    fn prefix_oracle(&self) -> Option<&dyn PrefixOracle> {
        None
    }

    /// 読みの変換品質（辞書に載らない割合と変換コスト）。第一仮説（打たれたとおりの読み）の
    /// 変換が良好かの判定と、補正仮説との比較に使う。既定は非対応（`None`）
    fn quality(&self, reading: &str) -> Option<ReadingQuality> {
        let _ = reading;
        None
    }

    /// 読みの仮説ごとの文全体変換（曖昧入力）。`hyps` は**第一仮説（打たれたとおりの読み）を
    /// 含まない**補正仮説で、第一仮説は呼び出し側が `convert_sentence` / [`ConversionBackend::quality`]
    /// で別に扱う。仮説の入力コストとの合成（順位付け）は呼び出し側が行うので、ここでは変換コスト
    /// だけを付ける。`hypothesis_index` は `hyps` の添字。結果は仮説の順（変換できなかった仮説は
    /// 飛ばしてよい）、最大 `limit` 件。既定は非対応で空を返す
    fn convert_hypotheses(
        &self,
        hyps: &[ReadingHypothesis],
        left_context: &str,
        limit: usize,
    ) -> Vec<ScoredSentence> {
        let _ = (hyps, left_context, limit);
        Vec::new()
    }

    /// 溜まった完了通知を取り出す（取り出すと空になる）
    fn take_completions(&self) -> Vec<Completion> {
        Vec::new()
    }

    /// 完了通知が積まれたときに呼ぶ関数を登録する
    fn set_waker(&self, _waker: Arc<dyn Fn() + Send + Sync>) {}
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
