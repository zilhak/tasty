use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::gpu::GpuState;
use crate::modal_trait::{Modal, ModalAction};
use crate::settings::Settings;
use crate::settings_ui::{self, SettingsUiState};

/// A modal window for settings. Uses egui only (no terminal renderer).
/// While open, all other windows have their input blocked.
pub struct ModalWindow {
    gpu: GpuState,
    window: Arc<Window>,
    pub settings: Settings,
    settings_ui_state: SettingsUiState,
    dirty: bool,
    /// Whether the window has been shown yet (starts hidden to avoid layout flash).
    shown: bool,
    /// Double-tap detector for the modal window's own key events.
    double_tap: crate::double_tap::DoubleTapDetector,
    /// Captured double-tap string for the keybinding recorder.
    captured_double_tap: Option<String>,
    /// Set to true when the user closes the modal.
    should_close: bool,
}

impl ModalWindow {
    pub fn new(gpu: GpuState, window: Arc<Window>, settings: Settings) -> Self {
        Self {
            gpu,
            window,
            settings,
            settings_ui_state: SettingsUiState::new(),
            dirty: true,
            shown: false,
            double_tap: crate::double_tap::DoubleTapDetector::new(),
            captured_double_tap: None,
            should_close: false,
        }
    }

    pub fn render_settings(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;

        let raw_input = self.gpu.take_egui_input(&self.window);
        let mut settings = self.settings.clone();
        let ui_state = &mut self.settings_ui_state;
        let captured_dt = &mut self.captured_double_tap;
        let mut action: Option<bool> = None;

        let full_output = self.gpu.run_egui(raw_input, |ctx| {
            action = settings_ui::draw_settings_panel(ctx, &mut settings, ui_state, captured_dt);
        });

        self.settings = settings;
        if let Some(_) = action {
            // Save or Cancel — close the modal
            self.should_close = true;
        }

        self.gpu.finish_egui_frame(&self.window, full_output);

        // Show window after first render to avoid layout flash
        if !self.shown {
            self.window.set_visible(true);
            self.shown = true;
        }

        if self.dirty {
            self.window.request_redraw();
        }
    }
}

impl Modal for ModalWindow {
    fn window(&self) -> &Arc<Window> {
        &self.window
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.window.request_redraw();
    }

    fn handle_window_event(
        &mut self,
        event: WindowEvent,
        _event_loop: &ActiveEventLoop,
    ) -> ModalAction {
        let (_, egui_repaint) = self.gpu.handle_egui_event(&self.window, &event);
        if egui_repaint {
            self.mark_dirty();
        }

        match event {
            WindowEvent::CloseRequested => {
                self.should_close = true;
                return ModalAction::Close;
            }
            WindowEvent::Resized(new_size) => {
                self.gpu.resize(new_size);
                self.mark_dirty();
            }
            WindowEvent::RedrawRequested => {
                self.render_settings();
            }
            WindowEvent::CursorMoved { .. } => {
                self.mark_dirty();
            }
            WindowEvent::KeyboardInput { ref event, .. } => {
                use winit::event::ElementState;

                self.double_tap.on_key_event(
                    &event.logical_key,
                    event.state == ElementState::Pressed,
                );
                if event.state == ElementState::Pressed {
                    if let Some(dt) = self.double_tap.take() {
                        self.captured_double_tap = Some(dt.binding_str().to_string());
                        self.mark_dirty();
                    }
                }
            }
            _ => {}
        }

        if self.should_close {
            ModalAction::Close
        } else {
            ModalAction::Pending
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
