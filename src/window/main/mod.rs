mod clipboard;
mod keyboard;
mod mouse;
mod redraw;
mod selection;

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::keyboard::ModifiersState;
use winit::window::CursorIcon;

use crate::gpu::{GpuState, ImePreeditState};
use crate::model::Rect;
use crate::selection::TextSelection;
use crate::state::{AppState, FocusedSurfaceType};
use crate::window::{
    sealed, terminal_host::MODELESS_MODALITY, Modality, TerminalHostWindow, Window, WindowAction,
    WindowBase, WindowCtx,
};
use crate::{AppEvent, ClipboardContext};

/// 메인 터미널 윈도우. 워크스페이스/사이드바/탭을 갖고 터미널 계열 Surface를 호스팅한다.
/// `TerminalHostWindow` 계열의 대표 구현체.
pub struct MainWindow {
    pub base: WindowBase,
    pub(crate) state: AppState,
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
    /// Detector for double-tap modifier shortcuts (e.g. Shift+Shift).
    pub(crate) double_tap: crate::double_tap::DoubleTapDetector,
    /// Native WebView instances keyed by surface ID.
    pub(crate) webviews: std::collections::HashMap<u32, crate::webview::PlatformWebView>,
}

impl MainWindow {
    pub fn new(
        gpu: GpuState,
        state: AppState,
        window: Arc<winit::window::Window>,
        proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    ) -> Self {
        Self {
            base: WindowBase::new(gpu, window),
            state,
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
            double_tap: crate::double_tap::DoubleTapDetector::new(),
            webviews: std::collections::HashMap::new(),
        }
    }

    /// Request this window to close (will be handled by the event loop).
    pub(crate) fn request_close(&mut self) {
        self.base.close_requested = true;
    }

    pub fn compute_terminal_rect(&self) -> Rect {
        let size = self.base.gpu.size();
        crate::model::compute_terminal_rect(
            size.width as f32,
            size.height as f32,
            self.state.sidebar_width,
            self.base.gpu.scale_factor(),
        )
    }

    fn clear_ime_preedit(&mut self) {
        self.ime_preedit = None;
        self.ime_cursor_advance = 0;
        self.ime_advance_base = (0, 0);
    }

    /// If there is an active IME preedit, commit its text to the **original** surface
    /// (the one where composition started) and reset all IME state.
    /// Call this before any focus change or shortcut consumption.
    fn flush_ime_preedit(&mut self) {
        let preedit = match self.ime_preedit.take() {
            Some(p) if !p.text.is_empty() => p,
            _ => {
                self.ime_cursor_advance = 0;
                self.ime_advance_base = (0, 0);
                return;
            }
        };
        if let Some(terminal) = self.state.find_terminal_by_id_mut(preedit.surface_id) {
            terminal.send_key(&preedit.text);
        }
        self.state.record_typing(preedit.surface_id);
        self.ime_cursor_advance = 0;
        self.ime_advance_base = (0, 0);
        self.mark_dirty();
    }

    /// Recalculate the preedit anchor position using the current terminal cursor.
    pub(crate) fn recalc_ime_preedit_anchor(&mut self) {
        if self.ime_cursor_advance == 0 {
            return;
        }

        let preedit = match &self.ime_preedit {
            Some(p) => p,
            None => return,
        };
        let surface_id = preedit.surface_id;
        let terminal = match self.state.find_terminal_by_id(surface_id) {
            Some(t) => t,
            None => return,
        };

        let (col, row) = terminal.surface().cursor_position();
        let cols = terminal.cols();

        let (base_col, base_row) = self.ime_advance_base;
        let raw_advance = if row > base_row {
            (row - base_row) * cols + col - base_col
        } else if col >= base_col {
            col - base_col
        } else {
            0
        };
        if raw_advance >= self.ime_cursor_advance {
            self.ime_cursor_advance = 0;
        } else {
            self.ime_cursor_advance -= raw_advance;
        }
        self.ime_advance_base = (col, row);

        let adjusted_col = col + self.ime_cursor_advance;
        let (anchor_col, anchor_row) = if cols > 0 && adjusted_col >= cols {
            (adjusted_col % cols, row + adjusted_col / cols)
        } else {
            (adjusted_col, row)
        };

        if let Some(p) = &mut self.ime_preedit {
            p.anchor_col = anchor_col;
            p.anchor_row = anchor_row;
        }
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
            self.base.gpu.cell_width(),
            self.base.gpu.cell_height(),
        ) else {
            return;
        };

        use winit::dpi::{PhysicalPosition, PhysicalSize};
        self.base.winit.set_ime_cursor_area(
            PhysicalPosition::new(cell_rect.x.round() as i32, cell_rect.y.round() as i32),
            PhysicalSize::new(
                cell_rect.width.max(1.0).round() as u32,
                cell_rect.height.max(1.0).round() as u32,
            ),
        );
    }
}

impl Window for MainWindow {
    fn base(&self) -> &WindowBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut WindowBase {
        &mut self.base
    }
    fn modality(&self) -> Modality {
        MODELESS_MODALITY
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_event(&mut self, event: WindowEvent, ctx: &mut WindowCtx<'_>) -> WindowAction {
        // ── Keyboard/IME routing ──
        // Keyboard and IME events are only forwarded to egui when an overlay
        // (settings, dialog, focused popup) is active. Otherwise the central
        // keyboard dispatcher in keyboard.rs handles routing to the correct
        // surface, and egui never sees the key event.
        let is_keyboard_event = matches!(
            &event,
            WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_)
        );
        let is_modifiers_event = matches!(&event, WindowEvent::ModifiersChanged(_));

        let overlay_open = self.state.settings_open
            || self.state.has_input_dialog_open()
            || self.state.popups.has_focused();

        // Non-terminal surfaces (Explorer, Markdown) use egui widgets (TextEdit etc.)
        // that need direct keyboard events from egui's input system.
        let egui_surface = matches!(
            self.state.focused_surface_type(),
            FocusedSurfaceType::Explorer | FocusedSurfaceType::Markdown
        );

        let (egui_consumed, egui_repaint) = if is_keyboard_event {
            if overlay_open || egui_surface {
                self.base.gpu.handle_egui_event(&self.base.winit, &event)
            } else {
                (false, false)
            }
        } else if is_modifiers_event {
            let (_, repaint) = self.base.gpu.handle_egui_event(&self.base.winit, &event);
            (false, repaint)
        } else {
            self.base.gpu.handle_egui_event(&self.base.winit, &event)
        };

        if egui_repaint {
            self.mark_dirty();
        }

        // If a modal is active, only allow Resized/RedrawRequested/ScaleFactorChanged/...
        if ctx.modal_active {
            match &event {
                WindowEvent::Resized(_)
                | WindowEvent::RedrawRequested
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::ModifiersChanged(_)
                | WindowEvent::Focused(_) => {}
                _ => return WindowAction::None,
            }
        }

        let was_dirty = self.base.dirty;

        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.base.gpu.sync_scale_factor(&self.base.winit);
                let new_size = self.base.winit.inner_size();
                self.base.gpu.resize(new_size);
                let terminal_rect = self.compute_terminal_rect();
                let (cols, rows) = self.base.gpu.grid_size_for_rect(&terminal_rect);
                self.state.update_grid_size(cols, rows);
                self.state.resize_all(
                    terminal_rect,
                    self.base.gpu.cell_width(),
                    self.base.gpu.cell_height(),
                );
                self.mark_dirty();
            }
            WindowEvent::Focused(focused) => {
                self.base.focused = focused;
                if !focused {
                    if self.ime_preedit.is_some() {
                        self.flush_ime_preedit();
                    }
                    self.base.modifiers = ModifiersState::empty();
                }
                self.mark_dirty();
            }
            WindowEvent::Occluded(false) => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.base.modifiers = modifiers.state();
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
                self.base.winit.set_cursor(CursorIcon::Default);
            }
            WindowEvent::MouseInput {
                state: button_state,
                button,
                ..
            } => {
                self.handle_mouse_input(button_state, button, egui_consumed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(delta, egui_consumed);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw(ctx.event_loop);
            }
            _ => {}
        }

        if self.base.dirty && !was_dirty {
            self.base.winit.request_redraw();
        }

        WindowAction::None
    }

    fn render(&mut self) {
        // 메인 윈도우는 별도 진입점인 handle_redraw 경로로 렌더한다.
        // Window::render는 트레잇 디스패치 호환을 위해 존재하며 현재 Main
        // 창에서는 호출되지 않는다.
    }
}

impl TerminalHostWindow for MainWindow {}

impl sealed::Sealed for MainWindow {}
