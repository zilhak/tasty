//! debug 전용 window 레벨 포인터 주입 (입력 재현, release 미노출).
//!
//! 불가침 원칙 1·3: 사용자 입력 재현(키/마우스 주입)은 release 에 없고
//! `#[cfg(debug_assertions)]` debug 격리로만 존재한다(`docs/dev-guide/debug-ipc.md`).
//! 본 모듈은 egui-mesh 입력 forward(A1-S7)를 헤드리스로 자체 검증하기 위한 debug
//! 표면이다 — 기존 `debug.inject_mouse`(터미널 PTY SGR 리포트 주입)와 달리 winit
//! 레벨 포인터 이벤트를 MainView 핸들러로 흘려, `handle_mouse_input`/`handle_mouse_wheel`/
//! `handle_cursor_moved` 의 실제 라우팅(egui-mesh 캡처 포함)을 그대로 탄다.

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

/// 주입할 휠 델타의 단위 — 어느 장치의 휠을 흉내 낼지 고른다.
///
/// 실제 입력에서 데스크톱 마우스 휠은 winit `LineDelta` → egui `Line` 로 오고,
/// 트랙패드 같은 픽셀 장치는 `PixelDelta` → `Point` 로 온다. 두 갈래는 논리 포인트로
/// 가는 배율이 다르고(`src/plugin_bridge/wire_scroll.rs`), 그 환산이 표면 종류마다
/// 어긋나는 것이 실제로 있었던 결함이다. 한 단위만 합성할 수 있으면 다른 단위의
/// 환산 경로는 주입으로 재현되지 않는다.
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
    /// IPC 파라미터 문자열 → 단위. 모르는 값은 `None` 이다 — 오타를 기본값으로
    /// 삼키면 테스트가 의도한 것과 다른 경로를 재고도 통과한다.
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
                // 이 주입이 세우는 메뉴만 관찰하도록 직전 포획본을 비운 뒤 핸들러 실행,
                // 실행 직후 live pending_native_menu 를 debug 슬롯으로 가로챈다. redraw 가
                // 실제 native 팝업(macOS/Windows 는 블로킹 모달)으로 소비하기 전에
                // 옮겨야 테스트가 멈추거나 팝업이 뜬 채 남지 않는다.
                self.debug_captured_menu = None;
                let state = if pressed {
                    ElementState::Pressed
                } else {
                    ElementState::Released
                };
                // 이 경로는 `handle_event` 를 거치지 않으므로 dismiss 삼킴 판정을 여기서
                // 직접 태운다(실제 winit 경로와 같은 단일 로직). 주입 테스트가 메뉴가 떠
                // 있는 상태의 라우팅을 실제와 다르게 관찰하지 않도록.
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

    /// window 정규화 좌표 (fx, fy ∈ [0,1] 논리) 에 포인터 동작을 egui 입력 큐로 주입한다.
    ///
    /// egui-mesh popup(A2)은 `draw_plugin_popups` 가 `ctx.input` 의 egui 이벤트를 읽어
    /// plugin 으로 forward 한다 — winit 핸들러를 거치는 [`debug_inject_mesh_pointer`] 와
    /// 달리, 합성 egui 이벤트를 다음 frame 입력에 직접 넣어 그 실제 forward 경로를 탄다.
    pub(crate) fn debug_inject_egui_pointer(
        &mut self,
        fx: f32,
        fy: f32,
        surface_id: Option<u32>,
        action: InjectPointer,
    ) -> bool {
        let ppp = self.base.gpu.egui_pixels_per_point().max(f32::EPSILON);
        // surface_id 지정 시 (fx,fy)를 그 surface rect 안 정규화 좌표로 해석한다 —
        // window 정규화는 고정 px 레이아웃(사이드바/탭바) 탓에 창 크기 의존적이라
        // 테스트가 취약하다. 미지정 시 기존대로 window 정규화([0,1]) 로 해석한다.
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
        // 직전 포획본을 비운다 — egui 프레임이 세우는 메뉴를 `process_pending_native_menu`
        // 의 suppress 훅이 이 슬롯에 새로 포획한다(mesh 경로와 동일 관찰 규약). 비우지
        // 않으면 메뉴를 안 세운 클릭이 직전 값을 반환해 관찰이 오염된다.
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

    /// 주입기가 요청한 단위를 그대로 egui 에 넣어야 한다. 여기가 한 갈래라도 다른
    /// 단위로 접히면, 그 단위를 재현하려던 검증이 **다른 환산 경로를 재고도** 통과한다.
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

    /// `Page` 는 winit 에 표현이 없다. 가장 가까운 것으로 접어 넣으면 주입은 성공했는데
    /// 실제로는 다른 단위가 흐르므로, 거절해서 호출자가 알게 한다.
    #[test]
    fn winit_level_refuses_page_instead_of_folding_it_into_lines() {
        assert!(ScrollUnit::Page.to_winit_delta(0.0, 1.0).is_none());
    }

    /// 모르는 이름을 기본값으로 삼키면 오타 난 검증이 조용히 통과한다.
    #[test]
    fn unknown_unit_names_are_rejected_not_defaulted() {
        assert_eq!(ScrollUnit::from_name("line"), Some(ScrollUnit::Line));
        assert_eq!(ScrollUnit::from_name("point"), Some(ScrollUnit::Point));
        assert_eq!(ScrollUnit::from_name("page"), Some(ScrollUnit::Page));
        assert_eq!(ScrollUnit::from_name("Line"), None);
        assert_eq!(ScrollUnit::from_name("lines"), None);
        assert_eq!(ScrollUnit::from_name(""), None);
    }
}
