//! Nagi 学習: 確定履歴の記録・スコアリング・TSV 永続化。
//!
//! 学習ファイルに確定候補の履歴を記録し、スコアリングと検索機能を提供する。
//! スコアは `ln(1+count)` に指数減衰を適用して計算される。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

pub mod next;
pub use next::{NextRow, NextScored, NextWordLearner};

/// 学習設定。
#[derive(Clone, Debug)]
pub struct LearnConfig {
    /// 最大保持行数。デフォルト 10000。
    pub max_entries: usize,
    /// スコア指数減衰の半減期（日）。デフォルト 14.0。
    pub half_life_days: f64,
    /// 文脈ハッシュ一致時のボーナス乗数。デフォルト 0.5。
    pub context_bonus: f64,
    /// 学習の強さ（スコアにかける倍率。0.0〜2.0 を想定）。デフォルト 1.0（従来どおり）。
    pub strength: f64,
    /// 最終使用からこの日数を過ぎた行を忘れる。0 なら忘れない（デフォルト）。
    pub forget_days: u32,
}

impl Default for LearnConfig {
    fn default() -> Self {
        Self {
            max_entries: 10000,
            half_life_days: 14.0,
            context_bonus: 0.5,
            strength: 1.0,
            forget_days: 0,
        }
    }
}

/// 学習ファイルの 1 行。`(reading, surface, context_hash)` がキー。
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// ひらがな読み。
    pub reading: String,
    /// 表記（確定した表現）。
    pub surface: String,
    /// 左文脈ハッシュ。
    pub context_hash: u64,
    /// 出現回数。
    pub count: u32,
    /// 最終使用時刻（unix 秒）。
    pub last_used: u64,
    /// データ出所（`learned` / `user_dict` など）。
    pub source: String,
}

/// スコア計算済みの候補。
#[derive(Clone, Debug, PartialEq)]
pub struct Scored {
    /// ひらがな読み。
    pub reading: String,
    /// 表記。
    pub surface: String,
    /// スコア（高いほど良好）。
    pub score: f64,
}

/// 学習ファイル読み込みレポート。
#[derive(Clone, Debug)]
pub struct LoadReport {
    /// 読み込んだ有効行数。
    pub rows: usize,
    /// スキップした不正行数。
    pub skipped_lines: usize,
    /// 壊れた行がある場合、バックアップ先。
    pub backed_up_to: Option<PathBuf>,
}

/// 学習エラー型。
#[derive(Debug)]
pub enum LearnError {
    /// I/O エラー。
    Io(io::Error),
    /// 未知のスキーマバージョン。
    UnsupportedSchema {
        /// 見つかったバージョン文字列。
        found: String,
    },
}

impl std::fmt::Display for LearnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LearnError::Io(e) => write!(f, "I/O error: {}", e),
            LearnError::UnsupportedSchema { found } => {
                write!(f, "Unsupported schema version: {}", found)
            }
        }
    }
}

impl std::error::Error for LearnError {}

impl From<io::Error> for LearnError {
    fn from(e: io::Error) -> Self {
        LearnError::Io(e)
    }
}

/// FNV-1a 64-bit ハッシュ。決定的で、実装固有の randomization がない。
/// ref: https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
fn fnv1a_64(data: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// 左文脈から学習の context_hash を生成。末尾 4 文字（Unicode スカラー）のみを対象。
/// 空文字列なら 0。
pub fn context_hash(left_context: &str) -> u64 {
    if left_context.is_empty() {
        return 0;
    }

    let chars: Vec<char> = left_context.chars().collect();
    let start = if chars.len() > 4 { chars.len() - 4 } else { 0 };

    let suffix: String = chars[start..].iter().collect();
    fnv1a_64(suffix.as_bytes())
}

/// 学習データ管理。
#[derive(Debug)]
pub struct Learner {
    config: LearnConfig,
    // キー: (reading, surface, context_hash)、値: Row
    rows: HashMap<RowKey, Row>,
    dirty: bool,
    /// 最後にファイルと同期した時点の内容（flush 時の 3 者マージの基準）。
    base: HashMap<RowKey, Row>,
    /// 同期した時点のファイルの世代（`clear` のたびに増える）。
    generation: u64,
    /// 最後の同期以降に `clear` した（ファイルの内容を引き継がない）。
    cleared: bool,
    /// 解釈できない行のバックアップを取った（flush で捨てる前に 1 度だけ取る）。
    backed_up: bool,
    /// 最後の同期以降に `forget` した `(reading, surface)`。ファイル側の行も消す。
    forgotten: HashSet<(String, String)>,
}

type RowKey = (String, String, u64);

/// ファイルから読んだ内容。
struct FileData {
    generation: u64,
    rows: HashMap<RowKey, Row>,
    skipped_lines: usize,
}

/// 現在時刻（unix 秒）。
pub(crate) fn system_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Learner {
    /// 新しい Learner を作成。
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

    /// 現在の設定を取得。
    pub fn config(&self) -> &LearnConfig {
        &self.config
    }

    /// 設定を変更。`max_entries` が下がったら次の `record` 呼び出しで削除が実行される。
    pub fn set_config(&mut self, config: LearnConfig) {
        self.config = config;
        // dirty フラグは立てない。実際の削除は record 時の eviction で
    }

    /// 確定を 1 回記録。同じ `(reading, surface, context_hash)` なら count+1 と last_used 更新。
    /// max_entries 超過なら低スコア行から削除。
    pub fn record(
        &mut self,
        reading: &str,
        surface: &str,
        context_hash: u64,
        source: &str,
        now: u64,
    ) {
        let key = (reading.to_string(), surface.to_string(), context_hash);

        if let Some(row) = self.rows.get_mut(&key) {
            // 既存行: count と last_used を更新
            row.count = row.count.saturating_add(1);
            row.last_used = now;
        } else {
            // 新規行
            self.rows.insert(
                key,
                Row {
                    reading: reading.to_string(),
                    surface: surface.to_string(),
                    context_hash,
                    count: 1,
                    last_used: now,
                    source: source.to_string(),
                },
            );
        }

        self.dirty = true;

        // eviction: max_entries を超過したら削除
        if self.rows.len() > self.config.max_entries {
            self.evict(now);
        }
    }

    /// reading 完全一致の候補を取得。(reading, surface) ごとに集約し、スコア計算。
    pub fn lookup_exact(&self, reading: &str, context_hash: u64, now: u64) -> Vec<Scored> {
        let mut candidates: HashMap<(String, String), (u32, u64, f64)> = HashMap::new();

        // 同じ reading の全行を処理: count と last_used を集約
        for ((r, s, ch), row) in &self.rows {
            if r != reading {
                continue;
            }

            let key = (r.clone(), s.clone());
            let entry = candidates.entry(key).or_insert_with(|| (0, 0, 0.0));
            entry.0 = entry.0.saturating_add(row.count);
            entry.1 = entry.1.max(row.last_used);

            // context_hash 一致時のボーナス加点（各行）
            if *ch == context_hash && context_hash != 0 {
                let bonus = self.config.context_bonus * (1.0 + row.count as f64).ln();
                entry.2 += bonus;
            }
        }

        let mut results = candidates
            .into_iter()
            .map(
                |((reading, surface), (total_count, last_used, bonus_score))| {
                    // base_score = ln(1 + Σcount) × 0.5^(経過日数/half_life)
                    let base_score =
                        calculate_score(total_count, last_used, now, self.config.half_life_days);
                    let score = (base_score + bonus_score) * self.config.strength;
                    Scored {
                        reading,
                        surface,
                        score,
                    }
                },
            )
            .collect::<Vec<_>>();

        // score 降順、同点は surface 昇順
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.surface.cmp(&b.surface))
        });

        results
    }

    /// reading 前方一致の候補を取得。最大 limit 件まで。
    pub fn lookup_prefix(
        &self,
        prefix: &str,
        context_hash: u64,
        now: u64,
        limit: usize,
    ) -> Vec<Scored> {
        let mut candidates: HashMap<(String, String), (u32, u64, f64)> = HashMap::new();

        for ((r, s, ch), row) in &self.rows {
            if !r.starts_with(prefix) {
                continue;
            }

            let base_score =
                calculate_score(row.count, row.last_used, now, self.config.half_life_days);

            let key = (r.clone(), s.clone());
            let entry = candidates.entry(key).or_insert_with(|| (0, 0, 0.0));
            entry.0 = entry.0.saturating_add(row.count);
            entry.1 = entry.1.max(row.last_used);
            entry.2 += base_score;

            if *ch == context_hash && context_hash != 0 {
                let bonus = self.config.context_bonus * (1.0 + row.count as f64).ln();
                entry.2 += bonus;
            }
        }

        let mut results = candidates
            .into_iter()
            .map(
                |((reading, surface), (_total_count, _last_used, score))| Scored {
                    reading,
                    surface,
                    score: score * self.config.strength,
                },
            )
            .collect::<Vec<_>>();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.surface.cmp(&b.surface))
        });

        results.truncate(limit);
        results
    }

    /// `(reading, surface)` の全行を削除（誤学習の削除）。削除したら true。
    pub fn forget(&mut self, reading: &str, surface: &str) -> bool {
        let mut found = false;
        let keys_to_remove: Vec<_> = self
            .rows
            .iter()
            .filter_map(|((r, s, ch), _)| {
                if r == reading && s == surface {
                    found = true;
                    Some((r.clone(), s.clone(), *ch))
                } else {
                    None
                }
            })
            .collect();

        for key in keys_to_remove {
            self.rows.remove(&key);
            self.base.remove(&key);
            self.dirty = true;
        }
        if found {
            self.forgotten
                .insert((reading.to_string(), surface.to_string()));
        }

        found
    }

    /// 全行削除。
    ///
    /// 次の `flush` でファイルの世代が進み、他インスタンスが持つ古い内容は復活しない。
    pub fn clear(&mut self) {
        self.rows.clear();
        self.base.clear();
        self.forgotten.clear();
        self.cleared = true;
        self.dirty = true;
    }

    /// 最終使用が `forget_days` 日より前の行を落とす（`forget_days` が 0 なら何もしない）。
    /// 落としたら true。
    pub fn prune(&mut self, now: u64) -> bool {
        let before = self.rows.len();
        prune_rows(&mut self.rows, self.config.forget_days, now);
        let pruned = self.rows.len() != before;
        if pruned {
            self.dirty = true;
        }
        pruned
    }

    /// 行数を取得。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空かどうか。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// dirty フラグを取得。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 現在の全行を取得。順序は `(reading, surface, context_hash)` 昇順で決定的。
    pub fn rows(&self) -> Vec<Row> {
        let mut result: Vec<_> = self.rows.values().cloned().collect();
        result.sort_by(|a, b| {
            a.reading
                .cmp(&b.reading)
                .then_with(|| a.surface.cmp(&b.surface))
                .then_with(|| a.context_hash.cmp(&b.context_hash))
        });
        result
    }

    /// ファイルへ書き出す。dirty でなければ Ok(false)。書いたら Ok(true) で dirty を解除。
    ///
    /// 手順:
    /// 1. 同じディレクトリの `<path>.lock`（削除しない）を flock
    /// 2. ロック下で最新のファイルを読み、最後の同期時点の内容と手元の差分を 3 者マージ
    ///    （回数は他プロセスの加算分を足し、最終使用は大きい方。`forget` は両方から消す。
    ///    `clear` はファイルの世代を進め、他インスタンスの古い内容を復活させない）
    /// 3. `<path>.tmp` に全行を書いて fsync、rename で置換
    /// 4. 手元の内容をマージ結果で置き換える（他プロセスの更新を取り込む）
    pub fn flush(&mut self, path: &Path) -> Result<bool, LearnError> {
        if !self.dirty {
            return Ok(false);
        }
        // ロックファイルは消さない（消すと別プロセスが別 inode をロックして排他が崩れる）
        let lock = FlushLock::acquire(path, None)?.ok_or_else(|| {
            LearnError::Io(io::Error::new(io::ErrorKind::TimedOut, "flock timed out"))
        })?;
        self.flush_with_lock(path, &lock)
    }

    /// 取得済みの [`FlushLock`]（`path` 用）の下で書き出す。他プロセスのロックを待つ部分を
    /// 呼び出し側が Mutex の外で済ませられるようにするための分割版（本体は [`Self::flush`] と同じ）
    pub fn flush_with_lock(&mut self, path: &Path, _lock: &FlushLock) -> Result<bool, LearnError> {
        if !self.dirty {
            return Ok(false);
        }
        self.flush_locked(path)?;
        Ok(true)
    }

    /// ロックを取った状態の flush 本体。
    fn flush_locked(&mut self, path: &Path) -> Result<(), LearnError> {
        let disk = match read_file(path) {
            Ok(d) => d,
            // ファイルが無い（ホストが削除した等）: 空として扱う
            Err(LearnError::Io(e)) if e.kind() == io::ErrorKind::NotFound => FileData {
                generation: 0,
                rows: HashMap::new(),
                skipped_lines: 0,
            },
            // 未知のスキーマや空ファイルは上書きしない
            Err(e) => return Err(e),
        };
        // 解釈できない行は書き戻すと消える: 消す前にバックアップを取る（load 時と同様。1 度だけ）
        if disk.skipped_lines > 0 && !self.backed_up {
            create_backup_file(path)?;
            self.backed_up = true;
        }

        let gen_changed = disk.generation != self.generation;
        let mut merged: HashMap<RowKey, Row> = HashMap::new();

        // ファイル側の行を土台にする
        if !self.cleared {
            for (key, drow) in disk.rows {
                if self.forgotten.contains(&(key.0.clone(), key.1.clone())) {
                    continue;
                }
                // 手元で消した（eviction・忘却）行で、ファイル側が基準から変わっていなければ消す
                if !gen_changed
                    && !self.rows.contains_key(&key)
                    && self.base.get(&key) == Some(&drow)
                {
                    continue;
                }
                merged.insert(key, drow);
            }
        }

        // 手元の変更分を重ねる
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

        // 忘却と容量
        let sys_now = system_now();
        prune_rows(&mut merged, self.config.forget_days, sys_now);
        let max_used = merged.values().map(|r| r.last_used).max().unwrap_or(0);

        let new_generation = if self.cleared {
            disk.generation.max(self.generation) + 1
        } else if disk.generation > self.generation {
            disk.generation
        } else if disk.generation < self.generation {
            // ファイルが消された・巻き戻された: 世代を進めて他インスタンスにも知らせる
            self.generation + 1
        } else {
            self.generation
        };

        let old_rows = std::mem::replace(&mut self.rows, merged);
        if self.rows.len() > self.config.max_entries {
            self.evict(max_used);
        }

        let write_result = write_file(path, new_generation, &self.rows());
        if let Err(e) = write_result {
            // 書けなかったら手元の状態は変えない
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

    /// ファイルから読み込む。ファイルが無ければ空の Learner。ロックファイル使用。
    pub fn load(path: &Path, config: LearnConfig) -> Result<(Learner, LoadReport), LearnError> {
        let mut learner = Learner::new(config);
        let mut report = LoadReport {
            rows: 0,
            skipped_lines: 0,
            backed_up_to: None,
        };

        // ファイルがなければ空で返す
        if !path.exists() {
            return Ok((learner, report));
        }

        let lock_file = open_lock(path)?;
        lock_file.lock_shared()?;
        let result = (|| -> Result<(), LearnError> {
            let data = read_file(path)?;
            report.rows = data.rows.len();
            learner.generation = data.generation;
            learner.rows = data.rows;
            // 壊れた行があったらバックアップ
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
        // 忘却は読み込み時にも適用（次の flush で消える）
        let now = system_now();
        learner.prune(now);
        Ok((learner, report))
    }

    /// eviction: スコア低い順に削除。超過分 + 全体の 10% を削除。
    fn evict(&mut self, now: u64) {
        let target_size = (self.config.max_entries as f64 * 0.9).ceil() as usize;
        let excess = self.rows.len().saturating_sub(target_size);

        if excess == 0 {
            return;
        }

        // スコアで並べ替え
        let mut entries: Vec<_> = self
            .rows
            .iter()
            .map(|(key, row)| {
                let score =
                    calculate_score(row.count, row.last_used, now, self.config.half_life_days);
                (
                    key.clone(),
                    score,
                    row.last_used,
                    row.reading.clone(),
                    row.surface.clone(),
                )
            })
            .collect();

        // スコア昇順、同点は last_used が古い順、最後に reading/surface の辞書順
        entries.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.3.cmp(&b.3))
                .then_with(|| a.4.cmp(&b.4))
        });

        for i in 0..excess {
            if i < entries.len() {
                self.rows.remove(&entries[i].0);
            }
        }
    }
}

/// TSV フィールドをエスケープ。バックスラッシュ・タブ・改行・CR を変換。
pub(crate) fn escape_tsv_field(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '\t' => vec!['\\', 't'],
            '\n' => vec!['\\', 'n'],
            '\r' => vec!['\\', 'r'],
            c => vec![c],
        })
        .collect()
}

/// TSV フィールドをアンエスケープ。
pub(crate) fn unescape_tsv_field(s: &str) -> Result<String, &'static str> {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                match next {
                    '\\' => {
                        chars.next();
                        result.push('\\');
                    }
                    't' => {
                        chars.next();
                        result.push('\t');
                    }
                    'n' => {
                        chars.next();
                        result.push('\n');
                    }
                    'r' => {
                        chars.next();
                        result.push('\r');
                    }
                    _ => return Err("Invalid escape sequence"),
                }
            } else {
                return Err("Trailing backslash");
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

/// TSV 行をパース。
fn parse_tsv_line(line: &str) -> Result<(String, String, u64, u32, u64, String), &'static str> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() != 6 {
        return Err("Invalid field count");
    }

    let reading = unescape_tsv_field(fields[0])?;
    let surface = unescape_tsv_field(fields[1])?;
    let context_hash = u64::from_str_radix(fields[2], 16).map_err(|_| "Invalid context_hash")?;
    let count = fields[3].parse::<u32>().map_err(|_| "Invalid count")?;
    let last_used = fields[4].parse::<u64>().map_err(|_| "Invalid last_used")?;
    let source = unescape_tsv_field(fields[5])?;

    Ok((reading, surface, context_hash, count, last_used, source))
}

/// 学習ファイルの `<path>.lock` に対する排他ロック。drop で解放する（ロックファイル自体は消さない）。
#[derive(Debug)]
pub struct FlushLock {
    _file: fs::File,
}

impl FlushLock {
    /// 排他ロックを取る。`timeout` が `None` ならロックが取れるまで待つ。`Some` なら、その時間内に
    /// 取れなければ `Ok(None)`（他プロセスが保持している。次回に回す）。親ディレクトリも作る
    pub fn acquire(
        path: &Path,
        timeout: Option<std::time::Duration>,
    ) -> Result<Option<FlushLock>, LearnError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let file = open_lock(path)?;
        let Some(timeout) = timeout else {
            file.lock()?;
            return Ok(Some(FlushLock { _file: file }));
        };
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Some(FlushLock { _file: file })),
                Err(fs::TryLockError::WouldBlock) => {
                    if std::time::Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }
}

/// バックアップファイルを作成。既にあれば `.bak.1`, `.bak.2`, ... を試す。
pub(crate) fn create_backup_file(path: &Path) -> Result<PathBuf, LearnError> {
    let mut backup_path = path.with_extension("bak");

    let mut counter = 1;
    while backup_path.exists() && counter < 1000 {
        backup_path = path.with_extension(format!("bak.{}", counter));
        counter += 1;
    }

    fs::copy(path, &backup_path)?;
    Ok(backup_path)
}

/// ロックファイルを開く（作るだけで、truncate も削除もしない）。
pub(crate) fn open_lock(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
}

/// 最終使用が `forget_days` 日より前の行を落とす。0 なら何もしない。
fn prune_rows(rows: &mut HashMap<RowKey, Row>, forget_days: u32, now: u64) {
    if forget_days == 0 {
        return;
    }
    let limit = u64::from(forget_days) * 24 * 3600;
    rows.retain(|_, r| now.saturating_sub(r.last_used) <= limit);
}

/// 学習ファイルを読む（世代の無い旧形式は世代 0）。ロックは呼び出し側で取る。
fn read_file(path: &Path) -> Result<FileData, LearnError> {
    let file = fs::File::open(path)?;
    let mut lines = BufReader::new(file).lines();

    // 1 行目: `#nagi-learn\tv1` または世代つきの `#nagi-learn\tv1\tgen=N`（旧版も v1 として読める）
    let generation = match lines.next() {
        Some(Ok(line)) => {
            let line = line.trim_end_matches('\r');
            match line.strip_prefix("#nagi-learn\tv1") {
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
        // 空ファイルもスキーマ行が無いので UnsupportedSchema
        None => {
            return Err(LearnError::UnsupportedSchema {
                found: String::new(),
            });
        }
    };

    let mut data = FileData {
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
        match parse_tsv_line(&line) {
            Ok((reading, surface, context_hash, count, last_used, source)) => {
                data.rows.insert(
                    (reading.clone(), surface.clone(), context_hash),
                    Row {
                        reading,
                        surface,
                        context_hash,
                        count,
                        last_used,
                        source,
                    },
                );
            }
            Err(_) => data.skipped_lines += 1,
        }
    }
    Ok(data)
}

/// 一時ファイルに書いて fsync し、rename で置換する。
fn write_file(path: &Path, generation: u64, rows: &[Row]) -> Result<(), LearnError> {
    let tmp_path = path.with_extension("tmp");
    let mut writer = BufWriter::new(fs::File::create(&tmp_path)?);
    writeln!(writer, "#nagi-learn\tv1\tgen={generation}")?;
    for row in rows {
        writeln!(
            writer,
            "{}\t{}\t{:016x}\t{}\t{}\t{}",
            escape_tsv_field(&row.reading),
            escape_tsv_field(&row.surface),
            row.context_hash,
            row.count,
            row.last_used,
            escape_tsv_field(&row.source)
        )?;
    }
    writer.flush()?;
    let file = writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// スコアを計算。`ln(1 + count) × 0.5^(経過日数 / half_life_days)`
pub(crate) fn calculate_score(count: u32, last_used: u64, now: u64, half_life_days: f64) -> f64 {
    let base = (1.0 + count as f64).ln();

    let elapsed_seconds = if now >= last_used {
        now.wrapping_sub(last_used)
    } else {
        0
    };

    let elapsed_days = elapsed_seconds as f64 / (24.0 * 3600.0);
    let decay = 0.5_f64.powf(elapsed_days / half_life_days);

    base * decay
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::process;

    fn test_temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

        let pid = process::id();
        let counter = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = env::temp_dir().join(format!("nagi-learn-test-{}-{}", pid, counter));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_context_hash() {
        // 決定的性: 固定値のスナップショット
        assert_eq!(context_hash("私の名前は"), context_hash("私の名前は"));
        assert_eq!(context_hash(""), 0);

        // 末尾 4 文字だけが効く
        let tail_4 = "の名前は";
        let with_prefix = "あああ私".to_string() + tail_4;
        assert_eq!(context_hash(tail_4), context_hash(&with_prefix));

        // 末尾 3 文字なら異なる
        let tail_3 = "名前は";
        assert_ne!(context_hash(tail_4), context_hash(tail_3));
    }

    #[test]
    fn test_config_default() {
        let cfg = LearnConfig::default();
        assert_eq!(cfg.max_entries, 10000);
        assert_eq!(cfg.half_life_days, 14.0);
        assert_eq!(cfg.context_bonus, 0.5);
    }

    #[test]
    fn test_record_and_lookup_exact() {
        let mut learner = Learner::new(LearnConfig::default());
        let now = 1000000;

        // 1 回目の記録
        learner.record(
            "きょうはいいてんきです",
            "今日はいい天気です",
            0,
            "learned",
            now,
        );
        assert_eq!(learner.len(), 1);
        assert!(learner.is_dirty());

        // lookup_exact
        let results = learner.lookup_exact("きょうはいいてんきです", 0, now);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reading, "きょうはいいてんきです");
        assert_eq!(results[0].surface, "今日はいい天気です");

        // スコア確認: count=1, now=last_used → ln(2) ≈ 0.693
        let expected_score = (1.0_f64 + 1.0_f64).ln() * 1.0;
        assert!((results[0].score - expected_score).abs() < 0.001);

        // 同じキーで 2 回目記録: count が増える
        learner.record(
            "きょうはいいてんきです",
            "今日はいい天気です",
            0,
            "learned",
            now + 1,
        );
        assert_eq!(learner.len(), 1);
        let results = learner.lookup_exact("きょうはいいてんきです", 0, now + 1);
        assert_eq!(results[0].reading, "きょうはいいてんきです");
        // count=2, ln(3) ≈ 1.099
        let expected_score = (1.0_f64 + 2.0_f64).ln() * 1.0;
        assert!((results[0].score - expected_score).abs() < 0.001);
    }

    #[test]
    fn test_context_bonus() {
        let mut learner = Learner::new(LearnConfig {
            context_bonus: 0.5,
            ..Default::default()
        });
        let now = 1000000;
        let ctx_hash = context_hash("左文脈");

        // context_hash が一致する記録
        learner.record("きょう", "今日", ctx_hash, "learned", now);
        learner.record("きょう", "今日", 0, "learned", now);

        let results = learner.lookup_exact("きょう", ctx_hash, now);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reading, "きょう");

        // (reading, surface) ごとに集約: Σcount = 2, base = ln(3)
        // context_hash 一致の bonus = 0.5 * ln(1+1) (その行の count=1)
        // スコア = ln(3) + 0.5*ln(2) ≈ 1.099 + 0.347 = 1.446
        let base = (1.0_f64 + 2.0_f64).ln();
        let bonus = 0.5 * (1.0_f64 + 1.0_f64).ln();
        let expected = base + bonus;
        assert!((results[0].score - expected).abs() < 0.001);
    }

    #[test]
    fn test_lookup_exact_aggregation() {
        let mut learner = Learner::new(LearnConfig::default());
        let now = 1000000;

        // 同じ (reading, surface) で異なる context_hash
        learner.record("きょう", "今日", 1, "learned", now);
        learner.record("きょう", "今日", 2, "learned", now);
        learner.record("きょう", "今日", 3, "learned", now);

        let results = learner.lookup_exact("きょう", 0, now);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].reading, "きょう");
        assert_eq!(results[0].surface, "今日");

        // スコア: Σcount = 3, base = ln(1 + 3) = ln(4) ≈ 1.386, context_hash=0 は bonus なし
        let expected = (1.0_f64 + 3.0_f64).ln();
        assert!((results[0].score - expected).abs() < 0.001);
    }

    #[test]
    fn test_decay() {
        let mut learner = Learner::new(LearnConfig {
            half_life_days: 14.0,
            ..Default::default()
        });

        let base_time = 1000000;
        learner.record("きょう", "今日", 0, "learned", base_time);

        // 14 日後: スコアが半減
        let after_14_days = base_time + 14 * 24 * 3600;
        let results = learner.lookup_exact("きょう", 0, after_14_days);
        let expected = (1.0_f64 + 1.0_f64).ln() * 0.5;
        assert!((results[0].score - expected).abs() < 0.001);

        // 28 日後: スコアが 1/4
        let after_28_days = base_time + 28 * 24 * 3600;
        let results = learner.lookup_exact("きょう", 0, after_28_days);
        let expected = (1.0_f64 + 1.0_f64).ln() * 0.25;
        assert!((results[0].score - expected).abs() < 0.001);
    }

    #[test]
    fn test_lookup_prefix() {
        let mut learner = Learner::new(LearnConfig::default());
        let now = 1000000;

        learner.record("きょう", "今日", 0, "learned", now);
        learner.record("きょう", "教養", 0, "learned", now + 1);
        learner.record("きょうしつ", "教室", 0, "learned", now);
        learner.record("あした", "明日", 0, "learned", now);

        // prefix "きょう" で完全一致と前方一致を検索
        let results = learner.lookup_prefix("きょう", 0, now, 100);
        assert_eq!(results.len(), 3);
        assert!(
            results
                .iter()
                .any(|r| r.reading == "きょう" && r.surface == "今日")
        );
        assert!(
            results
                .iter()
                .any(|r| r.reading == "きょう" && r.surface == "教養")
        );
        assert!(results.iter().any(|r| r.reading == "きょうしつ"));

        // limit
        let results = learner.lookup_prefix("きょう", 0, now, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_forget() {
        let mut learner = Learner::new(LearnConfig::default());
        let now = 1000000;

        learner.record("きょう", "今日", 0, "learned", now);
        learner.record("きょう", "教養", 0, "learned", now);

        assert!(learner.forget("きょう", "今日"));
        assert_eq!(learner.len(), 1);

        let results = learner.lookup_exact("きょう", 0, now);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].surface, "教養");

        // 存在しないキーで forget
        assert!(!learner.forget("きょう", "京都"));
    }

    #[test]
    fn test_clear() {
        let mut learner = Learner::new(LearnConfig::default());
        let now = 1000000;

        learner.record("きょう", "今日", 0, "learned", now);
        learner.record("あした", "明日", 0, "learned", now);

        learner.clear();
        assert_eq!(learner.len(), 0);
        assert!(learner.is_empty());
    }

    #[test]
    fn test_eviction() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let mut learner = Learner::new(LearnConfig {
            max_entries: 10,
            ..Default::default()
        });

        let now = 1000000;

        // 30 件記録
        for i in 0..30 {
            let reading = format!("reading{:02}", i);
            let surface = format!("surface{:02}", i);
            learner.record(&reading, &surface, 0, "learned", now + i as u64);
        }

        // max_entries=10 なので、最終的には ~9 件に
        assert!(learner.len() <= 10);

        // 新しくよく使うものが残っていることを確認
        let results = learner.lookup_exact("reading29", 0, now + 29);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_tsv_escape_unescape() {
        assert_eq!(
            escape_tsv_field("hello\tworld\ntest"),
            "hello\\tworld\\ntest"
        );
        assert_eq!(
            unescape_tsv_field("hello\\tworld\\ntest").unwrap(),
            "hello\tworld\ntest"
        );

        assert_eq!(escape_tsv_field("a\\b"), "a\\\\b");
        assert_eq!(unescape_tsv_field("a\\\\b").unwrap(), "a\\b");

        assert_eq!(unescape_tsv_field("a\\rb").unwrap(), "a\rb");

        // 無効なエスケープ
        assert!(unescape_tsv_field("a\\xb").is_err());
    }

    #[test]
    fn test_tsv_roundtrip() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("learn.tsv");

        let mut learner = Learner::new(LearnConfig::default());
        let now = 1000000;

        // 特殊文字を含む行を記録
        learner.record("きょう", "今日", 0, "learned", now);
        learner.record("tab\ttest", "タブ\tテスト", 100, "user_dict", now + 1);
        learner.record("改\n行", "改\n行テスト", 200, "learned", now + 2);
        learner.record(
            "バック\\スラッシュ",
            "バック\\スラッシュ",
            300,
            "learned",
            now + 3,
        );

        // flush
        assert!(learner.flush(&path).unwrap());
        assert!(!learner.is_dirty());

        // load
        let (_loaded, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(report.rows, 4);
        assert_eq!(report.skipped_lines, 0);
        assert!(report.backed_up_to.is_none());

        let rows = _loaded.rows();
        assert_eq!(rows.len(), 4);

        // 順序が (reading, surface, context_hash) 昇順
        assert_eq!(rows[0].reading, "tab\ttest");
        assert_eq!(rows[0].surface, "タブ\tテスト");
    }

    #[test]
    fn test_flush_creates_parent_dir() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("subdir").join("learn.tsv");

        let mut learner = Learner::new(LearnConfig::default());
        learner.record("test", "テスト", 0, "learned", 1000000);

        assert!(learner.flush(&path).unwrap());
        assert!(path.exists());
    }

    #[test]
    fn test_concurrent_flush() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("learn.tsv");

        // 最初の Learner
        let mut learner1 = Learner::new(LearnConfig::default());
        learner1.record("one", "1番", 0, "learned", 1000000);
        learner1.flush(&path).unwrap();

        // 2 番目の Learner（同じファイルに追記）
        let mut learner2 = Learner::new(LearnConfig::default());
        learner2.record("two", "2番", 0, "learned", 1000001);
        learner2.flush(&path).unwrap();

        // ロード後、最新のデータが保存されている
        let (loaded, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        // 後勝ちで上書きせず、両方の更新が残る
        assert_eq!(report.rows, 2);
        let rows = loaded.rows();
        assert_eq!(rows[0].reading, "one");
        assert_eq!(rows[1].reading, "two");
    }

    #[test]
    fn test_load_unsupported_schema() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("bad_schema.tsv");
        fs::write(&path, "#unknown-schema\tv999\n").unwrap();

        let result = Learner::load(&path, LearnConfig::default());
        assert!(matches!(result, Err(LearnError::UnsupportedSchema { .. })));

        // ファイルが壊れていないこと
        assert!(path.exists());
    }

    #[test]
    fn test_load_skip_invalid_lines() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("partial_bad.tsv");
        let content = "#nagi-learn\tv1\n\
                       valid1\tsurface1\t0000000000000000\t1\t1000000\tlearned\n\
                       invalid_not_enough_fields\n\
                       valid2\tsurface2\t0000000000000001\t2\t1000001\tlearned\n";
        fs::write(&path, content).unwrap();

        let (_loaded, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(report.rows, 2);
        assert_eq!(report.skipped_lines, 1);
        assert!(report.backed_up_to.is_some());

        // バックアップファイルが作成されている
        assert!(report.backed_up_to.as_ref().unwrap().exists());

        // 元のファイルはそのまま
        assert!(path.exists());
    }

    #[test]
    fn test_flush_backs_up_unparsable_lines_once() {
        let temp_dir = test_temp_dir();
        let path = temp_dir.join("flush_bad.tsv");
        fs::write(
            &path,
            "#nagi-learn\tv1\nvalid1\tsurface1\t0000000000000000\t1\t1000000\tlearned\ngarbage\n",
        )
        .unwrap();
        // load を経由しない（別プロセスが後から壊れた行を書いた等）
        let mut l = Learner::new(LearnConfig::default());
        l.record("あ", "亜", 0, "learned", 100);
        assert!(l.flush(&path).unwrap());
        assert!(path.with_extension("bak").exists(), "捨てる前にバックアップ");
        assert!(!fs::read_to_string(&path).unwrap().contains("garbage"));
        l.record("い", "胃", 0, "learned", 101);
        l.flush(&path).unwrap();
        assert!(!path.with_extension("bak.1").exists(), "バックアップは 1 度だけ");
    }

    #[test]
    fn test_flush_lock_times_out_while_another_holds_it() {
        let temp_dir = test_temp_dir();
        let path = temp_dir.join("lock_timeout.tsv");
        let held = FlushLock::acquire(&path, None).unwrap().unwrap();
        let start = std::time::Instant::now();
        let second =
            FlushLock::acquire(&path, Some(std::time::Duration::from_millis(50))).unwrap();
        assert!(second.is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        drop(held);
        assert!(
            FlushLock::acquire(&path, Some(std::time::Duration::from_millis(50)))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn test_load_empty_file() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("empty.tsv");
        fs::write(&path, "").unwrap();

        let result = Learner::load(&path, LearnConfig::default());
        // スキーマ行がないので UnsupportedSchema
        assert!(matches!(result, Err(LearnError::UnsupportedSchema { .. })));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("nonexistent.tsv");

        let (_learner, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert!(_learner.is_empty());
        assert_eq!(report.rows, 0);
        assert_eq!(report.skipped_lines, 0);
        assert!(report.backed_up_to.is_none());
    }

    #[test]
    fn test_load_crlf_line_endings() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let path = temp_dir.join("crlf.tsv");
        let content = "#nagi-learn\tv1\r\ntest\tsurface\t0000000000000000\t1\t1000000\tlearned\r\n";
        fs::write(&path, content).unwrap();

        let (_loaded, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(report.rows, 1);
    }

    #[test]
    fn test_dirty_flag() {
        let mut learner = Learner::new(LearnConfig::default());
        assert!(!learner.is_dirty());

        learner.record("test", "テスト", 0, "learned", 1000000);
        assert!(learner.is_dirty());

        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);
        let path = temp_dir.join("learn.tsv");

        learner.flush(&path).unwrap();
        assert!(!learner.is_dirty());

        learner.record("test2", "テスト2", 0, "learned", 1000001);
        assert!(learner.is_dirty());

        learner.forget("test2", "テスト2");
        assert!(learner.is_dirty());

        learner.clear();
        assert!(learner.is_dirty());
    }

    #[test]
    fn test_flush_not_dirty_returns_false() {
        let temp_dir = test_temp_dir();
        defer_cleanup(&temp_dir);

        let mut learner = Learner::new(LearnConfig::default());
        learner.record("test", "テスト", 0, "learned", 1000000);
        let path = temp_dir.join("learn.tsv");

        // 最初は dirty
        assert!(learner.flush(&path).unwrap());
        assert!(!learner.is_dirty());

        // 2 回目は dirty でないので false
        assert!(!learner.flush(&path).unwrap());
    }

    #[test]
    fn test_rows_deterministic_order() {
        let mut learner = Learner::new(LearnConfig::default());
        let now = 1000000;

        learner.record("z", "Z", 100, "learned", now);
        learner.record("a", "A", 200, "learned", now);
        learner.record("a", "A", 100, "learned", now);
        learner.record("b", "B", 50, "learned", now);

        let rows1 = learner.rows();
        let rows2 = learner.rows();

        // 順序が毎回同じ
        assert_eq!(rows1, rows2);

        // (reading, surface, context_hash) の昇順
        for i in 0..rows1.len() - 1 {
            let r1 = (&rows1[i].reading, &rows1[i].surface, rows1[i].context_hash);
            let r2 = (
                &rows1[i + 1].reading,
                &rows1[i + 1].surface,
                rows1[i + 1].context_hash,
            );
            assert!(r1 < r2);
        }
    }

    #[test]
    fn test_time_backward() {
        let mut learner = Learner::new(LearnConfig::default());

        // last_used より古い now を与えた場合、経過は 0 日として扱う
        learner.record("test", "テスト", 0, "learned", 2000000);
        let results = learner.lookup_exact("test", 0, 1000000);

        let expected = (1.0_f64 + 1.0_f64).ln(); // decay = 1.0
        assert!((results[0].score - expected).abs() < 0.001);
        let _temp_dir = test_temp_dir();
        defer_cleanup(&_temp_dir);
    }

    // cleanup 用ヘルパー関数（テスト終了時に OS が削除）
    fn defer_cleanup(_path: &Path) {
        // テスト実行後に OS が自動的に削除
    }

    fn count_of(l: &Learner, r: &str, sf: &str) -> u32 {
        l.rows()
            .iter()
            .filter(|x| x.reading == r && x.surface == sf)
            .map(|x| x.count)
            .sum()
    }

    #[test]
    fn test_two_instances_merge_without_loss() {
        let dir = test_temp_dir();
        defer_cleanup(&dir);
        let path = dir.join("learning.tsv");

        let mut base = Learner::new(LearnConfig::default());
        base.record("きょう", "今日", 0, "learned", 100);
        base.flush(&path).unwrap();

        let mut a = Learner::load(&path, LearnConfig::default()).unwrap().0;
        let mut b = Learner::load(&path, LearnConfig::default()).unwrap().0;
        a.record("きょう", "今日", 0, "learned", 200);
        a.record("あした", "明日", 0, "learned", 200);
        b.record("きょう", "今日", 0, "learned", 300);
        b.record("きのう", "昨日", 0, "learned", 300);
        a.flush(&path).unwrap();
        b.flush(&path).unwrap();

        let (c, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(count_of(&c, "きょう", "今日"), 3);
        assert_eq!(count_of(&c, "あした", "明日"), 1);
        assert_eq!(count_of(&c, "きのう", "昨日"), 1);
        assert_eq!(c.rows().iter().map(|r| r.last_used).max(), Some(300));
        // b も a の更新を取り込んでいる
        assert_eq!(count_of(&b, "あした", "明日"), 1);
        assert!(!b.is_dirty());
    }

    #[test]
    fn test_clear_not_resurrected_by_other_instance() {
        let dir = test_temp_dir();
        defer_cleanup(&dir);
        let path = dir.join("learning.tsv");

        let mut base = Learner::new(LearnConfig::default());
        base.record("きょう", "今日", 0, "learned", 100);
        base.record("あした", "明日", 0, "learned", 100);
        base.flush(&path).unwrap();

        let mut a = Learner::load(&path, LearnConfig::default()).unwrap().0;
        let mut b = Learner::load(&path, LearnConfig::default()).unwrap().0;
        a.clear();
        a.flush(&path).unwrap();
        // b は clear を知らずに新しい学習を 1 件足して flush
        b.record("きのう", "昨日", 0, "learned", 200);
        b.record("きょう", "今日", 0, "learned", 200);
        b.flush(&path).unwrap();

        let (c, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        // clear 前の「あした」は復活しない。b の新しい分だけが残る（きょうは増分 1 回分）
        assert_eq!(count_of(&c, "あした", "明日"), 0);
        assert_eq!(count_of(&c, "きのう", "昨日"), 1);
        assert_eq!(count_of(&c, "きょう", "今日"), 1);
    }

    #[test]
    fn test_clear_wins_over_stale_file_content() {
        let dir = test_temp_dir();
        defer_cleanup(&dir);
        let path = dir.join("learning.tsv");

        let mut a = Learner::new(LearnConfig::default());
        a.record("きょう", "今日", 0, "learned", 100);
        a.flush(&path).unwrap();
        let mut b = Learner::load(&path, LearnConfig::default()).unwrap().0;
        a.record("あした", "明日", 0, "learned", 150);
        a.flush(&path).unwrap();
        b.clear();
        b.flush(&path).unwrap();

        let (c, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert!(c.is_empty());
        // a の次の flush でも復活しない
        a.record("きのう", "昨日", 0, "learned", 200);
        a.flush(&path).unwrap();
        let (c, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(count_of(&c, "きのう", "昨日"), 1);
    }

    #[test]
    fn test_forget_propagates_and_file_deletion() {
        let dir = test_temp_dir();
        defer_cleanup(&dir);
        let path = dir.join("learning.tsv");

        let mut base = Learner::new(LearnConfig::default());
        base.record("きょう", "今日", 0, "learned", 100);
        base.record("きょう", "教養", 0, "learned", 100);
        base.flush(&path).unwrap();
        let mut a = Learner::load(&path, LearnConfig::default()).unwrap().0;
        let mut b = Learner::load(&path, LearnConfig::default()).unwrap().0;
        a.forget("きょう", "今日");
        a.flush(&path).unwrap();
        // b は何もしていない行を持ったまま別の学習を flush しても、忘れた行は戻らない
        b.record("あした", "明日", 0, "learned", 200);
        b.flush(&path).unwrap();
        let (c, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(count_of(&c, "きょう", "今日"), 0);
        assert_eq!(count_of(&c, "きょう", "教養"), 1);

        // ホストがファイルを直接消した場合も、古い行は復活しない
        fs::remove_file(&path).unwrap();
        b.record("あした", "明日", 0, "learned", 300);
        b.flush(&path).unwrap();
        let (c, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(count_of(&c, "あした", "明日"), 1);
    }

    #[test]
    fn test_lock_file_kept_and_v1_readable() {
        let dir = test_temp_dir();
        defer_cleanup(&dir);
        let path = dir.join("learning.tsv");
        fs::write(
            &path,
            "#nagi-learn\tv1\nきょう\t今日\t0000000000000000\t2\t1000000\tlearned\n",
        )
        .unwrap();
        let (mut l, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(count_of(&l, "きょう", "今日"), 2);
        l.record("きょう", "今日", 0, "learned", 1000001);
        l.flush(&path).unwrap();
        assert!(path.with_extension("lock").exists());
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .starts_with("#nagi-learn\tv1\tgen=0\n")
        );
        let (l, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(count_of(&l, "きょう", "今日"), 3);
    }

    #[test]
    fn test_concurrent_instances_flush_threads() {
        let dir = test_temp_dir();
        defer_cleanup(&dir);
        let path = dir.join("learning.tsv");
        let handles: Vec<_> = (0..4u64)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut l = Learner::new(LearnConfig::default());
                    for i in 0..10u64 {
                        l.record("きょう", "今日", 0, "learned", 100 + i);
                        l.record(&format!("r{t}"), &format!("s{i}"), 0, "learned", 100);
                        l.flush(&path).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let (c, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(report.skipped_lines, 0);
        assert_eq!(count_of(&c, "きょう", "今日"), 40);
        assert_eq!(c.len(), 1 + 4 * 10);
    }

    #[test]
    fn test_strength_scales_score() {
        let mut l = Learner::new(LearnConfig::default());
        l.record("きょう", "今日", 0, "learned", 100);
        let s1 = l.lookup_exact("きょう", 0, 100)[0].score;
        let p1 = l.lookup_prefix("きょ", 0, 100, 5)[0].score;
        l.set_config(LearnConfig {
            strength: 2.0,
            ..Default::default()
        });
        assert!((l.lookup_exact("きょう", 0, 100)[0].score - s1 * 2.0).abs() < 1e-9);
        assert!((l.lookup_prefix("きょ", 0, 100, 5)[0].score - p1 * 2.0).abs() < 1e-9);
        l.set_config(LearnConfig {
            strength: 0.0,
            ..Default::default()
        });
        assert_eq!(l.lookup_exact("きょう", 0, 100)[0].score, 0.0);
    }

    #[test]
    fn test_forget_days_prune_and_load_flush() {
        let day = 24 * 3600;
        let mut l = Learner::new(LearnConfig {
            forget_days: 30,
            ..Default::default()
        });
        l.record("ふるい", "古い", 0, "learned", 1000);
        l.record("あたらしい", "新しい", 0, "learned", 1000 + 40 * day);
        assert!(l.prune(1000 + 40 * day));
        assert_eq!(l.len(), 1);
        assert_eq!(count_of(&l, "あたらしい", "新しい"), 1);
        // 境界ちょうどは残す
        assert!(!l.prune(1000 + 40 * day + 30 * day));

        // forget_days = 0 は無期限
        let mut l = Learner::new(LearnConfig::default());
        l.record("ふるい", "古い", 0, "learned", 1);
        assert!(!l.prune(u64::MAX / 2));

        // flush / load でも落ちる（時刻 1 は十分古い）
        let dir = test_temp_dir();
        defer_cleanup(&dir);
        let path = dir.join("learning.tsv");
        let cfg = LearnConfig {
            forget_days: 30,
            ..Default::default()
        };
        let mut l = Learner::new(cfg.clone());
        l.record("ふるい", "古い", 0, "learned", 1);
        l.record("いま", "今", 0, "learned", system_now());
        l.flush(&path).unwrap();
        let (l2, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(l2.len(), 1);
        // 無期限で書いた古い行は、forget_days 付きの load で落ちて dirty になる
        let mut old = Learner::new(LearnConfig::default());
        old.record("ふるい", "古い", 0, "learned", 1);
        old.flush(&path).unwrap();
        let (l3, _) = Learner::load(&path, cfg).unwrap();
        assert!(l3.len() == 1 && l3.is_dirty());
    }
}
