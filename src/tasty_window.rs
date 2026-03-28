use std::sync::Arc;

use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::gpu::GpuState;
use crate::model::Rect;
use crate::state::AppState;
use crate::{ClipboardContext, DividerDrag};

/// A single Tasty window with its own GPU state, UI state, and input state.
/// In the future, multiple TastyWindows will be managed by App via HashMap<WindowId, TastyWindow>.
pub struct TastyWindow {
    /// GPU rendering state (wgpu surface, renderer, egui).
    pub gpu: GpuState,
    /// Application state (engine state + window-level UI state).
    /// Currently shared — will be split into per-window WindowState in a later phase.
    pub state: AppState,
    /// The OS window handle.
    pub window: Arc<Window>,
    /// Whether this window needs a redraw.
    pub dirty: bool,
    /// Current keyboard modifier state.
    pub modifiers: ModifiersState,
    /// Whether this window has OS focus.
    pub window_focused: bool,
    /// Current cursor position in physical pixels.
    pub cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    /// Active divider drag state.
    pub dragging_divider: Option<DividerDrag>,
    /// System clipboard.
    pub clipboard: Option<ClipboardContext>,
    /// IME preedit text (composing, not yet committed).
    pub preedit_text: String,
}

impl TastyWindow {
    pub fn new(gpu: GpuState, state: AppState, window: Arc<Window>) -> Self {
        Self {
            gpu,
            state,
            window,
            dirty: true,
            modifiers: ModifiersState::empty(),
            window_focused: true,
            cursor_position: None,
            dragging_divider: None,
            clipboard: ClipboardContext::new(),
            preedit_text: String::new(),
        }
    }

    /// Set dirty flag and request a redraw.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.window.request_redraw();
    }

    /// Compute the terminal rect for this window.
    pub fn compute_terminal_rect(&self) -> Rect {
        let size = self.gpu.size();
        crate::model::compute_terminal_rect(
            size.width as f32,
            size.height as f32,
            self.state.sidebar_width,
            self.gpu.scale_factor(),
        )
    }

    /// Paste clipboard text into the focused terminal.
    pub fn paste_to_terminal(&mut self) {
        let text = match &mut self.clipboard {
            Some(cb) => cb.get_text(),
            None => None,
        };
        if let Some(text) = text {
            if text.is_empty() {
                return;
            }
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
}
