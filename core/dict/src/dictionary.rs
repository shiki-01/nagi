//! 辞書本体（fst trie + 語エントリ表 + 連接コスト表）とバイナリ形式。
//!
//! # バイナリ形式 (version 3)
//!
//! mmap でそのまま参照できるよう、各セクションを先頭からの絶対オフセットで指し、
//! 8 バイト境界に揃えている。読み込み側はバッファ（[`DictBuffer`]）上のバイト列を
//! 参照するだけで、ヒープに複製しない。数値はすべてリトルエンディアン。
//!
//! ```text
//! 0   "NAGIDICT" (8B)
//! 8   version u32 (= 3)
//! 12  reserved u32 (= 0)
//! 16  セクション表 × 10: (offset u64, len u64)   offset はファイル先頭からの絶対位置（8 の倍数）
//! 176 セクション本体（昇順・重なり無し。セクション間と末尾は 0 埋めで 8 バイト境界に揃える）
//!
//! セクション:
//!   0. trie          fst（キー = 読みの UTF-8、値 = 語グループ番号）
//!   1. group_offsets u32 × (グループ数 + 1)   グループ g の語 = entries[offsets[g]..offsets[g+1]]
//!   2. lids          u16 × 語数
//!   3. rids          u16 × 語数
//!   4. costs         u16 × 語数
//!   5. surf_offsets  u32 × (語数 + 1)         表記文字列プール上のバイト位置
//!   6. surf_pool     UTF-8
//!   7. pos_defs      UTF-8（品詞文字列を '\n' 区切り。行番号 = 品詞 ID。行数は連接コスト表の
//!                    次元 N とは独立。conn_n は 8. の長さから復元する）
//!   8. conn          u16 × (N × N)            conn[rid_prev * N + lid_next]
//!   9. pos_ids       u16 × 語数                品詞 ID（`pos_defs` の行番号への参照。連接コスト
//!                                              ID（lid/rid）とは独立。Mozc 辞書では pos_id = lid）
//! ```
//!
//! 旧形式（version 2: 長さ前置きの連続配置）は読み込めない。`cargo xtask build-dict` で再生成する。
//!
//! 数値の読み出しは `from_le_bytes` で行い、バッファ先頭のアラインメントには依存しない
//! （`Vec<u8>` のような非アラインのバッファでも正しく動く。mmap ならページ境界に揃う）。

use crate::buffer::DictBuffer;
use crate::error::DictError;
use crate::pos::{Pos, PosClass, PosSub};
use fst::Map;
use std::ops::Range;
use std::sync::{Arc, Mutex};

#[cfg(target_endian = "big")]
compile_error!("nagi-dict のバイナリ形式はリトルエンディアン前提です");

const MAGIC: &[u8; 8] = b"NAGIDICT";
const VERSION: u32 = 3;
const N_SECTIONS: usize = 10;
const HEADER_LEN: usize = 16;
const DATA_START: usize = HEADER_LEN + N_SECTIONS * 16;
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

/// `Map` に渡すための、共有バッファの一部分への参照（ヒープに複製しない）
#[derive(Clone)]
struct Section {
    buf: Arc<DictBuffer>,
    range: Range<usize>,
}

impl AsRef<[u8]> for Section {
    fn as_ref(&self) -> &[u8] {
        &self.buf.as_ref().as_ref()[self.range.clone()]
    }
}

/// 各セクションのバッファ内範囲（`open` で検証済みなのでスライスしても範囲外にならない）
struct Layout {
    group_offsets: Range<usize>,
    lids: Range<usize>,
    rids: Range<usize>,
    costs: Range<usize>,
    surf_offsets: Range<usize>,
    surf_pool: Range<usize>,
    conn: Range<usize>,
    pos_ids: Range<usize>,
}

/// 読み込み済みの辞書（不変。`Send + Sync`）。数値表・文字列プール・trie はすべて
/// バッファ（ファイル mmap またはヒープ）上のバイト列を直接参照する
pub struct Dictionary {
    buf: Arc<DictBuffer>,
    trie: Map<Section>,
    layout: Layout,
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

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[2 * i], b[2 * i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
}

fn u64_at(b: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(b[offset..offset + 8].try_into().expect("8 バイト"))
}

fn format_err(msg: &str) -> DictError {
    DictError::Format(msg.into())
}

/// セクション表を読み、範囲を検証する（昇順・重なり無し・8 バイト境界・バッファ内）
fn read_section_table(bytes: &[u8]) -> Result<[Range<usize>; N_SECTIONS], DictError> {
    if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
        return Err(format_err("マジックが一致しません"));
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().expect("4 バイト"));
    if version != VERSION {
        return Err(DictError::Format(if version < VERSION {
            format!(
                "旧形式の辞書です（バージョン {version}、必要 {VERSION}）。`cargo xtask build-dict` で再生成してください"
            )
        } else {
            format!(
                "未対応のバージョン {version}（対応 {VERSION}）。Nagi を更新するか、辞書を再生成してください"
            )
        }));
    }
    if bytes.len() < DATA_START {
        return Err(format_err("データが途中で終わっています"));
    }
    let mut ranges: [Range<usize>; N_SECTIONS] = std::array::from_fn(|_| 0..0);
    let mut cursor = DATA_START;
    for (i, range) in ranges.iter_mut().enumerate() {
        let offset = u64_at(bytes, HEADER_LEN + i * 16);
        let len = u64_at(bytes, HEADER_LEN + i * 16 + 8);
        let (Ok(offset), Ok(len)) = (usize::try_from(offset), usize::try_from(len)) else {
            return Err(format_err("セクションが大きすぎます"));
        };
        if offset % ALIGN != 0 {
            return Err(format_err("セクションが 8 バイト境界に揃っていません"));
        }
        let end = offset
            .checked_add(len)
            .ok_or_else(|| format_err("セクション範囲が不正です"))?;
        if offset < cursor {
            return Err(format_err("セクションの配置が不正です"));
        }
        if end > bytes.len() {
            return Err(format_err("データが途中で終わっています"));
        }
        *range = offset..end;
        cursor = end;
    }
    Ok(ranges)
}

/// `b` を 8 バイト境界まで 0 埋めする
fn pad8(out: &mut Vec<u8>) {
    out.resize(out.len().next_multiple_of(ALIGN), 0);
}

impl Dictionary {
    /// 生データをバイナリ形式に直列化し、そのバッファ上の辞書として開く
    pub(crate) fn from_parts(p: RawParts) -> Result<Dictionary, DictError> {
        if p.conn.len() != p.conn_n * p.conn_n {
            return Err(format_err("セクションの長さが整合しません"));
        }
        let sections: [Vec<u8>; N_SECTIONS] = [
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
        let total = DATA_START + sections.iter().map(|s| s.len() + ALIGN).sum::<usize>();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.resize(DATA_START, 0);
        for (i, s) in sections.iter().enumerate() {
            pad8(&mut out);
            let table = HEADER_LEN + i * 16;
            let offset = out.len() as u64;
            out[table..table + 8].copy_from_slice(&offset.to_le_bytes());
            out[table + 8..table + 16].copy_from_slice(&(s.len() as u64).to_le_bytes());
            out.extend_from_slice(s);
        }
        pad8(&mut out);
        Dictionary::from_buffer(DictBuffer::from(out))
    }

    /// バイナリ辞書ファイルを読み込む。ファイルはメモリマップして参照する（全体をヒープに読まない）。
    /// マップできない環境ではヒープに読み込む
    ///
    /// マップ中にファイルを書き換える・切り詰めると不定動作（SIGBUS 等）になりうるため、
    /// 更新時は別名で書いてからリネームで差し替えること（Windows では使用中の差し替え自体が失敗する）。
    pub fn load(path: &std::path::Path) -> Result<Dictionary, DictError> {
        Dictionary::from_buffer(DictBuffer::map_file(path)?)
    }

    /// バイナリ辞書ファイルをヒープに読み込む（mmap を使わない）
    pub fn load_to_heap(path: &std::path::Path) -> Result<Dictionary, DictError> {
        Dictionary::from_buffer(DictBuffer::from(std::fs::read(path)?))
    }

    /// バイナリ辞書をバイト列から読み込む（バイト列はヒープに複製される。複製を避けたい場合は
    /// [`Dictionary::from_buffer`] に所有バッファを渡す）
    pub fn from_bytes(bytes: &[u8]) -> Result<Dictionary, DictError> {
        Dictionary::from_buffer(DictBuffer::from(bytes.to_vec()))
    }

    /// バッファ上のバイナリ辞書を検証して開く。以降はバッファを参照するだけで複製しない
    pub fn from_buffer(buffer: DictBuffer) -> Result<Dictionary, DictError> {
        let buf = Arc::new(buffer);
        let bytes: &[u8] = (*buf).as_ref();
        let r = read_section_table(bytes)?;
        let sec = |i: usize| &bytes[r[i].clone()];

        let pos_defs_text =
            std::str::from_utf8(sec(7)).map_err(|_| format_err("UTF-8 が不正です"))?;
        let pos_defs: Vec<String> = if pos_defs_text.is_empty() {
            Vec::new()
        } else {
            pos_defs_text.split('\n').map(str::to_string).collect()
        };

        let (group_offsets, surf_offsets) = (sec(1), sec(5));
        let (lids, rids, costs, pos_ids, conn) = (sec(2), sec(3), sec(4), sec(9), sec(8));
        if group_offsets.len() % 4 != 0
            || surf_offsets.len() % 4 != 0
            || [lids, rids, costs, pos_ids, conn]
                .iter()
                .any(|s| s.len() % 2 != 0)
        {
            return Err(format_err("配列の長さが不正"));
        }
        let n_entries = lids.len() / 2;
        let n_groups = (group_offsets.len() / 4).saturating_sub(1);
        // pos_defs の行数（品詞の種類数）と conn_n（連接コストIDの数）は独立しているため、
        // conn_n は conn 配列の長さ（N×N）から復元する
        let conn_len = conn.len() / 2;
        let conn_n = conn_len.isqrt();
        if rids.len() / 2 != n_entries
            || costs.len() / 2 != n_entries
            || pos_ids.len() / 2 != n_entries
            || surf_offsets.len() / 4 != n_entries + 1
            || (group_offsets.len() / 4 != 0
                && u32_at(group_offsets, n_groups) as usize != n_entries)
            || (group_offsets.is_empty() && n_entries != 0)
            || conn_n * conn_n != conn_len
        {
            return Err(format_err("セクションの長さが整合しません"));
        }
        let pool = sec(6);
        let monotonic = |b: &[u8]| (1..b.len() / 4).all(|i| u32_at(b, i - 1) <= u32_at(b, i));
        if !monotonic(group_offsets)
            || !monotonic(surf_offsets)
            || u32_at(surf_offsets, n_entries) as usize != pool.len()
        {
            return Err(format_err("オフセット表が不正です"));
        }
        std::str::from_utf8(pool).map_err(|_| format_err("UTF-8 が不正です"))?;
        if (0..n_entries)
            .any(|i| u16_at(lids, i) as usize >= conn_n || u16_at(rids, i) as usize >= conn_n)
        {
            return Err(format_err("品詞 ID が範囲外です"));
        }
        if (0..n_entries).any(|i| u16_at(pos_ids, i) as usize >= pos_defs.len()) {
            return Err(format_err("品詞 ID が範囲外です"));
        }

        let trie = Map::new(Section {
            buf: Arc::clone(&buf),
            range: r[0].clone(),
        })
        .map_err(|e| DictError::Format(format!("trie が不正です: {e}")))?;
        let layout = Layout {
            group_offsets: r[1].clone(),
            lids: r[2].clone(),
            rids: r[3].clone(),
            costs: r[4].clone(),
            surf_offsets: r[5].clone(),
            surf_pool: r[6].clone(),
            conn: r[8].clone(),
            pos_ids: r[9].clone(),
        };
        let pos = pos_defs.iter().map(|d| Pos::parse(d)).collect();
        let fallback = find_fallback(&pos_defs);
        Ok(Dictionary {
            buf,
            trie,
            layout,
            n_entries,
            n_groups,
            pos_defs,
            pos,
            conn_n,
            fallback,
            reverse: Mutex::new(None),
        })
    }

    /// バイナリ辞書にシリアライズする（バッファはすでにバイナリ形式そのものなので複製するだけ）
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes().to_vec()
    }

    fn bytes(&self) -> &[u8] {
        (*self.buf).as_ref()
    }

    fn slice(&self, range: &Range<usize>) -> &[u8] {
        &self.bytes()[range.clone()]
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
        let offsets = self.slice(&self.layout.surf_offsets);
        let a = u32_at(offsets, index) as usize;
        let b = u32_at(offsets, index + 1) as usize;
        // 読み込み時にプール全体の UTF-8 とオフセットの単調性を検証済み。文字境界から外れた
        // オフセットを持つ壊れた辞書は、パニックさせず空文字列を返す
        std::str::from_utf8(&self.slice(&self.layout.surf_pool)[a..b]).unwrap_or("")
    }

    pub(crate) fn group_range(&self, group: u32) -> std::ops::Range<usize> {
        let offsets = self.slice(&self.layout.group_offsets);
        u32_at(offsets, group as usize) as usize..u32_at(offsets, group as usize + 1) as usize
    }

    pub(crate) fn lid_of(&self, index: usize) -> u16 {
        u16_at(self.slice(&self.layout.lids), index)
    }

    pub(crate) fn rid_of(&self, index: usize) -> u16 {
        u16_at(self.slice(&self.layout.rids), index)
    }

    pub(crate) fn cost_of(&self, index: usize) -> u16 {
        u16_at(self.slice(&self.layout.costs), index)
    }

    pub(crate) fn pos_id_of(&self, index: usize) -> u16 {
        u16_at(self.slice(&self.layout.pos_ids), index)
    }

    /// 連接コスト（前の語の右 ID, 次の語の左 ID）
    pub(crate) fn connection(&self, rid: u16, lid: u16) -> i32 {
        i32::from(u16_at(
            self.slice(&self.layout.conn),
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
