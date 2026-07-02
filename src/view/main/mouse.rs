use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use super::{DividerDrag, DividerDragKind, HoveredLink, MainView};
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
        let prev = self.hovered_link.as_ref().map(|h| {
            (
                h.surface_id,
                h.highlight.start_col,
                h.highlight.end_col,
                h.highlight.absolute_row,
            )
        });

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
            != new_link.as_ref().map(|h| {
                (
                    h.surface_id,
                    h.highlight.start_col,
                    h.highlight.end_col,
                    h.highlight.absolute_row,
                )
            });
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
            start_col: span.start_col,
            end_col: span.end_col,
            absolute_row: span.absolute_row,
            fg: th.accent_primary().to_gpu_rgba(),
            bg: th.selection_bg.to_gpu_rgba(),
        };
        Some(HoveredLink {
            surface_id,
            uri: span.uri,
            highlight,
        })
    }

    pub(super) fn handle_cursor_moved(
        &mut self,
        position: winit::dpi::PhysicalPosition<f64>,
        egui_consumed: bool,
    ) {
        self.cursor_position = Some(position);
        let overlay_open = self.state.settings_open;
        if egui_consumed
            || overlay_open
            || self.state.popup_hovered
            || self.state.banner_hovered
            || self.state.modifier_hint_hovered
        {
            // 콘텐츠/오버레이 위에서는 리사이즈 커서를 띄우지 않는다(콘텐츠 우선).
            // early-return 경로에서도 반드시 리셋해야 가장자리→콘텐츠 이동 시 ↔ 커서가
            // 남지 않는다.
            self.state.pending_resize_cursor = None;
            if self.hovered_link.take().is_some() {
                self.mark_dirty();
            }
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
        if let Some((sid, _plugin_id, rect)) = self.egui_mesh_target_at(x, y) {
            self.egui_mesh_push_pointer_moved(sid, rect, x, y);
            self.mark_dirty();
            return;
        }

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

    pub(super) fn handle_mouse_input(
        &mut self,
        button_state: ElementState,
        button: MouseButton,
        egui_consumed: bool,
    ) {
        // 통합 리사이즈 hit-test (콘텐츠 우선 입력모델). 모든 egui 인터랙티브
        // 콘텐츠(사이드바 버튼·캡션 버튼·상태바 등)가 자동으로 우선권을 가지므로
        // (`!egui_consumed` && 오버레이류 없음), 좌클릭 press 가 창 가장자리 margin
        // 안일 때만 OS 리사이즈를 시작한다 — carve-out 불필요. egui_consumed
        // early-return 보다 위에 두되 `!egui_consumed` 와 오버레이 가드를 모두 검사한다.
        // macOS 는 네이티브 데코 창(`window_chrome::apply_csd_attributes`)이라 OS 가
        // 가장자리 리사이즈를 처리하므로 이 경로를 타지 않는다(cfg 가드).
        #[cfg(not(target_os = "macos"))]
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
                return;
            }
        }

        let overlay_open = self.state.settings_open;

        // click-to-activate swallow: 비활성 surface 를 좌클릭(press)하면 — 그 위에
        // 배너/egui 위젯이 있든 없든 — 그 첫 클릭은 "surface 전환" 이 통째로 소비한다
        // (macOS click-to-activate 모델). modal(`overlay_open`)·popup(`popup_hovered`)은
        // surface 비소속 상위 레이어라 이 전환보다 먼저 배제한다. banner/divider/terminal
        // 은 전환보다 아래 — 배너를 클릭해도 소속 surface 로 포커스가 간다. 전환이
        // 클릭을 소비하면 그 클릭은 selection/cursor/마우스 리포트로 흐르지 않는다(첫
        // 클릭은 활성화에만 쓰이고 한 번 더 클릭해야 동작). docs/architecture/input-layer.md.
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
                return;
            }
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

        // egui-mesh surface 입력 forward (A1-S7): 포인터가 egui-mesh surface 위면 버튼
        // 이벤트를 surface-local 좌표로 누적해 다음 set_context 로 보내고 소비한다.
        // (host 가 받은 실제 사용자 입력만 forward — identity 경계.)
        if let Some(pos) = self.cursor_position {
            let (x, y) = (pos.x as f32, pos.y as f32);
            if let Some((sid, _plugin_id, rect)) = self.egui_mesh_target_at(x, y) {
                let pressed = super::egui_mesh::is_pressed(button_state);
                if !pressed {
                    self.left_mouse_down = false;
                }
                self.egui_mesh_push_pointer_button(sid, rect, x, y, button, pressed);
                self.mark_dirty();
                return;
            }
        }

        if button == MouseButton::Right {
            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
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
                    // 비-terminal surface (markdown/image/explorer/html 등) — terminal
                    // 의 mouse-tracking 위임(ADR-0019/0022)은 해당 없음. T9 surface
                    // 컨텍스트 메뉴(잘라내기/여기로 이동 + copy surface id)를 띄운다.
                    if button_state == ElementState::Pressed {
                        let sf = self.base.gpu.scale_factor();
                        self.state.dialogs.pending_native_menu =
                            Some(crate::state::PendingNativeMenu::Surface {
                                surface_id,
                                x: x / sf,
                                y: y / sf,
                            });
                        self.mark_dirty();
                    }
                    return;
                };
                // 블랙리스트면 None 으로 격하 → 우클릭이 tasty 컨텍스트 메뉴로 빠진다.
                let tracking = self.effective_click_tracking(surface_id, tracking);
                let shift = self.base.modifiers.shift_key();
                // 트래킹 ON + Shift 없음: 우클릭을 앱에 보고 (ADR-0019 앱 위임 유지).
                // Shift+우클릭은 앱에 보고하지 않고 tasty 컨텍스트 메뉴로 우회한다 (ADR-0022
                // — 앱 위임을 깨지 않는 opt-in modifier 우회, xterm/iTerm2 표준 관례).
                // press·release 모두 report 경로로 새지 않도록 Shift 시 분기를 먼저 빠진다.
                if right_click_delegates_to_app(tracking, shift) {
                    // 트래킹 앱이 마우스를 캡처 중이라 우클릭이 앱으로 간다 — 텍스트 선택은
                    // Shift+드래그, tasty 메뉴는 Shift+우클릭으로 우회 가능함을 트래킹 세션당
                    // 1회 안내한다(Pressed 에서만, 설정 ON 일 때, ADR-0022 ②). 좌클릭 보고
                    // 경로와 같은 take_mouse_capture_hint() 를 공유해 먼저 발생한 쪽만 뜬다.
                    if button_state == ElementState::Pressed
                        && self.core_state.settings.general.mouse_capture_hint
                    {
                        let show = self
                            .core_state
                            .find_terminal_by_id(surface_id)
                            .is_some_and(|t| t.take_mouse_capture_hint());
                        if show {
                            self.state
                                .banners
                                .push(crate::adapters::ui::BannerState::persistent(
                                    crate::adapters::ui::banner::defs::BANNER_MOUSE_CAPTURE,
                                    crate::adapters::ui::BannerScope::Surface(surface_id),
                                ));
                        }
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
            return;
        }
        if button == MouseButton::Middle {
            // 트래킹 ON 에서만 미들클릭 보고 (트래킹 OFF 는 현재 무동작 유지).
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
            return;
        }
        if button == MouseButton::Left {
            if button_state == ElementState::Pressed {
                self.left_mouse_down = true;
                // mouse drag 시작은 vi copy mode 와 충돌 — 자동 종료. (R7)
                if self.vi_copy.is_some() {
                    self.vi_copy = None;
                    self.base.dirty = true;
                }
            } else {
                self.left_mouse_down = false;
            }

            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                // 수식키+클릭은 무조건 링크 클릭 동작으로 라우팅.
                // 링크 위면 열고, 링크 위가 아니면 아무것도 안 함 (selection 시작 안 함).
                let modifier =
                    LinkModifier::parse(&self.core_state.settings.general.link_click_modifier);
                let mods = &self.base.modifiers;
                let link_mods_match = !matches!(modifier, LinkModifier::None)
                    && modifier.matches(mods.control_key(), mods.alt_key(), mods.super_key());
                if link_mods_match && button_state == ElementState::Pressed {
                    if terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                        let engine = &mut self.core_state;
                        let changed_pane =
                            self.state
                                .focus_pane_at_position(engine, x, y, terminal_rect);
                        let changed_surf =
                            self.state
                                .focus_surface_at_position(engine, x, y, terminal_rect);
                        if changed_pane || changed_surf {
                            self.base.dirty = true;
                        }
                    }
                    if let Some(hovered) = self.hovered_link.clone() {
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
                    return;
                }
                if button_state == ElementState::Pressed {
                    let threshold = 4.0;
                    let engine = &mut self.core_state;
                    let pane_div =
                        self.state
                            .find_pane_divider_at(engine, x, y, terminal_rect, threshold);
                    let surf_div =
                        self.state
                            .find_surface_divider_at(engine, x, y, terminal_rect, threshold);
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
                        let (need_flush, mouse_tracking) = {
                            let old_surface = self.state.focused_surface_id(engine);
                            let changed_pane =
                                self.state
                                    .focus_pane_at_position(engine, x, y, terminal_rect);
                            let changed_surf =
                                self.state
                                    .focus_surface_at_position(engine, x, y, terminal_rect);
                            if changed_pane || changed_surf {
                                self.base.dirty = true;
                            }
                            let ime_active = self.ime_preedit.is_some();
                            let need_flush =
                                ime_active && self.state.focused_surface_id(engine) != old_surface;
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
                        // 블랙리스트면 None 으로 격하 → 좌클릭이 로컬 텍스트 선택으로
                        // 빠지고 앱 보고/캡처 안내 배너 경로엔 진입하지 않는다.
                        let mouse_tracking = self
                            .state
                            .focused_surface_id(&self.core_state)
                            .map(|sid| self.effective_click_tracking(sid, mouse_tracking))
                            .unwrap_or(mouse_tracking);
                        let shift = self.base.modifiers.shift_key();
                        if mouse_tracking != tasty_terminal::MouseTrackingMode::None {
                            if left_click_local_select(mouse_tracking, shift, false) {
                                // 트래킹 ON + Shift: 앱에 보고하지 않고 로컬 선택을 시작한다
                                // (xterm/iTerm2 표준 modifier 우회). press 시점 1회 판정을
                                // left_select_bypass 로 release 까지 유지 — motion/release 는
                                // 이 플래그로 라우팅한다(드래그 중 Shift 해제·멀티클릭 무관).
                                // 트래킹 ON 엔 이전 로컬 앵커가 없어 extend 가 아니라 start.
                                self.left_select_bypass = true;
                                self.start_selection(x, y, &terminal_rect);
                            } else {
                                // 트래킹 ON + Shift 없음: 버튼 press 를 앱에 보고 (ADR-0019 앱 위임).
                                // 단, 트래킹 진입 후 첫 캡처 상호작용이면 "마우스 캡처 중 —
                                // Shift 로 우회 가능" 안내를 1회 띄운다. 우클릭 경로와 같은
                                // take_mouse_capture_hint() 를 공유하므로 좌·우 중 먼저 발생한
                                // 쪽만 뜬다 (설정 ON 일 때만, ADR-0022 ②).
                                if let Some(sid) = self.state.focused_surface_id(&self.core_state) {
                                    if self.core_state.settings.general.mouse_capture_hint {
                                        let show = self
                                            .core_state
                                            .find_terminal_by_id(sid)
                                            .is_some_and(|t| t.take_mouse_capture_hint());
                                        if show {
                                            self.state.banners.push(
                                                crate::adapters::ui::BannerState::persistent(
                                                    crate::adapters::ui::banner::defs::BANNER_MOUSE_CAPTURE,
                                                    crate::adapters::ui::BannerScope::Surface(sid),
                                                ),
                                            );
                                        }
                                    }
                                    self.report_mouse_event(sid, x, y, 0, false, false);
                                }
                            }
                        } else if shift {
                            self.extend_selection(x, y, &terminal_rect);
                        } else {
                            self.start_selection(x, y, &terminal_rect);
                        }
                    }
                } else if button_state == ElementState::Released {
                    if self.dragging_divider.is_some() {
                        self.dragging_divider = None;
                        let cell_w = self.base.gpu.cell_width();
                        let cell_h = self.base.gpu.cell_height();
                        let engine = &mut self.core_state;
                        self.state.resize_all(engine, terminal_rect, cell_w, cell_h);
                        self.base.dirty = true;
                    }
                    // 트래킹 ON 이면 release 를 앱에 보고, 아니면 로컬 선택 완료.
                    // 단, Shift+좌클릭 우회 시퀀스(left_select_bypass)면 — dragging 여부와
                    // 무관하게(멀티클릭 word/line 은 dragging=false) — 앱 보고를 스킵하고
                    // 로컬 선택을 확정한다. 이게 dragging 가드 대신 전용 플래그를 쓰는 이유다.
                    let bypass = self.left_select_bypass;
                    let report_surface = if bypass {
                        None
                    } else {
                        self.state
                            .focused_surface_id(&self.core_state)
                            .filter(|sid| {
                                self.core_state
                                    .find_terminal_by_id(*sid)
                                    .map(|t| {
                                        self.effective_click_tracking(*sid, t.mouse_tracking())
                                            != tasty_terminal::MouseTrackingMode::None
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
                            // bypass 단일(빈) 클릭은 커서 이동 없이 선택만 클리어한다.
                            // 일반 단일 클릭은 클릭 위치로 커서 이동 후 클리어.
                            if !bypass {
                                self.move_cursor_to_click(x, y, &terminal_rect);
                            }
                            self.text_selection = None;
                        }
                    }
                    self.left_select_bypass = false;
                    self.mark_dirty();
                }
            }
        }
    }

    /// 클릭/드래그 픽셀 좌표를 해당 surface 의 viewport 1-based `(col, row)` 로 변환
    /// (마우스 리포팅 전송용). surface 를 못 찾으면 `(1, 1)`.
    fn mouse_cell_for_report(&self, surface_id: u32, x: f32, y: f32) -> (usize, usize) {
        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();
        let Some((scroll_offset, sb_len, (cols, rows))) = self
            .core_state
            .find_terminal_by_id(surface_id)
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
    /// 걸린 surface 면 실제 트래킹 모드와 무관하게 `None` 으로 격하해 클릭/드래그/버튼을
    /// 로컬 처리(선택·tasty 메뉴)하게 한다. **휠 경로는 이 헬퍼를 쓰지 않고** 실제
    /// `mouse_tracking()` 을 그대로 보므로 휠은 블랙리스트여도 앱에 보고된다(결정 ②).
    fn effective_click_tracking(
        &self,
        surface_id: u32,
        actual: tasty_terminal::MouseTrackingMode,
    ) -> tasty_terminal::MouseTrackingMode {
        if self
            .core_state
            .is_surface_mouse_capture_disabled(surface_id)
        {
            tasty_terminal::MouseTrackingMode::None
        } else {
            actual
        }
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
    use super::left_click_local_select;
    use tasty_terminal::MouseTrackingMode;

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
