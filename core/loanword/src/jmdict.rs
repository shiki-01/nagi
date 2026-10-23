//! JMdict（EDRDG）の JSON（jmdict-simplified の `jmdict-eng-*.json`）から、外来語の原語を取り出す。
//!
//! 使う構造（実物で確認。`docs/decisions.md` H-52）:
//!
//! ```json
//! {"version": "...", "words": [
//!   {"kana": [{"text": "アルバイト", "common": true}], "kanji": [{"text": "…", "common": false}],
//!    "sense": [{"languageSource": [{"lang": "ger", "full": true, "wasei": false, "text": "Arbeit"}],
//!               "gloss": [{"lang": "eng", "text": "part-time job"}]}]}]}
//! ```
//!
//! `words` だけを 1 語ずつ読み（118MB の JSON を丸ごとメモリに載せない）、見出し（kana / kanji の
//! `text`）がカタカナだけの語を対象にする。`languageSource.text` が null（綴りなし）の語義と、
//! `languageSource` が空の語義は、gloss が 1 語の英単語で音が近いときだけ「推定の原語」（低順位）にする。

use crate::index::Entry;
use crate::normalize::is_katakana_word;
use crate::phonetic::gloss_matches;
use serde::Deserialize;
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fmt;
use std::io::Read;

/// 綴りの最大文字数（これを超えるものは句や文なので候補にしない）
const MAX_SPELLING_CHARS: usize = 48;
/// 推定の原語の順位の下限（lsource 由来は 0〜3）
pub const INFERRED_RANK_BASE: u8 = 100;

#[derive(Deserialize)]
struct Headword {
    text: String,
    #[serde(default)]
    common: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LangSource {
    lang: String,
    #[serde(default = "yes")]
    full: bool,
    #[serde(default)]
    wasei: bool,
    text: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct Gloss {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sense {
    #[serde(default)]
    language_source: Vec<LangSource>,
    #[serde(default)]
    gloss: Vec<Gloss>,
}

#[derive(Deserialize)]
struct Word {
    #[serde(default)]
    kana: Vec<Headword>,
    #[serde(default)]
    kanji: Vec<Headword>,
    #[serde(default)]
    sense: Vec<Sense>,
}

/// `reader` の JSON から索引の入力を取り出し、1 件ずつ `sink` に渡す
pub fn extract_reader<R: Read>(reader: R, sink: &mut dyn FnMut(Entry)) -> Result<(), String> {
    let mut de = serde_json::Deserializer::from_reader(reader);
    let mut on_word = |w: Word| word_to_entries(&w, sink);
    Deserializer::deserialize_map(&mut de, TopVisitor(&mut on_word))
        .map_err(|e| format!("JMdict の JSON を読めません: {e}"))
}

/// 文字列の JSON から取り出す（テスト・小さな入力用）
pub fn extract(json: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    extract_reader(json.as_bytes(), &mut |e| out.push(e))?;
    Ok(out)
}

struct TopVisitor<'a>(&'a mut dyn FnMut(Word));

impl<'de> Visitor<'de> for TopVisitor<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("JMdict の JSON オブジェクト")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key == "words" {
                map.next_value_seed(WordsSeed(&mut *self.0))?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

struct WordsSeed<'a>(&'a mut dyn FnMut(Word));

impl<'de> DeserializeSeed<'de> for WordsSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for WordsSeed<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("words の配列")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(word) = seq.next_element::<Word>()? {
            (self.0)(word);
        }
        Ok(())
    }
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}')
}

/// 前半と後半が同じ語（ワクワク）か
fn is_reduplicated(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    c.len() >= 4 && c.len() % 2 == 0 && c[..c.len() / 2] == c[c.len() / 2..]
}

fn word_to_entries(w: &Word, sink: &mut dyn FnMut(Entry)) {
    // 見出し: カタカナだけの kana / kanji 表記（common なら順位が上）
    let mut heads: Vec<(&str, bool)> = Vec::new();
    for h in w.kana.iter().chain(w.kanji.iter()) {
        if is_katakana_word(&h.text) && !heads.iter().any(|(t, _)| *t == h.text) {
            heads.push((&h.text, h.common));
        }
    }
    if heads.is_empty() {
        return;
    }
    for sense in &w.sense {
        let spelled: Vec<&LangSource> = sense
            .language_source
            .iter()
            .filter(|s| {
                s.text.as_deref().is_some_and(|t| {
                    let t = t.trim();
                    !t.is_empty()
                        && t.chars().count() <= MAX_SPELLING_CHARS
                        && !t.chars().any(is_kana)
                })
            })
            .collect();
        if !spelled.is_empty() {
            // 同じ言語の綴りが 1 つの語義に複数あるものは複合語（hot + dog）なので捨てる
            // （言語が違うものは、同じ語の別の言語での綴り。espada: por / spa）
            for s in &spelled {
                if spelled.iter().filter(|t| t.lang == s.lang).count() > 1 {
                    continue;
                }
                let text = s.text.as_deref().unwrap_or("").trim();
                for (head, common) in &heads {
                    sink(Entry {
                        headword: (*head).to_string(),
                        lang: s.lang.clone(),
                        spelling: text.to_string(),
                        wasei: s.wasei,
                        rank: u8::from(!common) * 2 + u8::from(!s.full),
                    });
                }
            }
            continue;
        }
        // 綴りの無い語義: 和製英語の印があれば出さない。無ければ gloss から推定する
        if sense.language_source.iter().any(|s| s.wasei) {
            continue;
        }
        let lang = sense
            .language_source
            .first()
            .map_or("eng", |s| s.lang.as_str());
        for (head, common) in &heads {
            if is_reduplicated(head) {
                continue; // ワクワク・ペラペラなどの擬音語・擬態語は外来語ではない
            }
            // gloss は重要な順なので、音が合う最初の 1 つだけ採る（mail に email と male を並べない）
            if let Some(g) = sense
                .gloss
                .iter()
                .find(|g| !g.text.ends_with("ly") && gloss_matches(head, &g.text))
            {
                sink(Entry {
                    headword: (*head).to_string(),
                    lang: lang.to_string(),
                    spelling: g.text.clone(),
                    wasei: false,
                    rank: INFERRED_RANK_BASE + u8::from(!common),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
  "version": "3.6.2", "languages": ["eng"], "commonOnly": false, "dictDate": "2026-10-19",
  "dictRevisions": ["x"], "tags": {"n": "noun"},
  "words": [
    {"id": "1", "kanji": [], "kana": [{"text": "アルバイト", "common": true, "tags": [], "appliesToKanji": ["*"]}],
     "sense": [{"partOfSpeech": ["n"], "languageSource": [{"lang": "ger", "full": true, "wasei": false, "text": "Arbeit"}],
                "gloss": [{"lang": "eng", "text": "part-time job"}]}]},
    {"id": "2", "kana": [{"text": "コンピューター", "common": true}, {"text": "コンピュータ", "common": false}],
     "sense": [{"languageSource": [], "gloss": [{"lang": "eng", "text": "computer"}, {"lang": "eng", "text": "calculator"}]}]},
    {"id": "3", "kana": [{"text": "サラリーマン"}],
     "sense": [{"languageSource": [{"lang": "eng", "full": true, "wasei": true, "text": "salaried man"}], "gloss": [{"text": "office worker"}]}]},
    {"id": "4", "kana": [{"text": "ホットドッグ"}],
     "sense": [{"languageSource": [{"lang": "eng", "full": false, "wasei": false, "text": "hot"}, {"lang": "eng", "full": false, "wasei": false, "text": "dog"}], "gloss": []}]},
    {"id": "5", "kana": [{"text": "エスパダ"}],
     "sense": [{"languageSource": [{"lang": "por", "text": "espada"}, {"lang": "spa", "text": "espada"}]}]},
    {"id": "6", "kana": [{"text": "テレビ", "common": true}],
     "sense": [{"languageSource": [{"lang": "eng", "full": false, "wasei": false, "text": "television"}]}]},
    {"id": "7", "kanji": [{"text": "猫"}], "kana": [{"text": "ねこ"}],
     "sense": [{"languageSource": [{"lang": "eng", "text": "not-a-loan"}]}]},
    {"id": "8", "kana": [{"text": "カバン"}],
     "sense": [{"languageSource": [{"lang": "dut", "text": null}], "gloss": [{"text": "bag"}]}]},
    {"id": "9", "kana": [{"text": "マンション", "common": true}],
     "sense": [{"languageSource": [{"lang": "eng", "full": true, "wasei": false, "text": "mansion"}]}]},
    {"id": "10", "kana": [{"text": "ガス", "common": true}],
     "sense": [{"languageSource": [{"lang": "dut", "text": null}], "gloss": [{"text": "gas"}]}]}
  ]
}"#;

    fn find<'a>(v: &'a [Entry], head: &str) -> Vec<&'a Entry> {
        v.iter().filter(|e| e.headword == head).collect()
    }

    #[test]
    fn extracts_language_spelling_wasei_and_rank() {
        let v = extract(FIXTURE).unwrap();
        let a = find(&v, "アルバイト");
        assert_eq!(a.len(), 1);
        assert_eq!(
            (a[0].lang.as_str(), a[0].spelling.as_str()),
            ("ger", "Arbeit")
        );
        assert_eq!(a[0].rank, 0);
        assert!(find(&v, "サラリーマン")[0].wasei);
        assert_eq!(find(&v, "エスパダ").len(), 2);
        // full = false は順位が 1 つ下、common でなければさらに 2 下
        assert_eq!(find(&v, "テレビ")[0].rank, 1);
        assert!(!find(&v, "マンション")[0].wasei);
    }

    #[test]
    fn infers_from_single_word_gloss_only_when_phonetically_close() {
        let v = extract(FIXTURE).unwrap();
        let c = find(&v, "コンピューター");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].spelling, "computer");
        assert!(c[0].rank >= INFERRED_RANK_BASE);
        // 見出しが複数あれば、どれからも引ける（common でない方は順位が 1 つ下）
        assert_eq!(find(&v, "コンピュータ")[0].rank, INFERRED_RANK_BASE + 1);
        // 音が合わない gloss（bag / カバン）、綴りなしでも言語だけの語義は推定しない
        assert!(find(&v, "カバン").is_empty());
        // 言語が eng 以外で綴りが無くても、gloss が音で一致すれば推定（言語は lsource のもの）
        let g = find(&v, "ガス");
        assert_eq!((g[0].lang.as_str(), g[0].spelling.as_str()), ("dut", "gas"));
    }

    #[test]
    fn skips_compounds_wasei_without_spelling_and_non_katakana() {
        let v = extract(FIXTURE).unwrap();
        assert!(find(&v, "ホットドッグ").is_empty());
        assert!(v.iter().all(|e| e.headword != "猫" && e.headword != "ねこ"));
    }

    #[test]
    fn inference_takes_the_first_matching_gloss_and_skips_mimetics() {
        let v = extract(
            r#"{"words":[
            {"kana":[{"text":"メール"}],"sense":[{"gloss":[{"text":"email"},{"text":"male"}]}]},
            {"kana":[{"text":"ペラペラ"}],"sense":[{"gloss":[{"text":"volubly"}]}]},
            {"kana":[{"text":"ワクワク"}],"sense":[{"gloss":[{"text":"wakuwaku"}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(find(&v, "メール").len(), 1);
        assert_eq!(find(&v, "メール")[0].spelling, "email");
        assert!(find(&v, "ペラペラ").is_empty());
        assert!(find(&v, "ワクワク").is_empty());
    }

    #[test]
    fn broken_json_is_an_error() {
        assert!(extract("{").is_err());
        assert!(extract("[]").is_err());
        assert!(extract(r#"{"words": 3}"#).is_err());
    }

    #[test]
    fn streams_without_reading_unknown_keys_into_memory() {
        let mut n = 0;
        extract_reader(
            br#"{"tags":{"a":[1,2,3]},"words":[],"after":1}"#.as_slice(),
            &mut |_| n += 1,
        )
        .unwrap();
        assert_eq!(n, 0);
    }
}
