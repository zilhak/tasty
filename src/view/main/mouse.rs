use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::window::CursorIcon;

use super::{DividerDrag, DividerDragKind, HoveredLink, MainView, MeshHoverTarget};
use crate::core::intent::{DomainIntent, SendPayload};
use crate::settings::LinkModifier;
use crate::terminal_link::{self, LinkHighlight};
use crate::theme;
use crate::view::ui::View;
use tasty_type_geometry::length::PhysicalPx;

impl MainView {
    /// 현재 마우스 좌표와 수식키 상태로 hovered_link를 갱신한다.
    /// 변경이 있으면 true를 반환 (렌더 dirty 플래그를 켜기 위함).
    pub(crate) fn update_hovered_link(&mut self) -> bool {
        let engine = &mut self.core_state;
        let prev = self
            .hovered_link
            .as_ref()
            .map(|h| (h.surface_id, h.highlight.segments.clone()));

        let modifier = LinkModifier::parse(&engine.settings.general.link_click_modifier);
        let mods = &self.base.modifiers;
        let matches_mods = modifier.matches(mods.control_key(), mods.alt_key(), mods.super_key());

        let new_link = if !matches_mods
            || self.state.settings_open
            || self.state.popup_hovered
            || self.state.banner_hovered
        {
            None
        } else {
            self.compute_hovered_link()
        };

        let changed = prev
            != new_link
                .as_ref()
                .map(|h| (h.surface_id, h.highlight.segments.clone()));
        self.hovered_link = new_link;
        changed
    }

    fn compute_hovered_link(&self) -> Option<HoveredLink> {
        let engine = &self.core_state;
        let pos = self.cursor_position?;
        let terminal_rect = self.compute_terminal_rect();
        let x = pos.x as f32;
        let y = pos.y as f32;
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            return None;
        }
        // 마우스 아래 surface id를 구하고 그 surface의 terminal을 사용.
        // focused 기반이 아니라 실제 hover 위치의 surface로 판별해야 여러 pane 중
        // 어느 곳이든 동작한다.
        let surface_id = self
            .state
            .surface_id_at_position(engine, x, y, terminal_rect)?;
        let terminal = engine.find_terminal_by_id(surface_id)?;
        let surface_rect = self
            .state
            .surface_rect_by_id(engine, surface_id, terminal_rect)?;

        let (cols, rows) = terminal.dimensions();
        let point = crate::selection::pixel_to_grid(
            x,
            y,
            &surface_rect,
            self.base.gpu.cell_width(),
            self.base.gpu.cell_height(),
            cols,
            rows,
            terminal.scroll_offset(),
            terminal.scrollback_len(),
        );
        let span = terminal_link::link_at(terminal, point.col, point.absolute_row)?;
        let th = theme::theme();
        let highlight = LinkHighlight {
            segments: span.segments,
            fg: th.accent_primary().to_gpu_rgba(),
            bg: th.selection_bg.to_gpu_rgba(),
        };
        Some(HoveredLink {
            surface_id,
            uri: span.uri,
            highlight,
        })
    }

    /// `WindowEvent::CursorLeft` 처리 — 커서 상태 리셋 + hover 중이던 mesh
    /// surface 가 있으면 `PointerGone` 을 forward 한다(egui-mesh/attach mesh mirror
    /// 어느 쪽도 이 이벤트에서 직접 `PointerGone` 을 보내지 않던 gap 을 메운다).
    pub(super) fn handle_cursor_left(&mut self) {
        self.cursor_position = None;
        self.base.winit.set_cursor(CursorIcon::Default);
        self.update_mesh_hover(None);
    }

    /// mesh pointer hover 슬롯을 갱신한다(구성 요소는 `docs/dev-guide/egui-mesh-channel.md`
    /// 참고). 대상이 바뀌면(다른 mesh surface
    /// 로 전환되거나 어느 mesh surface 위도 아니게 되면) 이전 대상에 `PointerGone` 을
    /// 1 회 forward 한다 — 안 그러면 plugin 쪽 egui 가 마지막 `PointerMoved` 위치에
    /// 포인터가 계속 있다고 착각해 hover 하이라이트가 잔류할 수 있다.
    pub(super) fn update_mesh_hover(&mut self, target: Option<MeshHoverTarget>) {
        let (next, gone) = mesh_hover_transition(self.mesh_pointer_hover, target);
        self.mesh_pointer_hover = next;
        if let Some(prev) = gone {
            match prev {
                MeshHoverTarget::Local(sid) => self.egui_mesh_push_pointer_gone(sid),
                MeshHoverTarget::Attach(sid) => self.attach_mesh_push_pointer_gone(sid),
            }
        }
    }

    pub(super) fn handle_cursor_moved(
        &mut self,
        position: winit::dpi::PhysicalPosition<f64>,
        egui_consumed: bool,
    ) {
        self.cursor_position = Some(position);
        let overlay_open = self.state.settings_open;
        if cursor_moved_should_short_circuit(
            egui_consumed,
            overlay_open,
            self.state.popup_hovered,
            self.state.banner_hovered,
            self.state.modifier_hint_hovered,
        ) {
            // 콘텐츠/오버레이 위에서는 리사이즈 커서를 띄우지 않는다(콘텐츠 우선).
            // early-return 경로에서도 반드시 리셋해야 가장자리→콘텐츠 이동 시 ↔ 커서가
            // 남지 않는다.
            self.state.pending_resize_cursor = None;
            if self.hovered_link.take().is_some() {
                self.mark_dirty();
            }
            // 이 분기에 진입했다는 것 자체가 "이번 프레임엔 mesh surface 위가 아니다"라는
            // 뜻이므로(mouse hover early-return, `docs/dev-guide/egui-mesh-channel.md` 참고)
            // — 아래 mesh 판정 블록(180행대)에 도달하지 못한 채 return
            // 하면 hover 중이던 mesh surface 가 PointerGone 을 영영 못 받는 gap 이 생긴다.
            // mesh_hover_transition 이 멱등이라 같은 target 유지 중 매 프레임 호출돼도
            // thrashing 없다.
            self.update_mesh_hover(None);
            self.mark_dirty();
            return;
        }

        // 통합 리사이즈 커서 피드백 — 가장자리 margin 안이면 8방향을 저장하고
        // egui 프레임(`run_egui_frame`)이 `set_cursor_icon` 으로 적용한다(egui 가 매
        // 프레임 winit 커서를 덮으므로 프레임 내에서만 적용 가능). macOS 는 네이티브
        // 데코라 제외(cfg 가드) — 그 외 OS 는 항상 None 유지.
        #[cfg(not(target_os = "macos"))]
        {
            let size = self.base.gpu.size();
            self.state.pending_resize_cursor = if self.base.winit.is_maximized() {
                None
            } else {
                crate::platform::window_chrome::resize_direction_at(
                    position.x,
                    position.y,
                    f64::from(size.width),
                    f64::from(size.height),
                    crate::platform::window_chrome::RESIZE_EDGE_MARGIN,
                )
            };
        }

        let terminal_rect = self.compute_terminal_rect();
        let x = position.x as f32;
        let y = position.y as f32;

        // egui-mesh surface 위 포인터 이동 forward (A1-S7): hover/interact_pos 추적용.
        // 합성 채널이라 host 의 selection/링크 hover 와 무관 — 누적 후 소비한다.
        // 단 divider 드래그 진행 중에는 forward 하지 않는다: divider(입력 z-order 순서 6)
        // 가 surface 콘텐츠(순서 7, egui-mesh 포함)보다 우선해야, 드래그 중 커서가
        // egui-mesh surface 영역으로 들어가도 아래 divider 갱신이 계속 실행된다
        // (docs/architecture/input-layer.md). 이 가드가 없으면 여기서 early-return 되어
        // divider 가 커서를 따라오지 못하고 멈춘다.
        if self.dragging_divider.is_none()
            && let Some((sid, _plugin_id, rect)) = self.egui_mesh_target_at(x, y)
        {
            self.update_mesh_hover(Some(MeshHoverTarget::Local(sid)));
            self.egui_mesh_push_pointer_moved(sid, rect, x, y);
            self.mark_dirty();
            return;
        }
        // attach mesh mirror surface 위 포인터 이동 forward(`docs/dev-guide/egui-mesh-channel.md`
        // 의 "attach mesh mirror 소비 경로" 참고) — 위와 동형이되 목적지가 원격.
        if let Some((sid, rect)) = self.attach_mesh_target_at(x, y) {
            self.update_mesh_hover(Some(MeshHoverTarget::Attach(sid)));
            self.attach_mesh_push_pointer_moved(sid, rect, x, y);
            self.mark_dirty();
            return;
        }
        // 어느 mesh surface 위도 아니다 — divider 드래그로 위 두 분기를
        // 건너뛴 경우 포함. 직전까지 hover 중이던 mesh surface 가 있었다면
        // `PointerGone` 을 1 회 forward 한다(mesh→mesh, mesh→non-mesh 전환 공통 처리).
        self.update_mesh_hover(None);

        if self.update_hovered_link() {
            self.mark_dirty();
        }

        // Handle selection drag
        if self.left_mouse_down && self.dragging_divider.is_none() {
            // Shift+좌클릭 우회 시퀀스(left_select_bypass)면 트래킹 motion 보고를 건너뛰고
            // 곧장 로컬 선택 확장 경로로 떨어진다. 플래그 검사를 트래킹 보고 블록 *이전* 에
            // 두어 early-return 으로 로컬 경로가 막히는 것을 방지한다.
            if !self.left_select_bypass {
                // 트래킹 ON(CellMotion/AllMotion): 드래그 motion 을 앱에 보고 (셀 바뀔 때만).
                // 앱이 마우스를 소유하므로 로컬 선택 확장은 하지 않는다.
                let track = self
                    .state
                    .focused_surface_id(&self.core_state)
                    .and_then(|sid| {
                        self.core_state
                            .find_terminal_by_id(sid)
                            .map(|t| (sid, t.mouse_tracking()))
                    });
                if let Some((sid, mode)) = track
                    && matches!(
                        self.effective_click_tracking(sid, mode),
                        tasty_terminal::MouseTrackingMode::CellMotion
                            | tasty_terminal::MouseTrackingMode::AllMotion
                    )
                {
                    let cell = self.mouse_cell_for_report(sid, x, y);
                    if self.last_mouse_report_cell != Some(cell) {
                        self.report_mouse_event(sid, x, y, 0, true, false);
                    }
                    return;
                }
            }
            let is_dragging = self.text_selection.as_ref().is_some_and(|s| s.dragging);
            if is_dragging && let Some((point, _)) = self.mouse_to_grid(x, y, &terminal_rect) {
                if let Some(sel) = &mut self.text_selection {
                    sel.cursor = point;
                }
                self.mark_dirty();
            }
        }

        if let Some(drag) = self.dragging_divider {
            let cell_w = self.base.gpu.cell_width();
            let cell_h = self.base.gpu.cell_height();
            let changed = {
                let engine = &mut self.core_state;
                let changed = match drag.kind {
                    DividerDragKind::Pane => {
                        self.state
                            .update_pane_divider(engine, &drag.info, x, y, terminal_rect)
                    }
                    DividerDragKind::Surface => {
                        self.state
                            .update_surface_divider(engine, &drag.info, x, y, terminal_rect)
                    }
                };
                if changed {
                    self.state.resize_all(engine, terminal_rect, cell_w, cell_h);
                }
                changed
            };
            if changed {
                self.mark_dirty();
            }
        }
        // Cursor icon is determined in the egui render cycle (gpu/mod.rs)
    }

    /// 마우스 버튼 입력 라우팅 디스패처. 게이트(OS-resize → click-to-activate →
    /// egui/overlay 소비 → egui-mesh) 를 순서대로 태운 뒤 버튼별 핸들러로 위임한다.
    /// 좌표/라우팅 결정 수학은 이미 순수 함수(`resize_direction_at`·`pixel_to_grid`·
    /// `right_click_delegates_to_app`·`left_click_local_select`)로 밖에 있어 이 계층은
    /// **어느 state 메서드를 어떤 순서로** 부를지의 stateful 디스패치만 담당한다.
    pub(super) fn handle_mouse_input(
        &mut self,
        button_state: ElementState,
        button: MouseButton,
        egui_consumed: bool,
    ) {
        #[cfg(not(target_os = "macos"))]
        if self.try_begin_os_resize(button, button_state, egui_consumed) {
            return;
        }

        let overlay_open = self.state.settings_open;
        if self.try_click_to_activate(button, button_state, overlay_open) {
            return;
        }

        if egui_consumed
            || overlay_open
            || self.state.popup_hovered
            || self.state.banner_hovered
            || self.state.modifier_hint_hovered
        {
            // 비-좌클릭/Release/egui-크롬(사이드바·탭바) 클릭의 소비. 활성 surface 안
            // pane 포커스 갱신은 위 click-to-activate 단계가 흡수하므로 여기서는
            // Release 정리와 egui 소비 repaint 만 남긴다.
            if button_state == ElementState::Released {
                self.dragging_divider = None;
                self.left_mouse_down = false;
                self.left_select_bypass = false;
            }
            if egui_consumed {
                self.mark_dirty();
            }
            return;
        }

        // divider 드래그 진행 중이면 egui-mesh 로 버튼을 forward 하지 않는다: 마크다운/
        // 이미지(egui-mesh) surface 위에서 좌클릭을 떼도 release 가 handle_left_release
        // 로 흘러 divider 를 확정(resize 반영)하고 dragging_divider 를 해제해야 한다.
        // forward 로 소비되면 드래그가 확정/해제되지 않아 sticky divider 가 된다.
        if self.dragging_divider.is_none()
            && self.try_forward_egui_mesh_button(button, button_state)
        {
            return;
        }
        if self.try_forward_attach_mesh_button(button, button_state) {
            return;
        }

        match button {
            MouseButton::Right => self.handle_right_button(button_state),
            MouseButton::Middle => self.handle_middle_button(button_state),
            MouseButton::Left => self.handle_left_button(button_state),
            _ => {}
        }
    }

    /// OS 가장자리 리사이즈 hit-test (콘텐츠 우선 입력모델). 좌클릭 press 가 창
    /// 가장자리 margin 안이면 OS 리사이즈를 시작하고 `true`(클릭 소비). macOS 는
    /// 네이티브 데코라 이 경로를 타지 않는다(호출부 cfg 가드).
    #[cfg(not(target_os = "macos"))]
    fn try_begin_os_resize(
        &mut self,
        button: MouseButton,
        button_state: ElementState,
        egui_consumed: bool,
    ) -> bool {
        if button == MouseButton::Left
            && button_state == ElementState::Pressed
            && !egui_consumed
            && !self.state.settings_open
            && !self.state.popup_hovered
            && !self.state.banner_hovered
            && !self.base.winit.is_maximized()
            && let Some(pos) = self.cursor_position
        {
            let size = self.base.gpu.size();
            if let Some(dir) = crate::platform::window_chrome::resize_direction_at(
                pos.x,
                pos.y,
                f64::from(size.width),
                f64::from(size.height),
                crate::platform::window_chrome::RESIZE_EDGE_MARGIN,
            ) {
                if let Err(e) = self.base.winit.drag_resize_window(dir) {
                    tracing::warn!("window resize drag failed: {e}");
                }
                return true;
            }
        }
        false
    }

    /// click-to-activate swallow: 비활성 surface 를 좌클릭(press)하면 첫 클릭을
    /// "surface 전환" 이 통째로 소비한다(macOS 모델). 전환하면 `true`(클릭 소비).
    /// modal/popup 은 상위 레이어라 전환보다 먼저 배제(호출부 `overlay_open`).
    /// docs/architecture/input-layer.md.
    fn try_click_to_activate(
        &mut self,
        button: MouseButton,
        button_state: ElementState,
        overlay_open: bool,
    ) -> bool {
        if button == MouseButton::Left
            && button_state == ElementState::Pressed
            && !overlay_open
            && !self.state.popup_hovered
            && !self.state.modifier_hint_hovered
            && let Some(pos) = self.cursor_position
        {
            let terminal_rect = self.compute_terminal_rect();
            let (x, y) = (pos.x as f32, pos.y as f32);
            if let Some(sid) =
                self.state
                    .surface_id_at_position(&self.core_state, x, y, terminal_rect)
                && self.state.focused_surface_id(&self.core_state) != Some(sid)
            {
                let engine = &mut self.core_state;
                let changed_pane = self
                    .state
                    .focus_pane_at_position(engine, x, y, terminal_rect);
                let changed_surf =
                    self.state
                        .focus_surface_at_position(engine, x, y, terminal_rect);
                if changed_pane || changed_surf {
                    self.base.dirty = true;
                }
                self.mark_dirty();
                return true;
            }
        }
        false
    }

    /// egui-mesh surface 입력 forward (A1-S7): 포인터가 egui-mesh surface 위면 버튼
    /// 이벤트를 surface-local 좌표로 누적해 다음 set_context 로 보내고 소비(`true`).
    fn try_forward_egui_mesh_button(
        &mut self,
        button: MouseButton,
        button_state: ElementState,
    ) -> bool {
        if let Some(pos) = self.cursor_position {
            let (x, y) = (pos.x as f32, pos.y as f32);
            if let Some((sid, _plugin_id, rect)) = self.egui_mesh_target_at(x, y) {
                let pressed = super::egui_mesh::is_pressed(button_state);
                if !pressed {
                    self.left_mouse_down = false;
                }
                self.egui_mesh_push_pointer_button(sid, rect, x, y, button, pressed);
                self.mark_dirty();
                return true;
            }
        }
        false
    }

    /// attach mesh mirror surface 입력 forward — 위와 동형이되 목적지가 원격.
    fn try_forward_attach_mesh_button(
        &mut self,
        button: MouseButton,
        button_state: ElementState,
    ) -> bool {
        if let Some(pos) = self.cursor_position {
            let (x, y) = (pos.x as f32, pos.y as f32);
            if let Some((sid, rect)) = self.attach_mesh_target_at(x, y) {
                let pressed = super::egui_mesh::is_pressed(button_state);
                if !pressed {
                    self.left_mouse_down = false;
                }
                self.attach_mesh_push_pointer_button(sid, rect, x, y, button, pressed);
                self.mark_dirty();
                return true;
            }
        }
        false
    }

    /// 우클릭 라우팅: 트래킹 ON+Shift없음이면 앱 위임(ADR-0019), 아니면 tasty 컨텍스트
    /// 메뉴(terminal/비-terminal 별도). 결정은 순수 `right_click_delegates_to_app`.
    fn handle_right_button(&mut self, button_state: ElementState) {
        let terminal_rect = self.compute_terminal_rect();
        let Some(pos) = self.cursor_position else {
            return;
        };
        let (x, y) = (pos.x as f32, pos.y as f32);
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            return;
        }
        let Some(surface_id) =
            self.state
                .surface_id_at_position(&self.core_state, x, y, terminal_rect)
        else {
            return;
        };
        let tracking = self
            .core_state
            .find_terminal_by_id(surface_id)
            .map(|t| t.mouse_tracking());
        let Some(tracking) = tracking else {
            // 비-terminal surface(explorer/empty/markdown/image/webview/remote):
            // 컨텍스트 메뉴는 winit 이 만들지 않고 egui 프레임(release 시점)에 위임한다 —
            // egui_panels 의 emit_surface_menu_fallback 이 Surface 메뉴를, explorer 는
            // apply_explorer_action 이 Explorer 메뉴를 세팅한다. winit 은 terminal 전용
            // (mouse-tracking/ADR-0022).
            return;
        };
        // 블랙리스트면 None 으로 격하 → 우클릭이 tasty 컨텍스트 메뉴로 빠진다.
        let tracking = self.effective_click_tracking(surface_id, tracking);
        let shift = self.base.modifiers.shift_key();
        if right_click_delegates_to_app(tracking, shift) {
            // 트래킹 앱이 마우스를 캡처 중이라 우클릭이 앱으로 간다 — Shift+드래그/
            // Shift+우클릭 우회 안내를 트래킹 세션당 1회(Pressed, 설정 ON, ADR-0022 ②).
            if button_state == ElementState::Pressed {
                self.report_left_press_capture(surface_id);
            }
            self.report_mouse_event(
                surface_id,
                x,
                y,
                2,
                false,
                button_state == ElementState::Released,
            );
            return;
        }
        if button_state == ElementState::Pressed {
            let sf = self.base.gpu.scale_factor();
            self.state.dialogs.pending_native_menu =
                Some(crate::state::PendingNativeMenu::TerminalSurface {
                    surface_id,
                    x: x / sf,
                    y: y / sf,
                });
            self.mark_dirty();
        }
    }

    /// 미들클릭 라우팅: 트래킹 ON 에서만 앱에 보고 (트래킹 OFF 는 무동작 유지).
    fn handle_middle_button(&mut self, button_state: ElementState) {
        let terminal_rect = self.compute_terminal_rect();
        if let Some(pos) = self.cursor_position {
            let (x, y) = (pos.x as f32, pos.y as f32);
            if terminal_rect.contains(PhysicalPx(x), PhysicalPx(y))
                && let Some(surface_id) =
                    self.state
                        .surface_id_at_position(&self.core_state, x, y, terminal_rect)
                && self
                    .core_state
                    .find_terminal_by_id(surface_id)
                    .map(|t| {
                        self.effective_click_tracking(surface_id, t.mouse_tracking())
                            != tasty_terminal::MouseTrackingMode::None
                    })
                    .unwrap_or(false)
            {
                self.report_mouse_event(
                    surface_id,
                    x,
                    y,
                    1,
                    false,
                    button_state == ElementState::Released,
                );
            }
        }
    }

    /// 좌클릭 라우팅: 상태 갱신(left_mouse_down·vi_copy 종료) 후 링크클릭 →
    /// press(divider/selection) → release 로 위임.
    fn handle_left_button(&mut self, button_state: ElementState) {
        if button_state == ElementState::Pressed {
            self.left_mouse_down = true;
            // 새 클릭 사이클 진입 — 이전 클릭의 값이 새어 들어가지 않도록 명시적으로
            // 리셋. 링크가 실제로 열리면 `try_handle_link_click` 이 아래에서 다시 true 로
            // set 한다.
            self.link_click_consumed = false;
            // mouse drag 시작은 vi copy mode 와 충돌 — 자동 종료. (R7)
            if self.vi_copy.is_some() {
                self.vi_copy = None;
                self.base.dirty = true;
            }
        } else {
            self.left_mouse_down = false;
        }

        let terminal_rect = self.compute_terminal_rect();
        let Some(pos) = self.cursor_position else {
            return;
        };
        let (x, y) = (pos.x as f32, pos.y as f32);
        if self.try_handle_link_click(x, y, &terminal_rect, button_state) {
            return;
        }
        if button_state == ElementState::Pressed {
            self.handle_left_press(x, y, &terminal_rect);
        } else if button_state == ElementState::Released {
            self.handle_left_release(x, y, &terminal_rect);
        }
    }

    /// 수식키+좌클릭 링크 라우팅. 매치되면 focus 갱신 후 링크 위면 열고(파일/외부 URL),
    /// 아니면 아무것도 안 함 — 어느 쪽이든 `true`(selection 경로로 안 샘).
    fn try_handle_link_click(
        &mut self,
        x: f32,
        y: f32,
        terminal_rect: &crate::model::PhysicalRect,
        button_state: ElementState,
    ) -> bool {
        let modifier = LinkModifier::parse(&self.core_state.settings.general.link_click_modifier);
        let mods = &self.base.modifiers;
        let link_mods_match = !matches!(modifier, LinkModifier::None)
            && modifier.matches(mods.control_key(), mods.alt_key(), mods.super_key());
        if !(link_mods_match && button_state == ElementState::Pressed) {
            return false;
        }
        // hard 점유(readonly)는 로컬 텍스트 선택(selection)은 허용하지만 링크 클릭은
        // 계속 억제한다: 파일 열기/외부 URL 오픈은 되돌릴 수 없는 부수효과가 있고,
        // hard 점유 화면은 최대 3초 지연된 mirror 스냅샷이라 그 시점에 보이는 링크가
        // 실제 PTY 상태와 다를 수 있다(ADR-0049). false 를 반환하면 handle_left_button 이
        // 기존처럼 press/release(선택·드래그)로 위임한다.
        if let Some(sid) = self
            .state
            .surface_id_at_position(&self.core_state, x, y, *terminal_rect)
            && self.core_state.attach.is_hard_occupied(sid)
        {
            return false;
        }
        if terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            let engine = &mut self.core_state;
            let changed_pane = self
                .state
                .focus_pane_at_position(engine, x, y, *terminal_rect);
            let changed_surf = self
                .state
                .focus_surface_at_position(engine, x, y, *terminal_rect);
            if changed_pane || changed_surf {
                self.base.dirty = true;
            }
        }
        if let Some(hovered) = self.hovered_link.clone() {
            // 이 press 는 링크오픈으로 로컬 소비된다 — release 는 tracking 앱에 보고하지
            // 않는다(handle_left_release 참고, TODO 24). press 를 앱에 보고하지 않으면서
            // release 만 단독 전달되면 자체 URL-오픈 기능이 있는 TUI 앱이 중복으로 열 수
            // 있다.
            self.link_click_consumed = true;
            // 원격(mirror) surface 판별: 클릭한 surface 의 terminal 이 detached
            // mirror(자식 PTY 없음)면 화면 경로가 원격 호스트 경로라 로컬 핸들러로
            // 열 수 없다. ID(hovered.surface_id) 로 직접 판별 — 포커스 독립.
            let is_mirror = self
                .core_state
                .find_terminal_by_id(hovered.surface_id)
                .map(|t| t.process_id().is_none())
                .unwrap_or(false);
            match crate::file_dispatch::parse_link(&hovered.uri) {
                crate::file_dispatch::LinkKind::FileTarget(path) => {
                    if is_mirror {
                        // 원격 경로: 로컬 핸들러 lookup/identify 를 타지 않고 빈
                        // picker(placeholder)만 띄운다 — empty-state, 실제 동작 없음.
                        crate::file::dispatch::open_picker(
                            &mut self.state,
                            &mut self.core_state,
                            crate::file::format::FileTarget::new(path),
                            None,
                            Vec::new(),
                            false,
                            false,
                        );
                    } else {
                        self.state.dispatch_intent(
                            crate::core::intent::DomainIntent::DispatchFile {
                                target: crate::file::format::FileTarget::new(path),
                                depth: crate::file::format::DetectDepth::Deep,
                                origin_surface_id: None,
                                ignore_size_limit: false,
                            }
                            .from_user_menu("terminal_link_click"),
                        );
                    }
                }
                crate::file_dispatch::LinkKind::External(uri) => {
                    // 외부 URL(http:// 등)은 mirror 여부와 무관하게 기존대로 처리.
                    terminal_link::open_uri(&uri);
                }
            }
        }
        self.mark_dirty();
        true
    }

    /// 좌클릭 press: divider 히트 시 드래그 시작, 아니면 selection 시작으로 위임.
    fn handle_left_press(&mut self, x: f32, y: f32, terminal_rect: &crate::model::PhysicalRect) {
        let threshold = 4.0;
        let engine = &mut self.core_state;
        let pane_div = self
            .state
            .find_pane_divider_at(engine, x, y, *terminal_rect, threshold);
        let surf_div = self
            .state
            .find_surface_divider_at(engine, x, y, *terminal_rect, threshold);
        if let Some(info) = pane_div {
            self.dragging_divider = Some(DividerDrag {
                info,
                kind: DividerDragKind::Pane,
            });
        } else if let Some(info) = surf_div {
            self.dragging_divider = Some(DividerDrag {
                info,
                kind: DividerDragKind::Surface,
            });
        } else {
            self.begin_left_selection(x, y, terminal_rect);
        }
    }

    /// 좌클릭 로컬/보고 선택 시작. focus 전환 + IME flush 후, 순수
    /// `left_click_local_select` 결정에 따라 로컬 선택 시작 / 앱 보고 / Shift extend.
    fn begin_left_selection(&mut self, x: f32, y: f32, terminal_rect: &crate::model::PhysicalRect) {
        let engine = &mut self.core_state;
        let (need_flush, mouse_tracking) = {
            let old_surface = self.state.focused_surface_id(engine);
            let changed_pane = self
                .state
                .focus_pane_at_position(engine, x, y, *terminal_rect);
            let changed_surf = self
                .state
                .focus_surface_at_position(engine, x, y, *terminal_rect);
            if changed_pane || changed_surf {
                self.base.dirty = true;
            }
            let ime_active = self.ime_preedit.is_some();
            let need_flush = ime_active && self.state.focused_surface_id(engine) != old_surface;
            // Start text selection (only if not mouse-tracking or Shift held)
            let mouse_tracking = self
                .state
                .focused_terminal(engine)
                .map(|t| t.mouse_tracking())
                .unwrap_or(tasty_terminal::MouseTrackingMode::None);
            (need_flush, mouse_tracking)
        };
        if need_flush {
            self.flush_ime_preedit();
        }
        // 블랙리스트면 None 으로 격하 → 좌클릭이 로컬 텍스트 선택으로 빠지고 앱 보고/
        // 캡처 안내 배너 경로엔 진입하지 않는다.
        let mouse_tracking = self
            .state
            .focused_surface_id(&self.core_state)
            .map(|sid| self.effective_click_tracking(sid, mouse_tracking))
            .unwrap_or(mouse_tracking);
        let shift = self.base.modifiers.shift_key();
        if mouse_tracking != tasty_terminal::MouseTrackingMode::None {
            if left_click_local_select(mouse_tracking, shift, false) {
                // 트래킹 ON + Shift: 앱에 보고하지 않고 로컬 선택을 시작한다 (xterm/iTerm2
                // 표준 modifier 우회). press 시점 1회 판정을 left_select_bypass 로 release
                // 까지 유지 — motion/release 는 이 플래그로 라우팅한다. 트래킹 ON 엔 이전
                // 로컬 앵커가 없어 extend 가 아니라 start.
                self.left_select_bypass = true;
                self.start_selection(x, y, terminal_rect);
            } else {
                // 트래킹 ON + Shift 없음: 버튼 press 를 앱에 보고 (ADR-0019 앱 위임). 단,
                // 트래킹 진입 후 첫 캡처 상호작용이면 캡처 안내를 1회 띄운다.
                if let Some(sid) = self.state.focused_surface_id(&self.core_state) {
                    self.report_left_press_capture(sid);
                    self.report_mouse_event(sid, x, y, 0, false, false);
                }
            }
        } else if shift {
            self.extend_selection(x, y, terminal_rect);
        } else {
            self.start_selection(x, y, terminal_rect);
        }
    }

    /// 마우스 캡처 진입 후 첫 상호작용이면 "마우스 캡처 중 — Shift 로 우회 가능" 안내
    /// 배너를 1회 띄운다(설정 ON + 배너 억제 리스트 미매칭일 때). 좌·우 클릭 보고
    /// 경로가 같은 `take_mouse_capture_hint()` 를 공유하므로 먼저 발생한 쪽만 뜬다
    /// (ADR-0022 ②). `mouse_capture_banner_blacklist` 매칭 surface 는 캡처 자체는
    /// 유지한 채 이 함수 최상단에서 반환한다 — `take_mouse_capture_hint()` 를 아예
    /// 호출하지 않으므로 armed 플래그도 소모하지 않는다. 이렇게 해야 같은 트래킹
    /// 세션 도중 foreground 가 비억제 앱으로 바뀌면 그 시점에 배너를 정상적으로
    /// 띄울 수 있다.
    fn report_left_press_capture(&mut self, surface_id: u32) {
        if self
            .core_state
            .is_surface_mouse_capture_banner_suppressed(surface_id)
        {
            return;
        }
        if self.core_state.settings.general.mouse_capture_hint {
            let show = self
                .core_state
                .find_terminal_by_id(surface_id)
                .is_some_and(|t| t.take_mouse_capture_hint());
            if show {
                let generation = self.core_state.foreground_generation(surface_id);
                self.state.banners.push(
                    crate::adapters::ui::BannerState::persistent(
                        crate::adapters::ui::banner::defs::BANNER_MOUSE_CAPTURE,
                        crate::adapters::ui::BannerScope::Surface(surface_id),
                    )
                    .with_origin_generation(generation),
                );
            }
        }
    }

    /// 좌클릭 release: divider 드래그 확정(resize) 후, 트래킹 ON 이면 앱 보고,
    /// 아니면 로컬 선택 확정(빈 클릭은 커서 이동 + 선택 클리어). bypass 는 앱 보고 스킵.
    /// press 가 링크오픈으로 소비됐으면(`link_click_consumed`) 마찬가지로 앱 보고 스킵
    /// (TODO 24 — press/release 비대칭으로 인한 mouse-tracking 앱의 링크 중복 오픈 방지).
    fn handle_left_release(&mut self, x: f32, y: f32, terminal_rect: &crate::model::PhysicalRect) {
        if self.dragging_divider.is_some() {
            self.dragging_divider = None;
            let cell_w = self.base.gpu.cell_width();
            let cell_h = self.base.gpu.cell_height();
            let engine = &mut self.core_state;
            self.state
                .resize_all(engine, *terminal_rect, cell_w, cell_h);
            self.base.dirty = true;
        }
        // 트래킹 ON 이면 release 를 앱에 보고, 아니면 로컬 선택 완료. 단, Shift+좌클릭
        // 우회 시퀀스(left_select_bypass)면 — dragging 여부와 무관하게(멀티클릭 word/line
        // 은 dragging=false) — 앱 보고를 스킵하고 로컬 선택을 확정한다.
        let bypass = self.left_select_bypass;
        let link_click_consumed = self.link_click_consumed;
        let report_surface = if bypass {
            None
        } else {
            self.state
                .focused_surface_id(&self.core_state)
                .filter(|sid| {
                    self.core_state
                        .find_terminal_by_id(*sid)
                        .map(|t| {
                            should_report_release_to_app(
                                self.effective_click_tracking(*sid, t.mouse_tracking()),
                                link_click_consumed,
                            )
                        })
                        .unwrap_or(false)
                })
        };
        if let Some(sid) = report_surface {
            self.report_mouse_event(sid, x, y, 0, false, true);
        } else {
            let empty = if let Some(sel) = &mut self.text_selection {
                sel.dragging = false;
                sel.is_empty()
            } else {
                false
            };
            if empty {
                // bypass 단일(빈) 클릭은 커서 이동 없이 선택만 클리어한다. 일반 단일
                // 클릭은 클릭 위치로 커서 이동 후 클리어.
                if !bypass {
                    self.move_cursor_to_click(x, y, terminal_rect);
                }
                self.text_selection = None;
            }
        }
        self.left_select_bypass = false;
        self.link_click_consumed = false;
        self.mark_dirty();
    }

    /// 클릭/드래그 픽셀 좌표를 해당 surface 의 viewport 1-based `(col, row)` 로 변환
    /// (마우스 리포팅 전송용). surface 를 못 찾으면 `(1, 1)`.
    fn mouse_cell_for_report(&self, surface_id: u32, x: f32, y: f32) -> (usize, usize) {
        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();
        let Some((scroll_offset, sb_len, (cols, rows))) = self
            .core_state
            .visible_terminal(surface_id)
            .map(|t| (t.scroll_offset(), t.scrollback_len(), t.dimensions()))
        else {
            return (1, 1);
        };
        let Some(rect) = self
            .state
            .surface_rect_by_id(&self.core_state, surface_id, terminal_rect)
        else {
            return (1, 1);
        };
        let point = crate::selection::pixel_to_grid(
            x,
            y,
            &rect,
            cell_w,
            cell_h,
            cols,
            rows,
            scroll_offset,
            sb_len,
        );
        let viewport_top = sb_len.saturating_sub(scroll_offset);
        let row = point
            .absolute_row
            .saturating_sub(viewport_top)
            .min(rows.saturating_sub(1))
            + 1;
        let col = point.col.min(cols.saturating_sub(1)) + 1;
        (col, row)
    }

    /// surface 의 "유효 클릭 트래킹 모드". 마우스 캡처 블랙리스트(설정 + 1Hz 캐시)에
    /// 걸린 surface 거나 hard 점유(readonly) 중이면 실제 트래킹 모드와 무관하게
    /// `None` 으로 격하해 클릭/드래그/버튼을 로컬 처리(선택·tasty 메뉴)하게 한다.
    /// hard 점유는 사용자가 그 live 앱과 상호작용할 수 없는 상태이므로, 트래킹이
    /// 켜진 채였더라도 "앱에 보고" 분기로 빠져 조용히 무동작하지 않고 항상 로컬
    /// 선택으로 떨어져야 한다(ADR-0040). **휠 경로는 이 헬퍼를 쓰지 않고** 별도로
    /// hard 점유를 조기 차단한다(`handle_mouse_wheel`).
    fn effective_click_tracking(
        &self,
        surface_id: u32,
        actual: tasty_terminal::MouseTrackingMode,
    ) -> tasty_terminal::MouseTrackingMode {
        effective_click_tracking_decision(
            self.core_state.attach.is_hard_occupied(surface_id),
            self.core_state
                .is_surface_mouse_capture_disabled(surface_id),
            actual,
        )
    }

    /// 마우스 버튼/드래그 이벤트를 트래킹 앱(PTY)에 보고한다. `button` 0=left /
    /// 1=middle / 2=right, `motion` 드래그 여부, `release` 버튼 떼기. 좌표/SGR 여부는
    /// 보고 시점에 해당 surface 에서 조회한다.
    fn report_mouse_event(
        &mut self,
        surface_id: u32,
        x: f32,
        y: f32,
        button: u8,
        motion: bool,
        release: bool,
    ) {
        let (col, row) = self.mouse_cell_for_report(surface_id, x, y);
        let sgr = self
            .core_state
            .find_terminal_by_id(surface_id)
            .map(|t| t.sgr_mouse())
            .unwrap_or(false);
        let m = &self.base.modifiers;
        let cb = mouse_report_cb(button, motion, m.shift_key(), m.alt_key(), m.control_key());
        let bytes = tasty_terminal::encode_mouse_report(sgr, cb, col, row, release);
        self.state.dispatch_intent(
            DomainIntent::SendToSurface {
                surface_id,
                payload: SendPayload::Bytes(bytes),
            }
            .from_user_shortcut("mouse_report"),
        );
        self.last_mouse_report_cell = Some((col, row));
    }

    pub(super) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta, egui_consumed: bool) {
        let overlay_open = self.state.settings_open;
        if egui_consumed {
            self.mark_dirty();
        }
        if !egui_consumed
            && !overlay_open
            && !self.state.popup_hovered
            && !self.state.banner_hovered
            && !self.state.modifier_hint_hovered
        {
            // egui-mesh surface 휠 forward (A1-S7): 포인터가 egui-mesh surface 위면
            // 스크롤 델타를 논리 포인트로 변환해 누적하고 소비한다.
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if let Some((sid, _plugin_id, _rect)) = self.egui_mesh_target_at(x, y) {
                    let (dx, dy) = match delta {
                        MouseScrollDelta::LineDelta(lx, ly) => (lx * 50.0, ly * 50.0),
                        MouseScrollDelta::PixelDelta(p) => {
                            let ppp = self.base.gpu.scale_factor().max(f32::EPSILON);
                            (p.x as f32 / ppp, p.y as f32 / ppp)
                        }
                    };
                    self.egui_mesh_push_scroll(sid, dx, dy);
                    self.mark_dirty();
                    return;
                }
                // attach mesh mirror surface 휠 forward — 위와 동형이되
                // 목적지가 원격.
                if let Some((sid, _rect)) = self.attach_mesh_target_at(x, y) {
                    let (dx, dy) = match delta {
                        MouseScrollDelta::LineDelta(lx, ly) => (lx * 50.0, ly * 50.0),
                        MouseScrollDelta::PixelDelta(p) => {
                            let ppp = self.base.gpu.scale_factor().max(f32::EPSILON);
                            (p.x as f32 / ppp, p.y as f32 / ppp)
                        }
                    };
                    self.attach_mesh_push_scroll(sid, dx, dy);
                    self.mark_dirty();
                    return;
                }
            }

            // Find the surface under the cursor, falling back to the focused surface
            let terminal_rect = self.compute_terminal_rect();
            let target_id = self
                .cursor_position
                .and_then(|pos| {
                    let (x, y) = (pos.x as f32, pos.y as f32);
                    self.state
                        .surface_id_at_position(&self.core_state, x, y, terminal_rect)
                })
                .or_else(|| self.state.focused_surface_id(&self.core_state));

            if let Some(surface_id) = target_id {
                // hard 점유(readonly)는 목표상 휠을 요구하지 않으므로 여기서 조기
                // 차단한다. 트래킹 조회(아래 `t.mouse_tracking()`)는 `effective_click_
                // tracking`을 거치지 않고 live terminal을 직접 보고, 트래킹 OFF일 때의
                // 로컬 스크롤백 분기도 live terminal을 직접 mutate한다 — hard 점유가
                // 렌더하는 것은 mirror(`readonly_view`)라 이 mutate는 화면에 반영되지
                // 않으면서 live의 scroll_offset만 조용히 어긋나, 점유 해제 직후 스크롤이
                // 튀어 보이는 회귀를 만든다. 이 두 조회/mutate 이전에 막아야 한다.
                if self.core_state.attach.is_hard_occupied(surface_id) {
                    return;
                }
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
                };
                if lines == 0 {
                    return;
                }
                let info = self.core_state.find_terminal_by_id(surface_id).map(|t| {
                    (
                        t.is_alternate_screen(),
                        t.mouse_tracking(),
                        t.sgr_mouse(),
                        t.scroll_offset(),
                        t.scrollback_len(),
                        t.dimensions(),
                    )
                });
                let Some((is_alt, tracking, sgr, scroll_offset, sb_len, (cols, rows))) = info
                else {
                    return;
                };

                if tracking != tasty_terminal::MouseTrackingMode::None {
                    // 마우스 추적이 켜져 있으면 휠을 마우스 이벤트로 전송한다 (표준
                    // 동작). alt screen 이라고 무조건 arrow 로 바꾸면, 앱(예: Claude
                    // Code)이 그 arrow 를 history 이동으로 해석해 스크롤이 깨진다.
                    let cell_w = self.base.gpu.cell_width();
                    let cell_h = self.base.gpu.cell_height();
                    let (col, row) = self
                        .cursor_position
                        .and_then(|pos| {
                            let (x, y) = (pos.x as f32, pos.y as f32);
                            let rect = self.state.surface_rect_by_id(
                                &self.core_state,
                                surface_id,
                                terminal_rect,
                            )?;
                            let point = crate::selection::pixel_to_grid(
                                x,
                                y,
                                &rect,
                                cell_w,
                                cell_h,
                                cols,
                                rows,
                                scroll_offset,
                                sb_len,
                            );
                            // viewport 기준 1-based (col, row). alt screen 은 scrollback
                            // 이 없어 absolute_row 가 곧 viewport row.
                            let viewport_top = sb_len.saturating_sub(scroll_offset);
                            let row = point
                                .absolute_row
                                .saturating_sub(viewport_top)
                                .min(rows.saturating_sub(1))
                                + 1;
                            let col = point.col.min(cols.saturating_sub(1)) + 1;
                            Some((col, row))
                        })
                        .unwrap_or((1, 1));
                    // xterm wheel button: 64 = up, 65 = down.
                    let btn = if lines > 0 { 64 } else { 65 };
                    let count = lines.unsigned_abs() as usize;
                    let bytes = encode_wheel_report(sgr, btn, col, row, count);
                    self.state.dispatch_intent(
                        DomainIntent::SendToSurface {
                            surface_id,
                            payload: SendPayload::Bytes(bytes),
                        }
                        .from_user_shortcut("mouse_wheel"),
                    );
                } else if is_alt {
                    // 마우스 추적 OFF + alt screen — alternate scroll mode: 휠을 arrow
                    // 키로 변환 (vim/less 등에서 휠 스크롤). lines 만큼 한 Vec 에 concat
                    // 후 1 Intent (큐 폭증 회피).
                    let seq: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    let count = lines.unsigned_abs() as usize;
                    let mut bytes = Vec::with_capacity(seq.len() * count);
                    for _ in 0..count {
                        bytes.extend_from_slice(seq);
                    }
                    self.state.dispatch_intent(
                        DomainIntent::SendToSurface {
                            surface_id,
                            payload: SendPayload::Bytes(bytes),
                        }
                        .from_user_shortcut("mouse_wheel"),
                    );
                } else {
                    // 일반 화면 — scrollback (UI 자체 mutate, PTY 와 무관).
                    if let Some(terminal) = self.core_state.find_terminal_by_id_mut(surface_id) {
                        if lines > 0 {
                            terminal.scroll_up(lines as usize);
                        } else if lines < 0 {
                            terminal.scroll_down((-lines) as usize);
                        }
                    }
                    self.base.dirty = true;
                }
            }
        }
    }
}

/// 마우스 휠 이벤트를 마우스 리포팅 시퀀스로 인코딩한다. `sgr` 가 true 면 SGR
/// (`ESC [ < btn ; col ; row M`), 아니면 legacy X10 (`ESC [ M` + 32-offset 3 bytes).
/// `count` 만큼 반복 발행. `btn` 은 64(up)/65(down), `col`/`row` 는 1-based.
/// winit 버튼(0=left / 1=middle / 2=right) + 드래그/modifier → 마우스 리포팅 `cb` 코드.
/// shift=4 · alt(meta)=8 · ctrl=16, 드래그 motion=32. (xterm 표준 비트)
fn mouse_report_cb(button: u8, motion: bool, shift: bool, alt: bool, ctrl: bool) -> u8 {
    let mut cb = button;
    if motion {
        cb |= 32;
    }
    if shift {
        cb |= 4;
    }
    if alt {
        cb |= 8;
    }
    if ctrl {
        cb |= 16;
    }
    cb
}

fn encode_wheel_report(sgr: bool, btn: u32, col: usize, row: usize, count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..count {
        bytes.extend_from_slice(&tasty_terminal::encode_mouse_report(
            sgr, btn as u8, col, row, false,
        ));
    }
    bytes
}

/// 우클릭을 앱(PTY)에 위임할지 결정한다. 트래킹 ON 이고 Shift 가 없을 때만 위임하고
/// (ADR-0019), 트래킹 OFF 이거나 Shift+우클릭이면 tasty 컨텍스트 메뉴로 우회한다 (ADR-0022).
fn right_click_delegates_to_app(tracking: tasty_terminal::MouseTrackingMode, shift: bool) -> bool {
    tracking != tasty_terminal::MouseTrackingMode::None && !shift
}

/// 좌클릭 release 를 tracking 앱(PTY)에 보고할지 결정한다. 트래킹이 꺼져 있으면 항상
/// 안 보고. 트래킹이 켜져 있어도 이번 클릭의 press 가 링크오픈으로 로컬 소비됐다면
/// (`link_click_consumed`) 마찬가지로 안 보고한다 — press 는 tasty 가 로컬 소비(링크
/// 오픈)했는데 release 만 앱에 단독 전달되면, 자체 URL-오픈 기능이 있는 TUI 앱(vim의
/// `gx`/netrw, tmux url 플러그인 등)이 이를 클릭으로 해석해 링크를 중복으로 열 수
/// 있다(TODO 24).
fn should_report_release_to_app(
    tracking: tasty_terminal::MouseTrackingMode,
    link_click_consumed: bool,
) -> bool {
    tracking != tasty_terminal::MouseTrackingMode::None && !link_click_consumed
}

/// 좌클릭을 tasty 로컬 텍스트 선택으로 처리할지(true), 아니면 앱(PTY)에 보고할지(false)
/// 결정한다. 트래킹 OFF 면 항상 로컬. 트래킹 ON 이면 press 시점 Shift 우회이거나
/// (`shift`), 이미 우회 시퀀스가 활성(`bypass_active`)일 때만 로컬 — 그 외엔 앱에 보고.
/// `bypass_active` 는 press 에서 set 된 `left_select_bypass` 로, motion/release 가
/// Shift 재검사 없이 이 플래그만으로 같은 결정을 유지하게 한다 (멀티클릭 dragging=false 포함).
fn left_click_local_select(
    tracking: tasty_terminal::MouseTrackingMode,
    shift: bool,
    bypass_active: bool,
) -> bool {
    tracking == tasty_terminal::MouseTrackingMode::None || shift || bypass_active
}

/// `effective_click_tracking`(MainView 메서드)의 순수 결정 로직. hard 점유(readonly)
/// 이거나 마우스 캡처 블랙리스트에 걸리면 실제 트래킹 모드와 무관하게 `None`으로
/// 격하한다 — 그래야 `left_click_local_select`가 항상 로컬 선택으로 떨어진다.
fn effective_click_tracking_decision(
    is_hard_occupied: bool,
    capture_disabled: bool,
    actual: tasty_terminal::MouseTrackingMode,
) -> tasty_terminal::MouseTrackingMode {
    if is_hard_occupied || capture_disabled {
        tasty_terminal::MouseTrackingMode::None
    } else {
        actual
    }
}

/// `handle_cursor_moved`(MainView 메서드)의 early-return 판정을 뽑아낸 순수
/// 로직. 참이면 이번 프레임은 mesh surface 판정을 건너뛰고 `update_mesh_hover(None)`을
/// 호출한다 — `MainView`는 실제 GPU/winit 컨텍스트 없이 구성할 수 없어(`GpuState`가
/// 목/헤드리스 생성자를 제공하지 않음) `handle_cursor_moved` 자체를 단위 테스트로 직접
/// 구동할 수 없다. 대신 이 조건과 `mesh_hover_transition`을 각각 단위 테스트해 둘의
/// 조합(조건이 참일 때 `update_mesh_hover(None)`이 호출되고, 그 결과 슬롯이
/// `Some(prev)` → `None`으로 전이하며 `PointerGone`이 발생함)으로 배선을 간접 검증한다.
fn cursor_moved_should_short_circuit(
    egui_consumed: bool,
    overlay_open: bool,
    popup_hovered: bool,
    banner_hovered: bool,
    modifier_hint_hovered: bool,
) -> bool {
    egui_consumed || overlay_open || popup_hovered || banner_hovered || modifier_hint_hovered
}

/// `update_mesh_hover`(MainView 메서드)의 순수 결정 로직. 슬롯의 다음 값과,
/// `PointerGone` 을 보내야 할 이전 대상(있다면)을 반환한다. 대상이 안 바뀌면(같은
/// surface 에 머무르거나 계속 `None`) `PointerGone` 을 보내지 않는다.
fn mesh_hover_transition(
    current: Option<MeshHoverTarget>,
    new: Option<MeshHoverTarget>,
) -> (Option<MeshHoverTarget>, Option<MeshHoverTarget>) {
    if current == new {
        (current, None)
    } else {
        (new, current)
    }
}

#[cfg(test)]
mod wheel_tests {
    use super::encode_wheel_report;

    #[test]
    fn sgr_encodes_button_col_row() {
        assert_eq!(encode_wheel_report(true, 64, 3, 5, 1), b"\x1b[<64;3;5M");
        assert_eq!(encode_wheel_report(true, 65, 10, 20, 1), b"\x1b[<65;10;20M");
    }

    #[test]
    fn x10_encodes_with_32_offset() {
        // btn=64 → 96, col=1 → 33, row=1 → 33.
        assert_eq!(
            encode_wheel_report(false, 64, 1, 1, 1),
            vec![0x1b, b'[', b'M', 96, 33, 33]
        );
    }

    #[test]
    fn count_repeats_sequence() {
        assert_eq!(
            encode_wheel_report(true, 64, 1, 1, 3),
            b"\x1b[<64;1;1M\x1b[<64;1;1M\x1b[<64;1;1M"
        );
    }

    #[test]
    fn x10_clamps_large_coords() {
        // 32 + 300 = 332 → clamp 255.
        let out = encode_wheel_report(false, 64, 300, 1, 1);
        assert_eq!(out[4], 255);
    }
}

#[cfg(test)]
mod right_click_tests {
    use super::right_click_delegates_to_app;
    use tasty_terminal::MouseTrackingMode;

    #[test]
    fn tracking_on_no_shift_delegates_to_app() {
        // ADR-0019: 트래킹 ON + Shift 없음 → 앱에 위임 (tasty 메뉴 안 뜸).
        assert!(right_click_delegates_to_app(
            MouseTrackingMode::Click,
            false
        ));
        assert!(right_click_delegates_to_app(
            MouseTrackingMode::CellMotion,
            false
        ));
        assert!(right_click_delegates_to_app(
            MouseTrackingMode::AllMotion,
            false
        ));
    }

    #[test]
    fn tracking_on_with_shift_bypasses_to_menu() {
        // ADR-0022: 트래킹 ON + Shift → 앱에 보고 안 하고 tasty 컨텍스트 메뉴로 우회.
        assert!(!right_click_delegates_to_app(
            MouseTrackingMode::Click,
            true
        ));
        assert!(!right_click_delegates_to_app(
            MouseTrackingMode::AllMotion,
            true
        ));
    }

    #[test]
    fn tracking_off_always_shows_menu() {
        // 트래킹 OFF: Shift 유무와 무관하게 메뉴 (위임 안 함).
        assert!(!right_click_delegates_to_app(
            MouseTrackingMode::None,
            false
        ));
        assert!(!right_click_delegates_to_app(MouseTrackingMode::None, true));
    }
}

#[cfg(test)]
mod left_click_tests {
    use super::{effective_click_tracking_decision, left_click_local_select};
    use tasty_terminal::MouseTrackingMode;

    #[test]
    fn hard_occupied_forces_tracking_none_even_if_actually_on() {
        // hard 점유(readonly): live 트래킹이 켜져 있어도(AllMotion 등) 항상 None 으로
        // 격하해 로컬 선택으로 떨어져야 한다 — 조용한 무동작(앱 보고 스킵)을 방지.
        assert_eq!(
            effective_click_tracking_decision(true, false, MouseTrackingMode::AllMotion),
            MouseTrackingMode::None
        );
        assert_eq!(
            effective_click_tracking_decision(true, false, MouseTrackingMode::CellMotion),
            MouseTrackingMode::None
        );
    }

    #[test]
    fn hard_occupied_and_capture_disabled_both_force_none() {
        // 두 조건은 or — 어느 한쪽만 참이어도 None.
        assert_eq!(
            effective_click_tracking_decision(true, true, MouseTrackingMode::Click),
            MouseTrackingMode::None
        );
        assert_eq!(
            effective_click_tracking_decision(false, true, MouseTrackingMode::Click),
            MouseTrackingMode::None
        );
    }

    #[test]
    fn not_occupied_and_not_disabled_keeps_actual_tracking() {
        assert_eq!(
            effective_click_tracking_decision(false, false, MouseTrackingMode::CellMotion),
            MouseTrackingMode::CellMotion
        );
        assert_eq!(
            effective_click_tracking_decision(false, false, MouseTrackingMode::None),
            MouseTrackingMode::None
        );
    }

    #[test]
    fn tracking_on_shift_press_starts_local_select() {
        // 트래킹 ON + Shift+press → 로컬 선택 시작 (앱에 보고 안 함).
        assert!(left_click_local_select(
            MouseTrackingMode::AllMotion,
            true,
            false
        ));
        assert!(left_click_local_select(
            MouseTrackingMode::CellMotion,
            true,
            false
        ));
        assert!(left_click_local_select(
            MouseTrackingMode::Click,
            true,
            false
        ));
    }

    #[test]
    fn tracking_on_no_shift_reports_to_app() {
        // 트래킹 ON + Shift 없음 + bypass 비활성 → 앱에 보고 (로컬 선택 안 함).
        assert!(!left_click_local_select(
            MouseTrackingMode::AllMotion,
            false,
            false
        ));
        assert!(!left_click_local_select(
            MouseTrackingMode::CellMotion,
            false,
            false
        ));
    }

    #[test]
    fn tracking_on_bypass_active_stays_local() {
        // 트래킹 ON + bypass 활성(press 에서 set) → motion/release 는 로컬 경로 유지.
        // shift 가 false 여도(드래그 중 Shift 해제) bypass 가 결정을 유지한다.
        // 멀티클릭 word/line(dragging=false)도 같은 플래그로 로컬 유지 — 앱에 안 샌다.
        assert!(left_click_local_select(
            MouseTrackingMode::AllMotion,
            false,
            true
        ));
    }

    #[test]
    fn tracking_off_always_local() {
        // 트래킹 OFF → 항상 로컬 (Shift/bypass 무관).
        assert!(left_click_local_select(
            MouseTrackingMode::None,
            false,
            false
        ));
        assert!(left_click_local_select(
            MouseTrackingMode::None,
            true,
            false
        ));
        assert!(left_click_local_select(
            MouseTrackingMode::None,
            false,
            true
        ));
    }
}

#[cfg(test)]
mod link_click_release_tests {
    use super::should_report_release_to_app;

    #[test]
    fn release_report_skipped_when_press_consumed_by_link_click() {
        // tracking ON + 이번 클릭의 press가 링크오픈으로 소비됐다면 release도 보고 안 함
        assert!(!should_report_release_to_app(
            tasty_terminal::MouseTrackingMode::Click,
            true,
        ));
    }

    #[test]
    fn release_report_still_sent_for_normal_click_with_tracking_on() {
        // 기존 동작 보존: 링크오픈이 아닌 일반 클릭은 tracking ON 이면 그대로 보고
        assert!(should_report_release_to_app(
            tasty_terminal::MouseTrackingMode::Click,
            false,
        ));
    }

    #[test]
    fn release_report_skipped_when_tracking_off_regardless() {
        assert!(!should_report_release_to_app(
            tasty_terminal::MouseTrackingMode::None,
            false,
        ));
    }
}

#[cfg(test)]
mod mesh_hover_tests {
    use super::{MeshHoverTarget, mesh_hover_transition};

    #[test]
    fn same_target_is_not_a_transition() {
        // 같은 local surface 에 계속 머무르면 PointerGone 을 보내지 않는다.
        let (next, gone) = mesh_hover_transition(
            Some(MeshHoverTarget::Local(1)),
            Some(MeshHoverTarget::Local(1)),
        );
        assert_eq!(next, Some(MeshHoverTarget::Local(1)));
        assert_eq!(gone, None);
    }

    #[test]
    fn none_to_none_is_not_a_transition() {
        let (next, gone) = mesh_hover_transition(None, None);
        assert_eq!(next, None);
        assert_eq!(gone, None);
    }

    #[test]
    fn entering_a_surface_from_none_sends_no_gone() {
        // 이전에 아무것도 hover 하지 않았으면 보낼 PointerGone 대상이 없다.
        let (next, gone) = mesh_hover_transition(None, Some(MeshHoverTarget::Local(1)));
        assert_eq!(next, Some(MeshHoverTarget::Local(1)));
        assert_eq!(gone, None);
    }

    #[test]
    fn leaving_window_sends_gone_for_previous_local_surface() {
        // CursorLeft 등으로 target 이 None 이 되면 이전 local surface 에 PointerGone.
        let (next, gone) = mesh_hover_transition(Some(MeshHoverTarget::Local(1)), None);
        assert_eq!(next, None);
        assert_eq!(gone, Some(MeshHoverTarget::Local(1)));
    }

    #[test]
    fn switching_between_local_surfaces_sends_gone_for_the_old_one() {
        // 창을 벗어나지 않고 다른 mesh surface 로 바로 넘어가도 이전 surface 에
        // PointerGone 을 보낸다.
        let (next, gone) = mesh_hover_transition(
            Some(MeshHoverTarget::Local(1)),
            Some(MeshHoverTarget::Local(2)),
        );
        assert_eq!(next, Some(MeshHoverTarget::Local(2)));
        assert_eq!(gone, Some(MeshHoverTarget::Local(1)));
    }

    #[test]
    fn switching_from_local_to_attach_sends_gone_for_local() {
        let (next, gone) = mesh_hover_transition(
            Some(MeshHoverTarget::Local(1)),
            Some(MeshHoverTarget::Attach(9)),
        );
        assert_eq!(next, Some(MeshHoverTarget::Attach(9)));
        assert_eq!(gone, Some(MeshHoverTarget::Local(1)));
    }
}

/// `handle_cursor_moved`의 early-return 배선을 간접 검증한다. `MainView`는
/// 실제 GPU/winit 컨텍스트 없이 구성 불가능해 `handle_cursor_moved` 자체를 직접
/// 구동하는 단위 테스트는 이 코드베이스에 전례가 없다(다른 스테이트풀 메서드들도
/// 전부 순수 결정 로직만 추출해 테스트한다 — `mesh_hover_tests`, `right_click_tests`
/// 등). 대신 (1) early-return 판정이 참인 조건(Case A/B 포함)과 (2) 그 결과
/// `update_mesh_hover(None)`이 호출됐을 때의 상태 전이를 각각 단위 테스트해, 실제
/// `handle_cursor_moved` 코드(`self.update_mesh_hover(None)`이 early-return 블록
/// 안에서 `return` 이전에 호출됨)와 조합하면 배선이 성립함을 보인다.
#[cfg(test)]
mod cursor_moved_early_return_tests {
    use super::{MeshHoverTarget, cursor_moved_should_short_circuit, mesh_hover_transition};

    #[test]
    fn case_a_egui_consumed_short_circuits() {
        // Case A: mesh surface 에서 host UI chrome(사이드바 등)으로 넘어가는 전환
        // 이벤트 자체가 egui_consumed=true 다.
        assert!(cursor_moved_should_short_circuit(
            true, false, false, false, false
        ));
    }

    #[test]
    fn case_b_overlay_open_short_circuits() {
        // Case B: 설정창 등 오버레이가 열려 있는 동안은 좌표와 무관하게 항상 참.
        assert!(cursor_moved_should_short_circuit(
            false, true, false, false, false
        ));
    }

    #[test]
    fn popup_banner_modifier_hint_each_short_circuit() {
        assert!(cursor_moved_should_short_circuit(
            false, false, true, false, false
        ));
        assert!(cursor_moved_should_short_circuit(
            false, false, false, true, false
        ));
        assert!(cursor_moved_should_short_circuit(
            false, false, false, false, true
        ));
    }

    #[test]
    fn no_flag_set_does_not_short_circuit() {
        assert!(!cursor_moved_should_short_circuit(
            false, false, false, false, false
        ));
    }

    #[test]
    fn short_circuit_frame_transitions_hovered_mesh_target_to_none_with_pointer_gone() {
        // early-return 조건이 참인 프레임에서 `update_mesh_hover(None)`이 호출되면
        // (수정된 handle_cursor_moved 배선), 직전까지 hover 중이던 local mesh surface
        // 는 이 전환 이벤트 자체에서 None 으로 전이하고 PointerGone 이 1회 발생해야
        // 한다 — 수정 전에는 이 호출 자체가 생략되어 슬롯이 `Some(Local(sid))`로 남았다.
        assert!(cursor_moved_should_short_circuit(
            true, false, false, false, false
        ));
        let (next, gone) = mesh_hover_transition(
            Some(MeshHoverTarget::Local(7)),
            None, /* update_mesh_hover(None) */
        );
        assert_eq!(next, None);
        assert_eq!(gone, Some(MeshHoverTarget::Local(7)));
    }

    #[test]
    fn short_circuit_frame_is_idempotent_when_already_none() {
        // 오버레이가 열려있는 동안 여러 CursorMoved 가 연달아 이 분기를 타도(Case B),
        // mesh_hover_transition 이 멱등이라 이미 None 인 슬롯에 대해서는 PointerGone 을
        // 중복 발생시키지 않는다(thrashing 방지).
        let (next, gone) = mesh_hover_transition(None, None);
        assert_eq!(next, None);
        assert_eq!(gone, None);
    }
}
