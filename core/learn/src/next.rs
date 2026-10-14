//! 次語予測用の学習: 「直前に確定した語 → 次に確定した語」のバイグラム。
//!
//! 通常の学習（[`crate::Learner`]、`learning.tsv`）とは別ファイル（`learning-next.tsv`）に持つ。
//! 旧版は `learning.tsv` しか読まないので、このファイルの有無に影響されない。
//! flush は [`crate::Learner`] と同じ流儀（`<path>.lock` の flock・3 者マージ・世代・原子的置換）。

use crate::{
    FlushLock, LearnConfig, LearnError, LoadReport, calculate_score, create_backup_file,
    escape_tsv_field, open_lock, system_now, unescape_tsv_field,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// 次語学習ファイルの 1 行。`(prev, next)` がキー。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextRow {
    /// 直前に確定した語（表記）。
    pub prev: String,
    /// その次に確定した語（表記）。
    pub next: String,
    /// 出現回数。
    pub count: u32,
    /// 最終使用時刻（unix 秒）。
    pub last_used: u64,
}

/// スコア計算済みの次語候補。
#[derive(Clone, Debug, PartialEq)]
pub struct NextScored {
    /// 次語（表記）。
    pub surface: String,
    /// スコア（高いほど良好）。
    pub score: f64,
}

type NextKey = (String, String);

struct NextFileData {
    generation: u64,
    rows: HashMap<NextKey, NextRow>,
    skipped_lines: usize,
}

/// 次語予測の学習データ管理。
#[derive(Debug)]
pub struct NextWordLearner {
    config: LearnConfig,
    rows: HashMap<NextKey, NextRow>,
    dirty: bool,
    /// 最後にファイルと同期した時点の内容（3 者マージの基準）。
    base: HashMap<NextKey, NextRow>,
    generation: u64,
    cleared: bool,
    /// 解釈できない行のバックアップを取った（flush で捨てる前に 1 度だけ取る）。
    backed_up: bool,
    /// 最後の同期以降に忘れさせた次語の表記（ファイル側の行も消す）。
    forgotten: HashSet<String>,
}

impl NextWordLearner {
    /// 空の学習データを作る。
    pub fn new(config: LearnConfig) -> Self {
        Self {
            config,
            rows: HashMap::new(),
            dirty: false,
            base: HashMap::new(),
            generation: 0,
            cleared: false,
            backed_up: false,
            forgotten: HashSet::new(),
        }
    }

    /// 設定を変更する（`max_entries`・`half_life_days`・`forget_days` が効く）。
    pub fn set_config(&mut self, config: LearnConfig) {
        self.config = config;
    }

    /// 「`prev` の次に `next` を確定した」を 1 回記録する。どちらかが空なら何もしない。
    pub fn record(&mut self, prev: &str, next: &str, now: u64) {
        if prev.is_empty() || next.is_empty() {
            return;
        }
        let key = (prev.to_string(), next.to_string());
        match self.rows.get_mut(&key) {
            Some(row) => {
                row.count = row.count.saturating_add(1);
                row.last_used = now;
            }
            None => {
                self.rows.insert(
                    key,
                    NextRow {
                        prev: prev.to_string(),
                        next: next.to_string(),
                        count: 1,
                        last_used: now,
                    },
                );
            }
        }
        self.dirty = true;
        if self.rows.len() > self.config.max_entries {
            self.evict(now);
        }
    }

    /// `prev` の次に来やすい語（スコア降順、最大 `limit` 件）。
    pub fn lookup(&self, prev: &str, now: u64, limit: usize) -> Vec<NextScored> {
        let mut out: Vec<NextScored> = self
            .rows
            .values()
            .filter(|r| r.prev == prev)
            .map(|r| NextScored {
                surface: r.next.clone(),
                score: calculate_score(r.count, r.last_used, now, self.config.half_life_days),
            })
            .collect();
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.surface.cmp(&b.surface))
        });
        out.truncate(limit);
        out
    }

    /// 次語が `next` の行をすべて削除する（誤学習の削除に合わせる）。削除したら true。
    pub fn forget(&mut self, next: &str) -> bool {
        let before = self.rows.len();
        self.rows.retain(|_, r| r.next != next);
        self.base.retain(|_, r| r.next != next);
        let found = self.rows.len() != before;
        if found {
            self.forgotten.insert(next.to_string());
            self.dirty = true;
        }
        found
    }

    /// 全行削除。次の `flush` でファイルの世代が進み、他インスタンスの古い内容は復活しない。
    pub fn clear(&mut self) {
        self.rows.clear();
        self.base.clear();
        self.forgotten.clear();
        self.cleared = true;
        self.dirty = true;
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空かどうか。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// dirty か。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 全行（`(prev, next)` 昇順で決定的）。
    pub fn rows(&self) -> Vec<NextRow> {
        let mut v: Vec<_> = self.rows.values().cloned().collect();
        v.sort_by(|a, b| a.prev.cmp(&b.prev).then_with(|| a.next.cmp(&b.next)));
        v
    }

    /// ファイルへ書き出す。dirty でなければ Ok(false)。手順は [`crate::Learner::flush`] と同じ。
    pub fn flush(&mut self, path: &Path) -> Result<bool, LearnError> {
        if !self.dirty {
            return Ok(false);
        }
        let lock = FlushLock::acquire(path, None)?.ok_or_else(|| {
            LearnError::Io(io::Error::new(io::ErrorKind::TimedOut, "flock timed out"))
        })?;
        self.flush_with_lock(path, &lock)
    }

    /// 取得済みの [`FlushLock`] の下で書き出す（[`crate::Learner::flush_with_lock`] と同じ分割版）
    pub fn flush_with_lock(&mut self, path: &Path, _lock: &FlushLock) -> Result<bool, LearnError> {
        if !self.dirty {
            return Ok(false);
        }
        self.flush_locked(path)?;
        Ok(true)
    }

    fn flush_locked(&mut self, path: &Path) -> Result<(), LearnError> {
        let disk = match read_next_file(path) {
            Ok(d) => d,
            Err(LearnError::Io(e)) if e.kind() == io::ErrorKind::NotFound => NextFileData {
                generation: 0,
                rows: HashMap::new(),
                skipped_lines: 0,
            },
            Err(e) => return Err(e),
        };
        // 解釈できない行は書き戻すと消える: 消す前にバックアップを取る（load 時と同様。1 度だけ）
        if disk.skipped_lines > 0 && !self.backed_up {
            create_backup_file(path)?;
            self.backed_up = true;
        }

        let gen_changed = disk.generation != self.generation;
        let mut merged: HashMap<NextKey, NextRow> = HashMap::new();

        if !self.cleared {
            for (key, drow) in disk.rows {
                if self.forgotten.contains(&key.1) {
                    continue;
                }
                if !gen_changed
                    && !self.rows.contains_key(&key)
                    && self.base.get(&key) == Some(&drow)
                {
                    continue;
                }
                merged.insert(key, drow);
            }
        }

        for (key, lrow) in &self.rows {
            let b = self.base.get(key);
            let delta = match b {
                Some(b) => lrow.count.saturating_sub(b.count),
                None => lrow.count,
            };
            let changed = match b {
                Some(b) => delta > 0 || lrow.last_used > b.last_used,
                None => true,
            };
            if !changed {
                continue;
            }
            match merged.get_mut(key) {
                Some(o) => {
                    o.count = o.count.saturating_add(delta);
                    o.last_used = o.last_used.max(lrow.last_used);
                }
                None => {
                    let mut row = lrow.clone();
                    if b.is_some() {
                        row.count = delta.max(1);
                    }
                    merged.insert(key.clone(), row);
                }
            }
        }

        let sys_now = system_now();
        if self.config.forget_days > 0 {
            let limit = u64::from(self.config.forget_days) * 24 * 3600;
            merged.retain(|_, r| sys_now.saturating_sub(r.last_used) <= limit);
        }
        let max_used = merged.values().map(|r| r.last_used).max().unwrap_or(0);

        let new_generation = if self.cleared {
            disk.generation.max(self.generation) + 1
        } else if disk.generation > self.generation {
            disk.generation
        } else if disk.generation < self.generation {
            self.generation + 1
        } else {
            self.generation
        };

        let old_rows = std::mem::replace(&mut self.rows, merged);
        if self.rows.len() > self.config.max_entries {
            self.evict(max_used);
        }
        if let Err(e) = write_next_file(path, new_generation, &self.rows()) {
            self.rows = old_rows;
            return Err(e);
        }
        self.base = self.rows.clone();
        self.generation = new_generation;
        self.cleared = false;
        self.forgotten.clear();
        self.dirty = false;
        Ok(())
    }

    /// ファイルから読み込む。ファイルが無ければ空。壊れた行はスキップしてバックアップを残す。
    pub fn load(
        path: &Path,
        config: LearnConfig,
    ) -> Result<(NextWordLearner, LoadReport), LearnError> {
        let mut learner = NextWordLearner::new(config);
        let mut report = LoadReport {
            rows: 0,
            skipped_lines: 0,
            backed_up_to: None,
        };
        if !path.exists() {
            return Ok((learner, report));
        }
        let lock_file = open_lock(path)?;
        lock_file.lock_shared()?;
        let result = (|| -> Result<(), LearnError> {
            let data = read_next_file(path)?;
            report.rows = data.rows.len();
            learner.generation = data.generation;
            learner.rows = data.rows;
            if data.skipped_lines > 0 {
                report.skipped_lines = data.skipped_lines;
                report.backed_up_to = Some(create_backup_file(path)?);
                learner.backed_up = true;
            }
            Ok(())
        })();
        drop(lock_file);
        result?;
        learner.base = learner.rows.clone();
        if learner.config.forget_days > 0 {
            let limit = u64::from(learner.config.forget_days) * 24 * 3600;
            let now = system_now();
            let before = learner.rows.len();
            learner
                .rows
                .retain(|_, r| now.saturating_sub(r.last_used) <= limit);
            if learner.rows.len() != before {
                learner.dirty = true;
            }
        }
        Ok((learner, report))
    }

    fn evict(&mut self, now: u64) {
        let target = (self.config.max_entries as f64 * 0.9).ceil() as usize;
        let excess = self.rows.len().saturating_sub(target);
        if excess == 0 {
            return;
        }
        let mut entries: Vec<_> = self
            .rows
            .iter()
            .map(|(k, r)| {
                (
                    k.clone(),
                    calculate_score(r.count, r.last_used, now, self.config.half_life_days),
                    r.last_used,
                )
            })
            .collect();
        entries.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.0.cmp(&b.0))
        });
        for (key, _, _) in entries.into_iter().take(excess) {
            self.rows.remove(&key);
        }
    }
}

fn parse_next_line(line: &str) -> Result<NextRow, &'static str> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() != 4 {
        return Err("Invalid field count");
    }
    Ok(NextRow {
        prev: unescape_tsv_field(f[0])?,
        next: unescape_tsv_field(f[1])?,
        count: f[2].parse::<u32>().map_err(|_| "Invalid count")?,
        last_used: f[3].parse::<u64>().map_err(|_| "Invalid last_used")?,
    })
}

/// 次語学習ファイルを読む。1 行目は `#nagi-learn-next\tv1[\tgen=N]`。ロックは呼び出し側で取る。
fn read_next_file(path: &Path) -> Result<NextFileData, LearnError> {
    let file = fs::File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let generation = match lines.next() {
        Some(Ok(line)) => {
            let line = line.trim_end_matches('\r');
            match line.strip_prefix("#nagi-learn-next\tv1") {
                Some("") => 0,
                Some(rest) => rest
                    .strip_prefix("\tgen=")
                    .and_then(|g| g.parse::<u64>().ok())
                    .unwrap_or(0),
                None => {
                    return Err(LearnError::UnsupportedSchema {
                        found: line.to_string(),
                    });
                }
            }
        }
        Some(Err(e)) => return Err(LearnError::Io(e)),
        None => {
            return Err(LearnError::UnsupportedSchema {
                found: String::new(),
            });
        }
    };
    let mut data = NextFileData {
        generation,
        rows: HashMap::new(),
        skipped_lines: 0,
    };
    for line in lines {
        let Ok(line) = line else {
            data.skipped_lines += 1;
            continue;
        };
        if line.is_empty() {
            continue;
        }
        match parse_next_line(&line) {
            Ok(row) => {
                data.rows.insert((row.prev.clone(), row.next.clone()), row);
            }
            Err(_) => data.skipped_lines += 1,
        }
    }
    Ok(data)
}

fn write_next_file(path: &Path, generation: u64, rows: &[NextRow]) -> Result<(), LearnError> {
    let tmp_path = path.with_extension("tmp");
    let mut writer = BufWriter::new(fs::File::create(&tmp_path)?);
    writeln!(writer, "#nagi-learn-next\tv1\tgen={generation}")?;
    for r in rows {
        writeln!(
            writer,
            "{}\t{}\t{}\t{}",
            escape_tsv_field(&r.prev),
            escape_tsv_field(&r.next),
            r.count,
            r.last_used
        )?;
    }
    writer.flush()?;
    let file = writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nagi-next-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn count(l: &NextWordLearner, p: &str, n: &str) -> u32 {
        l.rows()
            .iter()
            .find(|r| r.prev == p && r.next == n)
            .map_or(0, |r| r.count)
    }

    #[test]
    fn record_and_lookup_ranks_by_frequency() {
        let mut l = NextWordLearner::new(LearnConfig::default());
        l.record("今日", "は", 100);
        l.record("今日", "は", 100);
        l.record("今日", "も", 100);
        l.record("", "x", 100);
        l.record("明日", "は", 100);
        let r = l.lookup("今日", 100, 5);
        assert_eq!(
            r.iter().map(|s| s.surface.as_str()).collect::<Vec<_>>(),
            ["は", "も"]
        );
        assert_eq!(l.lookup("今日", 100, 1).len(), 1);
        assert!(l.lookup("存在しない", 100, 5).is_empty());
        assert_eq!(l.len(), 3);
    }

    #[test]
    fn flush_load_roundtrip_and_escape() {
        let dir = temp_dir();
        let path = dir.join("learning-next.tsv");
        let mut l = NextWordLearner::new(LearnConfig::default());
        l.record("a\tb", "c\nd", 10);
        assert!(l.flush(&path).unwrap());
        assert!(!l.flush(&path).unwrap());
        let (c, report) = NextWordLearner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(report.rows, 1);
        assert_eq!(count(&c, "a\tb", "c\nd"), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_instances_merge_without_loss() {
        let dir = temp_dir();
        let path = dir.join("learning-next.tsv");
        let mut base = NextWordLearner::new(LearnConfig::default());
        base.record("今日", "は", 100);
        base.flush(&path).unwrap();

        let mut a = NextWordLearner::load(&path, LearnConfig::default())
            .unwrap()
            .0;
        let mut b = NextWordLearner::load(&path, LearnConfig::default())
            .unwrap()
            .0;
        a.record("今日", "は", 200);
        a.record("今日", "も", 200);
        b.record("今日", "は", 300);
        b.record("明日", "は", 300);
        a.flush(&path).unwrap();
        b.flush(&path).unwrap();

        let (c, _) = NextWordLearner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(count(&c, "今日", "は"), 3);
        assert_eq!(count(&c, "今日", "も"), 1);
        assert_eq!(count(&c, "明日", "は"), 1);
        // b は a の更新を取り込んでいる
        assert_eq!(count(&b, "今日", "も"), 1);
        assert!(!b.is_dirty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_is_not_resurrected_by_other_instance() {
        let dir = temp_dir();
        let path = dir.join("learning-next.tsv");
        let mut base = NextWordLearner::new(LearnConfig::default());
        base.record("今日", "は", 100);
        base.flush(&path).unwrap();
        let mut a = NextWordLearner::load(&path, LearnConfig::default())
            .unwrap()
            .0;
        let mut b = NextWordLearner::load(&path, LearnConfig::default())
            .unwrap()
            .0;
        a.clear();
        a.flush(&path).unwrap();
        // b は古い内容を持つが、変更していなければ復活させない
        b.record("明日", "も", 200);
        b.flush(&path).unwrap();
        let (c, _) = NextWordLearner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(count(&c, "今日", "は"), 0);
        assert_eq!(count(&c, "明日", "も"), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn forget_removes_next_surface_everywhere() {
        let dir = temp_dir();
        let path = dir.join("learning-next.tsv");
        let mut l = NextWordLearner::new(LearnConfig::default());
        l.record("今日", "は", 100);
        l.record("明日", "は", 100);
        l.record("今日", "も", 100);
        l.flush(&path).unwrap();
        assert!(l.forget("は"));
        l.flush(&path).unwrap();
        let (c, _) = NextWordLearner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(count(&c, "今日", "も"), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn eviction_and_forget_days() {
        let mut l = NextWordLearner::new(LearnConfig {
            max_entries: 10,
            ..LearnConfig::default()
        });
        for i in 0..20 {
            l.record("p", &format!("n{i}"), 100 + i);
        }
        assert!(l.len() <= 10);
        let dir = temp_dir();
        let path = dir.join("n.tsv");
        let mut l = NextWordLearner::new(LearnConfig::default());
        l.record("a", "b", 1); // 大昔
        l.flush(&path).unwrap();
        let cfg = LearnConfig {
            forget_days: 1,
            ..LearnConfig::default()
        };
        let (c, _) = NextWordLearner::load(&path, cfg).unwrap();
        assert!(c.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_lines_are_skipped_with_backup_and_unknown_schema_is_error() {
        let dir = temp_dir();
        let path = dir.join("n.tsv");
        fs::write(&path, "#nagi-learn-next\tv1\na\tb\t2\t5\nbroken\n").unwrap();
        let (l, report) = NextWordLearner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(report.skipped_lines, 1);
        assert!(report.backed_up_to.is_some());
        fs::write(&path, "#other\tv9\n").unwrap();
        assert!(matches!(
            NextWordLearner::load(&path, LearnConfig::default()),
            Err(LearnError::UnsupportedSchema { .. })
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
