//! 他の IME のユーザー辞書の取り込み（Mozc TSV への変換）。
//!
//! 取り込める形式は、各 IME の公開されているエクスポート形式:
//!
//! - Mozc / Google 日本語入力: TSV（`読み<TAB>単語<TAB>品詞[<TAB>コメント]`）
//! - Microsoft IME: テキスト（UTF-16。先頭に `!Microsoft IME Dictionary Tool`）
//! - ATOK: テキスト（先頭に `!!ATOK_TANGO_TEXT_HEADER_1`。UTF-16 / UTF-8 / Shift_JIS）
//! - macOS 標準のユーザ辞書: XML プロパティリスト（`phrase` と `shortcut` を持つ辞書の配列）
//! - 単純な `読み<TAB>単語` の 2 列テキスト
//!
//! 形式はファイルの先頭の行と構造から自動判定する（ヒントで上書き可）。結果は [`UserEntry`] の列で、
//! 呼び出し側が Mozc TSV として書き出す（正の形式は変えない）。重複は既存の登録を優先する。

use crate::user::{SNIPPET_POS, UserDictionary, UserEntry, decode_text, to_hiragana, unescape};
use std::collections::HashSet;

/// 取り込みの入力形式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// Mozc / Google 日本語入力の TSV
    MozcTsv,
    /// Microsoft IME のテキスト
    MsIme,
    /// ATOK のテキスト
    Atok,
    /// macOS 標準のユーザ辞書（XML plist）
    MacPlist,
    /// `読み<TAB>単語` の 2 列テキスト
    TwoColumn,
}

/// 取り込めなかった理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// 列が足りない（読みと単語が必要）
    MissingColumns,
    /// 読みが空、または読みに使えない文字（漢字など）を含む
    InvalidReading,
    /// 単語が空
    EmptySurface,
    /// 同じ読み・単語がファイル内に既にある
    DuplicateInFile,
    /// 同じ読み・単語が既に登録されている（既存を優先）
    DuplicateExisting,
}

impl SkipReason {
    /// 機械可読な識別子
    pub fn code(self) -> &'static str {
        match self {
            SkipReason::MissingColumns => "missing_columns",
            SkipReason::InvalidReading => "invalid_reading",
            SkipReason::EmptySurface => "empty_surface",
            SkipReason::DuplicateInFile => "duplicate_in_file",
            SkipReason::DuplicateExisting => "duplicate_existing",
        }
    }

    /// 表示用の説明（日本語）
    pub fn message(self) -> &'static str {
        match self {
            SkipReason::MissingColumns => "列が足りません（読みと単語が必要です）",
            SkipReason::InvalidReading => {
                "読みが不正です（空、または読みに使えない文字を含みます）"
            }
            SkipReason::EmptySurface => "単語が空です",
            SkipReason::DuplicateInFile => "ファイル内で重複しています",
            SkipReason::DuplicateExisting => "既に登録されています（既存を優先）",
        }
    }
}

/// スキップした 1 件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportSkip {
    /// 1 始まりの行番号（plist では辞書の通し番号）
    pub line: usize,
    /// 理由
    pub reason: SkipReason,
}

/// 取り込み結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    /// 判定（または指定）された形式
    pub format: ImportFormat,
    /// 読み取った項目の総数（取り込んだ数 + スキップした数）
    pub total: usize,
    /// 新たに追加する項目（Mozc TSV の行にできる形。読みはひらがな、品詞は Mozc の品詞名）
    pub entries: Vec<UserEntry>,
    /// スキップした項目
    pub skipped: Vec<ImportSkip>,
}

/// 取り込み全体の失敗
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// 項目が 1 つも無い
    #[error("取り込める項目がありません")]
    Empty,
    /// 形式を判定できない
    #[error("形式を判定できません")]
    Unrecognized,
    /// バイナリ形式の plist
    #[error("バイナリ形式の plist には対応していません（XML 形式で書き出してください）")]
    BinaryPlist,
    /// plist の構造が不正
    #[error("plist として読めません")]
    BrokenPlist,
}

/// バイト列を文字列にする。BOM つきの UTF-8 / UTF-16、BOM なしの UTF-16（NUL の並びで判定）、
/// UTF-8、それ以外は Shift_JIS（ATOK / Windows の旧来の書き出し）として読む
pub fn decode_import(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return decode_text(bytes);
    }
    let head = &bytes[..bytes.len().min(512)];
    let even_nul = head.iter().step_by(2).filter(|&&b| b == 0).count();
    let odd_nul = head.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
    // BOM なしの UTF-16 は、ASCII（改行・タブ）の上位バイトの NUL が片側の位置にだけ並ぶ
    if odd_nul >= 2 && even_nul == 0 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if even_nul >= 2 && odd_nul == 0 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS
            .decode_without_bom_handling(body)
            .0
            .into_owned(),
    }
}

/// 形式を判定する（ファイルの先頭の行と構造から）
fn detect(text: &str) -> Option<ImportFormat> {
    let head: String = text.chars().take(1024).collect();
    let head_trim = head.trim_start();
    if head_trim.starts_with("<?xml")
        || head_trim.starts_with("<!DOCTYPE plist")
        || head.contains("<plist")
    {
        return Some(ImportFormat::MacPlist);
    }
    let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let lower = first.to_ascii_lowercase();
    if lower.starts_with("!microsoft ime") {
        return Some(ImportFormat::MsIme);
    }
    if lower.starts_with("!!atok") {
        return Some(ImportFormat::Atok);
    }
    let (mut three, mut two) = (0usize, 0usize);
    for line in data_lines(text) {
        match line.1.split('\t').count() {
            0 | 1 => {}
            2 => two += 1,
            _ => three += 1,
        }
    }
    if three > 0 && three >= two {
        Some(ImportFormat::MozcTsv)
    } else if two + three > 0 {
        Some(ImportFormat::TwoColumn)
    } else {
        None
    }
}

/// 空行・コメント（`#`、`!` で始まる行。ヘッダー行を含む）を除いた (行番号, 行)
fn data_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split('\n').enumerate().filter_map(|(i, raw)| {
        let line = raw.trim_end_matches('\r');
        let line = if i == 0 {
            line.trim_start_matches('\u{feff}')
        } else {
            line
        };
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with('!') {
            None
        } else {
            Some((i + 1, line))
        }
    })
}

/// 読みとして使える文字か（ひらがな・長音・中黒・ASCII の英数字とハイフン）
fn valid_reading(reading: &str) -> bool {
    !reading.is_empty()
        && reading.chars().all(|c| {
            matches!(c, '\u{3041}'..='\u{3096}' | '\u{309b}'..='\u{309f}')
                || c == 'ー'
                || c == '・'
                || c.is_ascii_alphanumeric()
                || c == '-'
        })
}

/// Mozc のユーザー辞書の品詞名（`dict/src/overlay.rs` が対応づけるもの）
const MOZC_POS: &[&str] = &[
    "名詞",
    "固有名詞",
    "人名",
    "姓",
    "名",
    "地名",
    "組織",
    "名詞サ変",
    "副詞",
    "接続詞",
    "感動詞",
    "連体詞",
    "形容詞",
    "顔文字",
    "記号",
];

/// 他 IME の品詞名を Mozc の品詞名に写す。写せないものは `名詞`
fn map_pos(raw: &str, allow_snippet: bool) -> String {
    let p = raw.trim();
    if allow_snippet && p == SNIPPET_POS {
        return SNIPPET_POS.to_string();
    }
    if MOZC_POS.contains(&p) {
        return p.to_string();
    }
    let has = |s: &str| p.contains(s);
    if has("接尾") || has("接頭") || has("形容動詞") || has("名詞形動") {
        return "名詞".to_string();
    }
    if has("形容詞") {
        return "形容詞".to_string();
    }
    if has("動詞") || has("五段") || has("一段") || has("変格") {
        return map_verb(p);
    }
    if has("人名") {
        return "人名".to_string();
    }
    if p == "姓" {
        return "姓".to_string();
    }
    if has("地名") || has("地域") {
        return "地名".to_string();
    }
    if has("組織") || has("団体") {
        return "組織".to_string();
    }
    if has("固有") {
        return "固有名詞".to_string();
    }
    for pos in ["副詞", "接続詞", "感動詞", "連体詞", "顔文字", "記号"] {
        if has(pos) {
            return pos.to_string();
        }
    }
    if has("サ変") {
        return "名詞サ変".to_string();
    }
    "名詞".to_string()
}

/// 動詞の品詞名。活用の種類が分かれば Mozc の `動詞カ行五段` などにし、分からなければ `動詞`
fn map_verb(p: &str) -> String {
    if let Some(i) = p.find('行') {
        let row = p[..i].chars().next_back();
        let rest = &p[i + '行'.len_utf8()..];
        if let Some(row) = row
            && "ワカサタナバマラガハザダアヤ".contains(row)
        {
            for kind in ["五段", "四段", "変格"] {
                if rest.starts_with(kind) {
                    return format!("動詞{row}行{kind}");
                }
            }
        }
    }
    if p.contains("一段") {
        return "動詞一段".to_string();
    }
    if p.contains("カ変") {
        return "動詞カ行変格".to_string();
    }
    if p.contains("サ変") {
        return "動詞サ行変格".to_string();
    }
    "動詞".to_string()
}

/// 1 項目の候補（検証前）
struct Raw {
    line: usize,
    reading: String,
    surface: String,
    pos: String,
    comment: String,
}

fn parse_text(text: &str, format: ImportFormat) -> (Vec<Raw>, Vec<ImportSkip>) {
    let mut raws = Vec::new();
    let mut skipped = Vec::new();
    for (line_no, line) in data_lines(text) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            skipped.push(ImportSkip {
                line: line_no,
                reason: SkipReason::MissingColumns,
            });
            continue;
        }
        let pos = match format {
            ImportFormat::TwoColumn => "名詞".to_string(),
            _ => map_pos(
                cols.get(2).copied().unwrap_or(""),
                format == ImportFormat::MozcTsv,
            ),
        };
        let surface = if pos == SNIPPET_POS {
            unescape(cols[1])
        } else {
            cols[1].trim().to_string()
        };
        let comment = if format == ImportFormat::TwoColumn {
            String::new()
        } else {
            cols.get(3)
                .map(|c| c.trim().to_string())
                .unwrap_or_default()
        };
        raws.push(Raw {
            line: line_no,
            reading: to_hiragana(cols[0].trim()),
            surface,
            pos,
            comment,
        });
    }
    (raws, skipped)
}

// ---- plist（最小の XML 走査。phrase / shortcut を持つ辞書だけを拾う） ----

#[derive(Default)]
struct PlistFrame {
    phrase: Option<String>,
    shortcut: Option<String>,
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else { break };
        let ent = &rest[1..end];
        let decoded = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// plist から (通し番号, shortcut, phrase) を取り出す。`phrase` / `shortcut` のどちらも無い辞書は数えない
fn parse_plist(text: &str) -> Result<Vec<(usize, PlistFrame)>, ImportError> {
    let mut out = Vec::new();
    let mut stack: Vec<PlistFrame> = Vec::new();
    let mut index = 0usize;
    let mut pending_key: Option<String> = None;
    // 文字データを集めている要素（key / string）
    let mut capture: Option<&'static str> = None;
    let mut buf = String::new();
    let mut rest = text;
    let mut saw_plist = false;
    while let Some(lt) = rest.find('<') {
        if capture.is_some() {
            buf.push_str(&rest[..lt]);
        }
        rest = &rest[lt..];
        if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").ok_or(ImportError::BrokenPlist)?;
            if capture.is_some() {
                // CDATA はエスケープ済みの形で積む（あとでまとめて展開するため & を保護）
                buf.push_str(&body[..end].replace('&', "&amp;"));
            }
            rest = &body[end + 3..];
            continue;
        }
        if let Some(body) = rest.strip_prefix("<!--") {
            let end = body.find("-->").ok_or(ImportError::BrokenPlist)?;
            rest = &body[end + 3..];
            continue;
        }
        let gt = rest.find('>').ok_or(ImportError::BrokenPlist)?;
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        let name = tag
            .trim_start_matches('/')
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        if name == "plist" {
            saw_plist = true;
        }
        match (name, closing, self_closing) {
            ("dict", false, false) => stack.push(PlistFrame::default()),
            ("dict", true, _) => {
                index += 1;
                if let Some(frame) = stack.pop()
                    && (frame.phrase.is_some() || frame.shortcut.is_some())
                {
                    out.push((index, frame));
                }
                pending_key = None;
            }
            ("key", false, false) => {
                capture = Some("key");
                buf.clear();
            }
            ("key", true, _) => {
                pending_key = Some(xml_unescape(&buf));
                capture = None;
            }
            ("string", false, false) => {
                capture = Some("string");
                buf.clear();
            }
            ("string", false, true) | ("string", true, _) => {
                let value = if closing {
                    xml_unescape(&buf)
                } else {
                    String::new()
                };
                capture = None;
                if let (Some(key), Some(frame)) = (pending_key.take(), stack.last_mut()) {
                    match key.as_str() {
                        "phrase" => frame.phrase = Some(value),
                        "shortcut" => frame.shortcut = Some(value),
                        _ => {}
                    }
                }
            }
            ("array" | "plist" | "dict", _, _) => {}
            // integer / true / false / data / date など、値の要素が来たらキーを使い切る
            (_, false, _) => pending_key = None,
            _ => {}
        }
    }
    if !saw_plist && out.is_empty() {
        return Err(ImportError::BrokenPlist);
    }
    Ok(out)
}

/// 他 IME のユーザー辞書の内容（ファイルのバイト列）を、Mozc TSV の行にできる項目へ変換する。
/// `hint` があれば自動判定より優先する。`existing`（登録済みの辞書）と重複する項目は取り込まない
pub fn import_user_dict(
    bytes: &[u8],
    hint: Option<ImportFormat>,
    existing: &UserDictionary,
) -> Result<ImportOutcome, ImportError> {
    if bytes.starts_with(b"bplist") {
        return Err(ImportError::BinaryPlist);
    }
    let text = decode_import(bytes);
    let format = match hint {
        Some(f) => f,
        None => detect(&text).ok_or_else(|| {
            if text.trim().is_empty() {
                ImportError::Empty
            } else {
                ImportError::Unrecognized
            }
        })?,
    };
    let (raws, mut skipped) = if format == ImportFormat::MacPlist {
        let frames = parse_plist(&text)?;
        let raws = frames
            .into_iter()
            .map(|(line, f)| Raw {
                line,
                reading: to_hiragana(f.shortcut.as_deref().unwrap_or("").trim()),
                surface: f.phrase.unwrap_or_default().trim().to_string(),
                pos: "名詞".to_string(),
                comment: String::new(),
            })
            .collect();
        (raws, Vec::new())
    } else {
        parse_text(&text, format)
    };

    let mut seen: HashSet<(String, String)> = existing
        .entries
        .iter()
        .map(|e| (e.reading.clone(), e.surface.clone()))
        .collect();
    let mut file_seen: HashSet<(String, String)> = HashSet::new();
    let mut entries = Vec::new();
    let total = raws.len() + skipped.len();
    for raw in raws {
        let reason = if !valid_reading(&raw.reading) {
            Some(SkipReason::InvalidReading)
        } else if raw.surface.is_empty() {
            Some(SkipReason::EmptySurface)
        } else {
            let key = (raw.reading.clone(), raw.surface.clone());
            if file_seen.contains(&key) {
                Some(SkipReason::DuplicateInFile)
            } else if seen.contains(&key) {
                Some(SkipReason::DuplicateExisting)
            } else {
                file_seen.insert(key.clone());
                seen.insert(key);
                None
            }
        };
        match reason {
            Some(reason) => skipped.push(ImportSkip {
                line: raw.line,
                reason,
            }),
            None => entries.push(UserEntry {
                reading: raw.reading,
                surface: raw.surface,
                pos: raw.pos,
                comment: raw.comment,
            }),
        }
    }
    skipped.sort_by_key(|s| s.line);
    if total == 0 {
        return Err(ImportError::Empty);
    }
    Ok(ImportOutcome {
        format,
        total,
        entries,
        skipped,
    })
}
