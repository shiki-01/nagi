//! Nagi カタカナ語の原語候補（`docs/plan-loanword-translation.md`）。
//!
//! JMdict の `lsource`（外来語の原語の言語と綴り）から組み立てた fst 索引を引き、
//! カタカナ語の原語（computer、Arbeit など）を変換候補にする。
//!
//! - `normalize`: カタカナの正規化（長音・小書き・中黒・ヴ）と骨格
//! - `archive`: 取得済みの JMdict（gzip）から索引ファイルを組み立てる
//! - `jmdict`: JMdict の XML から索引の入力を取り出す
//! - `index`: fst 索引の組み立てと問い合わせ
//! - `rewriter`: `nagi-rewriter` の [`nagi_rewriter::Rewriter`] 実装

pub mod archive;
pub mod index;
pub mod jmdict;
pub mod normalize;
pub mod rewriter;

pub use index::{Entry, IndexBuilder, Loan, LoanwordError, LoanwordIndex, MAX_RESULTS};
pub use rewriter::{LOANWORD_REWRITER_ID, LoanwordRewriter};
