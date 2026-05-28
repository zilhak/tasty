//! `View` — GUI 어댑터. winit 윈도우, egui, modal/focus 식별자, event loop proxy
//! 등 *사용자의 화면* 측면을 모은다.
//!
//! Phase C 의 strangler fig 마이그레이션 중. sub-step 마다 한 필드씩 이동:
//! - C.1.2 — proxy: EventLoopProxy<AppEvent>  ← 현재
//! - C.1.3 — active_modal_id
//! - C.1.4 — focused_window_id
//! - C.4.x — windows HashMap, View trait

use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

use crate::AppEvent;

pub(crate) struct View {
    /// winit event loop 의 proxy. AppEvent 를 enqueue 하기 위한 채널.
    /// View 영역은 GUI 어댑터로서 winit 과 직접 결합되어 있다.
    pub proxy: EventLoopProxy<AppEvent>,
    /// When Some, a modal window is active and all other windows should ignore input.
    /// At most one modal can exist at a time.
    pub active_modal_id: Option<WindowId>,
    /// The window that currently has focus (receives IPC commands targeting "focused" window).
    pub focused_window_id: Option<WindowId>,
}

impl View {
    pub(crate) fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            proxy,
            active_modal_id: None,
            focused_window_id: None,
        }
    }

    /// Check if a modal is active.
    pub fn is_modal_active(&self) -> bool {
        self.active_modal_id.is_some()
    }
}
