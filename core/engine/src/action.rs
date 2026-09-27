//! キー入力から得られるアクション（Action enum）

use nagi_protocol::view::InputMode;
use std::fmt;

/// 移動方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// 左
    Left,
    /// 右
    Right,
}

/// キー入力のアクション
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 文字を挿入
    Insert(char),
    /// 複数の Unicode スカラーから成るテキストを挿入（絵文字の ZWJ 列や、配列が生成した合成文字）
    InsertText(String),
    /// 変換開始（Space）
    StartConvert,
    /// 予測変換（Tab。Composing 中のみ有効。学習キャッシュ・ユーザー辞書の前方一致）
    Predict,
    /// 次の候補へ（Space / Down / Ctrl+n）
    NextCand,
    /// 前の候補へ（Shift+Space / Up / Ctrl+p）
    PrevCand,
    /// 特定の候補を選択（1-based インデックス）
    SelectCand(usize),
    /// 確定（Enter）
    Commit,
    /// 取消（Escape）
    Cancel,
    /// バックスペース
    Backspace,
    /// 削除（Delete）
    DeleteForward,
    /// カーソル移動
    MoveCursor(Direction),
    /// カーソルをホームへ（Home / Ctrl+a）
    MoveCursorHome,
    /// カーソルを末尾へ（End / Ctrl+e）
    MoveCursorEnd,
    /// フォーカス（注目文節）を移動
    MoveFocus(Direction),
    /// 文節の区切りを伸縮（Shift+Left/Right）
    ResizeSegment(Direction),
    /// 複数文節を選択（Ctrl+Shift+Left/Right）
    ExtendSelection(Direction),
    /// 注目文節の学習を削除（Ctrl+Delete。誤学習の削除）
    DeleteLearning,
    /// カタカナに変換（Ctrl+K / F7）
    ToKatakana,
    /// ひらがなに変換（F6）
    ToHiragana,
    /// 半角化（カタカナ→半角カナ、または半角化可能な英数記号→半角。`nagi_rewriter` 経由）。
    /// 既定のキー割当は無く、キーマップ（`[keymap.converting]`）で明示的に割り当てる
    ToHankaku,
    /// 入力モードを設定
    SetMode(InputMode),
    /// 入力モードをトグル
    ToggleMode,
    /// 確定して キーを素通し（Ctrl/Alt/Meta 付き文字キー）
    CommitAndPassThrough,
    /// 直前の確定を取り消し、確定前の状態に戻す（Ctrl+Backspace。確定直後のみ有効）
    UndoCommit,
    /// キーを消費するだけで何もしない
    NoOp,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Insert(ch) => write!(f, "Insert({:?})", ch),
            Action::InsertText(t) => write!(f, "InsertText({:?})", t),
            Action::StartConvert => write!(f, "StartConvert"),
            Action::Predict => write!(f, "Predict"),
            Action::NextCand => write!(f, "NextCand"),
            Action::PrevCand => write!(f, "PrevCand"),
            Action::SelectCand(n) => write!(f, "SelectCand({})", n),
            Action::Commit => write!(f, "Commit"),
            Action::Cancel => write!(f, "Cancel"),
            Action::Backspace => write!(f, "Backspace"),
            Action::DeleteForward => write!(f, "DeleteForward"),
            Action::MoveCursor(Direction::Left) => write!(f, "MoveCursor(Left)"),
            Action::MoveCursor(Direction::Right) => write!(f, "MoveCursor(Right)"),
            Action::MoveCursorHome => write!(f, "MoveCursorHome"),
            Action::MoveCursorEnd => write!(f, "MoveCursorEnd"),
            Action::MoveFocus(Direction::Left) => write!(f, "MoveFocus(Left)"),
            Action::MoveFocus(Direction::Right) => write!(f, "MoveFocus(Right)"),
            Action::ResizeSegment(Direction::Left) => write!(f, "ResizeSegment(Left)"),
            Action::ResizeSegment(Direction::Right) => write!(f, "ResizeSegment(Right)"),
            Action::ExtendSelection(Direction::Left) => write!(f, "ExtendSelection(Left)"),
            Action::ExtendSelection(Direction::Right) => write!(f, "ExtendSelection(Right)"),
            Action::DeleteLearning => write!(f, "DeleteLearning"),
            Action::ToKatakana => write!(f, "ToKatakana"),
            Action::ToHiragana => write!(f, "ToHiragana"),
            Action::ToHankaku => write!(f, "ToHankaku"),
            Action::SetMode(mode) => write!(f, "SetMode({:?})", mode),
            Action::ToggleMode => write!(f, "ToggleMode"),
            Action::CommitAndPassThrough => write!(f, "CommitAndPassThrough"),
            Action::UndoCommit => write!(f, "UndoCommit"),
            Action::NoOp => write!(f, "NoOp"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_display() {
        assert_eq!(Action::Insert('a').to_string(), "Insert('a')");
        assert_eq!(Action::StartConvert.to_string(), "StartConvert");
        assert_eq!(Action::Commit.to_string(), "Commit");
    }

    #[test]
    fn test_direction() {
        assert_eq!(Direction::Left, Direction::Left);
        assert_ne!(Direction::Left, Direction::Right);
    }
}
