//! attach mesh mirror(TODO 19) pane 의 로컬→원격 입력 forward (TODO 20).
//!
//! [`egui_mesh`](super::egui_mesh) 는 host 가 **자기 프로세스**의 plugin 을 IPC 로
//! 구동하는 경로(로컬 `PluginManager`)다. 이 모듈은 그 대응이되 목적지가
//! **네트워크**(attach 스트림)다 — attach client 가 mirror pane 위에서 캡처한 입력을
//! `StreamControl::MeshContext`/`MeshInput` 으로 원격에 보내, 원격의 실제 plugin
//! 프로세스를 구동시킨다.
//!
//! # egui_mesh 와의 차이
//!
//! - **bootstrap/pending_full 없음**: 원격 surface 는 이미 존재하므로 `surface.create`
//!   가 필요 없고, 텍스처 delta 체인 복구는 서버측 명시 요청
//!   ([`StreamControl::MeshFullResendRequest`], TODO 19
//!   `dispatch_pending_mesh_full_resend_forwards`)이 이미 별도로 담당한다.
//! - **좌표/modifier 변환 재사용**: `mesh_local_point`/`mesh_modifiers`/
//!   `mesh_theme_snapshot`/`map_button`/`key_wire_event`(모두 `egui_mesh.rs`)를 그대로
//!   쓴다 — 로컬 마우스 이벤트가 attach mirror pane 위에 있든 로컬 plugin surface
//!   위에 있든 동일한 물리 좌표계·modifier 상태이기 때문.
//! - **App 경계를 건너는 2단계 forward**: `MainView`(이 모듈)는 `App.attach_client_sessions`
//!   에 접근할 수 없다 — `CoreState`의 `pending_mesh_context_forward`/
//!   `pending_mesh_input_forward` 큐에 쌓아두면, `App::about_to_wait`
//!   (`attach_client.rs::dispatch_pending_mesh_context_forwards`/
//!   `dispatch_pending_mesh_input_forwards`)가 다음 tick 에 drain 해 실제 네트워크
//!   전송을 한다(`pending_resize_forward`/`dispatch_pending_resize_forwards` 와 동형 —
//!   ADR-0045 패턴).

use winit::event::MouseButton;

use tasty_plugin_protocol::protocol::{RawInputEventWire, RawInputWire, ThemeWire};

use crate::core::AttachMeshContextForward;
use crate::model::{AttachMeshSurface, PhysicalPx, PhysicalRect};

use super::MainView;
use super::egui_mesh::{key_wire_event, map_button};

/// 한 attach mesh surface 의 forward 추적 상태(dedup 용) — [`super::egui_mesh::MeshForwardState`]
/// 의 축약판(모듈 doc "차이" 참고 — bootstrap/pending_full 없음).
#[derive(Default)]
pub(crate) struct AttachMeshForwardState {
    /// 마지막으로 보낸 (width_px, height_px, ppp.to_bits()). 변경 감지에 사용.
    last_geom: Option<(u32, u32, u32)>,
    /// 마지막으로 보낸 focused 상태.
    last_focused: Option<bool>,
    /// 마지막으로 보낸 Theme 스냅샷.
    last_theme: Option<ThemeWire>,
    /// 다음 forward 에 실어 보낼 누적 입력 이벤트(순서 보존).
    events: Vec<RawInputEventWire>,
}

impl MainView {
    /// (x, y) 물리 좌표가 attach mesh mirror surface 위에 있으면 (surface_id, rect) 반환.
    /// [`super::egui_mesh::MainView::egui_mesh_target_at`]의 attach 대응.
    pub(super) fn attach_mesh_target_at(&self, x: f32, y: f32) -> Option<(u32, PhysicalRect)> {
        let terminal_rect = self.compute_terminal_rect();
        for (_pane_id, _pane_rect, regions) in
            self.state.surface_regions(&self.core_state, terminal_rect)
        {
            for r in regions {
                if r.rect.contains(PhysicalPx(x), PhysicalPx(y))
                    && r.surface
                        .as_any()
                        .downcast_ref::<AttachMeshSurface>()
                        .is_some()
                {
                    return Some((r.id, r.rect));
                }
            }
        }
        None
    }

    /// 포커스된 surface 가 attach mesh mirror(`AttachMeshSurface`)면 그 surface_id 반환.
    /// [`super::egui_mesh::MainView::focused_egui_mesh_surface_id`]의 attach 대응.
    pub(super) fn focused_attach_mesh_surface_id(&self) -> Option<u32> {
        let sid = self.state.focused_surface_id(&self.core_state)?;
        let surface = self.core_state.find_surface_by_id(sid)?;
        surface
            .as_any()
            .downcast_ref::<AttachMeshSurface>()
            .map(|_| sid)
    }

    /// 포인터 버튼 누름/뗌을 attach mesh surface 에 누적.
    pub(super) fn attach_mesh_push_pointer_button(
        &mut self,
        surface_id: u32,
        rect: PhysicalRect,
        x: f32,
        y: f32,
        button: MouseButton,
        pressed: bool,
    ) {
        let Some(button) = map_button(button) else {
            return;
        };
        let (lx, ly) = self.mesh_local_point(rect, x, y);
        let modifiers = self.mesh_modifiers();
        let st = self.attach_mesh_input.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::PointerButton {
            x: lx,
            y: ly,
            button,
            pressed,
            modifiers,
        });
    }

    /// 포인터 이동을 attach mesh surface 에 누적.
    pub(super) fn attach_mesh_push_pointer_moved(
        &mut self,
        surface_id: u32,
        rect: PhysicalRect,
        x: f32,
        y: f32,
    ) {
        let (lx, ly) = self.mesh_local_point(rect, x, y);
        let st = self.attach_mesh_input.entry(surface_id).or_default();
        st.events
            .push(RawInputEventWire::PointerMoved { x: lx, y: ly });
    }

    /// 스크롤 델타(논리 포인트)를 attach mesh surface 에 누적.
    pub(super) fn attach_mesh_push_scroll(&mut self, surface_id: u32, dx: f32, dy: f32) {
        let st = self.attach_mesh_input.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Scroll { x: dx, y: dy });
    }

    /// 키 누름을 attach mesh surface 에 누적. [`super::egui_mesh::MainView::egui_mesh_push_key`]
    /// 와 동형(press-only — release 는 `handle_keyboard_input` 이 이미 걸러낸다).
    pub(super) fn attach_mesh_push_key(&mut self, surface_id: u32, event: &winit::event::KeyEvent) {
        let modifiers = self.mesh_modifiers();
        let Some(ev) = key_wire_event(
            &event.logical_key,
            event.physical_key,
            matches!(event.state, winit::event::ElementState::Pressed),
            event.repeat,
            modifiers,
        ) else {
            return;
        };
        let st = self.attach_mesh_input.entry(surface_id).or_default();
        st.events.push(ev);
    }

    /// 텍스트 입력을 attach mesh surface 에 누적.
    pub(super) fn attach_mesh_push_text(&mut self, surface_id: u32, text: &str) {
        if text.is_empty() {
            return;
        }
        let st = self.attach_mesh_input.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Text {
            text: text.to_string(),
        });
    }

    /// IME 조합 이벤트를 attach mesh surface 에 누적.
    pub(super) fn attach_mesh_push_ime(
        &mut self,
        surface_id: u32,
        event: tasty_plugin_protocol::protocol::ImeWire,
    ) {
        let st = self.attach_mesh_input.entry(surface_id).or_default();
        st.events.push(RawInputEventWire::Ime { event });
    }

    /// 활성 workspace 의 attach mesh mirror surface 들에 대해 geometry/theme/focus
    /// 변경 또는 누적 입력이 있으면 `CoreState`의 forward 큐에 쌓는다. 실제 네트워크
    /// 전송은 `App::about_to_wait`(`attach_client.rs`)가 다음 tick 에 수행한다(모듈
    /// doc "App 경계를 건너는 2단계 forward" 참고). [`MainView::handle_redraw`] 가
    /// 매 dirty frame 마다 부른다 — `PluginManager` 는 필요 없다(로컬에 plugin 프로세스가
    /// 없다).
    pub(super) fn forward_attach_mesh_context(&mut self) {
        let terminal_rect = self.compute_terminal_rect();
        let ppp = self.base.gpu.scale_factor();
        let focused = self.state.focused_surface_id(&self.core_state);
        let modifiers = self.mesh_modifiers();
        let current_theme = self.mesh_theme_snapshot();

        let mut targets: Vec<(u32, PhysicalRect)> = Vec::new();
        for (_pane_id, _pane_rect, regions) in
            self.state.surface_regions(&self.core_state, terminal_rect)
        {
            for r in regions {
                if r.surface
                    .as_any()
                    .downcast_ref::<AttachMeshSurface>()
                    .is_some()
                {
                    targets.push((r.id, r.rect));
                }
            }
        }

        // layout 에서 사라진 surface 의 추적 상태 정리(존재 기반 — egui_mesh 와 동형).
        let existing = self.state.attach_mesh_surfaces_existing(&self.core_state);
        let live: std::collections::HashSet<u32> = existing.into_iter().collect();
        self.attach_mesh_input.retain(|sid, _| live.contains(sid));

        for (sid, rect) in targets {
            let w = rect.width.value().round().max(1.0) as u32;
            let h = rect.height.value().round().max(1.0) as u32;
            let geom = (w, h, ppp.to_bits());
            let is_focused = focused == Some(sid);

            let st = self.attach_mesh_input.entry(sid).or_default();
            let geom_changed = st.last_geom != Some(geom);
            let theme_changed = st.last_theme.as_ref() != Some(&current_theme);
            let focus_changed = st.last_focused != Some(is_focused);
            let has_input = !st.events.is_empty();

            if geom_changed || theme_changed || focus_changed {
                st.last_geom = Some(geom);
                st.last_theme = Some(current_theme.clone());
                st.last_focused = Some(is_focused);
                self.core_state.pending_mesh_context_forward.insert(
                    sid,
                    AttachMeshContextForward {
                        width_px: w,
                        height_px: h,
                        pixels_per_point: ppp,
                        theme: Some(current_theme.clone()),
                        focused: is_focused,
                    },
                );
            }

            if has_input {
                let events = std::mem::take(&mut st.events);
                self.core_state.pending_mesh_input_forward.insert(
                    sid,
                    RawInputWire {
                        time: None,
                        focused: is_focused,
                        modifiers,
                        events,
                    },
                );
            }
        }
    }
}
