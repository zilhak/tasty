use winit::event_loop::ActiveEventLoop;

use crate::plugin::PluginManager;
use crate::view::ui::View;

use super::MainView;

impl MainView {
    pub(super) fn handle_redraw(
        &mut self,
        _event_loop: &ActiveEventLoop,
        plugin_manager: Option<&PluginManager>,
    ) {
        // Check if settings button was clicked (ui.rs sets state.settings_open = true)
        if self.state.settings_open {
            self.state.settings_open = false;
            crate::shortcuts::send_app_event(&self.proxy, crate::AppEvent::OpenSettings);
        }
        // Same flow for plugins modal.
        if self.state.plugins_open {
            self.state.plugins_open = false;
            crate::shortcuts::send_app_event(&self.proxy, crate::AppEvent::OpenPlugins);
        }

        // When targeted_pty_polling is off, process all terminals every frame.
        // When on, individual terminals are processed via TerminalOutput(Some(id)) events,
        // but we still call process_all() as a safety net (it's a no-op if channels are empty).
        if self.state.process_all(&mut self.core_state) {
            self.recalc_ime_preedit_anchor();
            self.base.dirty = true;
        }

        // D.3.C.C.8: TerminalEvent → CoreEvent 변환은 Core::process_pty_output 이
        // event_handler 의 AppEvent::TerminalOutput 처리 안에서 수행한다.
        // redraw 는 더 이상 collect_events 분기를 가지지 않는다.

        // Re-sync scale factor before render — macOS may not fire
        // ScaleFactorChanged reliably during monitor hot-swap or sleep/wake.
        if self.base.gpu.sync_scale_factor(&self.base.winit) {
            let new_size = self.base.winit.inner_size();
            self.base.gpu.resize(new_size);
            let terminal_rect = self.compute_terminal_rect();
            let (cols, rows) = self.base.gpu.grid_size_for_rect(&terminal_rect);
            self.core_state.update_grid_size(cols, rows);
            // Schedule another redraw to verify scale factor has stabilized.
            self.base.dirty = true;
        }

        // Resize all terminals to match their current layout rects.
        // After structural changes (split, new tab, close pane) the terminal's
        // internal cols/rows may not match the actual rendering area. This call
        // is cheap: terminal.resize() early-returns when cols/rows are unchanged.
        {
            let terminal_rect = self.compute_terminal_rect();
            let cell_w = self.base.gpu.cell_width();
            let cell_h = self.base.gpu.cell_height();
            crate::core::Core::resize_all_terminals(
                &self.state,
                &mut self.core_state,
                terminal_rect,
                cell_w,
                cell_h,
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
            let active_sel = self.active_text_selection();
            let vi_cursor = self.vi_copy.as_ref().map(|v| (v.surface_id, v.cursor));
            match self.base.gpu.render(
                &mut self.state,
                &mut self.core_state,
                &self.base.winit,
                self.ime_preedit.as_ref(),
                active_sel.as_ref(),
                vi_cursor,
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

        // Command palette pending dispatch — popup writes `pending_run` when
        // user hits Enter or clicks a row. We drain after render so the popup
        // is already closed by the time the action fires (avoids racing with
        // any window state the action might mutate).
        if let Some(cmd_id) = self.state.command_palette.pending_run.take() {
            self.dispatch_action_by_id(cmd_id);
        }

        // Process pending native context menu (after egui frame, before webview sync)
        self.process_pending_native_menu();

        // file handler picker result 슬롯은 App::dispatch_pending_picker_results
        // 가 다음 frame begin 에 drain (D.3.C.G.3.c) — redraw 인라인 호출 폐기.

        // 외부 drag&drop 으로 받은 파일 큐 처리.
        self.process_pending_file_drops();

        // Process pending file drag (after egui frame)
        if let Some(paths) = self.state.dialogs.pending_file_drag.take() {
            let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
            if let Err(e) = crate::file::drag::start_file_drag(&*self.base.winit, &path_refs) {
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
        let engine = &mut self.core_state;

        // Collect all Html surface IDs and their visibility/bounds
        let active_ws = self.state.active_workspace;
        let mut active_html: std::collections::HashMap<u32, crate::webview::WebViewBounds> =
            std::collections::HashMap::new();
        let mut all_html_ids: Vec<u32> = Vec::new();

        for (ws_idx, ws) in engine.workspaces.iter().enumerate() {
            let pane_rects = ws.pane_layout().compute_rects(terminal_rect);
            for (pane_id, pane_rect) in &pane_rects {
                if let Some(pane) = ws.pane_layout().find_pane(*pane_id) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        let surface = tab.surface();
                        if surface.webview_url().is_some() {
                            let Some(sid) = surface.surface_id() else {
                                continue;
                            };
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
                let url = self.find_webview_url(sid);
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
    fn find_webview_url(&self, surface_id: u32) -> Option<String> {
        let engine = &self.core_state;
        for ws in &engine.workspaces {
            for &pid in &ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        let surface = tab.surface();
                        if surface.surface_id() == Some(surface_id)
                            && let Some(url) = surface.webview_url()
                        {
                            return Some(url.to_string());
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
        let engine = &mut self.core_state;
        use crate::platform::native_menu::{MenuItem, show_context_menu};
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
                    .active_workspace(engine)
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
                    MenuItem::separator(),
                    MenuItem::new(5, crate::i18n::t("preset.context.save_as_tab_preset")),
                    MenuItem::new(6, crate::i18n::t("preset.context.save_as_pane_preset")),
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        // Rename
                        let current_name = self
                            .state
                            .active_workspace(engine)
                            .pane_layout()
                            .find_pane(pane_id)
                            .and_then(|p| p.tabs.get(tab_index))
                            .map(|t| t.display_name())
                            .unwrap_or_default();
                        let target = crate::state::RenameTarget::TabName { pane_id, tab_index };
                        let scope = target.popup_scope();
                        self.state.dialogs.rename = Some((target, current_name));
                        self.state.dispatch_intent(
                            crate::intent::UiIntent::OpenPopup {
                                id: "rename",
                                mode: crate::intent::OpenPopupMode::WithScope(scope),
                            }
                            .from_user_context_menu(),
                        );
                    }
                    Some(2) => {
                        // Close
                        let target_sid = self
                            .state
                            .active_workspace(engine)
                            .pane_layout()
                            .find_pane(pane_id)
                            .and_then(|p| p.tabs.get(tab_index))
                            .and_then(|tab| tab.all_surface_ids().first().copied());
                        if let Some(sid) = target_sid
                            && self.state.close_surface_by_id(engine, sid, true)
                        {
                            // 마지막 workspace 까지 닫혔다면 keyboard close 와 동일하게
                            // window 종료를 요청한다 (그렇지 않으면 다음 redraw 가
                            // active_workspace() 호출에서 패닉).
                            if engine.workspaces.is_empty() {
                                self.request_close();
                            }
                        }
                    }
                    Some(3) => {
                        // Move Left
                        if tab_index > 0
                            && let Some(pane) = self
                                .state
                                .active_workspace_mut(&mut self.core_state)
                                .pane_layout_mut()
                                .find_pane_mut(pane_id)
                        {
                            pane.move_tab(tab_index, tab_index - 1);
                        }
                    }
                    Some(4) => {
                        // Move Right
                        if let Some(pane) = self
                            .state
                            .active_workspace_mut(&mut self.core_state)
                            .pane_layout_mut()
                            .find_pane_mut(pane_id)
                        {
                            pane.move_tab(tab_index, tab_index + 1);
                        }
                    }
                    Some(5) => {
                        if let Err(e) = self.save_tab_preset_from_pane_tab(pane_id, tab_index) {
                            tracing::warn!("save tab preset failed: {e}");
                            self.state.toasts.push(
                                crate::i18n::t("preset.toast.save_failed"),
                                crate::adapters::ui::ToastKind::Error,
                                crate::adapters::ui::ToastScope::Window,
                            );
                        }
                    }
                    Some(6) => {
                        if let Err(e) = self.save_pane_preset_from_pane_id(pane_id) {
                            tracing::warn!("save pane preset failed: {e}");
                            self.state.toasts.push(
                                crate::i18n::t("preset.toast.save_failed"),
                                crate::adapters::ui::ToastKind::Error,
                                crate::adapters::ui::ToastScope::Window,
                            );
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
                    MenuItem::separator(),
                    MenuItem::new(6, crate::i18n::t("preset.context.save_as_pane_preset")),
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        self.state.active_workspace_mut(engine).focused_pane = pane_id;
                        if let Err(e) = self.state.add_tab(engine) {
                            tracing::warn!("add_tab from context menu failed: {e}");
                        }
                    }
                    Some(2) => {
                        // Create empty tab first, then show markdown popup targeting it
                        self.state.active_workspace_mut(engine).focused_pane = pane_id;
                        if let Some((_tab_id, surface_id)) = self.state.add_empty_tab(engine) {
                            // intent-exempt: surface_id 결과 의존 (후속 convert)

                            self.state.dialogs.markdown_convert_surface_id = Some(surface_id);
                            self.state.dialogs.file_open_pane_id = Some(pane_id);
                            self.state.dialogs.markdown_open_buffer.clear();
                            self.state.dispatch_intent(
                                crate::intent::UiIntent::OpenPopup {
                                    id: "markdown_open",
                                    mode: crate::intent::OpenPopupMode::WithScope(
                                        crate::adapters::ui::popup::PopupScope::Surface(surface_id),
                                    ),
                                }
                                .from_user_context_menu(),
                            );
                        }
                    }
                    Some(5) => {
                        self.state.active_workspace_mut(engine).focused_pane = pane_id;
                        if let Some((_tab_id, surface_id)) = self.state.add_empty_tab(engine) {
                            // intent-exempt: surface_id 결과 의존 (후속 convert)

                            self.state.dispatch_intent(
                                crate::intent::Intent::ConvertSurface {
                                    surface_id,
                                    target: crate::intent::ConvertTarget::Kind {
                                        cwd: None,
                                        kind: "image".to_string(),
                                        params: serde_json::json!({}),
                                    },
                                }
                                .from_user_context_menu(),
                            );
                        }
                    }
                    Some(6) => {
                        if let Err(e) = self.save_pane_preset_from_pane_id(pane_id) {
                            tracing::warn!("save pane preset failed: {e}");
                            self.state.toasts.push(
                                crate::i18n::t("preset.toast.save_failed"),
                                crate::adapters::ui::ToastKind::Error,
                                crate::adapters::ui::ToastScope::Window,
                            );
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::Workspace { ws_idx, x, y } => {
                let ws_count = engine.workspaces.len();
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
                    MenuItem::separator(),
                    MenuItem::new(5, crate::i18n::t("preset.context.save_as_workspace_preset")),
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                if ws_idx < engine.workspaces.len() {
                    match result {
                        Some(1) => {
                            let name = engine.workspaces[ws_idx].name.clone();
                            let target = crate::state::RenameTarget::WorkspaceName { ws_idx };
                            let scope = target.popup_scope();
                            self.state.dialogs.rename = Some((target, name));
                            self.state.dispatch_intent(
                                crate::intent::UiIntent::OpenPopup {
                                    id: "rename",
                                    mode: crate::intent::OpenPopupMode::WithScope(scope),
                                }
                                .from_user_context_menu(),
                            );
                        }
                        Some(2) => {
                            let subtitle = engine.workspaces[ws_idx].subtitle.clone();
                            let target = crate::state::RenameTarget::WorkspaceSubtitle { ws_idx };
                            let scope = target.popup_scope();
                            self.state.dialogs.rename = Some((target, subtitle));
                            self.state.dispatch_intent(
                                crate::intent::UiIntent::OpenPopup {
                                    id: "rename",
                                    mode: crate::intent::OpenPopupMode::WithScope(scope),
                                }
                                .from_user_context_menu(),
                            );
                        }
                        Some(3) => {
                            // Move Up
                            if ws_idx > 0 {
                                self.state.move_workspace(engine, ws_idx, ws_idx - 1);
                            }
                        }
                        Some(4) => {
                            // Move Down
                            if ws_idx + 1 < engine.workspaces.len() {
                                self.state.move_workspace(engine, ws_idx, ws_idx + 1);
                            }
                        }
                        Some(5) => {
                            if let Err(e) = self.save_workspace_preset_from_idx(ws_idx) {
                                tracing::warn!("save workspace preset failed: {e}");
                                self.state.toasts.push(
                                    crate::i18n::t("preset.toast.save_failed"),
                                    crate::adapters::ui::ToastKind::Error,
                                    crate::adapters::ui::ToastScope::Window,
                                );
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
                        crate::adapters::ui::ToastScope::Surface(surface_id),
                    );
                }
                self.mark_dirty();
            }
        }
    }
}
