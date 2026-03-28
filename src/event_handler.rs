use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, WindowId};

use crate::model::SplitDirection;
use crate::{App, AppEvent, DividerDrag, DividerDragKind};
use crate::tasty_window::TastyWindow;

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TerminalOutput => {
                if let Some(w) = &mut self.primary_window {
                    w.mark_dirty();
                }
            }
            AppEvent::IpcReady => {
                if self.process_ipc() {
                    if let Some(w) = &mut self.primary_window {
                        w.mark_dirty();
                    }
                }
            }
            AppEvent::EguiRepaint => {
                if let Some(w) = &mut self.primary_window {
                    w.mark_dirty();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.primary_window.is_some() || self.shell_setup_gpu.is_some() {
            return;
        }

        use std::sync::Arc;
        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("Tasty")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        let init_settings = crate::settings::Settings::load();

        let gpu = pollster::block_on(crate::gpu::GpuState::new(window.clone(), &init_settings.appearance, self.engine.proxy.clone()))
            .expect("failed to initialize GPU");

        let mut init_settings = init_settings;
        if !init_settings.general.is_shell_valid() {
            if let Some(detected) = crate::settings::GeneralSettings::detect_bash() {
                tracing::info!("configured shell invalid; auto-detected bash at {detected}");
                init_settings.general.shell = detected;
                if let Err(e) = init_settings.save() {
                    tracing::warn!("failed to save auto-detected shell: {e}");
                }
            } else {
                tracing::warn!("bash not found; entering shell setup mode");
                self.shell_setup_mode = true;
                self.shell_setup_path = String::new();
                self.shell_setup_gpu = Some(gpu);
                self.shell_setup_window = Some(window);
                return;
            }
        }

        window.set_ime_allowed(true);
        self.init_app_state(window, gpu, init_settings);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Shell setup mode — handled by App directly
        if self.shell_setup_mode {
            if let WindowEvent::RedrawRequested = &event {
                if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window) {
                    let result = gpu.render_shell_setup(window, &mut self.shell_setup_path);
                    match result {
                        Ok(crate::gpu::ShellSetupAction::None) => {}
                        Ok(crate::gpu::ShellSetupAction::Confirmed) => {
                            let mut settings = crate::settings::Settings::load();
                            settings.general.shell = self.shell_setup_path.clone();
                            if let Err(e) = settings.save() {
                                tracing::error!("failed to save settings: {e}");
                            }
                            self.shell_setup_mode = false;
                            let window = self.shell_setup_window.take().unwrap();
                            let gpu = self.shell_setup_gpu.take().unwrap();
                            self.init_app_state(window, gpu, settings);
                            if let Some(w) = &mut self.primary_window { w.mark_dirty(); }
                        }
                        Ok(crate::gpu::ShellSetupAction::Exit) => {
                            event_loop.exit();
                        }
                        Err(e) => {
                            tracing::warn!("shell setup render error: {e}");
                        }
                    }
                }
                // Still pass egui events for shell setup UI
                if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window) {
                    gpu.handle_egui_event(window, &event);
                }
                return;
            }
            // Pass non-redraw events to egui for shell setup
            if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window) {
                gpu.handle_egui_event(window, &event);
                if let WindowEvent::CloseRequested = &event {
                    event_loop.exit();
                }
            }
            return;
        }

        // Normal mode — delegate to TastyWindow
        if let WindowEvent::CloseRequested = &event {
            event_loop.exit();
            return;
        }

        if let Some(w) = &mut self.primary_window {
            let should_exit = w.handle_window_event(event, event_loop);
            if should_exit {
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.process_ipc() {
            if let Some(w) = &mut self.primary_window {
                w.mark_dirty();
            }
        }
    }
}
