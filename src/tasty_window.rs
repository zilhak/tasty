use std::sync::Arc;

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window};

use crate::gpu::GpuState;
use crate::model::{Rect, SplitDirection};
use crate::state::AppState;
use crate::{AppEvent, ClipboardContext, DividerDrag, DividerDragKind};

/// A single Tasty window with its own GPU state, UI state, and input state.
pub struct TastyWindow {
    pub gpu: GpuState,
    pub state: AppState,
    pub window: Arc<Window>,
    pub dirty: bool,
    pub modifiers: ModifiersState,
    pub window_focused: bool,
    pub cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    pub dragging_divider: Option<DividerDrag>,
    pub clipboard: Option<ClipboardContext>,
    pub preedit_text: String,
    pub proxy: winit::event_loop::EventLoopProxy<AppEvent>,
}

impl TastyWindow {
    pub fn new(gpu: GpuState, state: AppState, window: Arc<Window>, proxy: winit::event_loop::EventLoopProxy<AppEvent>) -> Self {
        Self {
            gpu, state, window,
            dirty: true,
            modifiers: ModifiersState::empty(),
            window_focused: true,
            cursor_position: None,
            dragging_divider: None,
            clipboard: ClipboardContext::new(),
            preedit_text: String::new(),
            proxy,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.window.request_redraw();
    }

    pub fn compute_terminal_rect(&self) -> Rect {
        let size = self.gpu.size();
        crate::model::compute_terminal_rect(
            size.width as f32, size.height as f32,
            self.state.sidebar_width, self.gpu.scale_factor(),
        )
    }

    pub fn paste_to_terminal(&mut self) {
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text {
            if text.is_empty() { return; }
            if let Some(terminal) = self.state.focused_terminal_mut() {
                if terminal.bracketed_paste() {
                    terminal.send_bytes(b"\x1b[200~");
                    terminal.send_key(&text);
                    terminal.send_bytes(b"\x1b[201~");
                } else {
                    terminal.send_key(&text);
                }
            }
        }
    }

    /// Handle a window event. `modal_active` indicates if a modal is blocking input.
    pub fn handle_window_event(&mut self, event: WindowEvent, _event_loop: &ActiveEventLoop, modal_active: bool) -> bool {
        // Let egui handle the event first
        let (egui_consumed, egui_repaint) = self.gpu.handle_egui_event(&self.window, &event);
        if egui_repaint {
            self.mark_dirty();
        }

        // If a modal is active, only allow Resized/RedrawRequested/ScaleFactorChanged
        if modal_active {
            match &event {
                WindowEvent::Resized(_) | WindowEvent::RedrawRequested | WindowEvent::ScaleFactorChanged { .. } => {}
                _ => return false,
            }
        }

        let was_dirty = self.dirty;

        match event {
            WindowEvent::Resized(new_size) => {
                self.gpu.resize(new_size);
                let terminal_rect = self.compute_terminal_rect();
                let (cols, rows) = self.gpu.grid_size_for_rect(&terminal_rect);
                self.state.update_grid_size(cols, rows);
                self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
                self.mark_dirty();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.gpu.update_scale_factor(scale_factor as f32);
                self.mark_dirty();
            }
            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                if !focused {
                    self.modifiers = ModifiersState::empty();
                }
                self.mark_dirty();
            }
            WindowEvent::Occluded(false) => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard_input(&event, egui_consumed);
            }
            WindowEvent::Ime(ime_event) => {
                self.handle_ime(ime_event, egui_consumed);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(position, egui_consumed);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
                self.window.set_cursor(CursorIcon::Default);
            }
            WindowEvent::MouseInput { state: button_state, button, .. } => {
                self.handle_mouse_input(button_state, button, egui_consumed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(delta, egui_consumed);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw(_event_loop);
            }
            _ => {}
        }

        // If this event made us dirty, request a redraw.
        if self.dirty && !was_dirty {
            self.window.request_redraw();
        }

        false // don't exit
    }

    fn handle_keyboard_input(&mut self, event: &winit::event::KeyEvent, egui_consumed: bool) {
        if event.state != ElementState::Pressed {
            return;
        }

        if event.logical_key == Key::Named(NamedKey::Escape) {
            if self.state.settings_open {
                self.state.settings_open = false;
                self.state.settings_ui_state = crate::settings_ui::SettingsUiState::new();
                self.mark_dirty();
                return;
            }
            if self.state.notification_panel_open {
                self.state.notification_panel_open = false;
                self.mark_dirty();
                return;
            }
        }

        let settings_open = self.state.settings_open;
        let notification_open = self.state.notification_panel_open;
        let overlay_open = settings_open || notification_open;

        if !overlay_open || (notification_open && !settings_open) {
            if self.handle_shortcut(&event.logical_key, self.modifiers) {
                self.mark_dirty();
                return;
            }
        }
        if egui_consumed || overlay_open {
            if egui_consumed { self.mark_dirty(); }
            return;
        }

        // Forward to terminal
        let typing_surface_id = self.state.focused_surface_id();
        if let Some(terminal) = self.state.focused_terminal_mut() {
            let dirty = Self::send_key_to_terminal(terminal, &event.logical_key, &event.text, self.modifiers);
            if dirty { self.dirty = true; }
        }
        if let Some(sid) = typing_surface_id {
            self.state.record_typing(sid);
        }
    }

    fn send_key_to_terminal(
        terminal: &mut tasty_terminal::Terminal,
        key: &Key,
        text: &Option<winit::keyboard::SmolStr>,
        modifiers: ModifiersState,
    ) -> bool {
        let app_cursor = terminal.application_cursor_keys();
        let is_alt_screen = terminal.is_alternate_screen();
        let mut dirty = false;

        let is_scrollback_key = !is_alt_screen && matches!(
            key.as_ref(),
            Key::Named(NamedKey::PageUp) | Key::Named(NamedKey::PageDown)
        );

        if !is_scrollback_key && terminal.scroll_offset > 0 {
            terminal.scroll_to_bottom();
            dirty = true;
        }

        match key.as_ref() {
            Key::Named(NamedKey::Enter) => terminal.send_bytes(b"\r"),
            Key::Named(NamedKey::Backspace) => terminal.send_bytes(b"\x7f"),
            Key::Named(NamedKey::Tab) => {
                if modifiers.shift_key() { terminal.send_bytes(b"\x1b[Z"); }
                else { terminal.send_bytes(b"\t"); }
            }
            Key::Named(NamedKey::Escape) => terminal.send_bytes(b"\x1b"),
            Key::Named(NamedKey::ArrowUp) => {
                if app_cursor { terminal.send_bytes(b"\x1bOA") } else { terminal.send_bytes(b"\x1b[A") }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if app_cursor { terminal.send_bytes(b"\x1bOB") } else { terminal.send_bytes(b"\x1b[B") }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if app_cursor { terminal.send_bytes(b"\x1bOC") } else { terminal.send_bytes(b"\x1b[C") }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if app_cursor { terminal.send_bytes(b"\x1bOD") } else { terminal.send_bytes(b"\x1b[D") }
            }
            Key::Named(NamedKey::Home) => terminal.send_bytes(b"\x1b[H"),
            Key::Named(NamedKey::End) => terminal.send_bytes(b"\x1b[F"),
            Key::Named(NamedKey::PageUp) => {
                if is_alt_screen { terminal.send_bytes(b"\x1b[5~"); }
                else { terminal.scroll_up(terminal.rows()); dirty = true; }
            }
            Key::Named(NamedKey::PageDown) => {
                if is_alt_screen { terminal.send_bytes(b"\x1b[6~"); }
                else { terminal.scroll_down(terminal.rows()); dirty = true; }
            }
            Key::Named(NamedKey::Insert) => terminal.send_bytes(b"\x1b[2~"),
            Key::Named(NamedKey::Delete) => terminal.send_bytes(b"\x1b[3~"),
            Key::Named(NamedKey::F1) => terminal.send_bytes(b"\x1bOP"),
            Key::Named(NamedKey::F2) => terminal.send_bytes(b"\x1bOQ"),
            Key::Named(NamedKey::F3) => terminal.send_bytes(b"\x1bOR"),
            Key::Named(NamedKey::F4) => terminal.send_bytes(b"\x1bOS"),
            Key::Named(NamedKey::F5) => terminal.send_bytes(b"\x1b[15~"),
            Key::Named(NamedKey::F6) => terminal.send_bytes(b"\x1b[17~"),
            Key::Named(NamedKey::F7) => terminal.send_bytes(b"\x1b[18~"),
            Key::Named(NamedKey::F8) => terminal.send_bytes(b"\x1b[19~"),
            Key::Named(NamedKey::F9) => terminal.send_bytes(b"\x1b[20~"),
            Key::Named(NamedKey::F10) => terminal.send_bytes(b"\x1b[21~"),
            Key::Named(NamedKey::F11) => terminal.send_bytes(b"\x1b[23~"),
            Key::Named(NamedKey::F12) => terminal.send_bytes(b"\x1b[24~"),
            _ => {
                if let Some(text) = text {
                    let s = text.as_str();
                    if !s.is_empty() { terminal.send_key(s); }
                }
            }
        }
        dirty
    }

    fn handle_ime(&mut self, ime_event: winit::event::Ime, egui_consumed: bool) {
        if egui_consumed { self.mark_dirty(); return; }
        match ime_event {
            winit::event::Ime::Preedit(text, _cursor) => {
                self.preedit_text = text;
                self.mark_dirty();
            }
            winit::event::Ime::Commit(text) => {
                self.preedit_text.clear();
                let sid = self.state.focused_surface_id();
                if let Some(terminal) = self.state.focused_terminal_mut() {
                    terminal.send_key(&text);
                }
                if let Some(sid) = sid {
                    self.state.record_typing(sid);
                }
                self.mark_dirty();
            }
            _ => {}
        }
    }

    fn handle_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>, egui_consumed: bool) {
        self.cursor_position = Some(position);
        let overlay_open = self.state.settings_open || self.state.notification_panel_open;
        if egui_consumed || overlay_open {
            if egui_consumed { self.mark_dirty(); }
            return;
        }

        let terminal_rect = self.compute_terminal_rect();
        let x = position.x as f32;
        let y = position.y as f32;

        if let Some(drag) = self.dragging_divider {
            let changed = match drag.kind {
                DividerDragKind::Pane => self.state.update_pane_divider(&drag.info, x, y, terminal_rect),
                DividerDragKind::Surface => self.state.update_surface_divider(&drag.info, x, y, terminal_rect),
            };
            if changed {
                self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
                self.mark_dirty();
            }
        } else {
            let threshold = 4.0;
            let divider = self.state.find_pane_divider_at(x, y, terminal_rect, threshold)
                .or_else(|| self.state.find_surface_divider_at(x, y, terminal_rect, threshold));
            match divider {
                Some(info) => {
                    let cursor = match info.direction {
                        SplitDirection::Vertical => CursorIcon::ColResize,
                        SplitDirection::Horizontal => CursorIcon::RowResize,
                    };
                    self.window.set_cursor(cursor);
                }
                None => self.window.set_cursor(CursorIcon::Default),
            }
        }
    }

    fn handle_mouse_input(&mut self, button_state: ElementState, button: MouseButton, egui_consumed: bool) {
        let overlay_open = self.state.settings_open || self.state.notification_panel_open;
        if egui_consumed || overlay_open {
            if button_state == ElementState::Released { self.dragging_divider = None; }
            if egui_consumed { self.mark_dirty(); }
            return;
        }
        if button == MouseButton::Left {
            if self.state.pane_context_menu.is_some() {
                self.state.pane_context_menu = None;
                self.dirty = true;
            }
            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if button_state == ElementState::Pressed {
                    let threshold = 4.0;
                    let pane_div = self.state.find_pane_divider_at(x, y, terminal_rect, threshold);
                    let surf_div = self.state.find_surface_divider_at(x, y, terminal_rect, threshold);
                    if let Some(info) = pane_div {
                        self.dragging_divider = Some(DividerDrag { info, kind: DividerDragKind::Pane });
                    } else if let Some(info) = surf_div {
                        self.dragging_divider = Some(DividerDrag { info, kind: DividerDragKind::Surface });
                    } else {
                        if self.state.focus_pane_at_position(x, y, terminal_rect) { self.dirty = true; }
                        if self.state.focus_surface_at_position(x, y, terminal_rect) { self.dirty = true; }
                    }
                } else if button_state == ElementState::Released && self.dragging_divider.is_some() {
                    self.dragging_divider = None;
                    self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
                    self.dirty = true;
                }
            }
        } else if button == MouseButton::Right && button_state == ElementState::Pressed {
            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if terminal_rect.contains(x, y) {
                    let ws = self.state.active_workspace();
                    let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
                    let scale = self.gpu.scale_factor();
                    for (pane_id, rect) in pane_rects {
                        if rect.contains(x, y) {
                            self.state.pane_context_menu = Some(crate::state::PaneContextMenu {
                                pane_id, x: x / scale, y: y / scale,
                            });
                            self.dirty = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta, egui_consumed: bool) {
        let overlay_open = self.state.settings_open || self.state.notification_panel_open;
        if egui_consumed { self.mark_dirty(); }
        if !egui_consumed && !overlay_open {
            if let Some(terminal) = self.state.focused_terminal_mut() {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
                };
                if terminal.is_alternate_screen() {
                    if lines > 0 {
                        for _ in 0..lines.unsigned_abs() { terminal.send_bytes(b"\x1b[A"); }
                    } else if lines < 0 {
                        for _ in 0..lines.unsigned_abs() { terminal.send_bytes(b"\x1b[B"); }
                    }
                } else {
                    if lines > 0 { terminal.scroll_up(lines as usize); }
                    else if lines < 0 { terminal.scroll_down((-lines) as usize); }
                    self.dirty = true;
                }
            }
        }
    }

    fn handle_redraw(&mut self, event_loop: &ActiveEventLoop) {
        // Process IPC is done by App::about_to_wait

        // Process PTY output
        if self.state.process_all() {
            self.dirty = true;
        }

        // Collect terminal events
        let events = self.state.collect_events();
        for event in &events {
            let surface_id = event.surface_id;
            match &event.kind {
                crate::terminal::TerminalEventKind::Notification { title, body } => {
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.window_focused
                        && self.state.engine.notifications.should_send_system_notification()
                    {
                        crate::notification::send_system_notification(title, body);
                    }
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(ws_id, surface_id, title.clone(), body.clone());
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Notification];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
                crate::terminal::TerminalEventKind::BellRing => {
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(ws_id, surface_id, "Bell".to_string(), String::new());
                    }
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.window_focused
                        && self.state.engine.notifications.should_send_system_notification()
                    {
                        crate::notification::send_system_notification("Tasty", "Bell");
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Bell];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
                crate::terminal::TerminalEventKind::TitleChanged(_) => { self.dirty = true; }
                crate::terminal::TerminalEventKind::CwdChanged(_) => { self.dirty = true; }
                crate::terminal::TerminalEventKind::ClipboardSet(data) => {
                    if let Some(cb) = &mut self.clipboard {
                        cb.set_text(data);
                    }
                }
                crate::terminal::TerminalEventKind::ProcessExited => {
                    let hook_events = vec![tasty_hooks::HookEvent::ProcessExit];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
            }
        }

        // Render
        if self.dirty {
            self.dirty = false;
            match self.gpu.render(&mut self.state, &self.window, &self.preedit_text) {
                Ok(_) => {}
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    self.gpu.resize(self.window.inner_size());
                }
                Err(wgpu::SurfaceError::OutOfMemory) => {
                    tracing::error!("GPU out of memory");
                    // Can't exit from here — App will handle
                }
                Err(e) => {
                    tracing::warn!("surface error: {e}");
                }
            }
        }

        if self.dirty {
            self.window.request_redraw();
        }
    }
}
