use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::gpu::GpuState;
use crate::i18n::t;

/// Result of the quit confirmation modal.
pub enum QuitModalResult {
    /// User chose to quit (exit the app).
    Quit,
    /// User chose to minimize (park state, keep running).
    Minimize,
    /// Modal was closed without choosing (e.g. Escape).
    Cancelled,
    /// Still waiting for user input.
    Pending,
}

/// A small modal window asking the user to quit or minimize.
pub struct QuitModal {
    pub gpu: GpuState,
    pub window: Arc<Window>,
    pub dirty: bool,
    shown: bool,
    result: QuitModalResult,
}

impl QuitModal {
    pub fn new(gpu: GpuState, window: Arc<Window>) -> Self {
        Self {
            gpu,
            window,
            dirty: true,
            shown: false,
            result: QuitModalResult::Pending,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.window.request_redraw();
    }

    /// Handle a window event. Returns the result if the modal should close.
    pub fn handle_window_event(&mut self, event: WindowEvent, _event_loop: &ActiveEventLoop) -> Option<QuitModalResult> {
        let (_, egui_repaint) = self.gpu.handle_egui_event(&self.window, &event);
        if egui_repaint {
            self.mark_dirty();
        }

        match event {
            WindowEvent::CloseRequested => {
                return Some(QuitModalResult::Quit);
            }
            WindowEvent::Resized(new_size) => {
                self.gpu.resize(new_size);
                self.mark_dirty();
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            WindowEvent::CursorMoved { .. } => {
                self.mark_dirty();
            }
            WindowEvent::KeyboardInput { ref event, .. } => {
                use winit::event::ElementState;
                use winit::keyboard::{Key, NamedKey};
                if event.state == ElementState::Pressed {
                    if let Key::Named(NamedKey::Escape) = &event.logical_key {
                        self.result = QuitModalResult::Cancelled;
                    }
                }
            }
            _ => {}
        }

        match self.result {
            QuitModalResult::Pending => None,
            _ => {
                let r = std::mem::replace(&mut self.result, QuitModalResult::Pending);
                Some(r)
            }
        }
    }

    fn render(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;

        let raw_input = self.gpu.take_egui_input(&self.window);
        let result = &mut self.result;

        let full_output = self.gpu.run_egui(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.heading(t("quit_modal.title"));
                    ui.add_space(12.0);
                    ui.label(t("quit_modal.message"));
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(t("quit_modal.settings_hint"))
                            .small()
                            .weak(),
                    );
                    ui.add_space(20.0);

                    let available_width = ui.available_width() - 40.0; // 20px padding each side
                    let button_width = available_width / 2.0 - 4.0; // 8px gap between buttons
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        if ui.add_sized([button_width, 24.0], egui::Button::new(t("quit_modal.quit_button"))).clicked() {
                            *result = QuitModalResult::Quit;
                        }
                        ui.add_space(8.0);
                        if ui.add_sized([button_width, 24.0], egui::Button::new(t("quit_modal.minimize_button"))).clicked() {
                            *result = QuitModalResult::Minimize;
                        }
                    });
                });
            });
        });

        self.gpu.finish_egui_frame(&self.window, full_output);

        if !self.shown {
            self.window.set_visible(true);
            self.shown = true;
        }

        if self.dirty {
            self.window.request_redraw();
        }
    }
}
