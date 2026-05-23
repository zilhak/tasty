use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use super::{DividerDrag, DividerDragKind, HoveredLink, MainWindow};
use crate::model::PhysicalPx;
use crate::settings::LinkModifier;
use crate::terminal_link::{self, LinkHighlight};
use crate::theme;
use crate::window::Window;

impl MainWindow {
    /// 현재 마우스 좌표와 수식키 상태로 hovered_link를 갱신한다.
    /// 변경이 있으면 true를 반환 (렌더 dirty 플래그를 켜기 위함).
    pub(crate) fn update_hovered_link(&mut self) -> bool {
        let engine = &mut self.engine_state;
        let _ = &mut *engine;
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

        let new_link = if !matches_mods {
            None
        } else if self.state.settings_open || self.state.popup_hovered {
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
        let engine = &self.engine_state;
        let _ = engine;
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
        let surface_id = self.state.surface_id_at_position(engine, x, y, terminal_rect)?;
        let terminal = engine.find_terminal_by_id(surface_id)?;
        let surface_rect = self.state.surface_rect_by_id(engine, surface_id, terminal_rect)?;

        let (cols, rows) = terminal.surface().dimensions();
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
            fg: th.blue.to_float(),
            bg: th.selection_bg,
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
        if egui_consumed || overlay_open || self.state.popup_hovered {
            if self.hovered_link.take().is_some() {
                self.mark_dirty();
            }
            self.mark_dirty();
            return;
        }

        let terminal_rect = self.compute_terminal_rect();
        let x = position.x as f32;
        let y = position.y as f32;

        if self.update_hovered_link() {
            self.mark_dirty();
        }

        // Handle selection drag
        if self.left_mouse_down && self.dragging_divider.is_none() {
            let is_dragging = self.text_selection.as_ref().map_or(false, |s| s.dragging);
            if is_dragging {
                if let Some((point, _)) = self.mouse_to_grid(x, y, &terminal_rect) {
                    if let Some(sel) = &mut self.text_selection {
                        sel.cursor = point;
                    }
                    self.mark_dirty();
                }
            }
        }

        if let Some(drag) = self.dragging_divider {
            let cell_w = self.base.gpu.cell_width();
            let cell_h = self.base.gpu.cell_height();
            let engine = &mut self.engine_state;
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
                drop(engine);
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
        let engine = &mut self.engine_state;
        let _ = &mut *engine;
        let overlay_open = self.state.settings_open;
        if egui_consumed || overlay_open || self.state.popup_hovered {
            // Even when egui consumes the event (e.g. egui-rendered panels),
            // we still need to update pane focus on left-click within the terminal area.
            if egui_consumed
                && !overlay_open
                && button == MouseButton::Left
                && button_state == ElementState::Pressed
            {
                let terminal_rect = self.compute_terminal_rect();
                if let Some(pos) = self.cursor_position {
                    let (x, y) = (pos.x as f32, pos.y as f32);
                    if terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                        if self.state.focus_pane_at_position(engine, x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                        // Also update surface focus within the tab so that
                        // clicking on an egui-rendered panel (Explorer, Markdown)
                        // correctly moves keyboard target to that surface.
                        if self.state.focus_surface_at_position(engine, x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                    }
                }
            }
            if button_state == ElementState::Released {
                self.dragging_divider = None;
                self.left_mouse_down = false;
            }
            if egui_consumed {
                self.mark_dirty();
            }
            return;
        }
        if button == MouseButton::Right && button_state == ElementState::Pressed {
            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                    return;
                }
                let Some(surface_id) = self.state.surface_id_at_position(engine, x, y, terminal_rect)
                else {
                    return;
                };
                if engine.find_terminal_by_id(surface_id).is_none() {
                    return;
                }
                let sf = self.base.gpu.scale_factor() as f32;
                self.state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::TerminalSurface {
                        surface_id,
                        x: x / sf,
                        y: y / sf,
                    });
                self.mark_dirty();
            }
            return;
        }
        if button == MouseButton::Left {
            if button_state == ElementState::Pressed {
                self.left_mouse_down = true;
            } else {
                self.left_mouse_down = false;
            }

            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                // 수식키+클릭은 무조건 링크 클릭 동작으로 라우팅.
                // 링크 위면 열고, 링크 위가 아니면 아무것도 안 함 (selection 시작 안 함).
                let modifier =
                    LinkModifier::parse(&engine.settings.general.link_click_modifier);
                let mods = &self.base.modifiers;
                let link_mods_match = !matches!(modifier, LinkModifier::None)
                    && modifier.matches(mods.control_key(), mods.alt_key(), mods.super_key());
                if link_mods_match && button_state == ElementState::Pressed {
                    if terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                        if self.state.focus_pane_at_position(engine, x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                        if self.state.focus_surface_at_position(engine, x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                    }
                    if let Some(hovered) = self.hovered_link.clone() {
                        match crate::file_dispatch::parse_link(&hovered.uri) {
                            crate::file_dispatch::LinkKind::FileTarget(path) => {
                                crate::file_dispatch::dispatch_file_target(
                                    &mut self.state,
                                    engine,
                                    crate::file::format::FileTarget::new(path),
                                    crate::file::format::DetectDepth::Deep,
                                );
                            }
                            crate::file_dispatch::LinkKind::External(uri) => {
                                terminal_link::open_uri(&uri);
                            }
                        }
                    }
                    self.mark_dirty();
                    return;
                }
                if button_state == ElementState::Pressed {
                    let threshold = 4.0;
                    let pane_div = self
                        .state
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
                        let old_surface = self.state.focused_surface_id(engine);
                        if self.state.focus_pane_at_position(engine, x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                        if self.state.focus_surface_at_position(engine, x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                        if self.ime_preedit.is_some()
                            && self.state.focused_surface_id(engine) != old_surface
                        {
                            self.flush_ime_preedit();
                        }

                        // Start text selection (only if not mouse-tracking or Shift held)
                        let mouse_tracking = self
                            .state
                            .focused_terminal(engine)
                            .map(|t| t.mouse_tracking())
                            .unwrap_or(tasty_terminal::MouseTrackingMode::None);
                        let shift = self.base.modifiers.shift_key();
                        if mouse_tracking == tasty_terminal::MouseTrackingMode::None || shift {
                            if shift {
                                self.extend_selection(x, y, &terminal_rect);
                            } else {
                                self.start_selection(x, y, &terminal_rect);
                            }
                        }
                    }
                } else if button_state == ElementState::Released {
                    if self.dragging_divider.is_some() {
                        self.dragging_divider = None;
                        self.state.resize_all(engine, 
                            terminal_rect,
                            self.base.gpu.cell_width(),
                            self.base.gpu.cell_height(),
                        );
                        self.base.dirty = true;
                    }
                    // Finish selection drag
                    if let Some(sel) = &mut self.text_selection {
                        sel.dragging = false;
                        if sel.is_empty() {
                            // Single click (no drag) — move cursor to clicked position
                            self.move_cursor_to_click(x, y, &terminal_rect);
                            self.text_selection = None;
                        }
                    }
                    self.mark_dirty();
                }
            }
        }
    }

    pub(super) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta, egui_consumed: bool) {
        let engine = &mut self.engine_state;
        let _ = &mut *engine;
        let overlay_open = self.state.settings_open;
        if egui_consumed {
            self.mark_dirty();
        }
        if !egui_consumed && !overlay_open && !self.state.popup_hovered {
            // Find the surface under the cursor, falling back to the focused surface
            let terminal_rect = self.compute_terminal_rect();
            let target_id = self
                .cursor_position
                .and_then(|pos| {
                    let (x, y) = (pos.x as f32, pos.y as f32);
                    self.state.surface_id_at_position(engine, x, y, terminal_rect)
                })
                .or_else(|| self.state.focused_surface_id(engine));

            if let Some(surface_id) = target_id {
                if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y as i32,
                        MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
                    };
                    if terminal.is_alternate_screen() {
                        if lines > 0 {
                            for _ in 0..lines.unsigned_abs() {
                                terminal.send_bytes(b"\x1b[A");
                            }
                        } else if lines < 0 {
                            for _ in 0..lines.unsigned_abs() {
                                terminal.send_bytes(b"\x1b[B");
                            }
                        }
                    } else {
                        if lines > 0 {
                            terminal.scroll_up(lines as usize);
                        } else if lines < 0 {
                            terminal.scroll_down((-lines) as usize);
                        }
                        self.base.dirty = true;
                    }
                }
            }
        }
    }
}
