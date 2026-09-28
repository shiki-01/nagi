//! 辞書の構築。Mozc OSS 辞書（`dictionaryNN.txt` / `id.def` / `connection_single_column.txt`）の
//! パーサと、メモリ上で語を積んでバイナリ相当の [`Dictionary`] を作るビルダ。

use crate::dictionary::{Dictionary, RawParts};
use crate::error::DictError;
use crate::pos::sudachi_pos_to_mozc_like;
use crate::user::to_hiragana;
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
    /// `pos_defs` の逆引き（品詞文字列 → ID）。`intern_pos` で品詞文字列を重複なく登録するために使う
    pos_def_index: HashMap<String, u16>,
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

    /// 品詞文字列を `pos_defs` に登録し ID を返す。既に登録済みなら既存の ID を返す
    fn intern_pos(&mut self, def: String) -> u16 {
        if let Some(&id) = self.pos_def_index.get(&def) {
            return id;
        }
        let id = self.pos_defs.len() as u16;
        self.pos_def_index.insert(def.clone(), id);
        self.pos_defs.push(def);
        id
    }

    /// SudachiDict の語彙 CSV（ヘッダ行 + `IndexForm,LeftId,RightId,Cost,Headword,POS1..POS6,
    /// ReadingForm,NormalizedForm,DictionaryForm,SplitA,SplitB,WordStructure,SynonymGroups,
    /// ReferenceId` の 19 列。フィールド内のカンマは `,` のような Unicode エスケープ表記に
    /// なっているため素朴に `,` で分割してよい）を追加する。
    ///
    /// - LeftId/RightId が負（`-1`）の行は、複合語の構成要素専用で単独では格子に出現しない語
    ///   （SudachiDict の A/B unit 分割専用エントリ）なので読み飛ばす
    /// - Headword が空の行は IndexForm を表記として使う（SudachiDict の慣例: Headword 省略は
    ///   見出し語がインデックスと同じであることを意味する）
    /// - 読み（ReadingForm）はカタカナなのでひらがなに変換する
    ///
    /// ヘッダ行（1 行目）はスキップする。空行は無視する。列が 19 に満たない行はエラーとする。
    pub fn add_sudachi_csv(&mut self, name: &str, text: &str) -> Result<(), DictError> {
        for (i, line) in text.split('\n').enumerate() {
            if i == 0 {
                continue;
            }
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split(',').collect();
            if cols.len() < 19 {
                return Err(DictError::parse(
                    name,
                    i + 1,
                    format!("列が19個ありません（実際 {}）", cols.len()),
                ));
            }
            let parse_i32 = |s: &str, what: &str| -> Result<i32, DictError> {
                s.parse::<i32>().map_err(|_| {
                    DictError::parse(name, i + 1, format!("{what} が数値でありません: {s:?}"))
                })
            };
            let lid_i = parse_i32(cols[1], "LeftId")?;
            let rid_i = parse_i32(cols[2], "RightId")?;
            if lid_i < 0 || rid_i < 0 {
                continue;
            }
            let lid = u16::try_from(lid_i).map_err(|_| {
                DictError::parse(name, i + 1, format!("LeftId が範囲外です: {lid_i}"))
            })?;
            let rid = u16::try_from(rid_i).map_err(|_| {
                DictError::parse(name, i + 1, format!("RightId が範囲外です: {rid_i}"))
            })?;
            let cost_i = parse_i32(cols[3], "Cost")?;
            let cost = cost_i.clamp(0, i32::from(u16::MAX)) as u16;
            let surface = if cols[4].is_empty() { cols[0] } else { cols[4] };
            let reading = to_hiragana(cols[11]);
            let pos_def = sudachi_pos_to_mozc_like(cols[5], cols[6]);
            let pos_id = self.intern_pos(pos_def);
            self.add_entry(&reading, surface, lid, rid, pos_id, cost);
        }
        Ok(())
    }

    /// MeCab 形式の `matrix.def`（1 行目 `lid_size rid_size`、以降 `rid_prev lid_next cost` を
    /// 1 行ずつ、計 `lid_size × rid_size` 行）を設定する。
    ///
    /// 3000万行超の密行列を想定し、行ごとの `String` アロケーションを避けて `str::split` の
    /// イテレータのみで処理する。
    pub fn set_connection_matrix_def_text(&mut self, text: &str) -> Result<(), DictError> {
        let mut lines = text.split('\n');
        let header = lines
            .next()
            .ok_or_else(|| DictError::parse("matrix.def", 1, "ファイルが空です"))?
            .trim_end_matches('\r');
        let mut header_cols = header.split_ascii_whitespace();
        let lid_size: usize = header_cols
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                DictError::parse(
                    "matrix.def",
                    1,
                    "先頭行が \"lid_size rid_size\" ではありません",
                )
            })?;
        let rid_size: usize = header_cols
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                DictError::parse(
                    "matrix.def",
                    1,
                    "先頭行が \"lid_size rid_size\" ではありません",
                )
            })?;
        if lid_size != rid_size {
            return Err(DictError::Build(format!(
                "matrix.def の lid_size と rid_size が一致しません（{lid_size} != {rid_size}）"
            )));
        }
        let n = lid_size;
        let mut conn = vec![0u16; n * n];
        let mut count = 0usize;
        for (i, line) in lines.enumerate() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let mut cols = line.split_ascii_whitespace();
            let parse_usize = |cols: &mut std::str::SplitAsciiWhitespace| -> Option<usize> {
                cols.next()?.parse().ok()
            };
            let rid = parse_usize(&mut cols).ok_or_else(|| {
                DictError::parse("matrix.def", i + 2, format!("行の形式が不正です: {line:?}"))
            })?;
            let lid = parse_usize(&mut cols).ok_or_else(|| {
                DictError::parse("matrix.def", i + 2, format!("行の形式が不正です: {line:?}"))
            })?;
            let cost: i32 = cols.next().and_then(|s| s.parse().ok()).ok_or_else(|| {
                DictError::parse("matrix.def", i + 2, format!("行の形式が不正です: {line:?}"))
            })?;
            if rid >= n || lid >= n {
                return Err(DictError::parse(
                    "matrix.def",
                    i + 2,
                    format!("ID が範囲外です: rid={rid}, lid={lid}, n={n}"),
                ));
            }
            conn[rid * n + lid] = cost.clamp(0, i32::from(u16::MAX)) as u16;
            count += 1;
        }
        if count != n * n {
            return Err(DictError::Build(format!(
                "連接コストの件数が N×N と一致しません（N={n}、実際 {count}）"
            )));
        }
        self.conn = conn;
        self.conn_n = n;
        Ok(())
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

/// SudachiDict の語彙CSVファイル群と matrix.def から辞書を構築する
pub fn build_from_sudachi_files(
    lex_paths: &[std::path::PathBuf],
    matrix_def_path: &Path,
) -> Result<Dictionary, DictError> {
    let mut builder = DictionaryBuilder::new();
    for path in lex_paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("lex.csv")
            .to_string();
        builder.add_sudachi_csv(&name, &std::fs::read_to_string(path)?)?;
    }
    builder.set_connection_matrix_def_text(&std::fs::read_to_string(matrix_def_path)?)?;
    builder.build()
}
