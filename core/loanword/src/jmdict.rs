//! JMdict（EDRDG）の XML から、外来語の原語（`lsource`）を取り出す。
//!
//! 使う構造（`JMdict_e` で確認した形。`docs/decisions.md` H-52）:
//!
//! ```xml
//! <entry>
//!   <k_ele><keb>…</keb><ke_pri>news1</ke_pri></k_ele>
//!   <r_ele><reb>アルバイト</reb><re_pri>ichi1</re_pri></r_ele>
//!   <sense><lsource xml:lang="ger">Arbeit</lsource>…</sense>
//!   <sense><lsource xml:lang="eng" ls_wasei="y">salaried man</lsource></sense>
//! </entry>
//! ```
//!
//! `xml:lang` は省略時 `eng`、`ls_type="part"` は一部だけの借用、`ls_wasei="y"` は和製英語。
//! 綴りが空の `lsource` は捨てる。XML パーサは使わず、この構造に必要な分だけ文字列で走査する
//! （DTD の実体宣言は読み飛ばす。要素の中身に出る実体は標準の 5 つだけ復号する）。

use crate::index::Entry;
use crate::normalize::is_katakana_word;

/// 綴りの最大文字数（これを超えるものは句や文なので候補にしない）
const MAX_SPELLING_CHARS: usize = 48;

/// `xml` から索引の入力を取り出す。見出し（`keb` / `reb`）がカタカナだけの語が対象
pub fn extract(xml: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<entry>") {
        let after = &rest[start + "<entry>".len()..];
        let Some(end) = after.find("</entry>") else {
            break;
        };
        entry_to_records(&after[..end], &mut out);
        rest = &after[end + "</entry>".len()..];
    }
    out
}

/// `<tag …>中身</tag>` の中身を順に返す（属性は無視）
fn elements<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    elements_with_attrs(xml, tag)
        .into_iter()
        .map(|(_, body)| body)
        .collect()
}

/// `<tag 属性>中身</tag>` の (属性文字列, 中身) を順に返す。`<tag/>`（中身なし）は中身を空にする
fn elements_with_attrs<'a>(xml: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        // `<lsource` と `<lsource_x` のような別名を取り違えない
        let boundary = after.chars().next();
        if !matches!(
            boundary,
            Some('>') | Some(' ') | Some('/') | Some('\n') | Some('\t')
        ) {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else {
            break;
        };
        let attrs = &after[..gt];
        if let Some(stripped) = attrs.strip_suffix('/') {
            out.push((stripped, ""));
            rest = &after[gt + 1..];
            continue;
        }
        let body_start = &after[gt + 1..];
        let Some(end) = body_start.find(&close) else {
            break;
        };
        out.push((attrs, &body_start[..end]));
        rest = &body_start[end + close.len()..];
    }
    out
}

/// 属性文字列から `name="value"` の value を取り出す
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let i = attrs.find(&needle)?;
    // 属性名の直前は空白（`xml:lang` を `lang` で取り違えない）
    if i > 0 && !attrs[..i].ends_with(char::is_whitespace) {
        return None;
    }
    let v = &attrs[i + needle.len()..];
    Some(&v[..v.find('"')?])
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 優先度タグ（`news1` `ichi1` `spec1` `gai1` `nf01` …）から順位（0 が最も一般的、3 が無印）
fn priority_rank(tags: &[&str]) -> u8 {
    let mut best = 3u8;
    for t in tags {
        let r = if t.ends_with('1') && !t.starts_with("nf") {
            0
        } else if t.ends_with('2') && !t.starts_with("nf") {
            1
        } else if let Some(n) = t.strip_prefix("nf").and_then(|n| n.parse::<u32>().ok()) {
            if n <= 12 {
                0
            } else if n <= 24 {
                1
            } else {
                2
            }
        } else {
            2
        };
        best = best.min(r);
    }
    best
}

fn entry_to_records(entry: &str, out: &mut Vec<Entry>) {
    let mut headwords: Vec<String> = Vec::new();
    for tag in ["keb", "reb"] {
        for h in elements(entry, tag) {
            let h = unescape(h.trim());
            if is_katakana_word(&h) && !headwords.contains(&h) {
                headwords.push(h);
            }
        }
    }
    if headwords.is_empty() {
        return;
    }
    let mut pri: Vec<&str> = Vec::new();
    for tag in ["ke_pri", "re_pri"] {
        pri.extend(elements(entry, tag).into_iter().map(str::trim));
    }
    let pri_rank = priority_rank(&pri);

    for sense in elements(entry, "sense") {
        let mut sources: Vec<(String, String, bool, bool)> = Vec::new(); // lang, spelling, wasei, part
        for (attrs, body) in elements_with_attrs(sense, "lsource") {
            let spelling = unescape(body.trim());
            if spelling.is_empty()
                || spelling.chars().count() > MAX_SPELLING_CHARS
                || spelling.chars().any(is_kana)
            {
                continue;
            }
            sources.push((
                attr(attrs, "xml:lang").unwrap_or("eng").to_string(),
                spelling,
                attr(attrs, "ls_wasei") == Some("y"),
                attr(attrs, "ls_type") == Some("part"),
            ));
        }
        // 同じ言語の綴りが 1 つの語義に複数あるものは複合語（hot dog → hot + dog）なので捨てる
        // （言語が違うものは、同じ語の別の言語での綴り。espada: por / spa）
        let multi: Vec<String> = sources
            .iter()
            .filter(|s| sources.iter().filter(|t| t.0 == s.0).count() > 1)
            .map(|s| s.0.clone())
            .collect();
        for (lang, spelling, wasei, part) in sources {
            if multi.contains(&lang) {
                continue;
            }
            let rank = pri_rank * 2 + u8::from(part);
            for h in &headwords {
                out.push(Entry {
                    headword: h.clone(),
                    lang: lang.clone(),
                    spelling: spelling.clone(),
                    wasei,
                    rank,
                });
            }
        }
    }
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE JMdict [
<!ENTITY n "noun (common) (futsuumeishi)">
]>
<JMdict>
<entry>
<ent_seq>1</ent_seq>
<r_ele><reb>コンピューター</reb><re_pri>ichi1</re_pri><re_pri>news1</re_pri></r_ele>
<r_ele><reb>コンピュータ</reb></r_ele>
<sense><pos>&n;</pos><lsource xml:lang="eng">computer</lsource><gloss>computer</gloss></sense>
</entry>
<entry>
<ent_seq>2</ent_seq>
<k_ele><keb>アルバイト</keb></k_ele>
<r_ele><reb>アルバイト</reb><re_pri>ichi1</re_pri></r_ele>
<sense><lsource xml:lang="ger">Arbeit</lsource></sense>
</entry>
<entry>
<ent_seq>3</ent_seq>
<r_ele><reb>サラリーマン</reb></r_ele>
<sense><lsource ls_wasei="y" xml:lang="eng">salaried man</lsource></sense>
</entry>
<entry>
<ent_seq>4</ent_seq>
<r_ele><reb>ホットドッグ</reb></r_ele>
<sense><lsource xml:lang="eng" ls_type="part">hot</lsource><lsource xml:lang="eng" ls_type="part">dog</lsource></sense>
</entry>
<entry>
<ent_seq>5</ent_seq>
<r_ele><reb>エスパダ</reb></r_ele>
<sense><lsource xml:lang="por">espada</lsource><lsource xml:lang="spa">espada</lsource></sense>
</entry>
<entry>
<ent_seq>6</ent_seq>
<r_ele><reb>テレビ</reb><re_pri>ichi1</re_pri></r_ele>
<sense><lsource ls_type="part">television</lsource><lsource/></sense>
</entry>
<entry>
<ent_seq>7</ent_seq>
<k_ele><keb>猫</keb></k_ele>
<r_ele><reb>ねこ</reb></r_ele>
<sense><lsource xml:lang="eng">not-a-loan</lsource></sense>
</entry>
<entry>
<ent_seq>8</ent_seq>
<r_ele><reb>ランドセル</reb></r_ele>
<sense><lsource xml:lang="dut"/></sense>
</entry>
<entry>
<ent_seq>9</ent_seq>
<r_ele><reb>ロックアンドロール</reb></r_ele>
<sense><lsource xml:lang="eng">rock &amp; roll</lsource></sense>
</entry>
</JMdict>
"#;

    fn find<'a>(v: &'a [Entry], head: &str) -> Vec<&'a Entry> {
        v.iter().filter(|e| e.headword == head).collect()
    }

    #[test]
    fn extracts_language_spelling_wasei_and_rank() {
        let v = extract(FIXTURE);
        let c = find(&v, "コンピューター");
        assert_eq!(c.len(), 1);
        assert_eq!(
            (c[0].lang.as_str(), c[0].spelling.as_str()),
            ("eng", "computer")
        );
        assert_eq!(c[0].rank, 0);
        assert!(!c[0].wasei);
        // 見出しが複数あれば、どれからも引ける
        assert_eq!(find(&v, "コンピュータ").len(), 1);
        // keb と reb が同じ綴りでも重複しない
        assert_eq!(find(&v, "アルバイト").len(), 1);
        assert_eq!(find(&v, "アルバイト")[0].lang, "ger");
        assert!(find(&v, "サラリーマン")[0].wasei);
        assert_eq!(find(&v, "エスパダ").len(), 2);
        // 省略時は eng、part は順位が 1 つ下
        let t = find(&v, "テレビ");
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].lang.as_str(), t[0].rank), ("eng", 1));
        assert_eq!(find(&v, "ロックアンドロール")[0].spelling, "rock & roll");
    }

    #[test]
    fn skips_compounds_empty_sources_and_non_katakana_headwords() {
        let v = extract(FIXTURE);
        assert!(find(&v, "ホットドッグ").is_empty());
        assert!(find(&v, "ランドセル").is_empty());
        assert!(v.iter().all(|e| e.headword != "猫" && e.headword != "ねこ"));
    }

    #[test]
    fn attr_does_not_confuse_names() {
        assert_eq!(
            attr(r#" xml:lang="ger" ls_wasei="y""#, "xml:lang"),
            Some("ger")
        );
        assert_eq!(attr(r#" xml:lang="ger""#, "lang"), None);
        assert_eq!(attr(r#" ls_wasei="y""#, "ls_wasei"), Some("y"));
    }

    #[test]
    fn priority_ranks() {
        assert_eq!(priority_rank(&["news1"]), 0);
        assert_eq!(priority_rank(&["news2", "nf30"]), 1);
        assert_eq!(priority_rank(&["nf30"]), 2);
        assert_eq!(priority_rank(&[]), 3);
    }
}
