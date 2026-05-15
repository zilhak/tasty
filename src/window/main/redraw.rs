use winit::event_loop::ActiveEventLoop;

use crate::plugin::PluginManager;
use crate::window::Window;

use super::MainWindow;

impl MainWindow {
    pub(super) fn handle_redraw(
        &mut self,
        _event_loop: &ActiveEventLoop,
        plugin_manager: Option<&PluginManager>,
    ) {
        // Check if settings button was clicked (ui.rs sets state.settings_open = true)
        if self.state.settings_open {
            self.state.settings_open = false;
            let _ = self.proxy.send_event(crate::AppEvent::OpenSettings);
        }
        // Same flow for plugins modal.
        if self.state.plugins_open {
            self.state.plugins_open = false;
            let _ = self.proxy.send_event(crate::AppEvent::OpenPlugins);
        }

        // When targeted_pty_polling is off, process all terminals every frame.
        // When on, individual terminals are processed via TerminalOutput(Some(id)) events,
        // but we still call process_all() as a safety net (it's a no-op if channels are empty).
        if self.state.process_all() {
            self.recalc_ime_preedit_anchor();
            self.base.dirty = true;
        }

        // Collect terminal events
        let events = self.state.collect_events();
        for event in &events {
            let surface_id = event.surface_id;
            match &event.kind {
                crate::terminal::TerminalEventKind::Notification { title, body } => {
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.base.focused
                        && self
                            .state
                            .engine
                            .notifications
                            .should_send_system_notification()
                    {
                        crate::notification::send_system_notification(title, body);
                    }
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(
                            ws_id,
                            surface_id,
                            title.clone(),
                            body.clone(),
                        );
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Notification];
                    self.state
                        .engine
                        .hook_manager
                        .check_and_fire(surface_id, &hook_events);
                    self.base.dirty = true;
                }
                crate::terminal::TerminalEventKind::BellRing => {
                    if self.state.engine.settings.notification.enabled {
                        let ws_id = self.state.active_workspace().id;
                        self.state.engine.notifications.add(
                            ws_id,
                            surface_id,
                            "Bell".to_string(),
                            String::new(),
                        );
                    }
                    if self.state.engine.settings.notification.enabled
                        && self.state.engine.settings.notification.system_notification
                        && !self.base.focused
                        && self
                            .state
                            .engine
                            .notifications
                            .should_send_system_notification()
                    {
                        crate::notification::send_system_notification("Tasty", "Bell");
                    }
                    let hook_events = vec![tasty_hooks::HookEvent::Bell];
                    self.state
                        .engine
                        .hook_manager
                        .check_and_fire(surface_id, &hook_events);
                    self.base.dirty = true;
                }
                crate::terminal::TerminalEventKind::TitleChanged(title) => {
                    self.state.enqueue_host_event(
                        crate::state::PendingHostEvent::SurfaceTitleChanged {
                            surface_id,
                            title: title.clone(),
                        },
                    );
                    self.base.dirty = true;
                }
                crate::terminal::TerminalEventKind::CwdChanged(_) => {
                    self.state.refresh_tab_display_name(surface_id);
                    self.base.dirty = true;
                }
                crate::terminal::TerminalEventKind::ClipboardSet(data) => {
                    if let Some(cb) = &mut self.clipboard {
                        cb.set_text(data);
                    }
                    self.state.engine.record_internal_copy(data);
                }
                crate::terminal::TerminalEventKind::ProcessExited => {
                    let hook_events = vec![tasty_hooks::HookEvent::ProcessExit];
                    self.state
                        .engine
                        .hook_manager
                        .check_and_fire(surface_id, &hook_events);
                    let kind = self.state.surface_kind(surface_id);
                    if self.state.close_surface_by_id_no_snapshot(surface_id) {
                        if let Some(k) = kind {
                            // ProcessExited는 PTY 종료 등 사용자가 직접 닫은 행위가 아니지만
                            // 에이전트 명령도 아니다. 닫힌 항목 복원 정책상 user-close로 분류한다.
                            self.state.enqueue_surface_closed(surface_id, k, true);
                        }
                    }
                    self.base.dirty = true;
                }
            }
        }

        // Re-sync scale factor before render — macOS may not fire
        // ScaleFactorChanged reliably during monitor hot-swap or sleep/wake.
        if self.base.gpu.sync_scale_factor(&self.base.winit) {
            let new_size = self.base.winit.inner_size();
            self.base.gpu.resize(new_size);
            let terminal_rect = self.compute_terminal_rect();
            let (cols, rows) = self.base.gpu.grid_size_for_rect(&terminal_rect);
            self.state.update_grid_size(cols, rows);
            // Schedule another redraw to verify scale factor has stabilized.
            self.base.dirty = true;
        }

        // Resize all terminals to match their current layout rects.
        // After structural changes (split, new tab, close pane) the terminal's
        // internal cols/rows may not match the actual rendering area. This call
        // is cheap: terminal.resize() early-returns when cols/rows are unchanged.
        {
            let terminal_rect = self.compute_terminal_rect();
            self.state.resize_all(
                terminal_rect,
                self.base.gpu.cell_width(),
                self.base.gpu.cell_height(),
            );
        }

        // Render
        if self.base.dirty {
            self.base.dirty = false;
            self.update_ime_cursor_area();
            let link_hover = self
                .hovered_link
                .as_ref()
                .map(|h| (h.surface_id, &h.highlight));
            match self.base.gpu.render(
                &mut self.state,
                &self.base.winit,
                self.ime_preedit.as_ref(),
                self.text_selection.as_ref(),
                link_hover,
                plugin_manager,
            ) {
                Ok(()) => {}
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    self.base.gpu.resize(self.base.winit.inner_size());
                    // Surface was lost/outdated; resize recovers it, but we must
                    // re-render now that it's ready. dirty was set to false above,
                    // so restore it and request another frame.
                    self.base.dirty = true;
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

        // Process pending file drag (after egui frame)
        if let Some(paths) = self.state.dialogs.pending_file_drag.take() {
            let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
            if let Err(e) = crate::file_drag::start_file_drag(&*self.base.winit, &path_refs) {
                tracing::warn!("File drag failed: {e}");
            }
        }

        // Sync webview lifecycle: create/destroy/reposition/visibility
        self.sync_webviews();

        if self.base.dirty {
            self.base.winit.request_redraw();
        }
    }

    /// Synchronize native WebView instances with the current state.
    /// Creates webviews for new Html panels, destroys removed ones,
    /// updates bounds and visibility based on active workspace/tab.
    fn sync_webviews(&mut self) {
        let terminal_rect = self.compute_terminal_rect();
        let scale_factor = self.base.gpu.scale_factor() as f64;
        let tab_bar_h = self.state.tab_bar_height.value() as f64;

        // Collect all Html surface IDs and their visibility/bounds
        let active_ws = self.state.active_workspace;
        let mut active_html: std::collections::HashMap<u32, crate::webview::WebViewBounds> =
            std::collections::HashMap::new();
        let mut all_html_ids: Vec<u32> = Vec::new();

        for (ws_idx, ws) in self.state.engine.workspaces.iter().enumerate() {
            let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
            for (pane_id, pane_rect) in &pane_rects {
                if let Some(pane) = ws.pane_layout().find_pane(*pane_id) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        let surface = tab.surface();
                        if surface.html_url().is_some() {
                            let Some(sid) = surface.surface_id() else { continue };
                            all_html_ids.push(sid);
                            // Only visible if: active workspace AND active tab
                            let is_active_tab = tab_idx == pane.active_tab;
                            if ws_idx == active_ws && is_active_tab {
                                // Inset bounds by divider drag threshold so that
                                // the native WebView does not cover the divider
                                // hit-test area, allowing pane resize via drag.
                                let inset = 4.0_f64;
                                let bounds = crate::webview::WebViewBounds {
                                    x: (pane_rect.x.value() as f64 + inset) / scale_factor,
                                    y: (pane_rect.y.value() as f64 + tab_bar_h) / scale_factor,
                                    width: (pane_rect.width.value() as f64 - inset * 2.0).max(1.0)
                                        / scale_factor,
                                    height: (pane_rect.height.value() as f64 - tab_bar_h - inset)
                                        .max(1.0)
                                        / scale_factor,
                                };
                                active_html.insert(sid, bounds);
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
                    self.base.winit.as_ref(),
                    active_html
                        .get(&sid)
                        .copied()
                        .unwrap_or(crate::webview::WebViewBounds {
                            x: 0.0,
                            y: 0.0,
                            width: 1.0,
                            height: 1.0,
                        }),
                    scale_factor,
                ) {
                    Ok(wv) => {
                        if let Some(url) = &url {
                            if url.starts_with("file://")
                                || url.starts_with("http://")
                                || url.starts_with("https://")
                            {
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
                        let surface = tab.surface();
                        if surface.surface_id() == Some(surface_id) {
                            if let Some(url) = surface.html_url() {
                                return Some(url.to_string());
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
            PendingNativeMenu::Tab {
                pane_id,
                tab_index,
                x,
                y,
            } => {
                let tab_count = self
                    .state
                    .active_workspace()
                    .pane_layout()
                    .find_pane(pane_id)
                    .map(|p| p.tabs.len())
                    .unwrap_or(0);
                let can_move_left = tab_index > 0;
                let can_move_right = tab_index + 1 < tab_count;

                let move_left = if can_move_left {
                    MenuItem::new(3, crate::i18n::t("tab_context_menu.move_left"))
                } else {
                    MenuItem::disabled(3, crate::i18n::t("tab_context_menu.move_left"))
                };
                let move_right = if can_move_right {
                    MenuItem::new(4, crate::i18n::t("tab_context_menu.move_right"))
                } else {
                    MenuItem::disabled(4, crate::i18n::t("tab_context_menu.move_right"))
                };

                let items = [
                    MenuItem::new(1, crate::i18n::t("tab_context_menu.rename")),
                    MenuItem::new(2, crate::i18n::t("tab_context_menu.close")),
                    MenuItem::separator(),
                    move_left,
                    move_right,
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        // Rename
                        let current_name = self
                            .state
                            .active_workspace()
                            .pane_layout()
                            .find_pane(pane_id)
                            .and_then(|p| p.tabs.get(tab_index))
                            .map(|t| t.display_name())
                            .unwrap_or_default();
                        let target = crate::state::RenameTarget::TabName { pane_id, tab_index };
                        let scope = target.popup_scope();
                        self.state.dialogs.rename = Some((target, current_name));
                        self.state.popups.open_with_scope("rename", scope);
                    }
                    Some(2) => {
                        // Close
                        let target_sid = self
                            .state
                            .active_workspace()
                            .pane_layout()
                            .find_pane(pane_id)
                            .and_then(|p| p.tabs.get(tab_index))
                            .and_then(|tab| tab.all_surface_ids().first().copied());
                        if let Some(sid) = target_sid {
                            let kind = self.state.surface_kind(sid);
                            if self.state.close_surface_by_id(sid) {
                                if let Some(k) = kind {
                                    self.state.enqueue_surface_closed(sid, k, true);
                                }
                            }
                        }
                    }
                    Some(3) => {
                        // Move Left
                        if tab_index > 0 {
                            if let Some(pane) = self
                                .state
                                .active_workspace_mut()
                                .pane_layout_mut()
                                .find_pane_mut(pane_id)
                            {
                                pane.move_tab(tab_index, tab_index - 1);
                            }
                        }
                    }
                    Some(4) => {
                        // Move Right
                        if let Some(pane) = self
                            .state
                            .active_workspace_mut()
                            .pane_layout_mut()
                            .find_pane_mut(pane_id)
                        {
                            pane.move_tab(tab_index, tab_index + 1);
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::Pane { pane_id, x, y } => {
                let items = [
                    MenuItem::new(1, crate::i18n::t("pane_context_menu.new_terminal")),
                    MenuItem::new(2, crate::i18n::t("pane_context_menu.new_markdown")),
                    MenuItem::new(4, crate::i18n::t("pane_context_menu.new_html")),
                    MenuItem::new(5, crate::i18n::t("pane_context_menu.new_image")),
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        self.state.active_workspace_mut().focused_pane = pane_id;
                        if let Err(e) = self.state.add_tab() {
                            tracing::warn!("add_tab from context menu failed: {e}");
                        }
                    }
                    Some(2) => {
                        // Create empty tab first, then show markdown popup targeting it
                        self.state.active_workspace_mut().focused_pane = pane_id;
                        if let Some((_tab_id, surface_id)) = self.state.add_empty_tab() {
                            self.state.dialogs.markdown_convert_surface_id = Some(surface_id);
                            self.state.dialogs.file_open_pane_id = Some(pane_id);
                            self.state.dialogs.markdown_open_buffer.clear();
                            self.state.popups.open_with_scope(
                                "markdown_open",
                                crate::ui::popup::PopupScope::Surface(surface_id),
                            );
                        }
                    }
                    Some(4) => {
                        // Create empty tab first, then show HTML popup targeting it
                        self.state.active_workspace_mut().focused_pane = pane_id;
                        if let Some((_tab_id, surface_id)) = self.state.add_empty_tab() {
                            self.state.dialogs.html_convert_surface_id = Some(surface_id);
                            self.state.dialogs.file_open_pane_id = Some(pane_id);
                            self.state.dialogs.html_open_buffer.clear();
                            self.state.popups.open_with_scope(
                                "html_open",
                                crate::ui::popup::PopupScope::Surface(surface_id),
                            );
                        }
                    }
                    Some(5) => {
                        self.state.active_workspace_mut().focused_pane = pane_id;
                        if let Some((_tab_id, surface_id)) = self.state.add_empty_tab() {
                            self.state.convert_surface_to_image(surface_id);
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::Workspace { ws_idx, x, y } => {
                let ws_count = self.state.engine.workspaces.len();
                let can_move_up = ws_idx > 0;
                let can_move_down = ws_idx + 1 < ws_count;

                let move_up = if can_move_up {
                    MenuItem::new(3, crate::i18n::t("context_menu.move_up"))
                } else {
                    MenuItem::disabled(3, crate::i18n::t("context_menu.move_up"))
                };
                let move_down = if can_move_down {
                    MenuItem::new(4, crate::i18n::t("context_menu.move_down"))
                } else {
                    MenuItem::disabled(4, crate::i18n::t("context_menu.move_down"))
                };

                let items = [
                    MenuItem::new(1, crate::i18n::t("context_menu.rename_title")),
                    MenuItem::new(2, crate::i18n::t("context_menu.rename_subtitle")),
                    MenuItem::separator(),
                    move_up,
                    move_down,
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                if ws_idx < self.state.engine.workspaces.len() {
                    match result {
                        Some(1) => {
                            let name = self.state.engine.workspaces[ws_idx].name.clone();
                            let target = crate::state::RenameTarget::WorkspaceName { ws_idx };
                            let scope = target.popup_scope();
                            self.state.dialogs.rename = Some((target, name));
                            self.state.popups.open_with_scope("rename", scope);
                        }
                        Some(2) => {
                            let subtitle = self.state.engine.workspaces[ws_idx].subtitle.clone();
                            let target = crate::state::RenameTarget::WorkspaceSubtitle { ws_idx };
                            let scope = target.popup_scope();
                            self.state.dialogs.rename = Some((target, subtitle));
                            self.state.popups.open_with_scope("rename", scope);
                        }
                        Some(3) => {
                            // Move Up
                            if ws_idx > 0 {
                                self.state.move_workspace(ws_idx, ws_idx - 1);
                            }
                        }
                        Some(4) => {
                            // Move Down
                            if ws_idx + 1 < self.state.engine.workspaces.len() {
                                self.state.move_workspace(ws_idx, ws_idx + 1);
                            }
                        }
                        _ => {}
                    }
                }
                self.mark_dirty();
            }
            PendingNativeMenu::TerminalSurface { surface_id, x, y } => {
                let items = [MenuItem::new(
                    1,
                    crate::i18n::t("terminal_context_menu.copy_surface_id"),
                )];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                if let Some(1) = result {
                    let text = surface_id.to_string();
                    if let Some(cb) = &mut self.clipboard {
                        cb.set_text(&text);
                    }
                    self.state.toasts.push_info(
                        crate::i18n::t("toast.copied"),
                        crate::ui::ToastScope::Surface(surface_id),
                    );
                }
                self.mark_dirty();
            }
        }
    }
}
