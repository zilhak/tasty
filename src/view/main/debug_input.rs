//! debug 전용 window 레벨 포인터 주입 (입력 재현, release 미노출).
//!
//! 불가침 원칙 1·3: 사용자 입력 재현(키/마우스 주입)은 release 에 없고
//! `#[cfg(debug_assertions)]` debug 격리로만 존재한다(`docs/dev-guide/debug-ipc.md`).
//! 본 모듈은 egui-mesh 입력 forward(A1-S7)를 헤드리스로 자체 검증하기 위한 debug
//! 표면이다 — 기존 `debug.inject_mouse`(터미널 PTY SGR 리포트 주입)와 달리 winit
//! 레벨 포인터 이벤트를 MainView 핸들러로 흘려, `handle_mouse_input`/`handle_mouse_wheel`/
//! `handle_cursor_moved` 의 실제 라우팅(egui-mesh 캡처 포함)을 그대로 탄다.

#![cfg(debug_assertions)]

use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use super::MainView;

/// 주입할 포인터 동작 종류.
pub(crate) enum InjectPointer {
    Move,
    Button { button: MouseButton, pressed: bool },
    Scroll { dx: f32, dy: f32 },
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
        let Some(rect) = self
            .state
            .surface_rect_by_id(&self.core_state, surface_id, terminal_rect)
        else {
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
                self.handle_mouse_input(state, button, false);
                if let Some(menu) = self.state.dialogs.pending_native_menu.take() {
                    self.debug_captured_menu = Some(menu);
                }
            }
            InjectPointer::Scroll { dx, dy } => {
                self.handle_mouse_wheel(MouseScrollDelta::LineDelta(dx, dy), false);
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
            let Some(rect) = self
                .state
                .surface_rect_by_id(&self.core_state, sid, terminal_rect)
            else {
                return false;
            };
            egui::pos2(
                (rect.x.value() + fx * rect.width.value()) / ppp,
                (rect.y.value() + fy * rect.height.value()) / ppp,
            )
        } else {
            let (w, h) = self.base.gpu.surface_config_size();
            egui::pos2((w as f32 / ppp) * fx, (h as f32 / ppp) * fy)
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
            InjectPointer::Scroll { dx, dy } => vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
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
