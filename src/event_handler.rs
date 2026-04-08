use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::{App, AppEvent};

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::CreateWindow => {
                self.create_new_window(event_loop);
            }
            AppEvent::OpenSettings => {
                self.open_settings_modal(event_loop);
            }
            AppEvent::TerminalOutput(surface_id) => {
                if let Some(sid) = surface_id {
                    // Targeted polling: process only the specific terminal, then wake its window
                    for w in self.windows.values_mut() {
                        if w.state.engine.find_terminal_by_id(sid).is_some() {
                            w.state.engine.process_surface(sid);
                            w.recalc_ime_preedit_anchor();
                            w.mark_dirty();
                            break;
                        }
                    }
                } else {
                    // Legacy: wake all windows
                    for w in self.windows.values_mut() {
                        w.mark_dirty();
                    }
                }
            }
            AppEvent::IpcReady => {
                if self.process_ipc() {
                    if let Some(w) = self.focused_window_mut() {
                        w.mark_dirty();
                    }
                }
            }
            AppEvent::EguiRepaint => {
                for w in self.windows.values_mut() {
                    w.mark_dirty();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() || self.shell_setup_gpu.is_some() {
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
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
                            if let Some(w) = self.focused_window_mut() { w.mark_dirty(); }
                        }
                        Ok(crate::gpu::ShellSetupAction::Exit) => {
                            event_loop.exit();
                        }
                        Err(e) => {
                            let msg = format!("shell setup render error: {e}");
                            tracing::warn!("{}", msg);
                            crate::crash_report::record_error(&msg);
                        }
                    }
                }
                if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window) {
                    gpu.handle_egui_event(window, &event);
                }
                return;
            }
            if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window) {
                gpu.handle_egui_event(window, &event);
                if let WindowEvent::CloseRequested = &event {
                    event_loop.exit();
                }
            }
            return;
        }

        // Modal window handling
        if let Some(modal_id) = self.engine.modal_window_id {
            if id == modal_id {
                if let Some(modal) = &mut self.modal {
                    let should_close = modal.handle_window_event(event, event_loop);
                    if should_close {
                        self.close_settings_modal();
                    }
                }
                return;
            }
        }

        // Normal mode — find the window by ID and delegate
        if let WindowEvent::CloseRequested = &event {
            self.windows.remove(&id);
            if self.engine.focused_window_id == Some(id) {
                // Focus moves to another window, or None
                self.engine.focused_window_id = self.windows.keys().next().copied();
            }
            if self.windows.is_empty() {
                event_loop.exit();
            }
            return;
        }

        // Track focused window on focus events
        if let WindowEvent::Focused(true) = &event {
            self.engine.focused_window_id = Some(id);
            // If a modal is active, bring it to the front so it's not buried
            if let Some(modal) = &self.modal {
                modal.window.focus_window();
            }
        }

        if let Some(w) = self.windows.get_mut(&id) {
            let modal_active = self.engine.is_modal_active();
            w.handle_window_event(event, event_loop, modal_active);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.process_ipc() {
            if let Some(w) = self.focused_window_mut() {
                w.mark_dirty();
            }
        }
    }
}
