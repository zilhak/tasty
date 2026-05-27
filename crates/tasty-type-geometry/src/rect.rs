//! `PhysicalRect` — 픽셀 좌표 사각형.

use crate::direction::SplitDirection;
use crate::length::PhysicalPx;

/// A pixel rectangle in physical (device) pixels, used for viewport/scissor calculations.
#[derive(Debug, Clone, Copy)]
pub struct PhysicalRect {
    pub x: PhysicalPx,
    pub y: PhysicalPx,
    pub width: PhysicalPx,
    pub height: PhysicalPx,
}

impl PhysicalRect {
    /// Check if a point (x, y) is inside this rectangle.
    pub fn contains(&self, x: PhysicalPx, y: PhysicalPx) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Check if two rects are approximately equal (within 1px tolerance).
    pub fn approx_eq(&self, other: &PhysicalRect) -> bool {
        (self.x - other.x).abs() < PhysicalPx(1.0)
            && (self.y - other.y).abs() < PhysicalPx(1.0)
            && (self.width - other.width).abs() < PhysicalPx(1.0)
            && (self.height - other.height).abs() < PhysicalPx(1.0)
    }

    /// 두 영역으로 분할. `gap` 은 두 분할 사이 시각적 보더 두께 (분할 자식의 *합* 에서 빠짐).
    /// 호출자가 명시적으로 gap 을 제공해야 한다 (이전 split() default 인자 제거 — 도메인 상수
    /// 의존을 type-geometry 에서 끊기 위해).
    pub fn split_with_gap(
        self,
        direction: SplitDirection,
        ratio: f32,
        gap: PhysicalPx,
    ) -> (PhysicalRect, PhysicalRect) {
        match direction {
            SplitDirection::Vertical => {
                let usable = (self.width - gap).max(PhysicalPx(0.0));
                let first_w = (usable * ratio).floor();
                let second_w = usable - first_w;
                (
                    PhysicalRect {
                        x: self.x,
                        y: self.y,
                        width: first_w,
                        height: self.height,
                    },
                    PhysicalRect {
                        x: self.x + first_w + gap,
                        y: self.y,
                        width: second_w,
                        height: self.height,
                    },
                )
            }
            SplitDirection::Horizontal => {
                let usable = (self.height - gap).max(PhysicalPx(0.0));
                let first_h = (usable * ratio).floor();
                let second_h = usable - first_h;
                (
                    PhysicalRect {
                        x: self.x,
                        y: self.y,
                        width: self.width,
                        height: first_h,
                    },
                    PhysicalRect {
                        x: self.x,
                        y: self.y + first_h + gap,
                        width: self.width,
                        height: second_h,
                    },
                )
            }
        }
    }
}

/// 분할 보더 (divider) 정보. 사용자가 hover 시 그릴 영역.
#[derive(Debug, Clone, Copy)]
pub struct DividerInfo {
    /// The direction of the split this divider belongs to.
    pub direction: SplitDirection,
    /// The rect of the parent split node that owns this divider.
    pub split_rect: PhysicalRect,
}
