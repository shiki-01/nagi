//! カタカナの正規化と「骨格」。索引のキーと問い合わせの両方に同じ関数を通す。

/// 長音符
const CHOON: char = 'ー';

/// オ段のカナ（直後の「ウ」は長音とみなす）
const O_ROW: &str = "オコゴソゾトドノホボポモヨロヲョ";
/// エ段のカナ（直後の「イ」は長音とみなす）
const E_ROW: &str = "エケゲセゼテデネヘベペメレ";

/// ひらがなをカタカナにする。ほかの文字はそのまま
fn fold_char(c: char) -> char {
    match c {
        '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
        'ヮ' => 'ワ',
        'ヵ' => 'カ',
        'ヶ' => 'ケ',
        _ => c,
    }
}

/// カタカナ（長音符・中黒を含む）だけの文字か
pub fn is_katakana_char(c: char) -> bool {
    matches!(
        c,
        '\u{30A1}'..='\u{30FA}' | 'ー' | '・' | '\u{30FD}' | '\u{30FE}'
    )
}

/// 空でなく、カタカナ（と長音符・中黒）だけの文字列か
pub fn is_katakana_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_katakana_char) && s.chars().any(|c| c != 'ー' && c != '・')
}

/// 索引のキーにする正規形。
///
/// - ひらがな → カタカナ、中黒・空白・`＝` を除く
/// - `ヴ` 系を `ブ` 系へ（ヴァ → バ、ヴ → ブ）
/// - 小書きの母音（ァィゥェォ）を普通の大きさへ（ウィ → ウイ、ファ → フア、ティ → テイ）
/// - 末尾の長音符を除く（コンピューター → コンピュータ。ただし 3 文字未満になるなら残す）
///
/// 語中の長音符は残す（ビール / ビル、ボール / ボウルは別の語）
pub fn normalize(s: &str) -> String {
    let chars: Vec<char> = s
        .chars()
        .filter(|c| !matches!(c, '・' | '＝' | ' ' | '　' | '·'))
        .map(fold_char)
        .collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == 'ヴ' {
            let next = chars.get(i + 1).copied();
            match next {
                Some('ァ') | Some('ア') => {
                    out.push('バ');
                    i += 2;
                }
                Some('ィ') | Some('イ') => {
                    out.push('ビ');
                    i += 2;
                }
                Some('ェ') | Some('エ') => {
                    out.push('ベ');
                    i += 2;
                }
                Some('ォ') | Some('オ') => {
                    out.push('ボ');
                    i += 2;
                }
                Some('ュ') => {
                    out.push('ビ');
                    out.push('ュ');
                    i += 2;
                }
                _ => {
                    out.push('ブ');
                    i += 1;
                }
            }
            continue;
        }
        let full = match c {
            'ァ' if !out.is_empty() => 'ア',
            'ィ' if !out.is_empty() => 'イ',
            'ゥ' if !out.is_empty() => 'ウ',
            'ェ' if !out.is_empty() => 'エ',
            'ォ' if !out.is_empty() => 'オ',
            other => other,
        };
        out.push(full);
        i += 1;
    }
    if out.last() == Some(&CHOON) && out.len() >= 4 {
        out.pop();
    }
    out.into_iter().collect()
}

/// 正規形から、長音・母音の細かい違いを落とした「骨格」。ゆるい一致（編集距離 1）に使う。
/// 長音符・促音・撥音を除き、オ段+ウ / エ段+イ の長音も除き、ヂ・ヅをジ・ズにする
pub fn skeleton(norm: &str) -> String {
    let mut out: Vec<char> = Vec::new();
    for c in norm.chars() {
        match c {
            CHOON | 'ッ' | 'ン' => {}
            'ウ' if out.last().is_some_and(|p| O_ROW.contains(*p)) => {}
            'イ' if out.last().is_some_and(|p| E_ROW.contains(*p)) => {}
            'ヂ' => out.push('ジ'),
            'ヅ' => out.push('ズ'),
            other => out.push(other),
        }
    }
    out.into_iter().collect()
}

/// 編集距離が 1 以下か（挿入・削除・置換・隣接する 2 文字の入れ替え）。同一は `false`
pub fn within_one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a == b || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let prefix = short
        .iter()
        .zip(long.iter())
        .take_while(|(x, y)| x == y)
        .count();
    if short.len() == long.len() {
        // 置換 1 か、隣接入れ替え
        let rest_a = &a[prefix..];
        let rest_b = &b[prefix..];
        if rest_a[1..] == rest_b[1..] {
            return true;
        }
        return rest_a.len() >= 2
            && rest_a[0] == rest_b[1]
            && rest_a[1] == rest_b[0]
            && rest_a[2..] == rest_b[2..];
    }
    // 長い方から 1 文字消して一致するか
    short[prefix..] == long[prefix + 1..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_variants_to_the_same_key() {
        assert_eq!(normalize("コンピューター"), normalize("コンピュータ"));
        assert_eq!(normalize("バイオリン"), normalize("ヴァイオリン"));
        assert_eq!(normalize("ヴィーナス"), normalize("ビーナス"));
        assert_eq!(normalize("ウィンドウ"), normalize("ウインドウ"));
        assert_eq!(normalize("パーティー"), normalize("パーテイー"));
        assert_eq!(normalize("ファイル"), normalize("フアイル"));
        assert_eq!(normalize("コンピュ・ータ"), normalize("コンピュータ"));
        assert_eq!(normalize("こんぴゅーたー"), normalize("コンピュータ"));
        assert_eq!(normalize("ヴ"), "ブ");
    }

    #[test]
    fn keeps_distinct_words_distinct() {
        assert_ne!(normalize("ビール"), normalize("ビル"));
        assert_ne!(normalize("ボール"), normalize("ボウル"));
        // 短い語の長音符は残す
        assert_eq!(normalize("カー"), "カー");
        assert_ne!(normalize("カー"), normalize("カ"));
    }

    #[test]
    fn skeleton_drops_long_vowels() {
        assert_eq!(skeleton("ボール"), skeleton("ボウル"));
        assert_eq!(skeleton("ケーキ"), skeleton("ケイキ"));
        assert_eq!(skeleton("バッグ"), skeleton("バグ"));
    }

    #[test]
    fn edit_distance_one() {
        assert!(within_one_edit("シミュレション", "シュミレション"));
        assert!(within_one_edit("ダイヤ", "ダイア"));
        assert!(within_one_edit("アイス", "アイ"));
        assert!(!within_one_edit("アイス", "アイス"));
        assert!(!within_one_edit("アイス", "ガラス"));
        assert!(!within_one_edit("アイスク", "アイ"));
    }

    #[test]
    fn katakana_word_check() {
        assert!(is_katakana_word("コンピューター"));
        assert!(is_katakana_word("ロック・ンロール"));
        assert!(!is_katakana_word("ー"));
        assert!(!is_katakana_word("コード1"));
        assert!(!is_katakana_word("かな"));
        assert!(!is_katakana_word(""));
    }
}
