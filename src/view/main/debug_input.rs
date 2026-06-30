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
                let state = if pressed {
                    ElementState::Pressed
                } else {
                    ElementState::Released
                };
                self.handle_mouse_input(state, button, false);
            }
            InjectPointer::Scroll { dx, dy } => {
                self.handle_mouse_wheel(MouseScrollDelta::LineDelta(dx, dy), false);
            }
        }
        true
    }
}
