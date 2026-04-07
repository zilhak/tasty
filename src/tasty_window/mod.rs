mod clipboard;
mod keyboard;
mod mouse;
mod redraw;
mod selection;

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Window};

use crate::gpu::{GpuState, ImePreeditState};
use crate::model::Rect;
use crate::selection::TextSelection;
use crate::state::AppState;
use crate::{AppEvent, ClipboardContext};

/// A single Tasty window with its own GPU state, UI state, and input state.
pub struct TastyWindow {
    pub(crate) gpu: GpuState,
    pub(crate) state: AppState,
    pub(crate) window: Arc<Window>,
    pub(crate) dirty: bool,
    pub(crate) modifiers: ModifiersState,
    pub(crate) window_focused: bool,
    pub(crate) cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    pub(crate) dragging_divider: Option<crate::DividerDrag>,
    pub(crate) clipboard: Option<ClipboardContext>,
    pub(crate) ime_preedit: Option<ImePreeditState>,
    pub(crate) proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    pub(crate) text_selection: Option<TextSelection>,
    pub(crate) left_mouse_down: bool,
    pub(crate) last_click_time: Option<std::time::Instant>,
    pub(crate) last_click_pos: Option<(usize, usize)>,
    pub(crate) click_count: u8,
    pub(crate) arrow_queue: Option<crate::click_cursor::ArrowQueue>,
    /// Whether IME composition is active (set by Ime::Enabled/Disabled).
    /// When true, KeyboardInput text is ignored — only Ime::Commit sends text.
    pub(crate) ime_active: bool,
    /// Accumulated cursor advance from IME commits (in terminal columns).
    /// After Ime::Commit, the PTY echo hasn't been processed yet, so
    /// cursor_position() returns a stale value. This offset compensates
    /// so the next Preedit anchor appears after the committed text.
    pub(crate) ime_cursor_advance: usize,
    /// Raw cursor position when ime_cursor_advance was last updated.
    /// Used to reconcile: if the raw cursor moved past this point, PTY
    /// echo has caught up and advance should be reduced accordingly.
    pub(crate) ime_advance_base: (usize, usize),
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
            ime_preedit: None,
            proxy,
            text_selection: None,
            left_mouse_down: false,
            last_click_time: None,
            last_click_pos: None,
            click_count: 0,
            arrow_queue: None,
            ime_active: false,
            ime_cursor_advance: 0,
            ime_advance_base: (0, 0),
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

    fn clear_ime_preedit(&mut self) {
        self.ime_preedit = None;
        self.ime_cursor_advance = 0;
        self.ime_advance_base = (0, 0);
    }

    pub(crate) fn update_ime_cursor_area(&self) {
        let Some(preedit) = &self.ime_preedit else {
            return;
        };
        let terminal_rect = self.compute_terminal_rect();
        let Some(cell_rect) = self.state.surface_cell_rect(
            terminal_rect,
            preedit.surface_id,
            preedit.anchor_col,
            preedit.anchor_row,
            self.gpu.cell_width(),
            self.gpu.cell_height(),
        ) else {
            return;
        };

        use winit::dpi::{PhysicalPosition, PhysicalSize};
        self.window.set_ime_cursor_area(
            PhysicalPosition::new(cell_rect.x.round() as i32, cell_rect.y.round() as i32),
            PhysicalSize::new(
                cell_rect.width.max(1.0).round() as u32,
                cell_rect.height.max(1.0).round() as u32,
            ),
        );
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
                // Re-fetch the physical size — the window's physical dimensions change
                // when the scale factor changes (e.g., macOS sleep/wake cycle).
                let new_size = self.window.inner_size();
                self.gpu.resize(new_size);
                let terminal_rect = self.compute_terminal_rect();
                let (cols, rows) = self.gpu.grid_size_for_rect(&terminal_rect);
                self.state.update_grid_size(cols, rows);
                self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
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
}
