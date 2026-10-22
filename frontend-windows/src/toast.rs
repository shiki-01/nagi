//! 通知パネル（トースト）の配置と表示時間（OS 非依存）。描画は Win32 層が行う。
//!
//! macOS 版は画面の右上に小パネルを出すが、Windows の通知は右下が慣例なので作業領域の右下に置く
//! （decisions.md H-41）。

use std::time::Duration;

use crate::candidates::{Point, Rect, Size};

/// 通知を出しておく時間
pub const TOAST_DURATION: Duration = Duration::from_secs(4);

/// 通知の幅の上限（96 DPI 基準。超える文言は折り返す）
pub const TOAST_MAX_WIDTH: i32 = 420;

/// 作業領域の端からの余白（96 DPI 基準）
pub const TOAST_MARGIN: i32 = 16;

/// `work`（作業領域）の右下に `size` の通知を置く左上位置。収まらない場合は作業領域の左上に寄せる
pub fn place_toast(work: Rect, size: Size, margin: i32) -> Point {
    Point {
        x: (work.right - margin - size.width).max(work.left),
        y: (work.bottom - margin - size.height).max(work.top),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_right_with_margin() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let p = place_toast(
            work,
            Size {
                width: 300,
                height: 60,
            },
            16,
        );
        assert_eq!(p, Point { x: 1604, y: 964 });
    }

    #[test]
    fn oversized_is_clamped_to_work_area() {
        let work = Rect {
            left: 100,
            top: 50,
            right: 200,
            bottom: 100,
        };
        let p = place_toast(
            work,
            Size {
                width: 500,
                height: 500,
            },
            16,
        );
        assert_eq!(p, Point { x: 100, y: 50 });
    }
}
