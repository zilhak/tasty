//! `View` — GUI 어댑터. winit 윈도우, egui, modal/focus 식별자, event loop proxy
//! 등 *사용자의 화면* 측면을 모은다.
//!
//! Phase C 의 strangler fig 마이그레이션 중. 현재는 빈 골격이며, sub-step 마다
//! 한 필드씩 이동:
//! - C.1.2 — proxy: EventLoopProxy<AppEvent>
//! - C.1.3 — active_modal_id
//! - C.1.4 — focused_window_id
//! - C.4.x — windows HashMap, View trait

#[allow(dead_code)]
pub(crate) struct View {
    // sub-step 마다 한 필드씩 추가됨.
}

impl View {
    pub(crate) fn new() -> Self {
        Self {}
    }
}
