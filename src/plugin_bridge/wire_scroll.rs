//! 와이어 `Scroll` 의 단위 규약과 그 환산 — egui-mesh 로 가는 휠 입력의 단일 출처.
//!
//! `RawInputEventWire::Scroll` 은 **논리 포인트**를 나른다. plugin SDK 가 받은 값을
//! `MouseWheelUnit::Point` 인 `MouseWheel` 로 그대로 egui 에 넣기 때문이다
//! (`crates/tasty-plugin-sdk/src/egui_surface.rs` 의 `push_scroll_events` / `wheel_event`).
//! 그래서 host 쪽 수집 지점은 자기 입력 소스의 단위를 포인트로 맞춘 뒤 보내야 한다.
//!
//! 수집 지점은 둘이고 입력 소스의 표현이 다르다:
//!
//! - **egui-mesh surface** (`src/view/main/mouse.rs` 의 `handle_mouse_wheel`) — winit
//!   `MouseScrollDelta` 를 직접 받는다. `LineDelta` 는 줄 수, `PixelDelta` 는 물리 픽셀.
//! - **popup / banner** (`popup_render.rs` · `banner_render.rs`) — host egui 가 한 번
//!   가공한 `Event::MouseWheel { unit, delta }` 를 받는다. egui-winit 이 winit 의
//!   `LineDelta` 를 `Line`(줄 수 그대로), `PixelDelta` 를 `Point`(÷ pixels_per_point)
//!   로 옮긴 것이라, 두 소스는 같은 물리 입력의 다른 표현일 뿐이다.
//!
//! 두 경로가 다른 배율을 쓰면 **같은 휠 한 칸이 표면 종류에 따라 다른 거리를 스크롤**한다
//! (`docs/architecture/input-layer.md` 의 입력 계층 일관성). 그래서 줄 → 포인트 배율을
//! 한 값으로 모으고 위 세 지점(surface · popup · banner)이 모두 그것을 읽는다.
//!
//! **줄 → 포인트 배율의 런타임 단일 출처는 egui `Options::line_scroll_speed` 다**
//! (ADR-0130). 그 옵션은 host 가 설정값(`GeneralSettings::wheel_line_scroll`)으로 직접
//! 채우고, 여기의 [`line_scroll`] 이 그것을 읽는다. 같은 옵션을 host egui 의
//! `ScrollArea` 전반과 `modifier_hint_overlay::modifier_free_wheel_y` 도 읽으므로,
//! plugin 표면과 host UI 가 한 값을 공유한다 — 표면 종류로 갈리지 않는다.
//!
//! 그래서 이 모듈은 배율 상수를 갖지 않는다. 기본값의 정의처는 설정 쪽
//! ([`tasty_settings::DEFAULT_WHEEL_LINE_SCROLL`])이고, 실행 중 값은 언제나 옵션이다.

use egui::{MouseWheelUnit, Vec2};
use tasty_type_geometry::length::LogicalPx;

/// 이 egui 컨텍스트가 지금 쓰는 휠 1 notch(= `MouseWheelUnit::Line` 1 줄 = winit
/// `LineDelta` 의 1.0) 거리. **런타임 단일 출처** — host 가 설정값을 이 옵션에 밀어
/// 넣으므로(ADR-0130), 여기서 읽으면 host 위젯이 스크롤하는 거리와 정확히 같다.
///
/// 상수로 박지 않는 이유가 그것이다. 종전에는 이 값이 상수 50 이었고 host egui 쪽은
/// 40 이라, 같은 창에서 표면 종류에 따라 휠 한 칸의 거리가 달랐다.
pub(crate) fn line_scroll(ctx: &egui::Context) -> LogicalPx {
    LogicalPx(ctx.options(|o| o.line_scroll_speed))
}

/// host egui 의 `Event::MouseWheel` 델타를 와이어 `Scroll` 이 요구하는 논리 포인트로 환산한다.
///
/// `line` 은 `Line` 단위 1 줄의 길이 — 호출자가 [`line_scroll`] 로 뽑아 넘긴다.
/// `page` 는 `Page` 단위 1 페이지의 길이다. egui 자신(`InputState::begin_pass`)과
/// `modifier_hint_overlay::modifier_free_wheel_y` 가 모두 **화면 높이**를 쓰고 가로·세로
/// 양축에 같은 값을 곱하므로 여기서도 같은 규칙을 따른다. 다만 egui-winit 은 데스크톱에서
/// `Line` 과 `Point` 만 만들어 내므로(`LineDelta` / `PixelDelta` 두 갈래) 이 갈래는 실제로
/// 도달하지 않는다 — 단위를 빠짐없이 다루기 위한 것이다.
pub(crate) fn wheel_delta_to_points(
    unit: MouseWheelUnit,
    delta: Vec2,
    page: LogicalPx,
    line: LogicalPx,
) -> (LogicalPx, LogicalPx) {
    // 단위 1.0 당 논리 포인트. `Point` 는 이미 포인트라 1:1.
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

    /// 마우스 휠 1 notch 는 어느 경로로 들어오든 같은 거리를 옮겨야 한다.
    ///
    /// - surface 경로: winit `LineDelta(0, ±1)` × 노치 거리
    /// - popup / banner 경로: egui `Line` 단위 델타 ±1 → 이 함수
    ///
    /// egui-winit 이 winit `LineDelta` 를 `Line` 단위로 값 그대로 옮기므로(줄 수 보존),
    /// 두 식의 입력은 같은 수이고 결과도 같아야 한다.
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

        // 아래로 굴리는 방향(음수)도 부호까지 같다.
        let (_, popup_up) = wheel_delta_to_points(
            MouseWheelUnit::Line,
            Vec2::new(0.0, -1.0),
            LogicalPx(800.0),
            notch,
        );
        assert_eq!(popup_up, notch * -1.0);
    }

    /// 노치 거리는 상수가 아니라 인자다 — 설정이 바뀌면 환산도 따라간다. 상수로 박혀
    /// 있으면 설정을 바꿔도 plugin 표면만 옛 값에 머물러 갈래가 되살아난다.
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

    /// `line` 인자는 `Line` 갈래에만 걸린다 — 트랙패드(Point)나 Page 를 함께 늘리지 않는다.
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

    /// 트랙패드(winit `PixelDelta` → egui `Point`)는 이미 논리 포인트라 그대로 통과한다 —
    /// surface 경로의 `PixelDelta` ÷ pixels_per_point 와 같은 값이다(egui-winit 이 그
    /// 나눗셈을 이미 했다).
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

    /// `Page` 는 양축 모두 화면 높이를 곱한다(egui `InputState::begin_pass` 와 같은 규칙).
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

    /// 환산 전 동작(단위를 버리고 델타를 그대로 전달)과의 차이를 수치로 고정한다 —
    /// 물리 마우스 휠 1 notch 가 1pt 에서 노치 거리로 바뀐다.
    #[test]
    fn line_unit_is_no_longer_delivered_as_one_point() {
        let raw_delta = Vec2::new(0.0, 1.0); // 환산 전에는 이 값이 그대로 와이어에 실렸다.
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

/// 노치 거리가 **자리마다 재도 같은가** — ADR-0130 의 결정을 그 자리에서 확인한다.
///
/// 위의 단위 테스트들은 [`wheel_delta_to_points`] 의 산술만 본다: 인자로 준 노치를
/// 따르는가. 그것은 **호출 자리가 그 노치를 어디서 얻는지는 말하지 않는다.** 상수를
/// 다시 박아도 그 테스트들은 초록이다 — 자기가 인자를 주기 때문이다.
///
/// 그래서 여기서는 인자를 주지 않는다. egui 컨텍스트 하나에 **기본값도 egui 기본값도
/// 아닌 값**을 심고, 각 경로가 그 컨텍스트에서 값을 실제로 집어 오는지를 잰다.
#[cfg(test)]
mod one_notch_per_context {
    use super::*;
    use egui::{Event, Modifiers, Pos2, RawInput, Rect, pos2, vec2};

    /// 기본값 50 도 egui native 기본값 40 도 아닌 값. 어느 쪽을 상수로 박아도
    /// 이 수와 어긋나므로 그 자리가 드러난다.
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

    /// egui 자신이 host `ScrollArea` 를 움직이는 거리 = 심어 둔 옵션값.
    /// 이 줄이 기준점이다 — 아래 두 경로는 여기에 맞춰야 한다.
    #[test]
    fn egui_moves_a_host_scroll_area_by_the_option_value() {
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.line_scroll_speed = NOTCH);
        let mut seen = f32::NAN;
        // `run` 의 FullOutput 은 그리지 않으므로 쓰지 않는다 — 여기서 재는 것은
        // 그 프레임 동안 각 경로가 컨텍스트에서 읽어 낸 값이다.
        let _frame = ctx.run(wheel_input(Modifiers::NONE, None), |ctx| {
            seen = ctx.input(|i| i.raw_scroll_delta.y);
        });
        assert_eq!(seen, NOTCH, "egui 가 옵션이 아닌 다른 값으로 스크롤한다");
    }

    /// plugin 표면(surface · popup · banner)이 와이어에 싣는 거리.
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
            "plugin 표면이 host 위젯과 다른 거리를 받는다"
        );
    }

    /// modifier hint overlay — egui 가 세로 휠을 zoom/가로로 전용하는 동안 대신 읽는 자리.
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
