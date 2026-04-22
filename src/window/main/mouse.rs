use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use crate::model::PhysicalPx;
use crate::window::Window;
use crate::{DividerDrag, DividerDragKind};

use super::MainWindow;

impl MainWindow {
    pub(super) fn handle_cursor_moved(
        &mut self,
        position: winit::dpi::PhysicalPosition<f64>,
        egui_consumed: bool,
    ) {
        self.cursor_position = Some(position);
        let overlay_open = self.state.settings_open;
        if egui_consumed || overlay_open || self.state.popup_hovered {
            self.mark_dirty();
            return;
        }

        let terminal_rect = self.compute_terminal_rect();
        let x = position.x as f32;
        let y = position.y as f32;

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
            let changed = match drag.kind {
                DividerDragKind::Pane => {
                    self.state
                        .update_pane_divider(&drag.info, x, y, terminal_rect)
                }
                DividerDragKind::Surface => {
                    self.state
                        .update_surface_divider(&drag.info, x, y, terminal_rect)
                }
            };
            if changed {
                self.state.resize_all(
                    terminal_rect,
                    self.base.gpu.cell_width(),
                    self.base.gpu.cell_height(),
                );
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
                        if self.state.focus_pane_at_position(x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                        // Also update surface focus within the tab so that
                        // clicking on an egui-rendered panel (Explorer, Markdown)
                        // correctly moves keyboard target to that surface.
                        if self.state.focus_surface_at_position(x, y, terminal_rect) {
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
        if button == MouseButton::Left {
            if button_state == ElementState::Pressed {
                self.left_mouse_down = true;
            } else {
                self.left_mouse_down = false;
            }

            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if button_state == ElementState::Pressed {
                    let threshold = 4.0;
                    let pane_div = self
                        .state
                        .find_pane_divider_at(x, y, terminal_rect, threshold);
                    let surf_div =
                        self.state
                            .find_surface_divider_at(x, y, terminal_rect, threshold);
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
                        let old_surface = self.state.focused_surface_id();
                        if self.state.focus_pane_at_position(x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                        if self.state.focus_surface_at_position(x, y, terminal_rect) {
                            self.base.dirty = true;
                        }
                        if self.ime_preedit.is_some()
                            && self.state.focused_surface_id() != old_surface
                        {
                            self.flush_ime_preedit();
                        }

                        // Start text selection (only if not mouse-tracking or Shift held)
                        let mouse_tracking = self
                            .state
                            .focused_terminal()
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
                        self.state.resize_all(
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
                    self.state.surface_id_at_position(x, y, terminal_rect)
                })
                .or_else(|| self.state.focused_surface_id());

            if let Some(surface_id) = target_id {
                if let Some(terminal) = self.state.find_terminal_by_id_mut(surface_id) {
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
