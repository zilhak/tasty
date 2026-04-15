use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::{App, AppEvent, quit_modal};

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
                    let mut found = false;
                    for w in self.windows.values_mut() {
                        if w.state.engine.find_terminal_by_id(sid).is_some() {
                            w.state.engine.process_surface(sid);
                            w.recalc_ime_preedit_anchor();
                            w.mark_dirty();
                            found = true;
                            break;
                        }
                    }
                    // If no window has this surface, it might be in the parked states
                    if !found {
                        for state in &mut self.parked_states {
                            if state.engine.find_terminal_by_id(sid).is_some() {
                                state.engine.process_surface(sid);
                                break;
                            }
                        }
                    }
                } else {
                    // Legacy: wake all windows
                    for w in self.windows.values_mut() {
                        w.mark_dirty();
                    }
                    // Also process parked state terminals
                    for state in &mut self.parked_states {
                        state.engine.process_all();
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
            AppEvent::Shutdown => {
                event_loop.exit();
            }
            AppEvent::Minimize => {
                #[cfg(target_os = "macos")]
                {
                    // macOS: destroy windows, park all states (dock reopen restores)
                    for (_, w) in self.windows.drain() {
                        self.parked_states.push(w.state);
                    }
                    self.engine.focused_window_id = None;
                    tracing::info!("minimized to background ({} states parked)", self.parked_states.len());
                }
                #[cfg(not(target_os = "macos"))]
                {
                    // Windows/Linux: minimize windows to taskbar (keep alive)
                    for w in self.windows.values() {
                        w.window.set_minimized(true);
                    }
                    tracing::info!("minimized {} window(s) to taskbar", self.windows.len());
                }
            }
            AppEvent::QuitRequested => {
                self.handle_quit_requested(event_loop);
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

        // Quit modal handling
        if let Some(quit_modal_id) = self.quit_modal_window_id {
            if id == quit_modal_id {
                if let Some(qm) = &mut self.quit_modal {
                    if let Some(result) = qm.handle_window_event(event, event_loop) {
                        match result {
                            crate::quit_modal::QuitModalResult::Quit => {
                                self.quit_modal = None;
                                self.quit_modal_window_id = None;
                                event_loop.exit();
                            }
                            crate::quit_modal::QuitModalResult::Minimize => {
                                self.quit_modal = None;
                                self.quit_modal_window_id = None;
                                let _ = self.engine.proxy.send_event(AppEvent::Minimize);
                            }
                            crate::quit_modal::QuitModalResult::Cancelled => {
                                self.quit_modal = None;
                                self.quit_modal_window_id = None;
                            }
                            crate::quit_modal::QuitModalResult::Pending => {}
                        }
                    }
                }
                return;
            }
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
            if self.windows.len() > 1 {
                // Multiple windows: just close this one (no modal needed)
                self.windows.remove(&id);
                if self.engine.focused_window_id == Some(id) {
                    self.engine.focused_window_id = self.windows.keys().next().copied();
                }
            } else {
                // Last window: route through quit logic
                self.handle_quit_requested(event_loop);
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

            // Check if the window requested to close (e.g. last workspace removed)
            if w.close_requested {
                if let Some(w) = self.windows.remove(&id) {
                    if self.windows.is_empty() {
                        tracing::info!("last window closed via request, parking state");
                        self.parked_states.push(w.state);
                    }
                }
                if self.engine.focused_window_id == Some(id) {
                    self.engine.focused_window_id = self.windows.keys().next().copied();
                }
            }
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

impl App {
    fn handle_quit_requested(&mut self, event_loop: &ActiveEventLoop) {
        // If quit modal is already open, treat as immediate quit
        if self.quit_modal.is_some() {
            self.quit_modal = None;
            self.quit_modal_window_id = None;
            event_loop.exit();
            return;
        }

        // Get close behavior from settings
        let behavior = self.focused_window()
            .map(|w| w.state.engine.settings.general.close_behavior.clone())
            .or_else(|| self.parked_states.first().map(|s| s.engine.settings.general.close_behavior.clone()))
            .unwrap_or_else(|| "ask".to_string());

        match behavior.as_str() {
            "quit" => {
                event_loop.exit();
            }
            "minimize" => {
                let _ = self.engine.proxy.send_event(AppEvent::Minimize);
            }
            _ => {
                // "ask" — close settings modal if open, then show quit modal
                self.close_settings_modal();
                self.open_quit_modal(event_loop);
            }
        }
    }

    fn open_quit_modal(&mut self, event_loop: &ActiveEventLoop) {
        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("Tasty")
            .with_inner_size(winit::dpi::LogicalSize::new(400, 200))
            .with_resizable(false)
            .with_visible(false);

        let window = std::sync::Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create quit modal window"),
        );

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &crate::settings::Settings::load().appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU for quit modal");

        let window_id = window.id();
        self.quit_modal = Some(quit_modal::QuitModal::new(gpu, window));
        self.quit_modal_window_id = Some(window_id);
    }
}
