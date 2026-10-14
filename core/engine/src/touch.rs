//! タッチ入力（12 キー・フリック・濁点の循環）の文字解決。
//!
//! フロントエンドはキー（行）と操作（タップ・トグル・フリック方向）を正規化して送るだけで、
//! どの文字になるか・連続タップでどう変わるか（ロジック）はここで決める。レイアウトの描画はフロント。

use nagi_protocol::methods::{KanaKey, KeyGesture, KeyTableEntry, KeyTableResult};

/// 12 キーのうちの文字キー（レイアウト順）。`key_table` の並びになる
const ALL_KEYS: [KanaKey; 11] = [
    KanaKey::A,
    KanaKey::Ka,
    KanaKey::Sa,
    KanaKey::Ta,
    KanaKey::Na,
    KanaKey::Ha,
    KanaKey::Ma,
    KanaKey::Ya,
    KanaKey::Ra,
    KanaKey::Wa,
    KanaKey::Punct,
];

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

/// 12 キーの文字表と濁点・小書きの循環を返す（`get_key_table`）。`key_chars` / `VARIANT_CYCLES` /
/// `gesture_index` と同じ表から作るので、`insert_reading` の実際の挙動と食い違わない（単一の情報源）。
/// トグルの循環は、連続タップで進む順（`key_chars` の先頭から末尾、末尾の次は先頭）
pub fn key_table() -> KeyTableResult {
    let keys = ALL_KEYS
        .iter()
        .map(|&key| {
            let chars = key_chars(key);
            let at = |gesture| chars[gesture_index(gesture)].to_string();
            KeyTableEntry {
                key,
                center: at(KeyGesture::Tap),
                left: at(KeyGesture::FlickLeft),
                up: at(KeyGesture::FlickUp),
                right: at(KeyGesture::FlickRight),
                down: at(KeyGesture::FlickDown),
                toggle: chars.iter().map(char::to_string).collect(),
            }
        })
        .collect();
    let variants = VARIANT_CYCLES
        .iter()
        .map(|cycle| cycle.iter().map(char::to_string).collect())
        .collect();
    KeyTableResult { keys, variants }
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
    fn key_table_matches_resolution_tables() {
        let table = key_table();
        assert_eq!(table.keys.len(), 11);
        let ka = table.keys.iter().find(|e| e.key == KanaKey::Ka).unwrap();
        assert_eq!(
            (&*ka.center, &*ka.left, &*ka.up, &*ka.right, &*ka.down),
            ("か", "き", "く", "け", "こ")
        );
        assert_eq!(ka.toggle, ["か", "き", "く", "け", "こ"]);
        for e in &table.keys {
            let chars = key_chars(e.key);
            assert_eq!(e.center, chars[gesture_index(KeyGesture::Tap)].to_string());
            assert_eq!(
                e.down,
                chars[gesture_index(KeyGesture::FlickDown)].to_string()
            );
        }
        assert_eq!(table.variants.len(), VARIANT_CYCLES.len());
        assert!(table.variants.iter().any(|c| c == &["は", "ば", "ぱ"]));
        // 循環の次の要素は next_variant と一致する
        for cycle in &table.variants {
            for (i, s) in cycle.iter().enumerate() {
                let next = &cycle[(i + 1) % cycle.len()];
                let c = s.chars().next().unwrap();
                assert_eq!(next_variant(c).map(String::from).as_deref(), Some(&**next));
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
