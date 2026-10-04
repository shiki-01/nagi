//! 辞書本体（FST + 語エントリ表 + 連接コスト表）とバイナリ形式。
//!
//! バイナリ形式 v3（すべてリトルエンディアン）は、mmap してゼロコピーで参照できるよう、
//! 各セクションを 8 バイト境界に置き、ロード時にヒープへ展開しない（`pos_defs` だけは件数が
//! 少ないので `Vec<String>` にする）:
//!
//! ```text
//! "NAGIDICT" (8B) | version u32 (=3) | reserved u32 (=0)
//! セクションテーブル × 10（それぞれ offset u64 + len u64。offset はファイル先頭からのバイト位置）
//! セクション本体（昇順・重ならない・各 offset は 8 の倍数・隙間は 0 埋め）:
//!   1. trie          fst（有限状態トランスデューサ。キー = 読みの UTF-8、値 = 語グループ番号）
//!   2. group_offsets u32 × (グループ数 + 1)   グループ g の語 = entries[offsets[g]..offsets[g+1]]
//!   3. lids          u16 × 語数
//!   4. rids          u16 × 語数
//!   5. costs         u16 × 語数
//!   6. surf_offsets  u32 × (語数 + 1)         表記文字列プール上のバイト位置
//!   7. surf_pool     UTF-8（表記を連結したもの）
//!   8. pos_defs      UTF-8（品詞文字列を '\n' 区切り。行番号 = 品詞 ID。行数は連接コスト表の
//!                    次元 N とは独立。conn_n は 9. の長さから復元する）
//!   9. conn          u16 × (N × N)            conn[rid_prev * N + lid_next]
//!  10. pos_ids       u16 × 語数                品詞 ID（`pos_defs` の行番号への参照。連接コスト
//!                                              ID（lid/rid）とは独立。Mozc 辞書では pos_id = lid）
//! ```
//!
//! 配列の要素は `u16::from_le_bytes` 等で都度読む（アラインメントや `unsafe` に依存しない）。
//! v2 以前（セクションが `len u64 + 本体` の連続）は読めない。`cargo xtask build-dict` で作り直す。

use crate::error::DictError;
use crate::pos::{Pos, PosClass, PosSub};
use fst::Map;
use std::ops::{Deref, Range};
use std::sync::{Arc, Mutex};

const MAGIC: &[u8; 8] = b"NAGIDICT";
const VERSION: u32 = 3;
const SECTION_COUNT: usize = 10;
/// マジック 8 + バージョン 4 + 予約 4 + セクションテーブル（16 × 10）
const HEADER_LEN: usize = 16 + SECTION_COUNT * 16;
/// 各セクションの先頭アラインメント
const ALIGN: usize = 8;

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

/// 辞書バイト列の所有者（ヒープ上の `Vec` か、feature `mmap` のファイルマップ）
enum Bytes {
    Heap(Vec<u8>),
    #[cfg(feature = "mmap")]
    Mapped(memmap2::Mmap),
}

impl Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Bytes::Heap(v) => v,
            #[cfg(feature = "mmap")]
            Bytes::Mapped(m) => m,
        }
    }
}

/// バイト列上の範囲（ロード時に境界検査済み）
#[derive(Debug, Clone, Copy, Default)]
struct Span {
    start: usize,
    len: usize,
}

/// FST が参照する、辞書バイト列の一部（`fst::Map<D: AsRef<[u8]>>` 用）
struct Region {
    bytes: Arc<Bytes>,
    span: Span,
}

impl AsRef<[u8]> for Region {
    fn as_ref(&self) -> &[u8] {
        self.bytes
            .get(self.span.start..self.span.start + self.span.len)
            .unwrap_or(&[])
    }
}

/// セクション表（`SECTION_COUNT` 個の `Span`。並びは形式の節番号 1〜10）
#[derive(Debug, Clone, Copy)]
struct Layout {
    trie: Span,
    group_offsets: Span,
    lids: Span,
    rids: Span,
    costs: Span,
    surf_offsets: Span,
    surf_pool: Span,
    pos_defs: Span,
    conn: Span,
    pos_ids: Span,
}

/// 読み込み済みの辞書（不変。`Send + Sync`）
///
/// 語表・連接コスト表・表記文字列はバイト列（ヒープまたは mmap）を直接参照し、ロード時に
/// 展開しない。ヒープに持つのは品詞定義（`pos_defs` / `pos`。件数が少ない）だけ。
pub struct Dictionary {
    bytes: Arc<Bytes>,
    layout: Layout,
    trie: Map<Region>,
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

fn format_err(msg: impl Into<String>) -> DictError {
    DictError::Format(msg.into())
}

/// `RawParts` を v3 形式のバイト列にする
fn serialize(p: &RawParts) -> Result<Vec<u8>, DictError> {
    let n_entries = p.lids.len();
    if p.rids.len() != n_entries
        || p.costs.len() != n_entries
        || p.surf_offsets.len() != n_entries + 1
        || p.pos_ids.len() != n_entries
        || p.group_offsets.last().copied().unwrap_or(0) as usize != n_entries
        || p.conn.len() != p.conn_n * p.conn_n
    {
        return Err(format_err("セクションの長さが整合しません"));
    }
    let u16s = |v: &[u16]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let u32s = |v: &[u32]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let sections: [Vec<u8>; SECTION_COUNT] = [
        p.trie_bytes.clone(),
        u32s(&p.group_offsets),
        u16s(&p.lids),
        u16s(&p.rids),
        u16s(&p.costs),
        u32s(&p.surf_offsets),
        p.surf_pool.clone().into_bytes(),
        p.pos_defs.join("\n").into_bytes(),
        u16s(&p.conn),
        u16s(&p.pos_ids),
    ];
    let mut out =
        Vec::with_capacity(HEADER_LEN + sections.iter().map(|s| s.len() + ALIGN).sum::<usize>());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    // 本体の配置（オフセット）を先に決めてテーブルを書く
    let mut offset = HEADER_LEN;
    let mut offsets = [0usize; SECTION_COUNT];
    for (i, s) in sections.iter().enumerate() {
        offset = offset.next_multiple_of(ALIGN);
        offsets[i] = offset;
        offset += s.len();
    }
    for (i, s) in sections.iter().enumerate() {
        out.extend_from_slice(&(offsets[i] as u64).to_le_bytes());
        out.extend_from_slice(&(s.len() as u64).to_le_bytes());
    }
    for (i, s) in sections.iter().enumerate() {
        out.resize(offsets[i], 0);
        out.extend_from_slice(s);
    }
    Ok(out)
}

/// ヘッダとセクション表を境界検査して `Layout` にする（本体の中身は読まない）
fn parse_layout(data: &[u8]) -> Result<Layout, DictError> {
    if data.len() < 12 {
        return Err(format_err("データが途中で終わっています"));
    }
    if &data[..8] != MAGIC {
        return Err(format_err("マジックが一致しません"));
    }
    let version = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    if version != VERSION {
        return Err(format_err(format!(
            "未対応のバージョン {version}（必要: {VERSION}）。辞書を `cargo xtask build-dict` で作り直してください"
        )));
    }
    if data.len() < HEADER_LEN {
        return Err(format_err("ヘッダが途中で終わっています"));
    }
    let read_u64 = |at: usize| -> u64 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&data[at..at + 8]);
        u64::from_le_bytes(b)
    };
    let mut spans = [Span::default(); SECTION_COUNT];
    let mut prev_end = HEADER_LEN;
    for (i, span) in spans.iter_mut().enumerate() {
        let off = usize::try_from(read_u64(16 + i * 16))
            .map_err(|_| format_err("セクションの位置が大きすぎます"))?;
        let len = usize::try_from(read_u64(24 + i * 16))
            .map_err(|_| format_err("セクションが大きすぎます"))?;
        if off % ALIGN != 0 {
            return Err(format_err(format!(
                "セクション {} の位置 {off} が {ALIGN} バイト境界にありません",
                i + 1
            )));
        }
        let end = off
            .checked_add(len)
            .ok_or_else(|| format_err("セクションの範囲が溢れています"))?;
        if off < prev_end {
            return Err(format_err(format!(
                "セクション {} が前のセクションやヘッダと重なっています",
                i + 1
            )));
        }
        if end > data.len() {
            return Err(format_err("データが途中で終わっています"));
        }
        *span = Span { start: off, len };
        prev_end = end;
    }
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
    ] = spans;
    Ok(Layout {
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
    })
}

impl Dictionary {
    /// バイト列（`bytes`）を辞書として開く。`full_verify` が真なら中身まで全検査する
    /// （ヒープ読み込みは真。mmap は全ページに触れてしまうため偽にし、必要なら [`Dictionary::verify`]）
    fn open(bytes: Bytes, full_verify: bool) -> Result<Dictionary, DictError> {
        let layout = parse_layout(&bytes)?;
        let data: &[u8] = &bytes;
        let slice = |s: Span| &data[s.start..s.start + s.len];
        let n_entries = layout.lids.len / 2;
        let n_groups_plus_1 = layout.group_offsets.len / 4;
        let conn_cells = layout.conn.len / 2;
        let conn_n = conn_cells.isqrt();
        if layout.lids.len % 2 != 0
            || layout.rids.len != layout.lids.len
            || layout.costs.len != layout.lids.len
            || layout.pos_ids.len != layout.lids.len
            || layout.group_offsets.len % 4 != 0
            || n_groups_plus_1 == 0
            || layout.surf_offsets.len % 4 != 0
            || layout.surf_offsets.len / 4 != n_entries + 1
            || layout.conn.len % 2 != 0
            || conn_n * conn_n != conn_cells
            || conn_n > usize::from(u16::MAX) + 1
        {
            return Err(format_err("セクションの長さが整合しません"));
        }
        let u32_at = |s: Span, i: usize| -> usize {
            let o = s.start + i * 4;
            u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize
        };
        // 先頭・末尾の番兵だけ O(1) で確認する（単調性などの全検査は verify）
        if u32_at(layout.group_offsets, 0) != 0
            || u32_at(layout.group_offsets, n_groups_plus_1 - 1) != n_entries
            || u32_at(layout.surf_offsets, 0) != 0
            || u32_at(layout.surf_offsets, n_entries) != layout.surf_pool.len
        {
            return Err(format_err("オフセット表が不正です"));
        }
        let pos_defs_text = std::str::from_utf8(slice(layout.pos_defs))
            .map_err(|_| format_err("品詞定義の UTF-8 が不正です"))?;
        let pos_defs: Vec<String> = if pos_defs_text.is_empty() {
            Vec::new()
        } else {
            pos_defs_text.split('\n').map(str::to_string).collect()
        };
        let pos = pos_defs.iter().map(|d| Pos::parse(d)).collect();
        let fallback = find_fallback(&pos_defs);
        let bytes = Arc::new(bytes);
        let trie = Map::new(Region {
            bytes: Arc::clone(&bytes),
            span: layout.trie,
        })
        .map_err(|e| format_err(format!("trie が不正です: {e}")))?;
        let dict = Dictionary {
            bytes,
            layout,
            trie,
            n_entries,
            n_groups: n_groups_plus_1 - 1,
            pos_defs,
            pos,
            conn_n,
            fallback,
            reverse: Mutex::new(None),
        };
        if full_verify {
            dict.verify()?;
        }
        Ok(dict)
    }

    pub(crate) fn from_parts(p: RawParts) -> Result<Dictionary, DictError> {
        Dictionary::from_vec(serialize(&p)?)
    }

    /// バイナリ辞書ファイルを `fs::read` でヒープに読み込む（全検査つき）
    pub fn load(path: &std::path::Path) -> Result<Dictionary, DictError> {
        Dictionary::from_vec(std::fs::read(path)?)
    }

    /// バイナリ辞書ファイルを mmap してゼロコピーで参照する（feature `mmap`）。
    ///
    /// 常駐メモリはクリーンなファイルページ（OS が回収できる）になり、ヒープには品詞定義しか
    /// 持たない。ヘッダ・セクション表は境界検査するが、全ページを舐める中身の検査
    /// （[`Dictionary::verify`]）は行わない。アクセスはすべて境界チェック付きなので、壊れた
    /// ファイルでも panic はせず（結果が空・不正になり得る）、FST の中身の破損だけは検出できない。
    /// 辞書ファイルの更新は別名で作ってからリネームすること（`src/mmap.rs` の安全性の根拠）
    #[cfg(feature = "mmap")]
    pub fn load_mmap(path: &std::path::Path) -> Result<Dictionary, DictError> {
        let map = crate::mmap::map_readonly(path)?;
        Dictionary::open(Bytes::Mapped(map), false)
    }

    /// バイナリ辞書をバイト列から読み込む（コピーしてヒープに持つ。全検査つき）
    pub fn from_bytes(bytes: &[u8]) -> Result<Dictionary, DictError> {
        Dictionary::from_vec(bytes.to_vec())
    }

    /// バイナリ辞書を `Vec<u8>` から（コピーせず所有して）読み込む（全検査つき）
    pub fn from_vec(bytes: Vec<u8>) -> Result<Dictionary, DictError> {
        Dictionary::open(Bytes::Heap(bytes), true)
    }

    /// 中身まで全検査する（全ページに触れる）。オフセット表の単調性・文字境界・ID の範囲・
    /// FST の整合（チェックサム）を確認する。FST の値（語グループ番号）の範囲は、参照時に
    /// `group_range` が範囲外を空として扱うので検査しない（全キーを舐めるのに約140msかかるため）。
    /// `load` / `from_bytes` は内部で呼ぶ
    pub fn verify(&self) -> Result<(), DictError> {
        let n = self.n_entries;
        let group_offsets_ok = (0..self.n_groups).all(|g| {
            self.u32_at(self.layout.group_offsets, g)
                <= self.u32_at(self.layout.group_offsets, g + 1)
        });
        let pool = self.pool_bytes();
        let pool_str =
            std::str::from_utf8(pool).map_err(|_| format_err("表記の UTF-8 が不正です"))?;
        let surf_ok = (0..n).all(|i| {
            let a = self.u32_at(self.layout.surf_offsets, i);
            let b = self.u32_at(self.layout.surf_offsets, i + 1);
            a <= b && pool_str.is_char_boundary(a) && pool_str.is_char_boundary(b)
        });
        if !group_offsets_ok || !surf_ok {
            return Err(format_err("オフセット表が不正です"));
        }
        let ids_ok = (0..n).all(|i| {
            usize::from(self.u16_at(self.layout.lids, i)) < self.conn_n
                && usize::from(self.u16_at(self.layout.rids, i)) < self.conn_n
                && usize::from(self.u16_at(self.layout.pos_ids, i)) < self.pos_defs.len()
        });
        if !ids_ok {
            return Err(format_err("品詞 ID が範囲外です"));
        }
        self.trie
            .as_fst()
            .verify()
            .map_err(|e| format_err(format!("trie が不正です: {e}")))?;
        Ok(())
    }

    /// v3 形式のバイト列そのもの（`to_bytes` のコピーを避けたい書き出し用）
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// バイナリ辞書にシリアライズする
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.to_vec()
    }

    fn data(&self) -> &[u8] {
        &self.bytes
    }

    fn pool_bytes(&self) -> &[u8] {
        let s = self.layout.surf_pool;
        self.data().get(s.start..s.start + s.len).unwrap_or(&[])
    }

    /// `s` の `i` 番目の u16（範囲外は 0。ロード時に検査済みなので通常は起きない）
    fn u16_at(&self, s: Span, i: usize) -> u16 {
        if i >= s.len / 2 {
            return 0;
        }
        let o = s.start + i * 2;
        self.data()
            .get(o..o + 2)
            .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// `s` の `i` 番目の u32（範囲外は 0）
    fn u32_at(&self, s: Span, i: usize) -> usize {
        if i >= s.len / 4 {
            return 0;
        }
        let o = s.start + i * 4;
        self.data()
            .get(o..o + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
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

    /// 表記（バイト列が壊れていて UTF-8 として不正な場合は空文字）
    pub(crate) fn surface(&self, index: usize) -> &str {
        let a = self.u32_at(self.layout.surf_offsets, index);
        let b = self.u32_at(self.layout.surf_offsets, index + 1);
        self.pool_bytes()
            .get(a..b)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    pub(crate) fn group_range(&self, group: u32) -> Range<usize> {
        let g = group as usize;
        if g >= self.n_groups {
            return 0..0;
        }
        let a = self.u32_at(self.layout.group_offsets, g);
        let b = self
            .u32_at(self.layout.group_offsets, g + 1)
            .min(self.n_entries);
        a.min(b)..b
    }

    pub(crate) fn lid_of(&self, index: usize) -> u16 {
        self.u16_at(self.layout.lids, index)
    }

    pub(crate) fn rid_of(&self, index: usize) -> u16 {
        self.u16_at(self.layout.rids, index)
    }

    pub(crate) fn cost_of(&self, index: usize) -> u16 {
        self.u16_at(self.layout.costs, index)
    }

    pub(crate) fn pos_id_of(&self, index: usize) -> u16 {
        self.u16_at(self.layout.pos_ids, index)
    }

    /// 連接コスト（前の語の右 ID, 次の語の左 ID）
    pub(crate) fn connection(&self, rid: u16, lid: u16) -> i32 {
        i32::from(self.u16_at(self.layout.conn, rid as usize * self.conn_n + lid as usize))
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

    /// 読みが辞書のいずれかの読みの接頭辞か（完全一致を含む。空文字は true）
    ///
    /// FST を辿るだけで語は取り出さない。曖昧入力の枝刈り用。
    pub fn has_prefix(&self, reading: &str) -> bool {
        // 遷移を辿れれば接頭辞（FST は最小化されており、到達できる節点の先には必ず語がある）。
        // `search` + ストリームより桁違いに速い（照会ごとの割り当てが無い）
        let fst = self.trie.as_fst();
        let mut node = fst.root();
        for &byte in reading.as_bytes() {
            let Some(index) = node.find_input(byte) else {
                return false;
            };
            node = fst.node(node.transition(index).addr);
        }
        true
    }

    /// 読みが辞書の読みとして完全一致で存在するか
    pub fn contains_reading(&self, reading: &str) -> bool {
        self.group_of(reading).is_some()
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
