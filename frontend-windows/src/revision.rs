//! revision による古い非同期通知の破棄（設計書 §3.2）。

use std::collections::HashMap;
use std::sync::Mutex;

/// セッションごとの revision を管理し、古い非同期通知を破棄する。
///
/// 同期応答（`process_key` 等）の revision を [`record`](Self::record) で記録し、
/// 非同期通知（`update_preedit` / `update_candidates`）は [`accept`](Self::accept) が true のときだけ反映する。
#[derive(Debug, Default)]
pub struct RevisionGate {
    latest: Mutex<HashMap<String, u32>>,
}

impl RevisionGate {
    /// 空のゲートを作る
    pub fn new() -> Self {
        Self::default()
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, u32>> {
        self.latest.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 同期応答の revision を記録する（小さい値では巻き戻さない）
    pub fn record(&self, session_id: &str, revision: u32) {
        let mut map = self.map();
        let current = map.get(session_id).copied().unwrap_or(0);
        if revision >= current {
            map.insert(session_id.to_string(), revision);
        }
    }

    /// 通知を反映してよいか。現在より古ければ false。新しい・同じなら記録して true
    pub fn accept(&self, session_id: &str, revision: u32) -> bool {
        let mut map = self.map();
        if let Some(&current) = map.get(session_id)
            && revision < current
        {
            return false;
        }
        map.insert(session_id.to_string(), revision);
        true
    }

    /// 全セッションの記録を捨てる（エンジン再起動時。revision は 0 から始まり直すため）
    pub fn reset(&self) {
        self.map().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discards_older_and_accepts_newer_or_equal() {
        let gate = RevisionGate::new();
        gate.record("s1", 5);
        assert!(!gate.accept("s1", 4));
        assert!(gate.accept("s1", 5));
        assert!(gate.accept("s1", 6));
        assert!(!gate.accept("s1", 5));
    }

    #[test]
    fn record_does_not_roll_back() {
        let gate = RevisionGate::new();
        gate.record("s1", 5);
        gate.record("s1", 3);
        assert!(!gate.accept("s1", 4));
    }

    #[test]
    fn sessions_are_independent_and_unknown_session_accepts() {
        let gate = RevisionGate::new();
        gate.record("s1", 9);
        assert!(gate.accept("s2", 1));
        assert!(!gate.accept("s1", 8));
    }

    #[test]
    fn reset_forgets_everything() {
        let gate = RevisionGate::new();
        gate.record("s1", 9);
        gate.reset();
        assert!(gate.accept("s1", 0));
    }
}
