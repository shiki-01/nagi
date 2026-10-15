//! 辞書本体（double-array trie + 語エントリ表 + 連接コスト表）とバイナリ形式。
//!
//! バイナリ形式（すべてリトルエンディアン）:
//!
//! ```text
//! "NAGIDICT" (8B) | version u32
//! セクション × 10（それぞれ len u64 + 本体）:
//!   1. trie          fst（有限状態トランスデューサ。キー = 読みの UTF-8、値 = 語グループ番号）
//!   2. group_offsets u32 × (グループ数 + 1)   グループ g の語 = entries[offsets[g]..offsets[g+1]]
//!   3. lids          u16 × 語数
//!   4. rids          u16 × 語数
//!   5. costs         u16 × 語数
//!   6. surf_offsets  u32 × (語数 + 1)         表記文字列プール上のバイト位置
//!   7. surf_pool     UTF-8
//!   8. pos_defs      UTF-8（品詞文字列を '\n' 区切り。行番号 = 品詞 ID。行数は連接コスト表の
//!                    次元 N とは独立。conn_n は 9. の長さから復元する）
//!   9. conn          u16 × (N × N)            conn[rid_prev * N + lid_next]
//!  10. pos_ids       u16 × 語数                品詞 ID（`pos_defs` の行番号への参照。連接コスト
//!                                              ID（lid/rid）とは独立。Mozc 辞書では pos_id = lid）
//! ```
//!
//! 読み込みは、ファイル全体を 1 つのバイト列（`Backing`）として保持し、各セクションはその範囲
//! （オフセット）として参照するだけにする。配列は要素ごとにリトルエンディアンで読むため
//! アラインメントを要求せず、形式（version 2）は変えていない。非 Windows の [`Dictionary::load`] は
//! ファイルを mmap するので、触れたページだけが物理メモリに載り、約 60MB の全量コピーを避けられる
//! （iOS キーボード拡張のメモリ上限対策。decisions.md H-3 / H-6）。
//!
//! 注意: mmap 中のファイルを同じ inode のまま書き換え・短縮するとアクセス時に SIGBUS になる。
//! 辞書の更新は一時ファイルに書いてから rename（別 inode）で置き換えること。Windows ではマップ中の
//! ファイルを置換できないため、[`Dictionary::load`] は mmap を使わず全量を読む。

use crate::error::DictError;
use crate::pos::{Pos, PosClass, PosSub};
use fst::Map;
use std::ops::Range;
use std::sync::{Arc, Mutex};

const MAGIC: &[u8; 8] = b"NAGIDICT";
const VERSION: u32 = 2;
const SECTION_COUNT: usize = 10;

/// 辞書の 1 語
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 表記
    pub surface: String,
    /// 左文脈 ID
    pub lid: u16,
    /// 右文脈 ID
    pub rid: u16,
    /// 品詞 ID（`pos_defs` の行番号への参照。連接コスト ID（lid/rid）とは独立）
    pub pos_id: u16,
    /// 語コスト（小さいほど出やすい）
    pub cost: u16,
}

/// 未知語（辞書に無い文字）に使う品詞 ID
#[derive(Debug, Clone, Copy)]
pub(crate) struct FallbackIds {
    pub noun: u16,
    pub symbol: u16,
    pub comma: u16,
    pub period: u16,
    pub bracket_open: u16,
    pub bracket_close: u16,
}

/// 辞書ファイル全体のバイト列の持ち主（メモリ上の `Vec` か、mmap）
enum Backing {
    Owned(Vec<u8>),
    #[cfg(not(windows))]
    Mapped(memmap2::Mmap),
}

impl Backing {
    fn as_slice(&self) -> &[u8] {
        match self {
            Backing::Owned(v) => v,
            #[cfg(not(windows))]
            Backing::Mapped(m) => m,
        }
    }
}

/// `Backing` の一部分（fst の `Map` に渡すためのデータ参照）
struct SharedSlice {
    backing: Arc<Backing>,
    range: Range<usize>,
}

impl AsRef<[u8]> for SharedSlice {
    fn as_ref(&self) -> &[u8] {
        &self.backing.as_slice()[self.range.clone()]
    }
}

/// 読み込み済みの辞書（不変。`Send + Sync`）
pub struct Dictionary {
    backing: Arc<Backing>,
    trie: Map<SharedSlice>,
    /// 各配列セクションの `backing` 上の範囲（`u16` / `u32` はリトルエンディアン）
    group_offsets: Range<usize>,
    lids: Range<usize>,
    rids: Range<usize>,
    costs: Range<usize>,
    surf_offsets: Range<usize>,
    surf_pool: Range<usize>,
    conn: Range<usize>,
    pos_ids: Range<usize>,
    n_entries: usize,
    n_groups: usize,
    pos_defs: Vec<String>,
    pos: Vec<Pos>,
    conn_n: usize,
    fallback: FallbackIds,
    /// 逆引き索引（表記 → 読み）。`reverse_convert` で最初に使われた時に構築し、以降キャッシュする
    /// （辞書全体を毎回舐めるコストがあるため遅延構築。使わない限りコストは無い）。実データでは
    /// 約300MB規模になるため（G-29 追記）、`release_reverse_index` で明示的に解放できるように
    /// `OnceLock` ではなく `Mutex<Option<_>>` にしている
    reverse: Mutex<Option<Arc<crate::reverse::ReverseIndex>>>,
}

impl std::fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dictionary")
            .field("entries", &self.n_entries)
            .field("groups", &self.n_groups)
            .field("pos_ids", &self.conn_n)
            .finish()
    }
}

/// 構築用の生データ（読み昇順・重複排除済みを前提にした並び）
pub(crate) struct RawParts {
    pub trie_bytes: Vec<u8>,
    pub group_offsets: Vec<u32>,
    pub lids: Vec<u16>,
    pub rids: Vec<u16>,
    pub costs: Vec<u16>,
    pub surf_offsets: Vec<u32>,
    pub surf_pool: String,
    pub pos_defs: Vec<String>,
    pub conn: Vec<u16>,
    pub conn_n: usize,
    pub pos_ids: Vec<u16>,
}

fn find_fallback(pos_defs: &[String]) -> FallbackIds {
    let first = |pred: &dyn Fn(&str) -> bool, default: u16| -> u16 {
        pos_defs
            .iter()
            .position(|d| pred(d))
            .map_or(default, |i| i as u16)
    };
    let noun = first(&|d| d == "名詞,一般,*,*,*,*,*", 1);
    let symbol = first(&|d| d == "記号,一般,*,*,*,*,*", noun);
    FallbackIds {
        noun,
        symbol,
        comma: first(&|d| d == "記号,読点,*,*,*,*,、", symbol),
        period: first(&|d| d == "記号,句点,*,*,*,*,。", symbol),
        bracket_open: first(&|d| d == "記号,括弧開,*,*,*,*,*", symbol),
        bracket_close: first(&|d| d == "記号,括弧閉,*,*,*,*,*", symbol),
    }
}

fn u16s_to_bytes(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn u32s_to_bytes(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// セクション列を辞書のバイナリ形式に並べる
fn assemble(sections: &[Vec<u8>; SECTION_COUNT]) -> Vec<u8> {
    let mut out = Vec::with_capacity(sections.iter().map(|s| s.len() + 8).sum::<usize>() + 12);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    for s in sections {
        out.extend_from_slice(&(s.len() as u64).to_le_bytes());
        out.extend_from_slice(s);
    }
    out
}

/// ヘッダを検査し、各セクションの範囲を返す
fn parse_sections(bytes: &[u8]) -> Result<[Range<usize>; SECTION_COUNT], DictError> {
    let mut pos = 0usize;
    let mut take = |n: usize| -> Result<Range<usize>, DictError> {
        let end = pos
            .checked_add(n)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| DictError::Format("データが途中で終わっています".into()))?;
        let range = pos..end;
        pos = end;
        Ok(range)
    };
    if &bytes[take(8)?] != MAGIC {
        return Err(DictError::Format("マジックが一致しません".into()));
    }
    let version = u32::from_le_bytes(bytes[take(4)?].try_into().expect("4 バイト"));
    if version != VERSION {
        return Err(DictError::Format(format!("未対応のバージョン {version}")));
    }
    let mut sections: Vec<Range<usize>> = Vec::with_capacity(SECTION_COUNT);
    for _ in 0..SECTION_COUNT {
        let len = u64::from_le_bytes(bytes[take(8)?].try_into().expect("8 バイト"));
        let len = usize::try_from(len)
            .map_err(|_| DictError::Format("セクションが大きすぎます".into()))?;
        sections.push(take(len)?);
    }
    Ok(sections.try_into().expect("セクション数は固定"))
}

fn u16_at(bytes: &[u8], section: &Range<usize>, index: usize) -> u16 {
    let p = section.start + index * 2;
    u16::from_le_bytes([bytes[p], bytes[p + 1]])
}

fn u32_at(bytes: &[u8], section: &Range<usize>, index: usize) -> u32 {
    let p = section.start + index * 4;
    u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]])
}

/// 昇順（単調非減少）か。先頭から末尾まで 1 回舐める
fn offsets_monotonic(bytes: &[u8], section: &Range<usize>, count: usize) -> bool {
    let mut prev = 0u32;
    for i in 0..count {
        let v = u32_at(bytes, section, i);
        if v < prev {
            return false;
        }
        prev = v;
    }
    true
}

impl Dictionary {
    pub(crate) fn from_parts(p: RawParts) -> Result<Dictionary, DictError> {
        let n_entries = p.lids.len();
        if p.rids.len() != n_entries
            || p.costs.len() != n_entries
            || p.surf_offsets.len() != n_entries + 1
            || p.group_offsets.last().copied().unwrap_or(0) as usize != n_entries
            || p.conn.len() != p.conn_n * p.conn_n
            || p.pos_ids.len() != n_entries
        {
            return Err(DictError::Format("セクションの長さが整合しません".into()));
        }
        let sections: [Vec<u8>; SECTION_COUNT] = [
            p.trie_bytes,
            u32s_to_bytes(&p.group_offsets),
            u16s_to_bytes(&p.lids),
            u16s_to_bytes(&p.rids),
            u16s_to_bytes(&p.costs),
            u32s_to_bytes(&p.surf_offsets),
            p.surf_pool.into_bytes(),
            p.pos_defs.join("\n").into_bytes(),
            u16s_to_bytes(&p.conn),
            u16s_to_bytes(&p.pos_ids),
        ];
        Dictionary::from_vec(assemble(&sections))
    }

    /// バイナリ辞書ファイルを読み込む。非 Windows では mmap し、失敗したら全量を読む
    pub fn load(path: &std::path::Path) -> Result<Dictionary, DictError> {
        #[cfg(not(windows))]
        if let Some(mapped) = map_file(path) {
            return Dictionary::from_backing(Backing::Mapped(mapped));
        }
        Dictionary::from_vec(std::fs::read(path)?)
    }

    /// バイナリ辞書をバイト列から読み込む（バイト列をコピーして保持する）
    pub fn from_bytes(bytes: &[u8]) -> Result<Dictionary, DictError> {
        Dictionary::from_vec(bytes.to_vec())
    }

    /// バイナリ辞書を所有するバイト列から読み込む
    pub fn from_vec(bytes: Vec<u8>) -> Result<Dictionary, DictError> {
        Dictionary::from_backing(Backing::Owned(bytes))
    }

    fn from_backing(backing: Backing) -> Result<Dictionary, DictError> {
        let backing = Arc::new(backing);
        let bytes = backing.as_slice();
        let [
            trie,
            group_offsets,
            lids,
            rids,
            costs,
            surf_offsets,
            surf_pool,
            pos_defs,
            conn,
            pos_ids,
        ] = parse_sections(bytes)?;

        let multiple = |section: &Range<usize>, size: usize, what: &str| {
            if section.len() % size == 0 {
                Ok(section.len() / size)
            } else {
                Err(DictError::Format(format!("{what} 配列の長さが不正")))
            }
        };
        let n_entries = multiple(&lids, 2, "u16")?;
        let n_group_offsets = multiple(&group_offsets, 4, "u32")?;
        let n_surf_offsets = multiple(&surf_offsets, 4, "u32")?;
        let n_conn = multiple(&conn, 2, "u16")?;
        let n_rids = multiple(&rids, 2, "u16")?;
        let n_costs = multiple(&costs, 2, "u16")?;
        let n_pos_ids = multiple(&pos_ids, 2, "u16")?;
        // pos_defs の行数（品詞の種類数）と conn_n（連接コストIDの数）は独立しているため、
        // conn_n は conn 配列の長さ（N×N）から復元する。完全平方数でなければ不正データ
        let conn_n = n_conn.isqrt();
        if n_rids != n_entries
            || n_costs != n_entries
            || n_surf_offsets != n_entries + 1
            || (n_group_offsets == 0 && n_entries != 0)
            || (n_group_offsets > 0
                && u32_at(bytes, &group_offsets, n_group_offsets - 1) as usize != n_entries)
            || conn_n * conn_n != n_conn
            || n_pos_ids != n_entries
        {
            return Err(DictError::Format("セクションの長さが整合しません".into()));
        }
        if !offsets_monotonic(bytes, &group_offsets, n_group_offsets)
            || !offsets_monotonic(bytes, &surf_offsets, n_surf_offsets)
            || u32_at(bytes, &surf_offsets, n_entries) as usize != surf_pool.len()
        {
            return Err(DictError::Format("オフセット表が不正です".into()));
        }
        if (0..n_entries).any(|i| {
            u16_at(bytes, &lids, i) as usize >= conn_n || u16_at(bytes, &rids, i) as usize >= conn_n
        }) {
            return Err(DictError::Format("品詞 ID が範囲外です".into()));
        }

        let pos_defs_text = std::str::from_utf8(&bytes[pos_defs])
            .map_err(|_| DictError::Format("UTF-8 が不正です".into()))?;
        let pos_defs: Vec<String> = if pos_defs_text.is_empty() {
            Vec::new()
        } else {
            pos_defs_text.split('\n').map(str::to_string).collect()
        };
        if (0..n_entries).any(|i| u16_at(bytes, &pos_ids, i) as usize >= pos_defs.len()) {
            return Err(DictError::Format("品詞 ID が範囲外です".into()));
        }
        let pos = pos_defs.iter().map(|d| Pos::parse(d)).collect();
        let fallback = find_fallback(&pos_defs);
        let trie = Map::new(SharedSlice {
            backing: Arc::clone(&backing),
            range: trie,
        })
        .map_err(|e| DictError::Format(format!("trie が不正です: {e}")))?;
        Ok(Dictionary {
            backing,
            trie,
            group_offsets,
            lids,
            rids,
            costs,
            surf_offsets,
            surf_pool,
            conn,
            pos_ids,
            n_entries,
            n_groups: n_group_offsets.saturating_sub(1),
            pos_defs,
            pos,
            conn_n,
            fallback,
            reverse: Mutex::new(None),
        })
    }

    /// バイナリ辞書にシリアライズする（読み込んだバイト列と同一の内容）
    pub fn to_bytes(&self) -> Vec<u8> {
        self.backing.as_slice().to_vec()
    }

    /// 語数
    pub fn entry_count(&self) -> usize {
        self.n_entries
    }

    /// 読みの種類数（trie のキー数）
    pub fn reading_count(&self) -> usize {
        self.n_groups
    }

    /// 品詞 ID の数（連接コスト表の一辺）
    pub fn pos_id_count(&self) -> usize {
        self.conn_n
    }

    pub(crate) fn entry(&self, index: usize) -> Entry {
        Entry {
            surface: self.surface(index).to_string(),
            lid: self.lid_of(index),
            rid: self.rid_of(index),
            pos_id: self.pos_id_of(index),
            cost: self.cost_of(index),
        }
    }

    pub(crate) fn surface(&self, index: usize) -> &str {
        let bytes = self.backing.as_slice();
        let a = u32_at(bytes, &self.surf_offsets, index) as usize;
        let b = u32_at(bytes, &self.surf_offsets, index + 1) as usize;
        let pool = &bytes[self.surf_pool.clone()];
        // 全量の UTF-8 検査は mmap のページを全て触るため読み込み時には行わず、参照時に検査する。
        // ビルダーが生成した辞書では常に成功し、壊れたファイルでも panic せず空文字になる
        std::str::from_utf8(&pool[a..b]).unwrap_or("")
    }

    pub(crate) fn group_range(&self, group: u32) -> std::ops::Range<usize> {
        let bytes = self.backing.as_slice();
        let g = group as usize;
        u32_at(bytes, &self.group_offsets, g) as usize
            ..u32_at(bytes, &self.group_offsets, g + 1) as usize
    }

    pub(crate) fn lid_of(&self, index: usize) -> u16 {
        u16_at(self.backing.as_slice(), &self.lids, index)
    }

    pub(crate) fn rid_of(&self, index: usize) -> u16 {
        u16_at(self.backing.as_slice(), &self.rids, index)
    }

    pub(crate) fn cost_of(&self, index: usize) -> u16 {
        u16_at(self.backing.as_slice(), &self.costs, index)
    }

    pub(crate) fn pos_id_of(&self, index: usize) -> u16 {
        u16_at(self.backing.as_slice(), &self.pos_ids, index)
    }

    /// 連接コスト（前の語の右 ID, 次の語の左 ID）
    pub(crate) fn connection(&self, rid: u16, lid: u16) -> i32 {
        i32::from(u16_at(
            self.backing.as_slice(),
            &self.conn,
            rid as usize * self.conn_n + lid as usize,
        ))
    }

    pub(crate) fn pos_of(&self, id: u16) -> Pos {
        self.pos.get(id as usize).copied().unwrap_or(Pos {
            class: PosClass::Other,
            sub: PosSub::Other,
        })
    }

    pub(crate) fn pos_defs(&self) -> &[String] {
        &self.pos_defs
    }

    pub(crate) fn fallback_ids(&self) -> FallbackIds {
        self.fallback
    }

    /// `reverse` フィールドへの参照（逆引き索引を構築・キャッシュ・解放するためのロック）
    pub(crate) fn reverse_index_cell(&self) -> &Mutex<Option<Arc<crate::reverse::ReverseIndex>>> {
        &self.reverse
    }

    /// 表記から読みへの trie（読み → 語グループ）を最初から辿るストリーム
    pub(crate) fn trie_stream(&self) -> fst::map::Stream<'_> {
        self.trie.stream()
    }

    /// 読みの完全一致（コスト昇順）
    pub fn lookup(&self, reading: &str) -> Vec<Entry> {
        match self.group_of(reading) {
            Some(g) => self.group_range(g).map(|i| self.entry(i)).collect(),
            None => Vec::new(),
        }
    }

    /// 読みが `prefix` で始まる語（`prefix` 自身を含む）が辞書にあるか。曖昧入力（`nagi-fuzzy`）が、
    /// 展開した読みの候補のうち辞書に接頭辞が無いものを早期に捨てるために使う。語の一覧は作らない
    pub fn has_reading_prefix(&self, prefix: &str) -> bool {
        use fst::{IntoStreamer, Streamer};
        let mut stream = self.trie.range().ge(prefix.as_bytes()).into_stream();
        stream
            .next()
            .is_some_and(|(key, _)| key.starts_with(prefix.as_bytes()))
    }

    /// `text` の先頭から始まる辞書の読みのバイト長（短い順）。語の一覧は作らない
    /// （[`Dictionary::common_prefix_lookup`] の軽量版）
    pub fn reading_prefix_lens(&self, text: &str) -> Vec<usize> {
        self.common_prefix_groups(text.as_bytes())
            .into_iter()
            .map(|(_, len)| len)
            .collect()
    }

    /// 完全一致するグループ番号
    pub(crate) fn group_of(&self, reading: &str) -> Option<u32> {
        self.trie.get(reading.as_bytes()).map(|v| v as u32)
    }

    /// `text` の先頭から始まる全ての読みについて（読みのバイト長, 語一覧）を返す（短い順）
    pub fn common_prefix_lookup(&self, text: &str) -> Vec<(usize, Vec<Entry>)> {
        self.common_prefix_groups(text.as_bytes())
            .into_iter()
            .map(|(g, len)| (len, self.group_range(g).map(|i| self.entry(i)).collect()))
            .collect()
    }

    /// `text` の先頭から始まる全ての読みについて（グループ番号, 読みのバイト長）を短い順に返す
    pub(crate) fn common_prefix_groups(&self, text: &[u8]) -> Vec<(u32, usize)> {
        let fst = self.trie.as_fst();
        let mut node = fst.root();
        let mut output = fst::raw::Output::zero();
        let mut found = Vec::new();
        for (i, &byte) in text.iter().enumerate() {
            let Some(index) = node.find_input(byte) else {
                break;
            };
            let transition = node.transition(index);
            output = output.cat(transition.out);
            node = fst.node(transition.addr);
            if node.is_final() {
                found.push((output.cat(node.final_output()).value() as u32, i + 1));
            }
        }
        found
    }
}

/// 辞書ファイルを mmap する。失敗（ファイルシステム非対応など）したら `None`（呼び出し側が全量読み込みに戻す）
#[cfg(not(windows))]
#[allow(unsafe_code)]
fn map_file(path: &std::path::Path) -> Option<memmap2::Mmap> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: 辞書ファイルは読み取り専用で開く。マップ中に同じ inode が書き換え・短縮されると
    // アクセス時に SIGBUS になりうるため、更新は別 inode への rename で行う運用とする（モジュール先頭の注意）
    unsafe { memmap2::Mmap::map(&file) }.ok()
}
