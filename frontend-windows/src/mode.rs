//! 入力モードの OS 側との同期（OS 非依存）。macOS 版の `requestMode` / `syncMode` に相当する。
//!
//! Windows のモード表示（タスクトレイの「あ / A」）は TSF のコンパートメント
//! `GUID_COMPARTMENT_KEYBOARD_OPENCLOSE`（開閉）と `GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION`
//! （変換モード）が正。コアのモードが真実で、コアの変化は OS へ書き、OS 側の操作（トレイのクリック・
//! 他の IME との往復）は `perform_action(SetMode)` でコアへ送る。**同じモードなら送らない・書かない**
//! （`SetMode` は入力中の文字列を確定するため。自分で書いた値の通知で往復しないようにする）。

use nagi_protocol::InputMode;

/// `TF_CONVERSIONMODE_NATIVE`（日本語入力）
pub const CONVERSION_NATIVE: u32 = 0x0001;
/// `TF_CONVERSIONMODE_FULLSHAPE`（全角）
pub const CONVERSION_FULLSHAPE: u32 = 0x0008;
/// `TF_CONVERSIONMODE_ROMAN`（ローマ字入力）
pub const CONVERSION_ROMAN: u32 = 0x0010;

/// コンパートメントに書く値
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompartmentValues {
    /// `KEYBOARD_OPENCLOSE`（IME が開いているか）
    pub open: bool,
    /// `KEYBOARD_INPUTMODE_CONVERSION`（`TF_CONVERSIONMODE_*` の組み合わせ）
    pub conversion: u32,
}

/// モード → コンパートメントの値。ひらがなは MS-IME と同じ「開・日本語・全角・ローマ字」。
/// 英数は「閉・ローマ字」（アプリからは IME オフに見える。MS-IME の半角英数と同様）
pub fn compartment_values(mode: InputMode) -> CompartmentValues {
    match mode {
        InputMode::Hiragana => CompartmentValues {
            open: true,
            conversion: CONVERSION_NATIVE | CONVERSION_FULLSHAPE | CONVERSION_ROMAN,
        },
        InputMode::Alphanumeric => CompartmentValues {
            open: false,
            conversion: CONVERSION_ROMAN,
        },
    }
}

/// コンパートメントの値 → モード。閉じていれば英数、開いていて日本語入力ビットがあればひらがな、
/// 開いていてビットが無ければ英数（カタカナ等はコアのモードではないのでひらがな扱い）
pub fn mode_from_compartments(open: bool, conversion: u32) -> InputMode {
    if open && conversion & CONVERSION_NATIVE != 0 {
        InputMode::Hiragana
    } else {
        InputMode::Alphanumeric
    }
}

/// OS とコアが合意している現在のモード
#[derive(Debug, Clone, Copy, Default)]
pub struct ModeSync {
    known: Option<InputMode>,
}

impl ModeSync {
    /// 未確定の状態で作る（最初の同期は必ず通る）
    pub fn new() -> Self {
        Self::default()
    }

    /// 合意しているモード
    pub fn known(&self) -> Option<InputMode> {
        self.known
    }

    /// コアでモードが変わった（応答の `mode`）。OS へ書くべき値があれば返す
    pub fn core_changed(&mut self, mode: InputMode) -> Option<CompartmentValues> {
        if self.known == Some(mode) {
            return None;
        }
        self.known = Some(mode);
        Some(compartment_values(mode))
    }

    /// OS のコンパートメントが変わった（または activate 時に読んだ）。コアへ `SetMode` を送るべきなら返す
    pub fn os_changed(&mut self, open: bool, conversion: u32) -> Option<InputMode> {
        let mode = mode_from_compartments(open, conversion);
        if self.known == Some(mode) {
            return None;
        }
        self.known = Some(mode);
        Some(mode)
    }

    /// エンジンの起動・再起動後に呼ぶ。新しいエンジンは既定のモード（ひらがな）で始まるため、
    /// 合意を既定に戻し、OS 側の値を再度コアへ送れるようにする
    pub fn engine_restarted(&mut self) {
        self.known = Some(InputMode::Hiragana);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        for mode in [InputMode::Hiragana, InputMode::Alphanumeric] {
            let v = compartment_values(mode);
            assert_eq!(mode_from_compartments(v.open, v.conversion), mode);
        }
    }

    #[test]
    fn open_without_native_is_alphanumeric_and_closed_is_alphanumeric() {
        assert_eq!(
            mode_from_compartments(true, CONVERSION_ROMAN),
            InputMode::Alphanumeric
        );
        assert_eq!(
            mode_from_compartments(false, CONVERSION_NATIVE),
            InputMode::Alphanumeric
        );
        // カタカナ入力（NATIVE | KATAKANA）はコアのモードではひらがな
        assert_eq!(
            mode_from_compartments(true, CONVERSION_NATIVE | 0x2),
            InputMode::Hiragana
        );
    }

    #[test]
    fn same_mode_is_not_sent_either_way() {
        let mut s = ModeSync::new();
        // コアが英数になった → OS へ書く
        assert!(s.core_changed(InputMode::Alphanumeric).is_some());
        // その書き込みの通知が戻ってきてもコアへ送らない（往復しない）
        let v = compartment_values(InputMode::Alphanumeric);
        assert_eq!(s.os_changed(v.open, v.conversion), None);
        // コアの応答が同じモードを重ねても書かない
        assert_eq!(s.core_changed(InputMode::Alphanumeric), None);
        // OS 側で日本語入力に切り替えた → コアへ送る
        let h = compartment_values(InputMode::Hiragana);
        assert_eq!(
            s.os_changed(h.open, h.conversion),
            Some(InputMode::Hiragana)
        );
        assert_eq!(s.known(), Some(InputMode::Hiragana));
    }

    #[test]
    fn first_sync_always_passes() {
        let mut s = ModeSync::new();
        assert_eq!(s.os_changed(true, 0x19), Some(InputMode::Hiragana));
        let mut s = ModeSync::new();
        assert!(s.core_changed(InputMode::Hiragana).is_some());
    }

    #[test]
    fn restart_resets_agreement_to_default() {
        let mut s = ModeSync::new();
        s.core_changed(InputMode::Alphanumeric);
        s.engine_restarted();
        // 新エンジンはひらがな。OS が英数のままなら SetMode を送って揃える
        assert_eq!(
            s.os_changed(false, CONVERSION_ROMAN),
            Some(InputMode::Alphanumeric)
        );
    }
}
