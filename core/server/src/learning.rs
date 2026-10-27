//! 学習: engine の確定通知（[`CommitObserver`]）を受けて、確定した文節を学習に記録する。
//!
//! 学習を止める条件（設計書 §4.11）: 設定で無効、secure input 中、パスワードマネージャ等の
//! アプリ（`learning.disabled_apps`）。停止中は記録もしない。誤学習の削除は候補ウィンドウの
//! `Ctrl+Delete`（engine の `DeleteLearning`）から [`CommitObserver::on_forget`] で来る。

use crate::runtime::Runtime;
use nagi_engine::{CommitContext, CommitObserver, CommittedSegment};
use nagi_learn::context_hash;
use nagi_protocol::view::SegmentKind;
use std::sync::Arc;

/// 確定を学習に記録する observer
pub struct LearningObserver {
    runtime: Arc<Runtime>,
}

impl LearningObserver {
    /// 作る
    pub fn new(runtime: Arc<Runtime>) -> Self {
        LearningObserver { runtime }
    }
}

impl CommitObserver for LearningObserver {
    fn on_commit(&self, ctx: &CommitContext<'_>, segments: &[CommittedSegment]) {
        if !self.runtime.learning_allowed(ctx.secure_input, ctx.app_id) {
            return;
        }
        for seg in segments {
            if seg.kind != SegmentKind::Japanese {
                continue;
            }
            // 日付・時刻の候補（きょう → 2026/10/27 など）は日が変われば古くなるので学習しない
            if nagi_rewriter::is_date_time_candidate(&seg.reading, &seg.surface) {
                continue;
            }
            // reading が空なのは確定直後の次語候補をそのまま選んだ場合（単語の学習はしない）
            if !seg.reading.is_empty() {
                self.runtime.learn_record(
                    &seg.reading,
                    &seg.surface,
                    context_hash(&seg.left_context),
                );
            }
            if !seg.prev_word.is_empty() && !seg.surface.is_empty() {
                self.runtime.learn_record_next(&seg.prev_word, &seg.surface);
            }
        }
    }

    fn on_forget(&self, ctx: &CommitContext<'_>, reading: &str, surface: &str) -> bool {
        // secure input 中は学習データに触れない（削除の有無も外に出さない）
        if ctx.secure_input {
            return false;
        }
        self.runtime.learn_forget(reading, surface)
    }
}
