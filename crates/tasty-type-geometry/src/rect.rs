//! `PhysicalRect` — 픽셀 좌표 사각형.

use crate::direction::SplitDirection;
use crate::length::{LogicalPx, PhysicalPx};

/// A pixel rectangle in physical (device) pixels, used for viewport/scissor calculations.
#[derive(Debug, Clone, Copy)]
pub struct PhysicalRect {
    pub x: PhysicalPx,
    pub y: PhysicalPx,
    pub width: PhysicalPx,
    pub height: PhysicalPx,
}

/// A pixel rectangle in logical (DPI-independent) pixels — [`PhysicalRect`] 의 짝.
///
/// egui 는 논리 좌표로 그리고 레이아웃은 물리 좌표로 계산되므로, 그 경계에서 사각형
/// 하나가 통째로 변환된다. 네 변을 각각 `÷ scale_factor` 하던 자리를 이 타입 하나로
/// 모아 **변환이 한 번만 일어나게** 한다 — 네 번 나누던 코드는 하나만 빠뜨려도
/// 컴파일이 통과했다.
///
/// egui 타입으로의 변환은 여기 두지 않는다. 이 crate 는 leaf 라 `egui` 에 의존하지
/// 않는다(`lib.rs` 참고) — 호출부가 경계에서 `.value()` 로 꺼낸다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: LogicalPx,
    pub y: LogicalPx,
    pub width: LogicalPx,
    pub height: LogicalPx,
}

impl LogicalRect {
    /// 논리 사각형을 물리 좌표로 올린다.
    pub fn to_physical(self, scale_factor: f32) -> PhysicalRect {
        PhysicalRect {
            x: self.x.to_physical(scale_factor),
            y: self.y.to_physical(scale_factor),
            width: self.width.to_physical(scale_factor),
            height: self.height.to_physical(scale_factor),
        }
    }
}

impl PhysicalRect {
    /// 물리 사각형을 egui 가 쓰는 논리 좌표로 내린다.
    pub fn to_logical(self, scale_factor: f32) -> LogicalRect {
        LogicalRect {
            x: self.x.to_logical(scale_factor),
            y: self.y.to_logical(scale_factor),
            width: self.width.to_logical(scale_factor),
            height: self.height.to_logical(scale_factor),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 왕복이 상쇄되지 않으면 두 좌표계 사이를 오갈 때마다 사각형이 조금씩 움직인다.
    /// 네 변을 손으로 나누던 시절에는 이 성질을 확인할 자리 자체가 없었다.
    #[test]
    fn physical_and_logical_round_trip_cancels() {
        let physical = PhysicalRect {
            x: PhysicalPx(120.0),
            y: PhysicalPx(48.0),
            width: PhysicalPx(640.0),
            height: PhysicalPx(360.0),
        };
        for sf in [1.0_f32, 1.5, 2.0, 3.0] {
            let back = physical.to_logical(sf).to_physical(sf);
            assert!(
                back.approx_eq(&physical),
                "sf={sf} 에서 왕복이 어긋난다: {back:?}"
            );
        }
    }

    #[test]
    fn to_logical_divides_every_side_by_the_scale_factor() {
        let l = PhysicalRect {
            x: PhysicalPx(100.0),
            y: PhysicalPx(200.0),
            width: PhysicalPx(300.0),
            height: PhysicalPx(400.0),
        }
        .to_logical(2.0);
        assert_eq!(l.x, LogicalPx(50.0));
        assert_eq!(l.y, LogicalPx(100.0));
        assert_eq!(l.width, LogicalPx(150.0));
        assert_eq!(l.height, LogicalPx(200.0));
    }
}
