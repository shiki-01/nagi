//! Nagi 学習: 確定履歴の記録・スコアリング・TSV 永続化。
//!
//! 学習ファイルに確定候補の履歴を記録し、スコアリングと検索機能を提供する。
//! スコアは `ln(1+count)` に指数減衰を適用して計算される。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

/// 学習設定。
#[derive(Clone, Debug)]
pub struct LearnConfig {
    /// 最大保持行数。デフォルト 10000。
    pub max_entries: usize,
    /// スコア指数減衰の半減期（日）。デフォルト 14.0。
    pub half_life_days: f64,
    /// 文脈ハッシュ一致時のボーナス乗数。デフォルト 0.5。
    pub context_bonus: f64,
}

impl Default for LearnConfig {
    fn default() -> Self {
        Self {
            max_entries: 10000,
            half_life_days: 14.0,
            context_bonus: 0.5,
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

/// 行のキー `(reading, surface, context_hash)`。
type Key = (String, String, u64);

/// 前回の load / flush 以降にこのプロセスが加えた変更の差分（flush 時にディスクの最新へ適用する）。
#[derive(Clone, Debug)]
struct Pending {
    /// 加算する出現回数。
    count: u32,
    /// このプロセスでの最終使用時刻。
    last_used: u64,
    /// 新規行として挿入する場合の出所。
    source: String,
}

/// 学習データ管理。
///
/// 複数プロセス（キーボード拡張と本体アプリなど）が同じ学習ファイルを共有しても更新が失われないよう、
/// `flush` は「ロック → ディスク最新を読む → 自プロセスの差分を適用 → 一時ファイル → rename」で行う。
#[derive(Debug)]
pub struct Learner {
    config: LearnConfig,
    // キー: (reading, surface, context_hash)、値: Row
    rows: HashMap<Key, Row>,
    dirty: bool,
    /// 前回 load / flush 以降の確定記録（キーごとの加算分）。
    pending: HashMap<Key, Pending>,
    /// 前回 load / flush 以降に `forget` した `(reading, surface)`。
    forgotten: HashSet<(String, String)>,
    /// 前回 load / flush 以降に `clear` した。
    cleared: bool,
}

impl Learner {
    /// 新しい Learner を作成。
    pub fn new(config: LearnConfig) -> Self {
        Self {
            config,
            rows: HashMap::new(),
            dirty: false,
            pending: HashMap::new(),
            forgotten: HashSet::new(),
            cleared: false,
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

        let pending = self
            .pending
            .entry((reading.to_string(), surface.to_string(), context_hash))
            .or_insert_with(|| Pending {
                count: 0,
                last_used: now,
                source: source.to_string(),
            });
        pending.count = pending.count.saturating_add(1);
        pending.last_used = pending.last_used.max(now);

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
                    let score = base_score + bonus_score;
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
                    score,
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
            self.dirty = true;
        }
        if found {
            self.pending
                .retain(|(r, s, _), _| r != reading || s != surface);
            self.forgotten
                .insert((reading.to_string(), surface.to_string()));
        }

        found
    }

    /// 全行削除。
    pub fn clear(&mut self) {
        self.rows.clear();
        self.pending.clear();
        self.forgotten.clear();
        self.cleared = true;
        self.dirty = true;
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
    /// 他プロセスが同じファイルを更新していても失われないよう read-modify-write で行う。
    ///
    /// 1. `<path>.lock` を排他ロック（ロックファイルは削除しない。削除すると別 inode を掴んだ
    ///    プロセスと排他できなくなるため）
    /// 2. ディスク上の最新を読み込む（無ければ空）
    /// 3. 前回 load / flush 以降の差分を適用する（マージ規則は `merge_pending`）
    /// 4. `<path>.tmp` に全行を書いて fsync → rename で置換 → 親ディレクトリを fsync
    /// 5. メモリ上の行をマージ結果に置き換える（他プロセスの更新も取り込まれる）
    ///
    /// ディスク上のファイルが未知のスキーマなら上書きせず `UnsupportedSchema` を返す。
    pub fn flush(&mut self, path: &Path) -> Result<bool, LearnError> {
        if !self.dirty {
            return Ok(false);
        }

        // 親ディレクトリを作成（なければ）
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        let lock_file = open_lock(path)?;
        lock_file.lock()?;

        let result = self.flush_locked(path);

        // 明示的に解除（drop でも解除される）。ロックファイルは削除しない。
        let _ = lock_file.unlock();
        drop(lock_file);

        let merged = result?;
        self.rows = merged;
        self.pending.clear();
        self.forgotten.clear();
        self.cleared = false;
        self.dirty = false;
        Ok(true)
    }

    /// ロック取得済みの状態で、ディスクの最新とマージして書き出す。マージ結果を返す。
    fn flush_locked(&self, path: &Path) -> Result<HashMap<Key, Row>, LearnError> {
        let mut merged = match read_rows(path) {
            Ok((rows, skipped)) => {
                if skipped > 0 {
                    // 壊れた行は書き直しで失われるので、先にバックアップを残す
                    create_backup_file(path)?;
                }
                rows
            }
            // 空ファイルは空として扱う（スキーマ行すら無く、失われる内容がない）
            Err(LearnError::UnsupportedSchema { found }) if found.is_empty() => HashMap::new(),
            Err(e) => return Err(e),
        };
        self.merge_pending(&mut merged);

        let tmp_path = path.with_extension("tmp");
        let tmp_file = fs::File::create(&tmp_path)?;
        let mut writer = BufWriter::new(tmp_file);
        writeln!(writer, "#nagi-learn\tv1")?;

        let mut rows: Vec<&Row> = merged.values().collect();
        rows.sort_by(|a, b| {
            a.reading
                .cmp(&b.reading)
                .then_with(|| a.surface.cmp(&b.surface))
                .then_with(|| a.context_hash.cmp(&b.context_hash))
        });
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
        drop(file);

        // rename で置換（原子的）。失敗時は一時ファイルを残さない
        if let Err(e) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(e.into());
        }
        sync_parent_dir(path);

        Ok(merged)
    }

    /// 自プロセスの差分をディスク由来の行 `rows` へ適用する。
    ///
    /// マージ規則（適用順）:
    /// 1. `clear` 済みならディスクの行を全て捨てる
    /// 2. `forget` した `(reading, surface)` の行を全て捨てる（他プロセスの同キー更新も消える）
    /// 3. 記録の差分を加算する: 既存行は `count += 差分`（飽和）・`last_used = max`、
    ///    無ければ差分を count とする新規行（`source` は自プロセスのもの。既存行の `source` は保持）
    /// 4. `max_entries` を超えたら、全行の最大 `last_used` を現在時刻として `evict`
    ///
    /// 頻度は絶対値ではなく差分で足すため、両プロセスの確定回数が合算される。
    fn merge_pending(&self, rows: &mut HashMap<Key, Row>) {
        if self.cleared {
            rows.clear();
        }
        if !self.forgotten.is_empty() {
            rows.retain(|(r, s, _), _| !self.forgotten.contains(&(r.clone(), s.clone())));
        }
        for (key, p) in &self.pending {
            match rows.get_mut(key) {
                Some(row) => {
                    row.count = row.count.saturating_add(p.count);
                    row.last_used = row.last_used.max(p.last_used);
                }
                None => {
                    rows.insert(
                        key.clone(),
                        Row {
                            reading: key.0.clone(),
                            surface: key.1.clone(),
                            context_hash: key.2,
                            count: p.count,
                            last_used: p.last_used,
                            source: p.source.clone(),
                        },
                    );
                }
            }
        }
        if rows.len() > self.config.max_entries {
            let now = rows.values().map(|r| r.last_used).max().unwrap_or(0);
            evict_rows(
                rows,
                self.config.max_entries,
                self.config.half_life_days,
                now,
            );
        }
    }

    /// ファイルから読み込む。ファイルが無ければ空の Learner。共有ロックを取る。
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

        // 書き込みは rename で原子的なので読み込み自体は壊れないが、
        // 壊れた行のバックアップ作成が flush と競合しないよう共有ロックを取る
        let lock_file = open_lock(path)?;
        lock_file.lock_shared()?;

        let result = (|| -> Result<(), LearnError> {
            let (rows, skipped) = read_rows(path)?;
            report.rows = rows.len();
            report.skipped_lines = skipped;
            learner.rows = rows;
            // 壊れた行があったらバックアップ
            if skipped > 0 {
                report.backed_up_to = Some(create_backup_file(path)?);
            }
            Ok(())
        })();

        let _ = lock_file.unlock();
        drop(lock_file);

        result?;
        Ok((learner, report))
    }

    /// eviction: スコア低い順に削除。超過分 + 全体の 10% を削除。
    fn evict(&mut self, now: u64) {
        evict_rows(
            &mut self.rows,
            self.config.max_entries,
            self.config.half_life_days,
            now,
        );
    }
}

/// ロックファイル（`<path>.lock`）を開く。無ければ作る。内容は使わない。
fn open_lock(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
}

/// rename の永続化のため親ディレクトリを fsync する。ディレクトリを開けない環境（Windows 等）では何もしない。
fn sync_parent_dir(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
}

/// 学習ファイルを読む。戻り値は (行, スキップした不正行数)。ファイルが無ければ空。
fn read_rows(path: &Path) -> Result<(HashMap<Key, Row>, usize), LearnError> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((HashMap::new(), 0)),
        Err(e) => return Err(e.into()),
    };
    let mut lines = BufReader::new(file).lines();

    // 1 行目がスキーマバージョンか確認
    match lines.next() {
        Some(Ok(line)) => {
            if !line.starts_with("#nagi-learn\tv1") {
                return Err(LearnError::UnsupportedSchema { found: line });
            }
        }
        Some(Err(e)) => return Err(LearnError::Io(e)),
        // ファイルが空の場合も UnsupportedSchema（スキーマ行がない）
        None => {
            return Err(LearnError::UnsupportedSchema {
                found: String::new(),
            });
        }
    }

    let mut rows = HashMap::new();
    let mut skipped = 0;
    for line in lines {
        let Ok(line) = line else {
            skipped += 1;
            continue;
        };
        if line.is_empty() {
            continue;
        }
        match parse_tsv_line(&line) {
            Ok((reading, surface, context_hash, count, last_used, source)) => {
                rows.insert(
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
            Err(_) => skipped += 1,
        }
    }
    Ok((rows, skipped))
}

/// eviction の本体: スコア低い順に、`max_entries` の 90% になるまで削除。
fn evict_rows(rows: &mut HashMap<Key, Row>, max_entries: usize, half_life_days: f64, now: u64) {
    let target_size = (max_entries as f64 * 0.9).ceil() as usize;
    let excess = rows.len().saturating_sub(target_size);
    if excess == 0 {
        return;
    }

    let mut entries: Vec<_> = rows
        .iter()
        .map(|(key, row)| {
            let score = calculate_score(row.count, row.last_used, now, half_life_days);
            (key.clone(), score, row.last_used)
        })
        .collect();

    // スコア昇順、同点は last_used が古い順、最後にキーの辞書順
    entries.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.0.cmp(&b.0))
    });

    for (key, _, _) in entries.into_iter().take(excess) {
        rows.remove(&key);
    }
}

/// TSV フィールドをエスケープ。バックスラッシュ・タブ・改行・CR を変換。
fn escape_tsv_field(s: &str) -> String {
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
fn unescape_tsv_field(s: &str) -> Result<String, &'static str> {
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

/// バックアップファイルを作成。既にあれば `.bak.1`, `.bak.2`, ... を試す。
fn create_backup_file(path: &Path) -> Result<PathBuf, LearnError> {
    let mut backup_path = path.with_extension("bak");

    let mut counter = 1;
    while backup_path.exists() && counter < 1000 {
        backup_path = path.with_extension(format!("bak.{}", counter));
        counter += 1;
    }

    fs::copy(path, &backup_path)?;
    Ok(backup_path)
}

/// スコアを計算。`ln(1 + count) × 0.5^(経過日数 / half_life_days)`
fn calculate_score(count: u32, last_used: u64, now: u64, half_life_days: f64) -> f64 {
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
    fn test_flush_merges_other_handle_updates() {
        let temp_dir = test_temp_dir();
        let path = temp_dir.join("learn.tsv");

        let mut a = Learner::new(LearnConfig::default());
        a.record("one", "1番", 0, "learned", 1000000);
        a.record("shared", "共有", 0, "learned", 1000000);
        a.flush(&path).unwrap();

        // 別ハンドル（別プロセス相当）。a の flush 後の状態を読まずに更新して flush する
        let mut b = Learner::new(LearnConfig::default());
        b.record("two", "2番", 0, "learned", 1000001);
        b.record("shared", "共有", 0, "learned", 1000005);
        b.flush(&path).unwrap();

        // b はマージ結果（a の更新を含む）を取り込んでいる
        assert_eq!(b.len(), 3);

        // a がもう一度 flush しても b の更新は残る（頻度は差分で合算、時刻は最大）
        a.record("shared", "共有", 0, "learned", 1000003);
        a.flush(&path).unwrap();

        let (loaded, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(report.rows, 3);
        let rows = loaded.rows();
        let shared = rows.iter().find(|r| r.reading == "shared").unwrap();
        assert_eq!(shared.count, 3);
        assert_eq!(shared.last_used, 1000005);
        assert!(rows.iter().any(|r| r.reading == "one"));
        assert!(rows.iter().any(|r| r.reading == "two"));
    }

    #[test]
    fn test_flush_forget_and_clear_apply_to_disk() {
        let temp_dir = test_temp_dir();
        let path = temp_dir.join("learn.tsv");

        let mut a = Learner::new(LearnConfig::default());
        a.record("きょう", "今日", 0, "learned", 1);
        a.record("あした", "明日", 0, "learned", 1);
        a.flush(&path).unwrap();

        let mut b = Learner::new(LearnConfig::default());
        b.record("きのう", "昨日", 0, "learned", 2);
        b.flush(&path).unwrap(); // ディスク: 3 件

        // a は b の更新をまだ知らない状態で forget
        assert!(a.forget("きょう", "今日"));
        a.flush(&path).unwrap();
        let (l, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        let readings: Vec<_> = l.rows().into_iter().map(|r| r.reading).collect();
        assert_eq!(readings, vec!["あした", "きのう"]);

        // clear 後の記録だけが残る
        a.clear();
        a.record("あさって", "明後日", 0, "learned", 3);
        a.flush(&path).unwrap();
        let (l, _) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn test_flush_refuses_unknown_schema() {
        let temp_dir = test_temp_dir();
        let path = temp_dir.join("learn.tsv");
        fs::write(&path, "#nagi-learn\tv999\n").unwrap();

        let mut l = Learner::new(LearnConfig::default());
        l.record("a", "A", 0, "learned", 1);
        assert!(matches!(
            l.flush(&path),
            Err(LearnError::UnsupportedSchema { .. })
        ));
        assert!(l.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "#nagi-learn\tv999\n");
        assert!(!path.with_extension("tmp").exists());
    }

    const CHILD_DIR_ENV: &str = "NAGI_LEARN_TEST_CHILD_DIR";
    const CHILD_ID_ENV: &str = "NAGI_LEARN_TEST_CHILD_ID";
    const CHILD_ROUNDS: u32 = 25;

    /// 子プロセス側の本体。環境変数が無ければ何もしない（通常のテスト実行では素通り）。
    #[test]
    fn child_process_flusher() {
        let (Ok(dir), Ok(id)) = (env::var(CHILD_DIR_ENV), env::var(CHILD_ID_ENV)) else {
            return;
        };
        let path = PathBuf::from(dir).join("learn.tsv");
        let mut learner = Learner::new(LearnConfig::default());
        for i in 0..CHILD_ROUNDS {
            let now = 1_000_000 + u64::from(i);
            learner.record(&format!("p{id}r{i}"), "固有", 0, "learned", now);
            learner.record("きょう", "共有", 0, "learned", now);
            learner.flush(&path).unwrap();
        }
    }

    /// 2 つの子プロセスが同じディレクトリへ同時に flush を繰り返しても、両方の更新が残る。
    #[test]
    fn test_two_processes_concurrent_flush() {
        if env::var(CHILD_DIR_ENV).is_ok() {
            return; // 子プロセス内では実行しない
        }
        let temp_dir = test_temp_dir();
        let exe = env::current_exe().unwrap();
        let spawn = |id: &str| {
            process::Command::new(&exe)
                .args([
                    "--exact",
                    "tests::child_process_flusher",
                    "--test-threads=1",
                ])
                .env(CHILD_DIR_ENV, &temp_dir)
                .env(CHILD_ID_ENV, id)
                .stdout(process::Stdio::null())
                .stderr(process::Stdio::null())
                .spawn()
                .unwrap()
        };
        let mut c1 = spawn("a");
        let mut c2 = spawn("b");
        assert!(c1.wait().unwrap().success());
        assert!(c2.wait().unwrap().success());

        let path = temp_dir.join("learn.tsv");
        let (loaded, report) = Learner::load(&path, LearnConfig::default()).unwrap();
        assert_eq!(report.skipped_lines, 0);
        // 固有キー 2 × ROUNDS + 共有キー 1
        assert_eq!(loaded.len(), (2 * CHILD_ROUNDS + 1) as usize);
        let shared = loaded
            .rows()
            .into_iter()
            .find(|r| r.reading == "きょう")
            .unwrap();
        assert_eq!(shared.count, 2 * CHILD_ROUNDS);
        assert!(!path.with_extension("tmp").exists());
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
}
