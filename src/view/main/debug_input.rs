//! debug 전용 포인터·키·문자 입력 주입. release에는 없다.
//! PTY 보고 문자열을 넣는 debug.inject_mouse와 달리 실제 MainView·egui 입력 경로를 사용한다.

#![cfg(debug_assertions)]

use crate::model::PhysicalPx;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use super::MainView;

/// 주입할 포인터 동작 종류.
pub(crate) enum InjectPointer {
    Move,
    Button { button: MouseButton, pressed: bool },
    Scroll { dx: f32, dy: f32, unit: ScrollUnit },
}

/// 휠 단위. 장치별 변환 경로를 각각 확인할 수 있도록 줄·포인트·페이지를 구분한다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ScrollUnit {
    /// 줄 수. 데스크톱 마우스 휠 한 칸이 1.0.
    Line,
    /// 포인트. winit 레벨로 넣을 때는 **물리 픽셀**이다(`PixelDelta` 가 물리 px 이고
    /// 수신 측이 scale factor 로 나눈다) — egui 레벨로 넣을 때는 논리 포인트다.
    Point,
    /// 페이지. egui 는 이 단위를 다루지만 egui-winit 이 데스크톱에서 만들지 않으므로
    /// winit 레벨에는 대응하는 델타가 없다.
    Page,
}

impl ScrollUnit {
    /// 단위 이름을 해석한다. 모르는 값은 거절한다.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "line" => Some(Self::Line),
            "point" => Some(Self::Point),
            "page" => Some(Self::Page),
            _ => None,
        }
    }

    /// egui 입력 큐에 넣을 단위.
    fn to_egui(self) -> egui::MouseWheelUnit {
        match self {
            Self::Line => egui::MouseWheelUnit::Line,
            Self::Point => egui::MouseWheelUnit::Point,
            Self::Page => egui::MouseWheelUnit::Page,
        }
    }

    /// winit 핸들러에 넣을 델타. `Page` 는 winit 에 표현이 없어 `None` — 그 경우
    /// 주입은 성공한 척하지 않고 거절된다.
    fn to_winit_delta(self, dx: f32, dy: f32) -> Option<MouseScrollDelta> {
        match self {
            Self::Line => Some(MouseScrollDelta::LineDelta(dx, dy)),
            Self::Point => Some(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                dx as f64, dy as f64,
            ))),
            Self::Page => None,
        }
    }
}

impl MainView {
    /// surface_id 영역의 정규화 좌표 (fx, fy ∈ [0,1]) 에 포인터 동작을 주입한다.
    ///
    /// surface 의 물리 rect 를 찾아 window-global 물리 좌표로 환산한 뒤, 실제
    /// WindowEvent 가 가는 것과 동일한 MainView 핸들러를 호출한다. surface 가 layout
    /// 에 없으면 `false`.
    pub(crate) fn debug_inject_mesh_pointer(
        &mut self,
        surface_id: u32,
        fx: f32,
        fy: f32,
        action: InjectPointer,
    ) -> bool {
        let terminal_rect = self.compute_terminal_rect();
        let Some(rect) = self.state.surface_rect_by_id(
            &self.core_state,
            surface_id,
            terminal_rect,
            self.base.gpu.scale_factor(),
        ) else {
            return false;
        };
        let x = rect.x.value() + fx * rect.width.value();
        let y = rect.y.value() + fy * rect.height.value();
        let pos = PhysicalPosition::new(x as f64, y as f64);
        // cursor_position 은 handle_mouse_input/wheel 의 hit-test 출발점이라 먼저 갱신.
        self.cursor_position = Some(pos);

        match action {
            InjectPointer::Move => self.handle_cursor_moved(pos, false),
            InjectPointer::Button { button, pressed } => {
                // 이전 메뉴 결과를 지우고 이번 요청을 OS가 표시하기 전에 debug 슬롯으로 옮긴다.
                self.debug_captured_menu = None;
                let state = if pressed {
                    ElementState::Pressed
                } else {
                    ElementState::Released
                };
                // handle_event를 거치지 않아 메뉴 닫기 입력 판정을 직접 호출한다.
                let swallow = self.take_menu_dismiss_swallow(state, button);
                self.handle_mouse_input(state, button, false, swallow);
                if let Some(menu) = self.state.dialogs.pending_native_menu.take() {
                    self.debug_captured_menu = Some(menu);
                }
            }
            InjectPointer::Scroll { dx, dy, unit } => {
                let Some(delta) = unit.to_winit_delta(dx, dy) else {
                    return false;
                };
                self.handle_mouse_wheel(delta, false);
            }
        }
        true
    }

    /// 정규화 좌표를 egui의 다음 프레임 입력에 넣어 plugin 팝업 전달을 검사한다.
    pub(crate) fn debug_inject_egui_pointer(
        &mut self,
        fx: f32,
        fy: f32,
        surface_id: Option<u32>,
        action: InjectPointer,
    ) -> bool {
        let ppp = self.base.gpu.egui_pixels_per_point().max(f32::EPSILON);
        // surface를 지정했으면 그 영역, 아니면 창 전체에 대한 정규화 좌표다.
        let pos = if let Some(sid) = surface_id {
            let terminal_rect = self.compute_terminal_rect();
            let Some(rect) = self.state.surface_rect_by_id(
                &self.core_state,
                sid,
                terminal_rect,
                self.base.gpu.scale_factor(),
            ) else {
                return false;
            };
            let point = PhysicalPx(rect.x.value() + fx * rect.width.value());
            let line = PhysicalPx(rect.y.value() + fy * rect.height.value());
            egui::pos2(point.to_logical(ppp).value(), line.to_logical(ppp).value())
        } else {
            let (w, h) = self.base.gpu.surface_config_size();
            let logical_w = PhysicalPx(w as f32).to_logical(ppp).value();
            let logical_h = PhysicalPx(h as f32).to_logical(ppp).value();
            egui::pos2(logical_w * fx, logical_h * fy)
        };
        // 이번 입력이 메뉴를 만들지 않았을 때 이전 결과가 남지 않게 한다.
        self.debug_captured_menu = None;
        let events = match action {
            // 클릭 전 hover 를 같은 pos 로 세팅해야 plugin egui 가 위젯 hit-test 를 맞춘다.
            InjectPointer::Move => vec![egui::Event::PointerMoved(pos)],
            InjectPointer::Button { button, pressed } => vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: map_egui_button(button),
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            InjectPointer::Scroll { dx, dy, unit } => vec![egui::Event::MouseWheel {
                unit: unit.to_egui(),
                delta: egui::vec2(dx, dy),
                modifiers: egui::Modifiers::default(),
            }],
        };
        self.base.gpu.debug_push_egui_events(events);
        true
    }

    /// 키 이벤트를 egui 입력 큐로 주입한다(popup Esc 등 검증용). 매핑 불가 키면 `false`.
    pub(crate) fn debug_inject_egui_key(&mut self, key_name: &str, pressed: bool) -> bool {
        let Some(key) = egui::Key::from_name(key_name) else {
            return false;
        };
        self.base.gpu.debug_push_egui_events(vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }]);
        true
    }

    /// 문자열 전체를 한 egui Text 이벤트로 넣는다. 빈 값·제어문자 등 실입력에서
    /// 전달되지 않는 문자열은 거절한다. Enter·Tab 같은 키 동작은 키 주입을 사용한다.
    pub(crate) fn debug_inject_egui_text(&mut self, text: &str) -> bool {
        if !text_reaches_egui_as_typed(text) {
            return false;
        }
        self.base
            .gpu
            .debug_push_egui_events(vec![egui::Event::Text(text.to_string())]);
        true
    }
}

/// egui-winit의 문자 판정을 따르고 TextEdit이 무시하는 빈 값은 제외한다.
/// 상류 함수가 비공개여서 같은 조건을 여기서 검사한다.
fn text_reaches_egui_as_typed(text: &str) -> bool {
    !text.is_empty() && text.chars().all(is_printable_char)
}

/// `egui-winit` 의 동명 판정을 옮겨 적은 것.
fn is_printable_char(chr: char) -> bool {
    let is_in_private_use_area = ('\u{e000}'..='\u{f8ff}').contains(&chr)
        || ('\u{f0000}'..='\u{ffffd}').contains(&chr)
        || ('\u{100000}'..='\u{10fffd}').contains(&chr);
    !is_in_private_use_area && !chr.is_ascii_control()
}

/// winit 마우스 버튼 → egui 포인터 버튼 (debug 주입용).
fn map_egui_button(button: MouseButton) -> egui::PointerButton {
    match button {
        MouseButton::Right => egui::PointerButton::Secondary,
        MouseButton::Middle => egui::PointerButton::Middle,
        _ => egui::PointerButton::Primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unit_reaches_egui_as_itself() {
        assert_eq!(ScrollUnit::Line.to_egui(), egui::MouseWheelUnit::Line);
        assert_eq!(ScrollUnit::Point.to_egui(), egui::MouseWheelUnit::Point);
        assert_eq!(ScrollUnit::Page.to_egui(), egui::MouseWheelUnit::Page);
    }

    /// winit 레벨은 두 갈래뿐이다 — 줄은 `LineDelta`, 포인트는 `PixelDelta`.
    #[test]
    fn winit_level_maps_line_and_point_to_the_two_winit_deltas() {
        assert!(matches!(
            ScrollUnit::Line.to_winit_delta(1.0, -3.0),
            Some(MouseScrollDelta::LineDelta(1.0, -3.0))
        ));
        let Some(MouseScrollDelta::PixelDelta(p)) = ScrollUnit::Point.to_winit_delta(1.0, -3.0)
        else {
            panic!("point 는 PixelDelta 여야 한다");
        };
        assert_eq!((p.x, p.y), (1.0, -3.0));
    }

    #[test]
    fn winit_level_refuses_page_instead_of_folding_it_into_lines() {
        assert!(ScrollUnit::Page.to_winit_delta(0.0, 1.0).is_none());
    }

    #[test]
    fn unknown_unit_names_are_rejected_not_defaulted() {
        assert_eq!(ScrollUnit::from_name("line"), Some(ScrollUnit::Line));
        assert_eq!(ScrollUnit::from_name("point"), Some(ScrollUnit::Point));
        assert_eq!(ScrollUnit::from_name("page"), Some(ScrollUnit::Page));
        assert_eq!(ScrollUnit::from_name("Line"), None);
        assert_eq!(ScrollUnit::from_name("lines"), None);
        assert_eq!(ScrollUnit::from_name(""), None);
    }

    #[test]
    fn text_that_the_text_edit_drops_is_refused_instead_of_reported_as_injected() {
        assert!(!text_reaches_egui_as_typed(""));
        assert!(!text_reaches_egui_as_typed("\n"));
        assert!(!text_reaches_egui_as_typed("\r"));
    }

    #[test]
    fn characters_the_real_path_never_carries_are_refused() {
        assert!(!text_reaches_egui_as_typed("git\tstatus"));
        assert!(!text_reaches_egui_as_typed("\u{1b}"));
        assert!(!text_reaches_egui_as_typed("\u{e000}"));
    }

    #[test]
    fn what_a_person_types_passes() {
        assert!(text_reaches_egui_as_typed("theme"));
        assert!(text_reaches_egui_as_typed("새 탭"));
        assert!(text_reaches_egui_as_typed(" "));
        assert!(text_reaches_egui_as_typed("--force"));
    }
}
