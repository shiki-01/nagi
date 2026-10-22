//! 未実装機能のスタブ（候補ウィンドウ・再変換）。呼び出し側の接続点だけを先に固定する。

use nagi_protocol::CandidatesView;

/// 候補ウィンドウ。
///
/// TODO: 描画は未実装。実装方針の候補:
/// (1) `ITfCandidateListUIElement` を実装して OS（Win11 の UI）に描かせる、
/// (2) 自前の Layered Window を composition の矩形（`ITfContextView::GetTextExt`）の下に出す。
/// 選択は `select_candidate` で、ページ送りはコアが決める（フロントはロジックを持たない）。
#[derive(Debug, Default)]
pub struct CandidateWindow;

impl CandidateWindow {
    /// 候補を表示（または更新）する。`None` なら隠す
    pub fn update(&self, _candidates: Option<&CandidatesView>) {
        // TODO: 候補ウィンドウの描画
    }
}

/// 再変換（`ITfFnReconversion`）。
///
/// TODO: 未実装。応答の `reconvert_selection == Some(true)` のとき、選択中のテキストを
/// 取得して `reconvert` を送り、結果の preedit で選択範囲を置き換える（macOS 版と同じ分担）。
/// 加えて `ITfFnReconversion::QueryRange` / `GetReconversion` / `Reconvert` を実装して
/// OS の再変換キー（変換キー）からも呼ばれるようにする。設計書 §5.3。
#[derive(Debug, Default)]
pub struct Reconversion;

impl Reconversion {
    /// 選択中のテキストを再変換する。未実装なので何もしない
    pub fn reconvert_selection(&self) {
        // TODO: ITfFnReconversion
    }
}
