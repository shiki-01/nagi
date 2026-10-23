//! カタカナ語と英単語の音の近さ（粗い子音の骨格の一致）。
//!
//! JMdict に原語の綴り（lsource）が無い外来語は、語義の gloss が 1 語の英単語で、その子音の骨格が
//! カタカナ表記と一致するときだけ「推定の原語」にする（`computer` ← コンピューター）。
//! 子音を調音の近いもの同士でまとめる（有声・無声、m/n、l/r、f/h など）粗い比較で、母音は見ない。
//! 一致は誤訳を避けるため完全一致のみ。

use crate::normalize::normalize;

/// 子音の分類を表す 1 文字
fn push(out: &mut String, c: char) {
    out.push(c);
}

/// 正規化済みカタカナの子音骨格。母音・半母音・促音・長音は無視する
pub fn katakana_class(katakana: &str) -> String {
    let mut out = String::new();
    for c in normalize(katakana).chars() {
        let class = match c {
            'カ' | 'キ' | 'ク' | 'ケ' | 'コ' | 'ガ' | 'ギ' | 'グ' | 'ゲ' | 'ゴ' => 'K',
            'サ' | 'シ' | 'ス' | 'セ' | 'ソ' | 'ザ' | 'ジ' | 'ズ' | 'ゼ' | 'ゾ' => 'S',
            'タ' | 'チ' | 'ツ' | 'テ' | 'ト' | 'ダ' | 'ヂ' | 'ヅ' | 'デ' | 'ド' => 'T',
            'ナ' | 'ニ' | 'ヌ' | 'ネ' | 'ノ' | 'マ' | 'ミ' | 'ム' | 'メ' | 'モ' | 'ン' => {
                'N'
            }
            'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => 'H',
            'バ' | 'ビ' | 'ブ' | 'ベ' | 'ボ' | 'パ' | 'ピ' | 'プ' | 'ペ' | 'ポ' => 'P',
            'ラ' | 'リ' | 'ル' | 'レ' | 'ロ' => 'R',
            _ => continue,
        };
        push(&mut out, class);
    }
    out
}

/// 英単語（小文字の ASCII）の子音骨格。綴りの典型的な発音の揺れだけを大まかに扱う
pub fn english_class(word: &str) -> String {
    let mut w = word.to_string();
    for (from, to) in [
        ("tion", "shn"),
        ("sion", "shn"),
        ("ture", "chr"),
        ("ph", "f"),
        ("ck", "k"),
        ("qu", "kw"),
        ("wh", "w"),
        ("kn", "n"),
        ("gh", ""),
        ("x", "ks"),
        ("sh", "s"),
        ("ch", "t"),
        ("th", "s"),
    ] {
        w = w.replace(from, to);
    }
    let chars: Vec<char> = w.chars().collect();
    let is_vowel = |c: char| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y');
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for (i, &c) in chars.iter().enumerate() {
        if prev == Some(c) {
            continue; // ll, tt, ss …
        }
        prev = Some(c);
        let next = chars.get(i + 1).copied();
        let class = match c {
            'c' => {
                if matches!(next, Some('e' | 'i' | 'y')) {
                    'S'
                } else {
                    'K'
                }
            }
            'k' | 'g' | 'q' => 'K',
            's' | 'z' | 'j' => 'S',
            't' | 'd' => 'T',
            'n' | 'm' => 'N',
            'f' | 'h' => 'H',
            'b' | 'p' | 'v' => 'P',
            // 母音の前の r だけ。語末・子音の前の r は長音（ー）になる
            'r' => {
                if next.is_some_and(is_vowel) {
                    'R'
                } else {
                    continue;
                }
            }
            'l' => 'R',
            _ => continue,
        };
        out.push(class);
    }
    out
}

/// `gloss` が 1 語の英単語（小文字の ASCII 3〜20 文字）か
pub fn is_single_english_word(gloss: &str) -> bool {
    (3..=20).contains(&gloss.len()) && gloss.bytes().all(|b| b.is_ascii_lowercase())
}

/// カタカナ表記 `headword` の推定の原語として `gloss` を採ってよいか
pub fn gloss_matches(headword: &str, gloss: &str) -> bool {
    if !is_single_english_word(gloss) {
        return false;
    }
    let k = katakana_class(headword);
    k.chars().count() >= 2 && k == english_class(gloss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_loanwords_match_their_english() {
        for (kata, en) in [
            ("コンピューター", "computer"),
            ("コンピュータ", "computer"),
            ("チーズ", "cheese"),
            ("ガス", "gas"),
            ("バス", "bus"),
            ("ミルク", "milk"),
            ("バター", "butter"),
            ("ファン", "fan"),
            ("ガード", "guard"),
            ("ウイスキー", "whisky"),
            ("ヴァイオリン", "violin"),
            ("ジュース", "juice"),
            ("スイッチ", "switch"),
            ("シミュレーション", "simulation"),
            ("ピザ", "pizza"),
            ("ケチャップ", "ketchup"),
        ] {
            assert!(gloss_matches(kata, en), "{kata} / {en}");
        }
    }

    #[test]
    fn unrelated_words_do_not_match() {
        for (kata, en) in [
            ("カバン", "bag"),
            ("パン", "bread"),
            ("ニンジン", "carrot"),
            ("ガタガタ", "rattling"),
            ("ワクワク", "excited"),
        ] {
            assert!(!gloss_matches(kata, en), "{kata} / {en}");
        }
        // 複数語・大文字・短すぎるものは対象外
        assert!(!gloss_matches("ホットドッグ", "hot dog"));
        assert!(!gloss_matches("ビル", "Building"));
        assert!(!gloss_matches("ティー", "tea"));
    }
}
