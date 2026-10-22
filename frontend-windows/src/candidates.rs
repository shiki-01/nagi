//! 候補ウィンドウのロジック（OS 非依存）: ページング・行モデル・寸法・表示位置。
//!
//! macOS 版 `CandidatePaging.swift` / `CandidatePanel.swift` と同じ挙動に揃える。
//! ページ送り・選択移動そのものはコアが決める（`CandidatesView.selected` が正）ので、ここは
//! 「コアの `CandidatesView` を、描画に渡せる行モデルと配置に変換する」だけを行う。
//! 文字幅の計測（GDI / DirectWrite）は [`TextMeasure`] で注入し、Win32 の描画は次の層が担う。
//!
//! 座標はすべてスクリーン座標（左上原点・y は下向き・ピクセル）。

use nagi_protocol::{CandidateSource, CandidatesView};

/// 出所アノテーションの表示名。アイコンの割り当てはフロントの責務（ここでは短い文字ラベル）
pub fn source_label(source: CandidateSource) -> &'static str {
    match source {
        CandidateSource::Learned => "学習",
        CandidateSource::UserDict => "ユーザー",
        CandidateSource::Llm => "AI",
        CandidateSource::SystemDict => "辞書",
        CandidateSource::Kana => "かな",
        CandidateSource::Rewriter => "変換",
        CandidateSource::Fuzzy => "もしかして",
    }
}

/// 複数の出所のラベル。同じラベルは 1 つにまとめ、「・」で連結する
pub fn source_labels(sources: &[CandidateSource]) -> String {
    let mut seen: Vec<&'static str> = Vec::new();
    for s in sources {
        let label = source_label(*s);
        if !seen.contains(&label) {
            seen.push(label);
        }
    }
    seen.join("・")
}

/// 候補ウィンドウの 1 行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRow {
    /// ページ内の番号（1 始まり。数字キー選択の表示用）
    pub number: u32,
    /// 全候補の中でのインデックス（`select_candidate` に渡す値）
    pub index: u32,
    /// 表示文字列
    pub surface: String,
    /// 出所ラベル（例: 「学習」「AI」）
    pub label: String,
    /// 選択中の行か
    pub is_selected: bool,
}

/// 現在ページの行モデル
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePage {
    /// 現在ページの行
    pub rows: Vec<CandidateRow>,
    /// 現在ページ（0 始まり）
    pub page_index: u32,
    /// 総ページ数
    pub page_count: u32,
    /// 全候補数
    pub total: u32,
}

impl CandidatePage {
    /// 非表示・候補なしは `None`（ウィンドウを閉じる）。選択は範囲内に丸め、`page_size` は 1 以上にする
    pub fn from_view(view: &CandidatesView) -> Option<Self> {
        if !view.visible || view.items.is_empty() {
            return None;
        }
        let total = view.items.len();
        let size = (view.page_size as usize).max(1);
        let selected = (view.selected as usize).min(total - 1);
        let page_index = selected / size;
        let start = page_index * size;
        let end = (start + size).min(total);
        let rows = (start..end)
            .map(|i| CandidateRow {
                number: (i - start + 1) as u32,
                index: i as u32,
                surface: view.items[i].surface.clone(),
                label: source_labels(&view.items[i].sources),
                is_selected: i == selected,
            })
            .collect();
        Some(Self {
            rows,
            page_index: page_index as u32,
            page_count: total.div_ceil(size) as u32,
            total: total as u32,
        })
    }

    /// ページ内の番号（1 始まり）から全候補中のインデックスを引く。範囲外は `None`
    pub fn index_for_number(&self, number: u32) -> Option<u32> {
        self.rows
            .iter()
            .find(|r| r.number == number)
            .map(|r| r.index)
    }

    /// 選択中の行
    pub fn selected_row(&self) -> Option<&CandidateRow> {
        self.rows.iter().find(|r| r.is_selected)
    }

    /// ページ表示（「2/5」）。1 ページなら `None`（フッターを出さない）
    pub fn footer_text(&self) -> Option<String> {
        (self.page_count > 1).then(|| format!("{}/{}", self.page_index + 1, self.page_count))
    }
}

/// 点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    /// x
    pub x: i32,
    /// y
    pub y: i32,
}

/// 大きさ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    /// 幅
    pub width: i32,
    /// 高さ
    pub height: i32,
}

/// 矩形（`RECT` と同じ。right / bottom は含まない端）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// 左
    pub left: i32,
    /// 上
    pub top: i32,
    /// 右
    pub right: i32,
    /// 下
    pub bottom: i32,
}

impl Rect {
    /// 位置と大きさから作る
    pub const fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            left: origin.x,
            top: origin.y,
            right: origin.x + size.width,
            bottom: origin.y + size.height,
        }
    }

    /// 幅
    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    /// 高さ
    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// 全成分が 0（キャレット位置を取得できなかったときの値）
    pub const fn is_zero(&self) -> bool {
        self.left == 0 && self.top == 0 && self.right == 0 && self.bottom == 0
    }

    /// 点を含むか
    pub const fn contains(&self, p: Point) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }
}

/// 描画に使う文字の役割（フォントの大きさが違う）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontRole {
    /// 行番号（等幅数字）
    Number,
    /// 候補文字列
    Surface,
    /// 出所ラベル・ページ表示（小さめ）
    Label,
}

/// 文字列の幅の計測（ピクセル）。GDI / DirectWrite で実装して注入する
pub trait TextMeasure {
    /// `text` を `role` のフォントで描いたときの幅
    fn width(&self, text: &str, role: FontRole) -> i32;
}

/// 寸法の定数（macOS 版の `CandidateListView` と同じ値。DPI の拡大は呼び出し側で掛ける）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    /// 行の高さ
    pub row_height: i32,
    /// 左右の余白
    pub padding: i32,
    /// 候補文字列とラベルの間隔
    pub gap: i32,
    /// 番号欄に足す余白
    pub number_extra: i32,
    /// フッター（ページ表示）の高さ。複数ページのときだけ加わる
    pub footer_height: i32,
    /// 上端から最初の行までの余白（枠線ぶん）
    pub top_inset: i32,
    /// 下端の余白（上端と合わせた縦の余白の合計 = `top_inset` + この値）
    pub bottom_inset: i32,
    /// ウィンドウ幅の下限
    pub min_width: i32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            row_height: 24,
            padding: 8,
            gap: 12,
            number_extra: 6,
            footer_height: 18,
            top_inset: 2,
            bottom_inset: 2,
            min_width: 80,
        }
    }
}

impl Metrics {
    /// DPI 倍率を掛けた寸法（96 DPI が等倍。整数に四捨五入する）
    pub fn scaled(&self, dpi: u32) -> Self {
        let s = |v: i32| scale_px(v, dpi);
        Self {
            row_height: s(self.row_height),
            padding: s(self.padding),
            gap: s(self.gap),
            number_extra: s(self.number_extra),
            footer_height: s(self.footer_height),
            top_inset: s(self.top_inset),
            bottom_inset: s(self.bottom_inset),
            min_width: s(self.min_width),
        }
    }
}

/// 96 DPI 基準の長さを `dpi` に合わせて拡大する（四捨五入。`dpi` が 0 なら等倍）
pub fn scale_px(value: i32, dpi: u32) -> i32 {
    if dpi == 0 {
        return value;
    }
    let scaled = (i64::from(value) * i64::from(dpi) + 48) / 96;
    i32::try_from(scaled).unwrap_or(value)
}

/// モニターの作業領域の列を「先頭がプライマリ」の順にする（`place_window` の前提）。
/// 入力は `(作業領域, プライマリか)`。プライマリが複数・無い場合は元の順を保つ
pub fn primary_first(monitors: &[(Rect, bool)]) -> Vec<Rect> {
    let mut out: Vec<Rect> = monitors
        .iter()
        .filter(|(_, primary)| *primary)
        .map(|(r, _)| *r)
        .collect();
    out.extend(monitors.iter().filter(|(_, p)| !*p).map(|(r, _)| *r));
    out
}

/// 候補ウィンドウの内部レイアウト
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowLayout {
    /// ウィンドウ全体の大きさ
    pub size: Size,
    /// 番号の描画 x
    pub number_x: i32,
    /// 候補文字列の描画 x
    pub surface_x: i32,
    /// ラベルの描画 x。ラベルが 1 つも無いとき（幅 0）は `None`
    pub label_x: Option<i32>,
    /// 各行の矩形（ウィンドウ内座標。選択行の塗りつぶしに使う）
    pub row_rects: Vec<Rect>,
    /// フッターの矩形（複数ページのときだけ）
    pub footer_rect: Option<Rect>,
}

/// 行モデルから寸法を計算する
pub fn layout(page: &CandidatePage, measure: &dyn TextMeasure, m: &Metrics) -> WindowLayout {
    let number_width = measure.width("0", FontRole::Number) + m.number_extra;
    let surface_width = page
        .rows
        .iter()
        .map(|r| measure.width(&r.surface, FontRole::Surface))
        .max()
        .unwrap_or(0);
    let label_width = page
        .rows
        .iter()
        .map(|r| measure.width(&r.label, FontRole::Label))
        .max()
        .unwrap_or(0);
    let mut width = m.padding + number_width + surface_width + m.padding;
    if label_width > 0 {
        width += m.gap + label_width;
    }
    width = width.max(m.min_width);

    let rows = page.rows.len() as i32;
    let footer = if page.page_count > 1 {
        m.footer_height
    } else {
        0
    };
    let height = rows * m.row_height + footer + m.top_inset + m.bottom_inset;

    let row_rects = (0..rows)
        .map(|i| {
            let top = m.top_inset + i * m.row_height;
            Rect {
                left: 2,
                top,
                right: width - 2,
                bottom: top + m.row_height,
            }
        })
        .collect();
    let footer_rect = (footer > 0).then(|| {
        let top = m.top_inset + rows * m.row_height;
        Rect {
            left: 0,
            top,
            right: width,
            bottom: top + footer,
        }
    });
    let surface_x = m.padding + number_width;
    WindowLayout {
        size: Size { width, height },
        number_x: m.padding,
        surface_x,
        label_x: (label_width > 0).then_some(surface_x + surface_width + m.gap),
        row_rects,
        footer_rect,
    }
}

/// キャレットと候補ウィンドウの間隔
pub const CARET_GAP: i32 = 2;

/// 候補ウィンドウの配置結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// ウィンドウ左上（スクリーン座標）
    pub origin: Point,
    /// キャレットの上側に反転して出したか
    pub above: bool,
}

/// 基準にするキャレット矩形。取得できなかった（`None` またはゼロ矩形）ときは
/// `fallback`（マウス位置など）の点を大きさ 0 の矩形として使う
pub fn anchor_rect(caret: Option<Rect>, fallback: Point) -> Rect {
    match caret {
        Some(r) if !r.is_zero() => r,
        _ => Rect::from_origin_size(fallback, Size::default()),
    }
}

/// ウィンドウの表示位置。既定はキャレット行の下。
/// 作業領域（タスクバーを除く）の下端に収まらなければ上側に反転し、水平は作業領域内に収める。
/// `work_areas` はモニターごとの作業領域（先頭がプライマリ）。キャレットを含むものを使い、無ければ先頭。
/// 空なら丸めない
pub fn place_window(caret: Rect, size: Size, work_areas: &[Rect]) -> Placement {
    let anchor_point = Point {
        x: caret.left,
        y: caret.top,
    };
    let below_top = caret.bottom + CARET_GAP;
    let mut origin = Point {
        x: caret.left,
        y: below_top,
    };
    let mut above = false;
    let work = work_areas
        .iter()
        .find(|w| w.contains(anchor_point))
        .or_else(|| work_areas.first());
    if let Some(w) = work {
        if below_top + size.height > w.bottom {
            let above_top = caret.top - size.height - CARET_GAP;
            if above_top >= w.top {
                origin.y = above_top;
                above = true;
            } else {
                // 上にも収まらない: 作業領域内に押し込む（上端を優先して見えるようにする）
                origin.y = below_top.min(w.bottom - size.height).max(w.top);
            }
        }
        origin.x = origin.x.min(w.right - size.width).max(w.left);
    }
    Placement { origin, above }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn scale_rounds_and_identity_at_96() {
        assert_eq!(scale_px(24, 96), 24);
        assert_eq!(scale_px(24, 144), 36);
        assert_eq!(scale_px(2, 120), 3); // 2.5 → 3
        assert_eq!(scale_px(7, 0), 7);
        let m = Metrics::default();
        assert_eq!(m.scaled(96), m);
        assert_eq!(m.scaled(192).row_height, 48);
    }

    #[test]
    fn primary_comes_first_others_keep_order() {
        let r = |l| Rect {
            left: l,
            top: 0,
            right: l + 10,
            bottom: 10,
        };
        let got = primary_first(&[(r(-10), false), (r(0), true), (r(10), false)]);
        assert_eq!(got, [r(0), r(-10), r(10)]);
        assert_eq!(primary_first(&[]), Vec::<Rect>::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nagi_protocol::CandidateView;

    fn view(n: usize, selected: u32, page_size: u32) -> CandidatesView {
        CandidatesView {
            items: (0..n)
                .map(|i| CandidateView {
                    surface: format!("候補{i}"),
                    sources: vec![CandidateSource::SystemDict],
                    covers: None,
                    reading: None,
                })
                .collect(),
            selected,
            page_size,
            visible: true,
        }
    }

    /// 全角 = 10px、半角 = 5px（数字は 6px）の固定幅
    struct Fixed;
    impl TextMeasure for Fixed {
        fn width(&self, text: &str, role: FontRole) -> i32 {
            match role {
                FontRole::Number => 6,
                _ => text
                    .chars()
                    .map(|c| if c.is_ascii() { 5 } else { 10 })
                    .sum(),
            }
        }
    }

    #[test]
    fn labels_dedupe_and_join() {
        assert_eq!(source_label(CandidateSource::Llm), "AI");
        assert_eq!(
            source_labels(&[
                CandidateSource::Learned,
                CandidateSource::Llm,
                CandidateSource::Learned
            ]),
            "学習・AI"
        );
        assert_eq!(source_labels(&[]), "");
        assert_eq!(source_label(CandidateSource::Fuzzy), "もしかして");
    }

    #[test]
    fn hidden_or_empty_closes_window() {
        let mut v = view(3, 0, 9);
        v.visible = false;
        assert!(CandidatePage::from_view(&v).is_none());
        assert!(CandidatePage::from_view(&view(0, 0, 9)).is_none());
    }

    #[test]
    fn first_page_and_numbering() {
        let p = CandidatePage::from_view(&view(20, 2, 9)).unwrap();
        assert_eq!((p.page_index, p.page_count, p.total), (0, 3, 20));
        assert_eq!(p.rows.len(), 9);
        assert_eq!(p.rows[0].number, 1);
        assert_eq!(p.rows[8].number, 9);
        assert_eq!(p.selected_row().unwrap().index, 2);
        assert_eq!(p.footer_text().as_deref(), Some("1/3"));
    }

    #[test]
    fn selection_moves_to_later_page() {
        let p = CandidatePage::from_view(&view(20, 11, 9)).unwrap();
        assert_eq!(p.page_index, 1);
        assert_eq!(p.rows.first().unwrap().index, 9);
        assert_eq!(p.rows.first().unwrap().number, 1);
        assert_eq!(p.selected_row().unwrap().number, 3);
        // 最終ページは端数
        let p = CandidatePage::from_view(&view(20, 19, 9)).unwrap();
        assert_eq!(p.rows.len(), 2);
        assert_eq!(p.footer_text().as_deref(), Some("3/3"));
    }

    #[test]
    fn selection_and_page_size_are_clamped() {
        let p = CandidatePage::from_view(&view(3, 99, 0)).unwrap();
        assert_eq!(p.rows.len(), 1);
        assert_eq!(p.rows[0].index, 2);
        assert_eq!(p.page_count, 3);
        let p = CandidatePage::from_view(&view(3, 99, 9)).unwrap();
        assert_eq!(p.selected_row().unwrap().index, 2);
    }

    #[test]
    fn number_key_maps_to_absolute_index() {
        let p = CandidatePage::from_view(&view(20, 10, 9)).unwrap();
        assert_eq!(p.index_for_number(1), Some(9));
        assert_eq!(p.index_for_number(9), Some(17));
        assert_eq!(p.index_for_number(0), None);
        let last = CandidatePage::from_view(&view(20, 19, 9)).unwrap();
        assert_eq!(last.index_for_number(2), Some(19));
        assert_eq!(last.index_for_number(3), None);
    }

    #[test]
    fn single_page_has_no_footer() {
        let p = CandidatePage::from_view(&view(5, 0, 9)).unwrap();
        assert_eq!(p.footer_text(), None);
        let l = layout(&p, &Fixed, &Metrics::default());
        assert!(l.footer_rect.is_none());
        assert_eq!(l.size.height, 5 * 24 + 4);
    }

    #[test]
    fn layout_matches_macos_formula() {
        let p = CandidatePage::from_view(&view(20, 0, 3)).unwrap();
        let l = layout(&p, &Fixed, &Metrics::default());
        // 番号 6+6=12、候補「候補0」= 25、ラベル「辞書」= 20
        assert_eq!(l.size.width, 8 + 12 + 25 + 8 + 12 + 20);
        assert_eq!(l.size.height, 3 * 24 + 18 + 4);
        assert_eq!(l.number_x, 8);
        assert_eq!(l.surface_x, 20);
        assert_eq!(l.label_x, Some(20 + 25 + 12));
        assert_eq!(l.row_rects.len(), 3);
        assert_eq!(l.row_rects[1].top, 2 + 24);
        assert_eq!(l.footer_rect.unwrap().top, 2 + 72);
    }

    #[test]
    fn layout_without_labels_uses_min_width() {
        let mut v = view(1, 0, 9);
        v.items[0].sources.clear();
        v.items[0].surface = "あ".into();
        let l = layout(
            &CandidatePage::from_view(&v).unwrap(),
            &Fixed,
            &Metrics::default(),
        );
        assert_eq!(l.label_x, None);
        assert_eq!(l.size.width, 80);
    }

    const WORK: Rect = Rect {
        left: 0,
        top: 0,
        right: 1000,
        bottom: 800,
    };
    const SIZE: Size = Size {
        width: 200,
        height: 100,
    };

    fn caret(x: i32, y: i32) -> Rect {
        Rect {
            left: x,
            top: y,
            right: x + 2,
            bottom: y + 20,
        }
    }

    #[test]
    fn placed_below_caret_by_default() {
        let p = place_window(caret(100, 100), SIZE, &[WORK]);
        assert_eq!(p.origin, Point { x: 100, y: 122 });
        assert!(!p.above);
    }

    #[test]
    fn flips_above_when_bottom_does_not_fit() {
        let p = place_window(caret(100, 750), SIZE, &[WORK]);
        assert!(p.above);
        assert_eq!(p.origin.y, 750 - 100 - CARET_GAP);
    }

    #[test]
    fn exact_fit_stays_below() {
        // 下端ちょうど（below_top 700 + 高さ 100 = 作業領域の下端 800）は反転しない
        let p = place_window(caret(0, 678), SIZE, &[WORK]);
        assert_eq!(p.origin.y, 700);
        assert!(!p.above);
    }

    #[test]
    fn clamps_horizontally() {
        let p = place_window(caret(950, 100), SIZE, &[WORK]);
        assert_eq!(p.origin.x, 800);
        let p = place_window(caret(-30, 100), SIZE, &[WORK]);
        assert_eq!(p.origin.x, 0);
    }

    #[test]
    fn neither_side_fits_pushes_into_work_area() {
        let small = Rect {
            left: 0,
            top: 0,
            right: 500,
            bottom: 90,
        };
        let p = place_window(caret(10, 40), SIZE, &[small]);
        assert!(!p.above);
        assert_eq!(p.origin.y, 0);
    }

    #[test]
    fn uses_monitor_containing_caret() {
        // 右にセカンダリ（作業領域が低い）
        let secondary = Rect {
            left: 1000,
            top: 0,
            right: 2000,
            bottom: 500,
        };
        let p = place_window(caret(1500, 450), SIZE, &[WORK, secondary]);
        assert!(p.above);
        let p = place_window(caret(1900, 100), SIZE, &[WORK, secondary]);
        assert_eq!(p.origin.x, 1800);
    }

    #[test]
    fn no_work_area_means_no_clamping() {
        let p = place_window(caret(5000, 5000), SIZE, &[]);
        assert_eq!(p.origin, Point { x: 5000, y: 5022 });
    }

    #[test]
    fn anchor_falls_back_to_mouse_when_caret_unknown() {
        let mouse = Point { x: 40, y: 60 };
        assert_eq!(anchor_rect(None, mouse).left, 40);
        assert_eq!(anchor_rect(Some(Rect::default()), mouse).top, 60);
        let c = caret(1, 2);
        assert_eq!(anchor_rect(Some(c), mouse), c);
        let a = anchor_rect(None, mouse);
        assert_eq!((a.width(), a.height()), (0, 0));
    }
}
