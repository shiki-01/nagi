//! タッチ入力（12 キー・フリック・濁点の循環）の文字解決。
//!
//! フロントエンドはキー（行）と操作（タップ・トグル・フリック方向）を正規化して送るだけで、
//! どの文字になるか・連続タップでどう変わるか（ロジック）はここで決める。レイアウトの描画はフロント。

use nagi_protocol::methods::{KanaKey, KeyGesture};

/// キーの文字を、中央（タップ）・左・上・右・下の順に返す。連続するトグルもこの順に進む
pub(crate) fn key_chars(key: KanaKey) -> [char; 5] {
    match key {
        KanaKey::A => ['あ', 'い', 'う', 'え', 'お'],
        KanaKey::Ka => ['か', 'き', 'く', 'け', 'こ'],
        KanaKey::Sa => ['さ', 'し', 'す', 'せ', 'そ'],
        KanaKey::Ta => ['た', 'ち', 'つ', 'て', 'と'],
        KanaKey::Na => ['な', 'に', 'ぬ', 'ね', 'の'],
        KanaKey::Ha => ['は', 'ひ', 'ふ', 'へ', 'ほ'],
        KanaKey::Ma => ['ま', 'み', 'む', 'め', 'も'],
        KanaKey::Ya => ['や', '（', 'ゆ', '）', 'よ'],
        KanaKey::Ra => ['ら', 'り', 'る', 'れ', 'ろ'],
        KanaKey::Wa => ['わ', 'を', 'ん', 'ー', '〜'],
        KanaKey::Punct => ['、', '。', '？', '！', '…'],
    }
}

/// 操作に対応する `key_chars` の添字
pub(crate) fn gesture_index(gesture: KeyGesture) -> usize {
    match gesture {
        KeyGesture::Tap | KeyGesture::Toggle => 0,
        KeyGesture::FlickLeft => 1,
        KeyGesture::FlickUp => 2,
        KeyGesture::FlickRight => 3,
        KeyGesture::FlickDown => 4,
    }
}

/// 濁点・半濁点・小書きの循環（先頭から末尾まで進み、末尾の次は先頭に戻る）
const VARIANT_CYCLES: &[&[char]] = &[
    &['か', 'が'],
    &['き', 'ぎ'],
    &['く', 'ぐ'],
    &['け', 'げ'],
    &['こ', 'ご'],
    &['さ', 'ざ'],
    &['し', 'じ'],
    &['す', 'ず'],
    &['せ', 'ぜ'],
    &['そ', 'ぞ'],
    &['た', 'だ'],
    &['ち', 'ぢ'],
    &['つ', 'っ', 'づ'],
    &['て', 'で'],
    &['と', 'ど'],
    &['は', 'ば', 'ぱ'],
    &['ひ', 'び', 'ぴ'],
    &['ふ', 'ぶ', 'ぷ'],
    &['へ', 'べ', 'ぺ'],
    &['ほ', 'ぼ', 'ぽ'],
    &['あ', 'ぁ'],
    &['い', 'ぃ'],
    &['う', 'ぅ', 'ゔ'],
    &['え', 'ぇ'],
    &['お', 'ぉ'],
    &['や', 'ゃ'],
    &['ゆ', 'ゅ'],
    &['よ', 'ょ'],
    &['わ', 'ゎ'],
];

/// `c` の次の濁点・半濁点・小書きの形。循環に含まれない文字は `None`
pub(crate) fn next_variant(c: char) -> Option<char> {
    VARIANT_CYCLES.iter().find_map(|cycle| {
        let i = cycle.iter().position(|&x| x == c)?;
        Some(cycle[(i + 1) % cycle.len()])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_cycles_wrap_around() {
        assert_eq!(next_variant('か'), Some('が'));
        assert_eq!(next_variant('が'), Some('か'));
        assert_eq!(next_variant('は'), Some('ば'));
        assert_eq!(next_variant('ば'), Some('ぱ'));
        assert_eq!(next_variant('ぱ'), Some('は'));
        assert_eq!(next_variant('つ'), Some('っ'));
        assert_eq!(next_variant('っ'), Some('づ'));
        assert_eq!(next_variant('づ'), Some('つ'));
        assert_eq!(next_variant('う'), Some('ぅ'));
        assert_eq!(next_variant('ゔ'), Some('う'));
        assert_eq!(next_variant('ん'), None);
        assert_eq!(next_variant('a'), None);
    }

    #[test]
    fn every_variant_cycle_member_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for cycle in VARIANT_CYCLES {
            for &c in *cycle {
                assert!(seen.insert(c), "{c} が複数の循環にある");
            }
        }
    }

    #[test]
    fn keys_have_five_distinct_chars() {
        use KanaKey::*;
        for key in [A, Ka, Sa, Ta, Na, Ha, Ma, Ya, Ra, Wa, Punct] {
            let chars = key_chars(key);
            let unique: std::collections::HashSet<_> = chars.iter().collect();
            assert_eq!(unique.len(), 5, "{key:?}");
        }
    }
}
