use std::sync::Arc;

use winit::event::WindowEvent;

use crate::AppEvent;
use crate::adapters::ui::window::{
    ModalWindow, Modality, Window, WindowAction, WindowBase, WindowCtx, modal::MODAL_MODALITY,
    sealed,
};
use crate::gpu::GpuState;
use crate::i18n::t;

/// 종료 확인 다이얼로그. 사용자에게 종료/최소화를 묻는다.
pub struct QuitWindow {
    pub base: WindowBase,
    shown: bool,
    pending_action: WindowAction,
}

impl QuitWindow {
    pub fn new(gpu: GpuState, winit: Arc<winit::window::Window>) -> Self {
        Self {
            base: WindowBase::new(gpu, winit),
            shown: false,
            pending_action: WindowAction::None,
        }
    }
}

impl Window for QuitWindow {
    fn base(&self) -> &WindowBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut WindowBase {
        &mut self.base
    }
    fn modality(&self) -> Modality {
        MODAL_MODALITY
    }

    fn as_modal(&self) -> Option<&dyn ModalWindow> {
        Some(self)
    }
    fn as_modal_mut(&mut self) -> Option<&mut dyn ModalWindow> {
        Some(self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_event(&mut self, event: WindowEvent, _ctx: &mut WindowCtx<'_>) -> WindowAction {
        let (_, egui_repaint) = self.base.gpu.handle_egui_event(&self.base.winit, &event);
        if egui_repaint {
            self.mark_dirty();
        }

        match event {
            WindowEvent::CloseRequested => return WindowAction::Close,
            WindowEvent::Resized(new_size) => {
                self.base.gpu.resize(new_size);
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
                        return self.on_escape();
                    }
                }
            }
            _ => {}
        }

        match &self.pending_action {
            WindowAction::None => WindowAction::None,
            _ => std::mem::replace(&mut self.pending_action, WindowAction::None),
        }
    }

    fn render(&mut self) {
        if !self.base.dirty {
            return;
        }
        self.base.dirty = false;

        let raw_input = self.base.gpu.take_egui_input(&self.base.winit);
        let pending = &mut self.pending_action;

        let full_output = self.base.gpu.run_egui(raw_input, |ctx| {
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
                            *pending = WindowAction::CloseWithEvent(AppEvent::Shutdown);
                        }
                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                [button_width, 28.0],
                                egui::Button::new(t("quit_modal.minimize_button")),
                            )
                            .clicked()
                        {
                            *pending = WindowAction::CloseWithEvent(AppEvent::Minimize);
                        }
                    });
                });

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

        self.base
            .gpu
            .finish_egui_frame(&self.base.winit, full_output);

        self.reveal_after_first_render();

        if self.base.dirty {
            self.base.winit.request_redraw();
        }
    }
}

impl ModalWindow for QuitWindow {
    fn shown(&self) -> bool {
        self.shown
    }
    fn set_shown(&mut self, v: bool) {
        self.shown = v;
    }
}

impl sealed::Sealed for QuitWindow {}
