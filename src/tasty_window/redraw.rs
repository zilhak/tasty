use winit::event_loop::ActiveEventLoop;

use super::TastyWindow;

impl TastyWindow {
    pub(super) fn handle_redraw(&mut self, _event_loop: &ActiveEventLoop) {
        // Process queued arrow keys (one per frame for Claude Code surfaces)
        if let Some(queue) = &self.arrow_queue {
            let sid = queue.surface_id;
            let _arrow = queue.arrow;
            if let Some(terminal) = self.state.find_terminal_by_id_mut(sid) {
                let mut q = self.arrow_queue.take().unwrap();
                let has_more = q.tick(terminal);
                if has_more {
                    self.arrow_queue = Some(q);
                    self.dirty = true;
                    self.window.request_redraw(); // Schedule next frame
                }
            } else {
                self.arrow_queue = None;
            }
        }

        // Check if settings button was clicked (ui.rs sets state.settings_open = true)
        if self.state.settings_open {
            self.state.settings_open = false;
            let _ = self.proxy.send_event(crate::AppEvent::OpenSettings);
        }

        // When targeted_pty_polling is off, process all terminals every frame.
        // When on, individual terminals are processed via TerminalOutput(Some(id)) events,
        // but we still call process_all() as a safety net (it's a no-op if channels are empty).
        if self.state.process_all() {
            self.recalc_ime_preedit_anchor();
            self.dirty = true;
        }

        // Collect terminal events
        let events = self.state.collect_events();
        for event in &events {
            let surface_id = event.surface_id;
            match &event.kind {
                crate::terminal::TerminalEventKind::Notification { title, body } => {
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.window_focused
                        && self.state.engine.notifications.should_send_system_notification()
                    {
                        crate::notification::send_system_notification(title, body);
                    }
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(ws_id, surface_id, title.clone(), body.clone());
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Notification];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
                crate::terminal::TerminalEventKind::BellRing => {
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(ws_id, surface_id, "Bell".to_string(), String::new());
                    }
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.window_focused
                        && self.state.engine.notifications.should_send_system_notification()
                    {
                        crate::notification::send_system_notification("Tasty", "Bell");
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Bell];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.dirty = true;
                }
                crate::terminal::TerminalEventKind::TitleChanged(_) => { self.dirty = true; }
                crate::terminal::TerminalEventKind::CwdChanged(_) => { self.dirty = true; }
                crate::terminal::TerminalEventKind::ClipboardSet(data) => {
                    if let Some(cb) = &mut self.clipboard {
                        cb.set_text(data);
                    }
                }
                crate::terminal::TerminalEventKind::ProcessExited => {
                    let hook_events = vec![tasty_hooks::HookEvent::ProcessExit];
                    self.state.engine.hook_manager.check_and_fire(surface_id, &hook_events);
                    self.state.close_surface_by_id(surface_id);
                    self.dirty = true;
                }
            }
        }

        // Re-sync scale factor before render — macOS may not fire
        // ScaleFactorChanged reliably during monitor hot-swap or sleep/wake.
        if self.gpu.sync_scale_factor(&self.window) {
            let new_size = self.window.inner_size();
            self.gpu.resize(new_size);
            let terminal_rect = self.compute_terminal_rect();
            let (cols, rows) = self.gpu.grid_size_for_rect(&terminal_rect);
            self.state.update_grid_size(cols, rows);
            self.state.resize_all(terminal_rect, self.gpu.cell_width(), self.gpu.cell_height());
            // Schedule another redraw to verify scale factor has stabilized.
            self.dirty = true;
        }

        // Render
        if self.dirty {
            self.dirty = false;
            self.update_ime_cursor_area();
            match self.gpu.render(&mut self.state, &self.window, self.ime_preedit.as_ref(), self.text_selection.as_ref()) {
                Ok(()) => {}
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    self.gpu.resize(self.window.inner_size());
                    // Surface was lost/outdated; resize recovers it, but we must
                    // re-render now that it's ready. dirty was set to false above,
                    // so restore it and request another frame.
                    self.dirty = true;
                }
                Err(wgpu::SurfaceError::OutOfMemory) => {
                    tracing::error!("GPU out of memory");
                    crate::crash_report::record_error("GPU out of memory");
                }
                Err(e) => {
                    let msg = format!("surface error: {e}");
                    tracing::warn!("{}", msg);
                    crate::crash_report::record_error(&msg);
                }
            }
        }

        if self.dirty {
            self.window.request_redraw();
        }
    }
}
