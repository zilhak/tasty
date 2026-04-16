use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::gpu::GpuState;
use crate::i18n::t;
use crate::modal_trait::{Modal, ModalAction};
use crate::AppEvent;

/// A small modal window asking the user to quit or minimize.
pub struct QuitModal {
    gpu: GpuState,
    window: Arc<Window>,
    dirty: bool,
    shown: bool,
    action: ModalAction,
}

impl QuitModal {
    pub fn new(gpu: GpuState, window: Arc<Window>) -> Self {
        Self {
            gpu,
            window,
            dirty: true,
            shown: false,
            action: ModalAction::Pending,
        }
    }

    fn render(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;

        let raw_input = self.gpu.take_egui_input(&self.window);
        let action = &mut self.action;

        let full_output = self.gpu.run_egui(raw_input, |ctx| {
            // Bottom: fixed button area
            egui::TopBottomPanel::bottom("quit_buttons")
                .exact_height(52.0)
                .show(ctx, |ui| {
                    ui.add_space(12.0);
                    let available_width = ui.available_width() - 40.0;
                    let button_width = available_width / 2.0 - 4.0;
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        if ui
                            .add_sized(
                                [button_width, 28.0],
                                egui::Button::new(t("quit_modal.quit_button")),
                            )
                            .clicked()
                        {
                            *action = ModalAction::CloseWithEvent(AppEvent::Shutdown);
                        }
                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                [button_width, 28.0],
                                egui::Button::new(t("quit_modal.minimize_button")),
                            )
                            .clicked()
                        {
                            *action = ModalAction::CloseWithEvent(AppEvent::Minimize);
                        }
                    });
                });

            // Top: text content with top padding
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

impl Modal for QuitModal {
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
                return ModalAction::Close;
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
                        return ModalAction::Close;
                    }
                }
            }
            _ => {}
        }

        match self.action {
            ModalAction::Pending => ModalAction::Pending,
            _ => std::mem::replace(&mut self.action, ModalAction::Pending),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
