//! 플러그인에 보내는 Scroll은 논리 포인트다.
//! surface의 winit 입력과 popup·banner의 egui 입력을 같은 배율로 환산한다.
//! Line의 길이는 호스트가 설정한 Options::line_scroll_speed에서 읽는다.
//! 기본값은 tasty_settings::DEFAULT_WHEEL_LINE_SCROLL이며 별도 상수를 두지 않는다.

use egui::{MouseWheelUnit, Vec2};
use tasty_type_geometry::length::LogicalPx;

/// 현재 egui 컨텍스트에서 휠 한 줄에 해당하는 논리 포인트 길이를 읽는다.
pub(crate) fn line_scroll(ctx: &egui::Context) -> LogicalPx {
    LogicalPx(ctx.options(|o| o.line_scroll_speed))
}

/// 휠 델타를 논리 포인트로 변환한다. line은 한 줄, page는 한 페이지의 길이다.
/// 호출자는 line_scroll의 값과 화면 높이를 넘긴다. Page는 두 축에 같은 길이를 사용한다.
pub(crate) fn wheel_delta_to_points(
    unit: MouseWheelUnit,
    delta: Vec2,
    page: LogicalPx,
    line: LogicalPx,
) -> (LogicalPx, LogicalPx) {
    let per_unit = match unit {
        MouseWheelUnit::Point => LogicalPx(1.0),
        MouseWheelUnit::Line => line,
        MouseWheelUnit::Page => page,
    };
    (per_unit * delta.x, per_unit * delta.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_wheel_notch_moves_the_same_distance_on_both_paths() {
        let notch = LogicalPx(tasty_settings::DEFAULT_WHEEL_LINE_SCROLL);
        let surface_dy = notch * 1.0;
        let (_, popup_dy) = wheel_delta_to_points(
            MouseWheelUnit::Line,
            Vec2::new(0.0, 1.0),
            LogicalPx(800.0),
            notch,
        );
        assert_eq!(popup_dy, surface_dy);
        assert_eq!(popup_dy, LogicalPx(50.0));

        let (_, popup_up) = wheel_delta_to_points(
            MouseWheelUnit::Line,
            Vec2::new(0.0, -1.0),
            LogicalPx(800.0),
            notch,
        );
        assert_eq!(popup_up, notch * -1.0);
    }

    #[test]
    fn the_notch_distance_follows_its_argument() {
        let (_, slow) = wheel_delta_to_points(
            MouseWheelUnit::Line,
            Vec2::new(0.0, 1.0),
            LogicalPx(800.0),
            LogicalPx(20.0),
        );
        let (_, fast) = wheel_delta_to_points(
            MouseWheelUnit::Line,
            Vec2::new(0.0, 1.0),
            LogicalPx(800.0),
            LogicalPx(120.0),
        );
        assert_eq!(slow, LogicalPx(20.0));
        assert_eq!(fast, LogicalPx(120.0));
    }

    #[test]
    fn the_notch_distance_does_not_leak_into_the_other_units() {
        let (_, point) = wheel_delta_to_points(
            MouseWheelUnit::Point,
            Vec2::new(0.0, 7.0),
            LogicalPx(800.0),
            LogicalPx(120.0),
        );
        let (_, page) = wheel_delta_to_points(
            MouseWheelUnit::Page,
            Vec2::new(0.0, 1.0),
            LogicalPx(800.0),
            LogicalPx(120.0),
        );
        assert_eq!(point, LogicalPx(7.0));
        assert_eq!(page, LogicalPx(800.0));
    }

    #[test]
    fn point_unit_passes_through_unscaled() {
        let (dx, dy) = wheel_delta_to_points(
            MouseWheelUnit::Point,
            Vec2::new(3.5, -12.0),
            LogicalPx(800.0),
            LogicalPx(tasty_settings::DEFAULT_WHEEL_LINE_SCROLL),
        );
        assert_eq!(dx, LogicalPx(3.5));
        assert_eq!(dy, LogicalPx(-12.0));
    }

    #[test]
    fn page_unit_scales_both_axes_by_the_page_length() {
        let (dx, dy) = wheel_delta_to_points(
            MouseWheelUnit::Page,
            Vec2::new(1.0, -2.0),
            LogicalPx(600.0),
            LogicalPx(tasty_settings::DEFAULT_WHEEL_LINE_SCROLL),
        );
        assert_eq!(dx, LogicalPx(600.0));
        assert_eq!(dy, LogicalPx(-1200.0));
    }

    #[test]
    fn line_unit_is_no_longer_delivered_as_one_point() {
        let raw_delta = Vec2::new(0.0, 1.0);
        let (_, converted) = wheel_delta_to_points(
            MouseWheelUnit::Line,
            raw_delta,
            LogicalPx(800.0),
            LogicalPx(tasty_settings::DEFAULT_WHEEL_LINE_SCROLL),
        );
        assert_eq!(LogicalPx(raw_delta.y), LogicalPx(1.0));
        assert_eq!(converted, LogicalPx(50.0));
    }
}

// 환산 함수에 직접 인자를 넣는 시험과 달리, 각 호출 경로가 컨텍스트의 옵션을 읽는지 검사한다.
#[cfg(test)]
mod one_notch_per_context {
    use super::*;
    use egui::{Event, Modifiers, Pos2, RawInput, Rect, pos2, vec2};

    // 호스트 기본값 50과 egui 기본값 40을 하드코딩한 경우와 구별할 값.
    const NOTCH: f32 = 77.0;

    fn wheel_input(modifiers: Modifiers, pointer: Option<Pos2>) -> RawInput {
        let mut events = Vec::new();
        if let Some(p) = pointer {
            events.push(Event::PointerMoved(p));
        }
        events.push(Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            modifiers,
        });
        RawInput {
            events,
            modifiers,
            ..Default::default()
        }
    }

    #[test]
    fn egui_moves_a_host_scroll_area_by_the_option_value() {
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.line_scroll_speed = NOTCH);
        let mut seen = f32::NAN;
        // 렌더 출력 대신 컨텍스트에서 읽은 스크롤 거리를 확인한다.
        let _frame = ctx.run(wheel_input(Modifiers::NONE, None), |ctx| {
            seen = ctx.input(|i| i.raw_scroll_delta.y);
        });
        assert_eq!(seen, NOTCH, "egui 가 옵션이 아닌 다른 값으로 스크롤한다");
    }

    #[test]
    fn the_wire_conversion_takes_its_notch_from_the_same_context() {
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.line_scroll_speed = NOTCH);
        assert_eq!(line_scroll(&ctx), LogicalPx(NOTCH));
        let (_, dy) = wheel_delta_to_points(
            MouseWheelUnit::Line,
            vec2(0.0, 1.0),
            LogicalPx(800.0),
            line_scroll(&ctx),
        );
        assert_eq!(
            dy,
            LogicalPx(NOTCH),
            "플러그인에 전달한 스크롤 거리가 호스트 위젯과 다르다"
        );
    }

    #[test]
    fn the_modifier_hint_overlay_takes_its_notch_from_the_same_context() {
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.line_scroll_speed = NOTCH);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let mods = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        let mut seen = f32::NAN;
        let _frame = ctx.run(wheel_input(mods, Some(pos2(10.0, 10.0))), |ctx| {
            seen = crate::adapters::ui::modifier_hint_overlay::modifier_free_wheel_y(ctx, rect);
        });
        assert_eq!(seen, NOTCH, "overlay 가 다른 거리로 스크롤한다");
    }
}
