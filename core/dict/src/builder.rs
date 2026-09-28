//! 辞書の構築。Mozc OSS 辞書（`dictionaryNN.txt` / `id.def` / `connection_single_column.txt`）の
//! パーサと、メモリ上で語を積んでバイナリ相当の [`Dictionary`] を作るビルダ。

use crate::dictionary::{Dictionary, RawParts};
use crate::error::DictError;
use fst::MapBuilder;
use std::collections::HashMap;
use std::path::Path;

/// 連接コストのソース側の並び（`connection_single_column.txt` の行の並び順）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionLayout {
    /// 行 index = `右ID(前の語) * N + 左ID(次の語)`
    RidMajor,
    /// 行 index = `左ID(次の語) * N + 右ID(前の語)`
    LidMajor,
}

struct RawEntry {
    reading: String,
    surface: String,
    lid: u16,
    rid: u16,
    pos_id: u16,
    cost: u16,
}

/// 辞書ビルダ
#[derive(Default)]
pub struct DictionaryBuilder {
    entries: Vec<RawEntry>,
    pos_defs: Vec<String>,
    conn: Vec<u16>,
    conn_n: usize,
}

impl DictionaryBuilder {
    /// 空のビルダを作る
    pub fn new() -> Self {
        Self::default()
    }

    /// 語を 1 つ追加する
    pub fn add_entry(
        &mut self,
        reading: &str,
        surface: &str,
        lid: u16,
        rid: u16,
        pos_id: u16,
        cost: u16,
    ) {
        self.entries.push(RawEntry {
            reading: reading.to_string(),
            surface: surface.to_string(),
            lid,
            rid,
            pos_id,
            cost,
        });
    }

    /// Mozc の辞書テキスト（1 行 `読み\t左ID\t右ID\tコスト\t表記`）を追加する。
    ///
    /// 6 列目以降を持つ行（`SPELLING_CORRECTION` = 綴り誤りの救済エントリ）は通常の候補に
    /// したくないので読み飛ばす。空行は無視する。`name` はエラー表示用のファイル名。
    pub fn add_dictionary_text(&mut self, name: &str, text: &str) -> Result<(), DictError> {
        for (i, line) in text.split('\n').enumerate() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() > 5 {
                continue;
            }
            if cols.len() < 5 {
                return Err(DictError::parse(name, i + 1, "列が 5 つありません"));
            }
            let num = |s: &str, what: &str| -> Result<u16, DictError> {
                s.parse::<u16>().map_err(|_| {
                    DictError::parse(name, i + 1, format!("{what} が数値でありません: {s:?}"))
                })
            };
            let lid = num(cols[1], "左ID")?;
            self.add_entry(
                cols[0],
                cols[4],
                lid,
                num(cols[2], "右ID")?,
                lid,
                num(cols[3], "コスト")?,
            );
        }
        Ok(())
    }

    /// `id.def`（1 行 `ID 品詞文字列`。ID は 0 から連番）を設定する
    pub fn set_pos_defs_text(&mut self, text: &str) -> Result<(), DictError> {
        let mut defs = Vec::new();
        for (i, line) in text.split('\n').enumerate() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let (id, def) = line.split_once(' ').ok_or_else(|| {
                DictError::parse("id.def", i + 1, "`ID 品詞` の形式ではありません")
            })?;
            let id: usize = id.parse().map_err(|_| {
                DictError::parse("id.def", i + 1, format!("ID が数値でありません: {id:?}"))
            })?;
            if id != defs.len() {
                return Err(DictError::parse(
                    "id.def",
                    i + 1,
                    format!("ID が連番ではありません（期待 {}、実際 {id}）", defs.len()),
                ));
            }
            defs.push(def.to_string());
        }
        self.pos_defs = defs;
        Ok(())
    }

    /// `connection_single_column.txt`（1 行目に ID 数 N、続く N×N 行に 1 行 1 コスト）を設定する
    pub fn set_connection_text(
        &mut self,
        text: &str,
        layout: ConnectionLayout,
    ) -> Result<(), DictError> {
        let mut lines = text.split('\n').map(|l| l.trim_end_matches('\r'));
        let n: usize = lines
            .next()
            .and_then(|l| l.trim().parse().ok())
            .ok_or_else(|| DictError::parse("connection", 1, "先頭行が ID 数ではありません"))?;
        let mut source = Vec::with_capacity(n * n);
        for (i, line) in lines.enumerate() {
            if line.is_empty() {
                continue;
            }
            let v: i32 = line.trim().parse().map_err(|_| {
                DictError::parse(
                    "connection",
                    i + 2,
                    format!("コストが数値でありません: {line:?}"),
                )
            })?;
            source.push(v.clamp(0, i32::from(u16::MAX)) as u16);
        }
        if source.len() != n * n {
            return Err(DictError::Build(format!(
                "連接コストの件数が N×N と一致しません（N={n}、実際 {}）",
                source.len()
            )));
        }
        self.conn = match layout {
            ConnectionLayout::RidMajor => source,
            ConnectionLayout::LidMajor => {
                let mut t = vec![0u16; n * n];
                for lid in 0..n {
                    for rid in 0..n {
                        t[rid * n + lid] = source[lid * n + rid];
                    }
                }
                t
            }
        };
        self.conn_n = n;
        Ok(())
    }

    /// 連接コストを直接設定する（`conn[rid_prev * n + lid_next]`）
    pub fn set_connection(&mut self, n: usize, costs: Vec<u16>) {
        self.conn_n = n;
        self.conn = costs;
    }

    /// 品詞定義を直接設定する（行番号 = 品詞 ID）
    pub fn set_pos_defs(&mut self, defs: Vec<String>) {
        self.pos_defs = defs;
    }

    /// 辞書を構築する。同じ (読み, 表記, 左ID, 右ID) は最小コストだけ残す
    pub fn build(mut self) -> Result<Dictionary, DictError> {
        if self.pos_defs.is_empty() || self.conn_n == 0 {
            return Err(DictError::Build("品詞定義と連接コストが未設定です".into()));
        }
        // 読み → コスト → 表記 の順に整列して重複を除く
        self.entries.sort_by(|a, b| {
            a.reading
                .as_bytes()
                .cmp(b.reading.as_bytes())
                .then(a.cost.cmp(&b.cost))
                .then_with(|| a.surface.cmp(&b.surface))
                .then(a.lid.cmp(&b.lid))
                .then(a.rid.cmp(&b.rid))
        });
        let mut seen: HashMap<(&str, u16, u16), ()> = HashMap::new();
        let mut keep = vec![true; self.entries.len()];
        let mut current: Option<&str> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if current != Some(e.reading.as_str()) {
                seen.clear();
                current = Some(e.reading.as_str());
            }
            if seen
                .insert((e.surface.as_str(), e.lid, e.rid), ())
                .is_some()
            {
                keep[i] = false;
            }
        }
        let mut keep_iter = keep.into_iter();
        self.entries.retain(|_| keep_iter.next().unwrap_or(false));

        let n_entries = self.entries.len();
        if n_entries > u32::MAX as usize / 2 {
            return Err(DictError::Build("語数が多すぎます".into()));
        }
        let mut group_offsets: Vec<u32> = Vec::new();
        let mut keys: Vec<(&[u8], u32)> = Vec::new();
        let mut lids = Vec::with_capacity(n_entries);
        let mut rids = Vec::with_capacity(n_entries);
        let mut costs = Vec::with_capacity(n_entries);
        let mut pos_ids = Vec::with_capacity(n_entries);
        let mut surf_offsets: Vec<u32> = Vec::with_capacity(n_entries + 1);
        let mut surf_pool = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            if i == 0 || self.entries[i - 1].reading != e.reading {
                keys.push((e.reading.as_bytes(), group_offsets.len() as u32));
                group_offsets.push(i as u32);
            }
            lids.push(e.lid);
            rids.push(e.rid);
            costs.push(e.cost);
            pos_ids.push(e.pos_id);
            surf_offsets.push(surf_pool.len() as u32);
            surf_pool.push_str(&e.surface);
        }
        group_offsets.push(n_entries as u32);
        surf_offsets.push(surf_pool.len() as u32);
        let mut trie_builder = MapBuilder::memory();
        for (key, value) in &keys {
            trie_builder
                .insert(key, u64::from(*value))
                .map_err(|e| DictError::Build(format!("trie 構築に失敗: {e}")))?;
        }
        let trie_bytes = trie_builder
            .into_inner()
            .map_err(|e| DictError::Build(format!("trie 構築に失敗: {e}")))?;
        Dictionary::from_parts(RawParts {
            trie_bytes,
            group_offsets,
            lids,
            rids,
            costs,
            surf_offsets,
            surf_pool,
            pos_defs: self.pos_defs,
            conn: self.conn,
            conn_n: self.conn_n,
            pos_ids,
        })
    }
}

/// Mozc OSS 辞書のディレクトリ（`dictionary00.txt`〜、`id.def`、`connection_single_column.txt`）から辞書を構築する
pub fn build_from_mozc_dir(dir: &Path, layout: ConnectionLayout) -> Result<Dictionary, DictError> {
    let mut builder = DictionaryBuilder::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("dictionary") && n.ends_with(".txt"))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(DictError::Build(format!(
            "{} に dictionary*.txt がありません",
            dir.display()
        )));
    }
    for file in files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("dictionary")
            .to_string();
        builder.add_dictionary_text(&name, &std::fs::read_to_string(&file)?)?;
    }
    builder.set_pos_defs_text(&std::fs::read_to_string(dir.join("id.def"))?)?;
    builder.set_connection_text(
        &std::fs::read_to_string(dir.join("connection_single_column.txt"))?,
        layout,
    )?;
    builder.build()
}
