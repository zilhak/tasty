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
                    self.state.close_surface_by_id_no_snapshot(surface_id);
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

        // Process pending native context menu (after egui frame, before webview sync)
        self.process_pending_native_menu();

        // Sync webview lifecycle: create/destroy/reposition/visibility
        self.sync_webviews();

        if self.dirty {
            self.window.request_redraw();
        }
    }

    /// Synchronize native WebView instances with the current state.
    /// Creates webviews for new Html panels, destroys removed ones,
    /// updates bounds and visibility based on active workspace/tab.
    fn sync_webviews(&mut self) {
        let terminal_rect = self.compute_terminal_rect();
        let scale_factor = self.gpu.scale_factor() as f64;
        let tab_bar_h = self.state.tab_bar_height as f64;

        // Collect all Html surface IDs and their visibility/bounds
        let active_ws = self.state.active_workspace;
        let mut active_html: std::collections::HashMap<u32, crate::webview::WebViewBounds> = std::collections::HashMap::new();
        let mut all_html_ids: Vec<u32> = Vec::new();

        for (ws_idx, ws) in self.state.engine.workspaces.iter().enumerate() {
            let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
            for (pane_id, pane_rect) in &pane_rects {
                if let Some(pane) = ws.pane_layout().find_pane(*pane_id) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        if let Some(html) = tab.surface().as_html() {
                            all_html_ids.push(html.id);
                            // Only visible if: active workspace AND active tab
                            let is_active_tab = tab_idx == pane.active_tab;
                            if ws_idx == active_ws && is_active_tab {
                                let bounds = crate::webview::WebViewBounds {
                                    x: pane_rect.x as f64 / scale_factor,
                                    y: (pane_rect.y as f64 + tab_bar_h) / scale_factor,
                                    width: pane_rect.width as f64 / scale_factor,
                                    height: (pane_rect.height as f64 - tab_bar_h).max(1.0) / scale_factor,
                                };
                                active_html.insert(html.id, bounds);
                            }
                        }
                    }
                }
            }
        }

        // Create new webviews for Html panels that don't have one yet
        for &sid in &all_html_ids {
            if !self.webviews.contains_key(&sid) {
                // Find the URL for this surface
                let url = self.find_html_url(sid);
                match crate::webview::PlatformWebView::new(
                    self.window.as_ref(),
                    active_html.get(&sid).copied().unwrap_or(crate::webview::WebViewBounds {
                        x: 0.0, y: 0.0, width: 1.0, height: 1.0,
                    }),
                    scale_factor,
                ) {
                    Ok(wv) => {
                        if let Some(url) = &url {
                            if url.starts_with("file://") || url.starts_with("http://") || url.starts_with("https://") {
                                wv.load_url(url);
                            } else {
                                wv.load_html(url);
                            }
                        }
                        // Start hidden if not active
                        if !active_html.contains_key(&sid) {
                            wv.set_visible(false);
                        }
                        self.webviews.insert(sid, wv);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create WebView for surface {}: {}", sid, e);
                    }
                }
            }
        }

        // When any egui overlay (context menu, popup, dialog) is open,
        // hide all WebViews so they don't cover the overlay.
        // Native views are always above the wgpu render surface in OS z-order.
        let overlay_open = self.state.has_egui_overlay_open();

        // Update bounds/visibility for existing webviews
        for (sid, wv) in &self.webviews {
            if overlay_open {
                wv.set_visible(false);
            } else if let Some(bounds) = active_html.get(sid) {
                wv.set_bounds(*bounds, scale_factor);
                wv.set_visible(true);
            } else if all_html_ids.contains(sid) {
                wv.set_visible(false);
            }
        }

        // Remove webviews for closed Html surfaces
        self.webviews.retain(|sid, _| all_html_ids.contains(sid));
    }

    /// Find the URL for an Html panel by surface ID.
    fn find_html_url(&self, surface_id: u32) -> Option<String> {
        for ws in &self.state.engine.workspaces {
            for &pid in &ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if let Some(html) = tab.surface().as_html() {
                            if html.id == surface_id {
                                return Some(html.url.clone());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Process pending native context menu request.
    /// Called after egui frame so we have access to the window handle.
    fn process_pending_native_menu(&mut self) {
        use crate::native_menu::{MenuItem, show_context_menu};
        use crate::state::PendingNativeMenu;

        let pending = match self.state.dialogs.pending_native_menu.take() {
            Some(p) => p,
            None => return,
        };

        match pending {
            PendingNativeMenu::Tab { pane_id, tab_index, x, y } => {
                let rename_label = crate::i18n::t("tab_context_menu.rename");
                let close_label = crate::i18n::t("tab_context_menu.close");
                let items = [
                    MenuItem::new(1, rename_label),
                    MenuItem::new(2, close_label),
                ];
                let result = show_context_menu(self.window.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        // Rename
                        let current_name = self.state.active_workspace()
                            .pane_layout()
                            .find_pane(pane_id)
                            .and_then(|p| p.tabs.get(tab_index))
                            .map(|t| t.display_name())
                            .unwrap_or_default();
                        self.state.dialogs.tab_rename = Some((pane_id, tab_index, current_name));
                    }
                    Some(2) => {
                        // Close
                        if let Some(pane) = self.state.active_workspace().pane_layout().find_pane(pane_id) {
                            if let Some(tab) = pane.tabs.get(tab_index) {
                                let ids = tab.surface().all_surface_ids();
                                if let Some(&sid) = ids.first() {
                                    self.state.close_surface_by_id(sid);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::Pane { pane_id, x, y } => {
                let items = [
                    MenuItem::new(1, "Open Markdown..."),
                    MenuItem::new(2, "Open Explorer"),
                    MenuItem::new(3, "Open HTML..."),
                ];
                let result = show_context_menu(self.window.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        // Create empty tab first, then show markdown dialog targeting it
                        self.state.active_workspace_mut().focused_pane = pane_id;
                        if let Some((_tab_id, surface_id)) = self.state.add_empty_tab() {
                            self.state.dialogs.markdown_convert_surface_id = Some(surface_id);
                            self.state.dialogs.markdown_path = Some((pane_id, String::new()));
                        }
                    }
                    Some(2) => {
                        let home = directories::BaseDirs::new()
                            .map(|d| d.home_dir().to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string());
                        self.state.active_workspace_mut().focused_pane = pane_id;
                        let _ = self.state.add_explorer_tab(home);
                    }
                    Some(3) => {
                        // Create empty tab first, then show HTML dialog targeting it
                        self.state.active_workspace_mut().focused_pane = pane_id;
                        if let Some((_tab_id, surface_id)) = self.state.add_empty_tab() {
                            self.state.dialogs.html_convert_surface_id = Some(surface_id);
                            self.state.dialogs.html_url = Some((pane_id, String::new()));
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
        }
    }
}
