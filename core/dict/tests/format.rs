//! バイナリ形式 v3（mmap 可能・ゼロコピー参照）のテスト: 参照モデルとの一致・mmap 版との同値性・
//! 壊れた入力の拒否（panic しないこと）・アラインメント。

use nagi_dict::{Dictionary, DictionaryBuilder};
use std::collections::BTreeMap;

const HEADER_LEN: usize = 176;

/// (読み, 表記, lid, rid, pos_id, コスト)
type Word = (&'static str, &'static str, u16, u16, u16, u16);

const WORDS: &[Word] = &[
    ("きょう", "今日", 1, 1, 1, 3000),
    ("きょう", "京", 1, 1, 1, 5000),
    ("きょう", "強", 2, 2, 2, 5000),
    ("きょう", "きょう", 1, 1, 1, 9000),
    ("きょうは", "今日は", 1, 1, 1, 20000),
    ("は", "は", 2, 2, 2, 500),
    ("は", "葉", 1, 1, 1, 6000),
    ("てんき", "天気", 1, 1, 1, 3000),
    ("です", "です", 3, 3, 3, 300),
    ("いく", "行く", 2, 2, 2, 3000),
    ("とうきょう", "東京", 1, 1, 1, 3000),
    ("とう", "塔", 1, 1, 1, 7000),
    ("。", "。", 3, 3, 3, 100),
    ("😀", "笑顔", 1, 1, 1, 4000),
];

const N: usize = 4;

fn build() -> Dictionary {
    let mut b = DictionaryBuilder::new();
    for &(reading, surface, lid, rid, pos_id, cost) in WORDS {
        b.add_entry(reading, surface, lid, rid, pos_id, cost);
    }
    b.set_pos_defs(vec![
        "BOS/EOS,*,*,*,*,*,*".into(),
        "名詞,一般,*,*,*,*,*".into(),
        "助詞,係助詞,*,*,*,*,*".into(),
        "助動詞,*,*,*,特殊・デス,基本形,です".into(),
    ]);
    // 連接コスト: 値が (rid, lid) ごとに異なるようにして、読み違いを検出できるようにする
    let costs: Vec<u16> = (0..N * N).map(|i| (i as u16) * 37 % 211).collect();
    b.set_connection(N, costs);
    b.build().unwrap()
}

/// 語の属性（表記, lid, rid, pos_id, コスト）
type Attrs = (&'static str, u16, u16, u16, u16);

/// 参照モデル: 読み → コスト昇順の (表記, lid, rid, pos_id, コスト)
fn model() -> BTreeMap<&'static str, Vec<Attrs>> {
    let mut m: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for &(reading, surface, lid, rid, pos_id, cost) in WORDS {
        m.entry(reading)
            .or_default()
            .push((surface, lid, rid, pos_id, cost));
    }
    for v in m.values_mut() {
        v.sort_by(|a, b| a.4.cmp(&b.4).then(a.0.cmp(b.0)));
    }
    m
}

fn lookup_tuples(d: &Dictionary, reading: &str) -> Vec<(String, u16, u16, u16, u16)> {
    d.lookup(reading)
        .into_iter()
        .map(|e| (e.surface, e.lid, e.rid, e.pos_id, e.cost))
        .collect()
}

fn assert_matches_model(d: &Dictionary) {
    let m = model();
    assert_eq!(d.entry_count(), WORDS.len());
    assert_eq!(d.reading_count(), m.len());
    assert_eq!(d.pos_id_count(), N);
    for (reading, expected) in &m {
        let expected: Vec<_> = expected
            .iter()
            .map(|&(s, a, b, c, e)| (s.to_string(), a, b, c, e))
            .collect();
        assert_eq!(lookup_tuples(d, reading), expected, "{reading}");
        assert!(d.contains_reading(reading));
        assert!(d.has_prefix(reading));
    }
    assert!(d.lookup("ない").is_empty());
    assert!(!d.contains_reading("きょ"));
    assert!(d.has_prefix("きょ"));
    assert!(d.has_prefix(""));
    assert!(!d.has_prefix("きょうだい"));
    // common_prefix_lookup: 読みのバイト長の短い順に、テキストの接頭辞となる読みを全部返す
    for text in ["きょうはてんき", "とうきょうに", "😀です", "あ"] {
        let expected: Vec<(usize, Vec<_>)> = m
            .iter()
            .filter(|(r, _)| text.starts_with(**r))
            .map(|(r, v)| {
                (
                    r.len(),
                    v.iter()
                        .map(|&(s, a, b, c, e)| (s.to_string(), a, b, c, e))
                        .collect(),
                )
            })
            .collect::<Vec<_>>();
        let mut expected = expected;
        expected.sort_by_key(|(len, _)| *len);
        let actual: Vec<(usize, Vec<_>)> = d
            .common_prefix_lookup(text)
            .into_iter()
            .map(|(len, es)| {
                (
                    len,
                    es.into_iter()
                        .map(|e| (e.surface, e.lid, e.rid, e.pos_id, e.cost))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(actual, expected, "{text}");
    }
}

#[test]
fn built_dictionary_matches_reference_model() {
    assert_matches_model(&build());
}

#[test]
fn bytes_round_trip_matches_reference_model() {
    let d = build();
    let bytes = d.to_bytes();
    assert_eq!(d.as_bytes(), &bytes[..]);
    let loaded = Dictionary::from_bytes(&bytes).unwrap();
    assert_matches_model(&loaded);
    assert_eq!(loaded.to_bytes(), bytes);
    assert_eq!(
        Dictionary::from_vec(bytes.clone()).unwrap().to_bytes(),
        bytes
    );
    for text in ["きょうはてんきです", "とうきょうにいく", "😀"] {
        assert_eq!(loaded.convert(text), d.convert(text));
        assert_eq!(
            loaded.segment_candidates(text, 5),
            d.segment_candidates(text, 5)
        );
    }
    loaded.verify().unwrap();
}

fn section(bytes: &[u8], i: usize) -> (usize, usize) {
    let rd = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
    (rd(16 + i * 16), rd(24 + i * 16))
}

fn set_u64(bytes: &mut [u8], at: usize, v: u64) {
    bytes[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn sections_are_aligned_ordered_and_in_bounds() {
    let bytes = build().to_bytes();
    assert_eq!(&bytes[..8], b"NAGIDICT");
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 3);
    let mut prev_end = HEADER_LEN;
    for i in 0..10 {
        let (off, len) = section(&bytes, i);
        assert_eq!(off % 8, 0, "section {i}");
        assert!(off >= prev_end, "section {i}");
        prev_end = off + len;
        assert!(prev_end <= bytes.len(), "section {i}");
    }
}

#[test]
fn old_format_asks_for_rebuild() {
    let mut bytes = build().to_bytes();
    bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
    let err = Dictionary::from_bytes(&bytes).unwrap_err().to_string();
    assert!(err.contains("build-dict"), "{err}");
    bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
    assert!(Dictionary::from_bytes(&bytes).is_err());
}

#[test]
fn broken_headers_are_errors_not_panics() {
    let bytes = build().to_bytes();
    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert!(Dictionary::from_bytes(&bad_magic).is_err());
    assert!(Dictionary::from_bytes(&[]).is_err());
    for cut in [
        0,
        5,
        11,
        12,
        100,
        HEADER_LEN - 1,
        HEADER_LEN,
        bytes.len() / 2,
    ] {
        assert!(Dictionary::from_bytes(&bytes[..cut]).is_err(), "cut={cut}");
    }
    for cut in 0..bytes.len() {
        // どの位置で切っても panic しない（末尾が空の最終セクションだけ切れる場合は Ok もあり得る）
        let _ = Dictionary::from_bytes(&bytes[..cut]);
    }
    assert!(Dictionary::from_bytes(&bytes[..bytes.len() - 1]).is_err());

    for i in 0..10 {
        let (off, len) = section(&bytes, i);
        // アラインメント違反
        let mut b = bytes.clone();
        set_u64(&mut b, 16 + i * 16, off as u64 + 1);
        assert!(Dictionary::from_bytes(&b).is_err(), "misaligned {i}");
        // 範囲外オフセット
        let mut b = bytes.clone();
        set_u64(&mut b, 16 + i * 16, (bytes.len() as u64 + 7) & !7);
        assert!(Dictionary::from_bytes(&b).is_err(), "offset oob {i}");
        let mut b = bytes.clone();
        set_u64(&mut b, 16 + i * 16, u64::MAX & !7);
        assert!(Dictionary::from_bytes(&b).is_err(), "offset huge {i}");
        // 長さの溢れ・範囲外
        let mut b = bytes.clone();
        set_u64(&mut b, 24 + i * 16, u64::MAX);
        assert!(Dictionary::from_bytes(&b).is_err(), "len huge {i}");
        let mut b = bytes.clone();
        set_u64(&mut b, 24 + i * 16, (bytes.len() - off) as u64 + 8);
        assert!(Dictionary::from_bytes(&b).is_err(), "len oob {i}");
        // 重なり（ヘッダ内を指す）
        let mut b = bytes.clone();
        set_u64(&mut b, 16 + i * 16, 8);
        assert!(Dictionary::from_bytes(&b).is_err(), "overlap {i}");
        // 長さを 1 バイトずらすと、配列の要素数が合わなくなるか UTF-8 等が壊れる
        if i != 0 && len > 0 && i != 6 && i != 7 {
            let mut b = bytes.clone();
            set_u64(&mut b, 24 + i * 16, len as u64 - 1);
            assert!(Dictionary::from_bytes(&b).is_err(), "len-1 {i}");
        }
    }
    // ヘッダのどのバイトを壊しても panic しない
    for at in 0..HEADER_LEN {
        for v in [0u8, 1, 0x7f, 0xff] {
            let mut b = bytes.clone();
            b[at] = v;
            let _ = Dictionary::from_bytes(&b);
        }
    }
}

#[test]
fn inconsistent_contents_are_rejected_by_full_verification() {
    let bytes = build().to_bytes();
    // lid を連接コスト表の範囲外にする
    let (lids_off, _) = section(&bytes, 2);
    let mut b = bytes.clone();
    b[lids_off..lids_off + 2].copy_from_slice(&200u16.to_le_bytes());
    assert!(Dictionary::from_bytes(&b).is_err());
    // pos_id を品詞定義の範囲外にする
    let (pos_off, _) = section(&bytes, 9);
    let mut b = bytes.clone();
    b[pos_off..pos_off + 2].copy_from_slice(&200u16.to_le_bytes());
    assert!(Dictionary::from_bytes(&b).is_err());
    // 表記プールの UTF-8 を壊す
    let (pool_off, _) = section(&bytes, 6);
    let mut b = bytes.clone();
    b[pool_off] = 0xff;
    assert!(Dictionary::from_bytes(&b).is_err());
    // 語グループ表の単調性を壊す（末尾の番兵は保ったまま先頭付近を大きくする）
    let (g_off, _) = section(&bytes, 1);
    let mut b = bytes.clone();
    b[g_off + 4..g_off + 8].copy_from_slice(&1000u32.to_le_bytes());
    assert!(Dictionary::from_bytes(&b).is_err());
}

#[cfg(feature = "mmap")]
mod mmap {
    use super::*;

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nagi-dict-format-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn mmap_matches_from_bytes() {
        let heap = build();
        let path = temp_file("equiv.bin", &heap.to_bytes());
        let mapped = Dictionary::load_mmap(&path).unwrap();
        assert_matches_model(&mapped);
        mapped.verify().unwrap();
        assert_eq!(mapped.to_bytes(), heap.to_bytes());
        let loaded = Dictionary::load(&path).unwrap();
        for text in [
            "きょうはてんきです",
            "とうきょうにいく",
            "😀です。",
            "ないものはない",
            "",
        ] {
            assert_eq!(mapped.convert(text), heap.convert(text), "{text}");
            assert_eq!(loaded.convert(text), heap.convert(text), "{text}");
            assert_eq!(
                mapped.segment_candidates(text, 5),
                heap.segment_candidates(text, 5)
            );
            assert_eq!(
                mapped.common_prefix_lookup(text),
                heap.common_prefix_lookup(text)
            );
        }
        // mmap 中にリネームで置き換えても、既存のマップは旧内容のまま（原子的置換運用の確認）
        let replacement = temp_file("equiv.bin.new", b"not a dictionary");
        std::fs::rename(&replacement, &path).unwrap();
        assert_matches_model(&mapped);
        assert!(Dictionary::load_mmap(&path).is_err());
    }

    #[test]
    fn mmap_rejects_broken_files() {
        let bytes = build().to_bytes();
        assert!(Dictionary::load_mmap(&temp_file("empty.bin", b"")).is_err());
        assert!(Dictionary::load_mmap(std::path::Path::new("/nonexistent/nagi-dict.bin")).is_err());
        let mut old = bytes.clone();
        old[8..12].copy_from_slice(&2u32.to_le_bytes());
        let err = Dictionary::load_mmap(&temp_file("old.bin", &old))
            .unwrap_err()
            .to_string();
        assert!(err.contains("build-dict"), "{err}");
        for cut in [0, 7, 12, HEADER_LEN, bytes.len() / 2, bytes.len() - 1] {
            assert!(
                Dictionary::load_mmap(&temp_file("cut.bin", &bytes[..cut])).is_err(),
                "cut={cut}"
            );
        }
        let mut misaligned = bytes.clone();
        let (off, _) = section(&bytes, 3);
        set_u64(&mut misaligned, 16 + 3 * 16, off as u64 + 2);
        assert!(Dictionary::load_mmap(&temp_file("mis.bin", &misaligned)).is_err());
    }

    /// mmap 版は中身を全検査しないが、FST 以外の本体が壊れていても参照で panic しない
    #[test]
    fn mmap_access_to_corrupt_tables_does_not_panic() {
        let bytes = build().to_bytes();
        for sec in [1usize, 2, 3, 4, 5, 6, 8, 9] {
            let (off, len) = section(&bytes, sec);
            for at in off..off + len {
                for v in [0u8, 0xff] {
                    let mut b = bytes.clone();
                    b[at] = v;
                    let path = temp_file("corrupt.bin", &b);
                    let Ok(d) = Dictionary::load_mmap(&path) else {
                        continue;
                    };
                    for text in ["きょうはてんきです", "とうきょうにいく", "😀です。"]
                    {
                        let _ = d.convert(text);
                        let _ = d.segment_candidates(text, 5);
                        let _ = d.common_prefix_lookup(text);
                        let _ = d.lookup("きょう");
                    }
                }
            }
        }
    }
}
