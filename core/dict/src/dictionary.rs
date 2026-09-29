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

use crate::error::DictError;
use crate::pos::{Pos, PosClass, PosSub};
use fst::Map;
use std::sync::{Arc, Mutex};

const MAGIC: &[u8; 8] = b"NAGIDICT";
const VERSION: u32 = 2;

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

/// 読み込み済みの辞書（不変。`Send + Sync`）
pub struct Dictionary {
    trie_bytes: Vec<u8>,
    trie: Map<Vec<u8>>,
    group_offsets: Vec<u32>,
    lids: Vec<u16>,
    rids: Vec<u16>,
    costs: Vec<u16>,
    surf_offsets: Vec<u32>,
    surf_pool: String,
    pos_defs: Vec<String>,
    pos: Vec<Pos>,
    conn: Vec<u16>,
    pos_ids: Vec<u16>,
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
            .field("entries", &self.lids.len())
            .field("groups", &self.group_offsets.len().saturating_sub(1))
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

fn bytes_to_u16s(b: &[u8]) -> Result<Vec<u16>, DictError> {
    if b.len() % 2 != 0 {
        return Err(DictError::Format("u16 配列の長さが不正".into()));
    }
    Ok(b.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect())
}

fn bytes_to_u32s(b: &[u8]) -> Result<Vec<u32>, DictError> {
    if b.len() % 4 != 0 {
        return Err(DictError::Format("u32 配列の長さが不正".into()));
    }
    Ok(b.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

impl Dictionary {
    pub(crate) fn from_parts(p: RawParts) -> Result<Dictionary, DictError> {
        let n_entries = p.lids.len();
        let n_groups = p.group_offsets.len().saturating_sub(1);
        if p.rids.len() != n_entries
            || p.costs.len() != n_entries
            || p.surf_offsets.len() != n_entries + 1
            || p.group_offsets.last().copied().unwrap_or(0) as usize != n_entries
            || p.conn.len() != p.conn_n * p.conn_n
            || p.pos_ids.len() != n_entries
        {
            return Err(DictError::Format("セクションの長さが整合しません".into()));
        }
        if p.group_offsets.windows(2).any(|w| w[0] > w[1])
            || p.surf_offsets.windows(2).any(|w| w[0] > w[1])
            || p.surf_offsets.last().copied().unwrap_or(0) as usize != p.surf_pool.len()
        {
            return Err(DictError::Format("オフセット表が不正です".into()));
        }
        if p.lids
            .iter()
            .chain(p.rids.iter())
            .any(|&id| id as usize >= p.conn_n)
        {
            return Err(DictError::Format("品詞 ID が範囲外です".into()));
        }
        if p.pos_ids.iter().any(|&id| id as usize >= p.pos_defs.len()) {
            return Err(DictError::Format("品詞 ID が範囲外です".into()));
        }
        let _ = n_groups;
        let pos = p.pos_defs.iter().map(|d| Pos::parse(d)).collect();
        let fallback = find_fallback(&p.pos_defs);
        Ok(Dictionary {
            trie: Map::new(p.trie_bytes.clone())
                .map_err(|e| DictError::Format(format!("trie が不正です: {e}")))?,
            trie_bytes: p.trie_bytes,
            group_offsets: p.group_offsets,
            lids: p.lids,
            rids: p.rids,
            costs: p.costs,
            surf_offsets: p.surf_offsets,
            surf_pool: p.surf_pool,
            pos_defs: p.pos_defs,
            pos,
            conn: p.conn,
            conn_n: p.conn_n,
            pos_ids: p.pos_ids,
            fallback,
            reverse: Mutex::new(None),
        })
    }

    /// バイナリ辞書ファイルを読み込む
    pub fn load(path: &std::path::Path) -> Result<Dictionary, DictError> {
        Dictionary::from_bytes(&std::fs::read(path)?)
    }

    /// バイナリ辞書をバイト列から読み込む
    pub fn from_bytes(bytes: &[u8]) -> Result<Dictionary, DictError> {
        let mut rest = bytes;
        let mut take = |n: usize| -> Result<&[u8], DictError> {
            if rest.len() < n {
                return Err(DictError::Format("データが途中で終わっています".into()));
            }
            let (head, tail) = rest.split_at(n);
            rest = tail;
            Ok(head)
        };
        if take(8)? != MAGIC {
            return Err(DictError::Format("マジックが一致しません".into()));
        }
        let version = u32::from_le_bytes(take(4)?.try_into().expect("4 バイト"));
        if version != VERSION {
            return Err(DictError::Format(format!("未対応のバージョン {version}")));
        }
        let mut sections: Vec<&[u8]> = Vec::with_capacity(10);
        for _ in 0..10 {
            let len = u64::from_le_bytes(take(8)?.try_into().expect("8 バイト"));
            let len = usize::try_from(len)
                .map_err(|_| DictError::Format("セクションが大きすぎます".into()))?;
            sections.push(take(len)?);
        }
        let text = |b: &[u8]| -> Result<String, DictError> {
            String::from_utf8(b.to_vec()).map_err(|_| DictError::Format("UTF-8 が不正です".into()))
        };
        let pos_defs_text = text(sections[7])?;
        let pos_defs: Vec<String> = if pos_defs_text.is_empty() {
            Vec::new()
        } else {
            pos_defs_text.split('\n').map(str::to_string).collect()
        };
        let conn = bytes_to_u16s(sections[8])?;
        // pos_defs の行数（品詞の種類数）と conn_n（連接コストIDの数）は独立しているため、
        // conn_n は conn 配列の長さ（N×N）から復元する。conn.len() が完全平方数でなければ
        // 後続の RawParts バリデーション（conn.len() != conn_n * conn_n）で不正データとして弾かれる
        let conn_n = conn.len().isqrt();
        let pos_ids = bytes_to_u16s(sections[9])?;
        Dictionary::from_parts(RawParts {
            trie_bytes: sections[0].to_vec(),
            group_offsets: bytes_to_u32s(sections[1])?,
            lids: bytes_to_u16s(sections[2])?,
            rids: bytes_to_u16s(sections[3])?,
            costs: bytes_to_u16s(sections[4])?,
            surf_offsets: bytes_to_u32s(sections[5])?,
            surf_pool: text(sections[6])?,
            pos_defs,
            conn,
            conn_n,
            pos_ids,
        })
    }

    /// バイナリ辞書にシリアライズする
    pub fn to_bytes(&self) -> Vec<u8> {
        let sections: [Vec<u8>; 10] = [
            self.trie_bytes.clone(),
            u32s_to_bytes(&self.group_offsets),
            u16s_to_bytes(&self.lids),
            u16s_to_bytes(&self.rids),
            u16s_to_bytes(&self.costs),
            u32s_to_bytes(&self.surf_offsets),
            self.surf_pool.clone().into_bytes(),
            self.pos_defs.join("\n").into_bytes(),
            u16s_to_bytes(&self.conn),
            u16s_to_bytes(&self.pos_ids),
        ];
        let mut out = Vec::with_capacity(sections.iter().map(|s| s.len() + 8).sum::<usize>() + 12);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        for s in &sections {
            out.extend_from_slice(&(s.len() as u64).to_le_bytes());
            out.extend_from_slice(s);
        }
        out
    }

    /// 語数
    pub fn entry_count(&self) -> usize {
        self.lids.len()
    }

    /// 読みの種類数（trie のキー数）
    pub fn reading_count(&self) -> usize {
        self.group_offsets.len().saturating_sub(1)
    }

    /// 品詞 ID の数（連接コスト表の一辺）
    pub fn pos_id_count(&self) -> usize {
        self.conn_n
    }

    pub(crate) fn entry(&self, index: usize) -> Entry {
        let a = self.surf_offsets[index] as usize;
        let b = self.surf_offsets[index + 1] as usize;
        Entry {
            surface: self.surf_pool[a..b].to_string(),
            lid: self.lids[index],
            rid: self.rids[index],
            pos_id: self.pos_ids[index],
            cost: self.costs[index],
        }
    }

    pub(crate) fn surface(&self, index: usize) -> &str {
        let a = self.surf_offsets[index] as usize;
        let b = self.surf_offsets[index + 1] as usize;
        &self.surf_pool[a..b]
    }

    pub(crate) fn group_range(&self, group: u32) -> std::ops::Range<usize> {
        self.group_offsets[group as usize] as usize..self.group_offsets[group as usize + 1] as usize
    }

    pub(crate) fn lid_of(&self, index: usize) -> u16 {
        self.lids[index]
    }

    pub(crate) fn rid_of(&self, index: usize) -> u16 {
        self.rids[index]
    }

    pub(crate) fn cost_of(&self, index: usize) -> u16 {
        self.costs[index]
    }

    pub(crate) fn pos_id_of(&self, index: usize) -> u16 {
        self.pos_ids[index]
    }

    /// 連接コスト（前の語の右 ID, 次の語の左 ID）
    pub(crate) fn connection(&self, rid: u16, lid: u16) -> i32 {
        i32::from(self.conn[rid as usize * self.conn_n + lid as usize])
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
