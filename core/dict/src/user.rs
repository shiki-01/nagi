//! ユーザー辞書（Mozc TSV 互換）とスニペット。
//!
//! 形式（1 行 `読み<TAB>単語<TAB>品詞[<TAB>コメント]`）は Mozc / Google 日本語入力 / MS-IME の
//! エクスポートと共通。インポート・エクスポートは Mozc TSV を正とする（設計書 §4.13）。
//! 品詞が `スニペット` の行は、読み → 定型文（複数行可。`\n` `\t` `\\` をエスケープとして解釈）。

use std::path::Path;

/// スニペットを表す特殊な品詞名
pub const SNIPPET_POS: &str = "スニペット";

/// ユーザー辞書の 1 行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserEntry {
    /// 読み（ひらがな。カタカナで書かれていればひらがなに直す）
    pub reading: String,
    /// 単語（スニペットでは展開後の定型文）
    pub surface: String,
    /// 品詞名（Mozc のユーザー辞書の品詞名。`スニペット` は定型文）
    pub pos: String,
    /// コメント
    pub comment: String,
}

impl UserEntry {
    /// スニペット（読み → 定型文）か
    pub fn is_snippet(&self) -> bool {
        self.pos == SNIPPET_POS
    }
}

/// 読み込み時の問題（その行は無視される）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDictIssue {
    /// ファイル名
    pub file: String,
    /// 1 始まりの行番号
    pub line: usize,
    /// 内容
    pub message: String,
}

/// ユーザー辞書
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserDictionary {
    /// 全エントリ（読み込み順）
    pub entries: Vec<UserEntry>,
}

/// ひらがな化（カタカナ → ひらがな。長音などはそのまま）
pub(crate) fn to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{30a1}'..='\u{30f6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

/// バイト列を文字列にする。UTF-8（BOM 付き可）、UTF-16 LE/BE（BOM 付き。MS-IME のエクスポート）に対応
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

impl UserDictionary {
    /// Mozc TSV のテキストを解析して追加する。不正な行は `issues` に記録して読み飛ばす
    pub fn add_tsv(&mut self, file: &str, text: &str, issues: &mut Vec<UserDictIssue>) {
        for (i, raw) in text.split('\n').enumerate() {
            let line = raw.trim_end_matches('\r');
            // 空行・コメント（`#`、MS-IME のヘッダ `!`）
            if line.trim().is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let mut problem = |message: &str| {
                issues.push(UserDictIssue {
                    file: file.to_string(),
                    line: i + 1,
                    message: message.to_string(),
                });
            };
            if cols.len() < 3 {
                problem("列が 3 つ未満です（読み・単語・品詞が必要です）");
                continue;
            }
            let reading = to_hiragana(cols[0].trim());
            let pos = cols[2].trim().to_string();
            let surface = if pos == SNIPPET_POS {
                unescape(cols[1])
            } else {
                cols[1].trim().to_string()
            };
            if reading.is_empty() || surface.is_empty() {
                problem("読みまたは単語が空です");
                continue;
            }
            self.entries.push(UserEntry {
                reading,
                surface,
                pos,
                comment: cols
                    .get(3)
                    .map(|c| c.trim().to_string())
                    .unwrap_or_default(),
            });
        }
    }

    /// ディレクトリ内の `*.txt` / `*.tsv` をファイル名順にすべて読み込む。ディレクトリが無ければ空
    pub fn load_dir(dir: &Path) -> (UserDictionary, Vec<UserDictIssue>) {
        let mut dict = UserDictionary::default();
        let mut issues = Vec::new();
        let Ok(read) = std::fs::read_dir(dir) else {
            return (dict, issues);
        };
        let mut files: Vec<_> = read
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("tsv")
                    })
            })
            .collect();
        files.sort();
        for path in files {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            match std::fs::read(&path) {
                Ok(bytes) => dict.add_tsv(&name, &decode_text(&bytes), &mut issues),
                Err(e) => issues.push(UserDictIssue {
                    file: name,
                    line: 0,
                    message: format!("読み込めません: {e}"),
                }),
            }
        }
        (dict, issues)
    }

    /// Mozc TSV としてエクスポートする（スニペットの改行・タブ・`\` はエスケープする）
    pub fn to_mozc_tsv(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            let surface = if e.is_snippet() {
                escape(&e.surface)
            } else {
                e.surface.clone()
            };
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                e.reading, surface, e.pos, e.comment
            ));
        }
        out
    }

    /// 語（スニペット以外）の数
    pub fn word_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_snippet()).count()
    }

    /// スニペットの数
    pub fn snippet_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_snippet()).count()
    }
}
