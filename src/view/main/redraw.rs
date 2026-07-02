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

        // PTY drain 은 전적으로 AppEvent::TerminalOutput 핸들러 몫이다. 과거의
        // per-frame process_all safety net 은 제거됨 — 코얼레싱 게이트의
        // early-reset(drain 전 게이트 해제)과 reader 의 EOF 최종 wake 가
        // 스킵된 wake 의 데이터까지 커버한다 (형식적 메모리모델 잔여 윈도우는
        // 실하드웨어에서 사실상 0 — ns 급 store 가시화 지연 vs µs 급 핸들러 경로).

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
            // egui-mesh surface 에 렌더 컨텍스트(크기/ppp/입력) forward (A1-S7) — 합성
            // (gpu.render) 직전. plugin 이 PaintFrame 으로 회신하면 합성기가 그린다.
            // link_hover 등 self 불변 차용을 잡기 *전*에 호출한다(&mut self).
            if let Some(mgr) = plugin_manager {
                self.forward_egui_mesh_context(mgr);
            }
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

            // egui-mesh full 재전송 요청 drain — 렌더 prepare 가 textures_delta 체인
            // 단절을 감지한 대상들. surface 는 forward 추적 상태에, popup/banner 는
            // AppState 에 옮겨 두면 다음 tick 의 forward 가 need_full_textures
            // set_context 를 보낸다. plugin 은 스스로 재송신하지 않으므로 다음 tick 을
            // dirty 로 보장한다.
            let full_reqs = self.base.gpu.take_egui_mesh_full_requests();
            let popup_full_reqs = self.base.gpu.take_egui_mesh_popup_full_requests();
            let banner_full_reqs = self.base.gpu.take_egui_mesh_banner_full_requests();
            if !full_reqs.is_empty() || !popup_full_reqs.is_empty() || !banner_full_reqs.is_empty()
            {
                for sid in full_reqs {
                    self.egui_mesh.entry(sid).or_default().set_pending_full();
                }
                self.state
                    .plugin_mesh_popup_full_requests
                    .extend(popup_full_reqs);
                self.state
                    .plugin_mesh_banner_full_requests
                    .extend(banner_full_reqs);
                self.base.dirty = true;
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

    /// 해당 surface 가 현재 사용자에게 보이는가.
    ///
    /// 가시 기준은 `sync_webviews` 의 webview 표시 기준(`ws_idx == active_ws &&
    /// is_active_tab`)과 동일하다 — 활성 워크스페이스의 각 pane 에서 active_tab 에
    /// 속한 surface 만 화면에 렌더되고, 비활성 탭/비활성 워크스페이스의 surface 는
    /// 숨겨진다. split tab 은 active_tab 안의 모든 surface 가 동시에 보이므로
    /// `tab.contains_surface` 로 판정한다.
    ///
    /// P3: 안 보이는 surface 의 PTY 출력은 보이는 창의 콘텐츠를 바꾸지 않으므로
    /// 이 판정으로 redraw 요청을 게이트한다(데이터 drain 은 그대로 수행).
    pub(crate) fn is_surface_visible(&self, surface_id: u32) -> bool {
        let Some(ws) = self.core_state.workspaces.get(self.state.active_workspace) else {
            return false;
        };
        for pane_id in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id)
                && let Some(tab) = pane.tabs.get(pane.active_tab)
                && tab.contains_surface(surface_id)
            {
                return true;
            }
        }
        false
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
                // 생성 시 적용할 plugin 설정(부재 시 default) 을 미리 해석한다.
                let settings = self.resolve_webview_settings(sid);
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
                        // 생성 직후 HTML viewer 설정(zoom/JS/scheme/remote) 적용 + 기록.
                        settings.apply(&wv);
                        // Start hidden if not active
                        if !active_html.contains_key(&sid) {
                            wv.set_visible(false);
                        }
                        self.webviews.insert(sid, wv);
                        self.webview_applied_settings.insert(sid, settings);
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

        // Update bounds/visibility for existing webviews.
        // reveal(=native 페이지 노출)은 navigation 이 성공 완료(Done)일 때만 — Loading/Failed/
        // Idle 이면 native overlay 를 숨겨 그 자리에 egui chrome(spinner/error/placeholder)이
        // 보이게 한다(S-W3 가 지적한 "loading 중 overlay 가 spinner 를 덮는" 문제 해소).
        for (sid, wv) in &self.webviews {
            // active 면 bounds 는 숨겨져 있어도 갱신(다음 reveal 대비).
            if let Some(bounds) = active_html.get(sid) {
                wv.set_bounds(*bounds, scale_factor);
            }
            let reveal = !overlay_open
                && active_html.contains_key(sid)
                && wv.nav_state() == crate::webview::NavState::Done;
            wv.set_visible(reveal);
        }

        // Remove webviews for closed Html surfaces
        self.webviews.retain(|sid, _| all_html_ids.contains(sid));
        self.webview_applied_settings
            .retain(|sid, _| all_html_ids.contains(sid));

        // 설정 변경 재적용 — 살아있는 webview 마다 현재 설정을 해석해, 마지막 적용값과
        // 다를 때만 backend 에 재적용한다(변경 없으면 backend 호출 0 — 매 프레임 호출 회피).
        let live_sids: Vec<u32> = self.webviews.keys().copied().collect();
        for sid in live_sids {
            let resolved = self.resolve_webview_settings(sid);
            if self.webview_applied_settings.get(&sid) != Some(&resolved) {
                if let Some(wv) = self.webviews.get(&sid) {
                    resolved.apply(wv);
                }
                self.webview_applied_settings.insert(sid, resolved);
            }
        }

        // native nav_state 를 RemoteSurface 로 mirror — egui 렌더 경로(egui_panels →
        // webview_chrome)가 다음 프레임에 읽어 loading/error chrome 을 그린다. borrow 충돌
        // 회피를 위해 (sid, nav) 를 먼저 수집한 뒤 기록한다. 전이가 있으면 mark_dirty 로
        // 한 프레임 더 그려 chrome 을 갱신한다(가시성 전환 자체는 위에서 native 즉시 적용).
        let navs: Vec<(u32, crate::webview::NavState)> = self
            .webviews
            .iter()
            .map(|(s, w)| (*s, w.nav_state()))
            .collect();
        let mut nav_changed = false;
        for (sid, nav) in navs {
            if let Some(rs) = self.find_remote_surface(sid)
                && rs.nav_state() != nav
            {
                rs.set_nav_state(nav);
                nav_changed = true;
            }
        }
        if nav_changed {
            self.mark_dirty();
        }
    }

    /// surface_id 로 RemoteSurface 를 찾아 반환. `find_webview_url` 과 같은 순회 +
    /// RemoteSurface 다운캐스트. nav_state mirror 기록에 쓴다.
    fn find_remote_surface(
        &self,
        surface_id: u32,
    ) -> Option<&crate::plugin_bridge::remote_surface::RemoteSurface> {
        for ws in &self.core_state.workspaces {
            for &pid in &ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        let surface = tab.surface();
                        if surface.surface_id() == Some(surface_id) {
                            return surface
                                .as_any()
                                .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>(
                                );
                        }
                    }
                }
            }
        }
        None
    }

    /// webview surface 의 kind. `find_webview_url` 과 같은 순회.
    fn webview_surface_kind(&self, surface_id: u32) -> Option<&'static str> {
        for ws in &self.core_state.workspaces {
            for &pid in &ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        let surface = tab.surface();
                        if surface.surface_id() == Some(surface_id) {
                            return Some(surface.kind());
                        }
                    }
                }
            }
        }
        None
    }

    /// surface 의 plugin 설정을 해석해 webview 에 적용할 값으로 만든다. html webview(`com.tasty.html`)
    /// 만 generic 설정을 소비하고, 그 외 webview kind 는 default 를 쓴다(미래 kind 안전).
    /// 키 부재 시 manifest default(zoom 100 / sandbox true→JS off / remote false / scheme follow).
    fn resolve_webview_settings(&self, surface_id: u32) -> crate::webview::HtmlWebViewSettings {
        use crate::settings::PluginSettingValue;
        use crate::webview::{ColorScheme, HtmlWebViewSettings};

        let plugin_id = match self.webview_surface_kind(surface_id) {
            Some("html") => "com.tasty.html",
            _ => return HtmlWebViewSettings::default(),
        };
        let s = &self.core_state.settings;
        let zoom_percent = match s.plugin_setting(plugin_id, "zoom") {
            Some(PluginSettingValue::Number(n)) => *n,
            _ => 100.0,
        };
        let sandbox = match s.plugin_setting(plugin_id, "sandbox_scripts") {
            Some(PluginSettingValue::Bool(b)) => *b,
            _ => true,
        };
        let allow_remote_content = match s.plugin_setting(plugin_id, "allow_remote_content") {
            Some(PluginSettingValue::Bool(b)) => *b,
            _ => false,
        };
        let color_scheme = match s.plugin_setting(plugin_id, "color_scheme") {
            Some(PluginSettingValue::Text(t)) => match t.as_str() {
                "light" => ColorScheme::Light,
                "dark" => ColorScheme::Dark,
                _ => ColorScheme::Follow,
            },
            _ => ColorScheme::Follow,
        };
        HtmlWebViewSettings {
            zoom_percent,
            javascript_enabled: !sandbox, // "Sandbox scripts" on(기본) → JS off
            allow_remote_content,
            color_scheme,
        }
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
                        // Close tab (모든 surface 포함). 이전엔 첫 surface 만 닫아 split
                        // 상태에서 surface 하나만 사라지던 버그가 있었음.
                        if self.state.close_tab(engine, pane_id, tab_index)
                            && engine.workspaces.is_empty()
                        {
                            self.request_close();
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
                    MenuItem::separator(),
                    MenuItem::new(7, crate::i18n::t("preset.context.apply_tab_preset")),
                    MenuItem::new(8, crate::i18n::t("preset.context.apply_pane_preset")),
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
                    Some(7) => {
                        self.state.active_workspace_mut(engine).focused_pane = pane_id;
                        self.state.dialogs.preset_picker_selected = None;
                        self.state.dispatch_intent(
                            crate::intent::UiIntent::OpenPopup {
                                id: crate::adapters::ui::popup::preset_apply::APPLY_TAB_POPUP_ID,
                                mode: crate::intent::OpenPopupMode::CenteredFocused,
                            }
                            .from_user_context_menu(),
                        );
                    }
                    Some(8) => {
                        self.state.active_workspace_mut(engine).focused_pane = pane_id;
                        self.state.dialogs.preset_picker_selected = None;
                        self.state.dispatch_intent(
                            crate::intent::UiIntent::OpenPopup {
                                id: crate::adapters::ui::popup::preset_apply::APPLY_PANE_POPUP_ID,
                                mode: crate::intent::OpenPopupMode::CenteredFocused,
                            }
                            .from_user_context_menu(),
                        );
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

                let mut items = vec![
                    MenuItem::new(1, crate::i18n::t("context_menu.rename_title")),
                    MenuItem::new(2, crate::i18n::t("context_menu.rename_subtitle")),
                    MenuItem::separator(),
                    move_up,
                    move_down,
                    MenuItem::separator(),
                    MenuItem::new(5, crate::i18n::t("preset.context.save_as_workspace_preset")),
                    MenuItem::separator(),
                    MenuItem::new(6, crate::i18n::t("context_menu.close_workspace")),
                ];

                // 카테고리 토글 on — "카테고리로 이동"(현재 소속 제외 평면 나열, 선택지 B)
                // + "새 카테고리". move_targets[i] = (cat_id) 로 결과 id(200+i) 매핑.
                let mut move_targets: Vec<crate::model::WorkspaceCategoryId> = Vec::new();
                if engine.settings.general.workspace_categories_enabled
                    && ws_idx < engine.workspaces.len()
                {
                    let cur_cat = engine.workspaces[ws_idx].category;
                    items.push(MenuItem::separator());
                    // 비클릭 헤더(disabled) + 대상 카테고리 항목들.
                    items.push(MenuItem::disabled(
                        0,
                        crate::i18n::t("workspace_category.move_to_category"),
                    ));
                    for cat in engine.categories() {
                        if cat.id == cur_cat {
                            continue;
                        }
                        let label = if cat.is_normal() {
                            crate::i18n::t("sidebar.workspaces_heading").to_string()
                        } else {
                            cat.name.clone()
                        };
                        items.push(MenuItem::new(200 + move_targets.len() as u32, label));
                        move_targets.push(cat.id);
                    }
                    items.push(MenuItem::separator());
                    items.push(MenuItem::new(
                        100,
                        crate::i18n::t("workspace_category.new_category"),
                    ));
                }

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
                        // 의도적 비축약 — close_workspace_at 은 부수효과 호출이라
                        // match guard 로 옮기면 guard 에 부수효과를 기대하지 않는
                        // 독자에게 함정이 된다.
                        #[allow(clippy::collapsible_match)]
                        Some(6) => {
                            // Close workspace (모든 surface + closed_item snapshot)
                            if self.state.close_workspace_at(engine, ws_idx)
                                && engine.workspaces.is_empty()
                            {
                                self.request_close();
                            }
                        }
                        Some(100) => {
                            // 새 카테고리 생성 다이얼로그.
                            crate::adapters::ui::category_actions::open_new_category_dialog(
                                &mut self.state,
                            );
                        }
                        Some(id) if id >= 200 => {
                            // 카테고리로 이동 — move_targets[id-200] 로 소속 변경.
                            if let Some(&cat_id) = move_targets.get((id - 200) as usize) {
                                let ws_id = engine.workspaces[ws_idx].id;
                                if let Err(e) = engine.set_workspace_category(ws_id, cat_id) {
                                    tracing::warn!("set_workspace_category failed: {e:?}");
                                }
                                engine.mark_layout_dirty();
                            }
                        }
                        _ => {}
                    }
                }
                self.mark_dirty();
            }
            PendingNativeMenu::WorkspaceCategoryHeader { cat_id, x, y } => {
                // 카테고리 헤더 우클릭 — Add workspace 선두(모든 헤더), 비-normal 만
                // 이름변경/삭제, 공통 새 카테고리 (2026-07-02 디자인 — 조립·순서는
                // `category_header_menu_items` 가 고정). 토글 off 면 애초에 라우팅되지 않음.
                let is_normal = engine
                    .categories()
                    .iter()
                    .find(|c| c.id == cat_id)
                    .map(|c| c.is_normal())
                    .unwrap_or(true);
                let items = category_header_menu_items(is_normal);
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(3) => {
                        // 이 카테고리 소속으로 새 워크스페이스 — 레일 `---` 팝업의
                        // Add workspace 와 동일 인텐트 (소스 문자열만 구별).
                        self.state.dispatch_intent(
                            crate::intent::Intent::NewWorkspace {
                                kind: None,
                                params: serde_json::Value::Null,
                                category: Some(cat_id),
                            }
                            .from_user_menu("category_header/add_workspace"),
                        );
                    }
                    Some(1) => {
                        crate::adapters::ui::category_actions::open_rename_category_dialog(
                            &mut self.state,
                            engine,
                            cat_id,
                        );
                    }
                    Some(2) => {
                        crate::adapters::ui::category_actions::open_delete_category_confirm(
                            &mut self.state,
                            cat_id,
                        );
                    }
                    Some(100) => {
                        crate::adapters::ui::category_actions::open_new_category_dialog(
                            &mut self.state,
                        );
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::SidebarBackground { x, y } => {
                // 빈 배경 우클릭 — 새 카테고리.
                let items = [MenuItem::new(
                    100,
                    crate::i18n::t("workspace_category.new_category"),
                )];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                if let Some(100) = result {
                    crate::adapters::ui::category_actions::open_new_category_dialog(
                        &mut self.state,
                    );
                }
                self.mark_dirty();
            }
            PendingNativeMenu::TerminalSurface { surface_id, x, y } => {
                // Show copy items only when there is an active (non-empty) selection.
                let has_selection = self.text_selection.as_ref().is_some_and(|s| !s.is_empty());
                let mut items = Vec::new();
                if has_selection {
                    items.push(MenuItem::new(
                        2,
                        crate::i18n::t("terminal_context_menu.copy"),
                    ));
                    items.push(MenuItem::new(
                        3,
                        crate::i18n::t("terminal_context_menu.copy_no_newline"),
                    ));
                    items.push(MenuItem::separator());
                }
                items.push(MenuItem::new(
                    1,
                    crate::i18n::t("terminal_context_menu.copy_surface_id"),
                ));
                // T9: surface 공용 잘라내기 / 여기로 이동 tail.
                items.push(MenuItem::separator());
                items.push(MenuItem::new(
                    10,
                    crate::i18n::t("surface_context_menu.cut"),
                ));
                if engine.pending_move_surface.is_some() {
                    items.push(MenuItem::new(
                        11,
                        crate::i18n::t("surface_context_menu.move_here"),
                    ));
                }
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        let text = surface_id.to_string();
                        if let Some(cb) = &mut self.clipboard {
                            cb.set_text(&text);
                        }
                        self.state.toasts.push_info(
                            crate::i18n::t("toast.copied"),
                            crate::adapters::ui::ToastScope::Surface(surface_id),
                        );
                    }
                    Some(2) => {
                        self.copy_selection_to_clipboard();
                    }
                    Some(3) => {
                        self.copy_selection_no_newline();
                    }
                    Some(10) => {
                        // 잘라내기 마킹 — 사용자 우클릭 조작(release 경로). 도메인 mutate 아님.
                        engine.pending_move_surface = Some(surface_id);
                    }
                    Some(11) => {
                        if let Some(source) = engine.pending_move_surface.take() {
                            self.state.dispatch_intent(
                                crate::core::intent::DomainIntent::MoveSurface {
                                    source_surface_id: source,
                                    target_surface_id: surface_id,
                                }
                                .from_user_context_menu(),
                            );
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::Surface { surface_id, x, y } => {
                // 비-terminal surface: 전용 항목(copy surface id) + 구분선 +
                // 잘라내기 / (대기 있을 때) 여기로 이동.
                let mut items = vec![MenuItem::new(
                    1,
                    crate::i18n::t("terminal_context_menu.copy_surface_id"),
                )];
                items.push(MenuItem::separator());
                items.push(MenuItem::new(
                    10,
                    crate::i18n::t("surface_context_menu.cut"),
                ));
                if engine.pending_move_surface.is_some() {
                    items.push(MenuItem::new(
                        11,
                        crate::i18n::t("surface_context_menu.move_here"),
                    ));
                }
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        let text = surface_id.to_string();
                        if let Some(cb) = &mut self.clipboard {
                            cb.set_text(&text);
                        }
                        self.state.toasts.push_info(
                            crate::i18n::t("toast.copied"),
                            crate::adapters::ui::ToastScope::Surface(surface_id),
                        );
                    }
                    Some(10) => {
                        engine.pending_move_surface = Some(surface_id);
                    }
                    Some(11) => {
                        if let Some(source) = engine.pending_move_surface.take() {
                            self.state.dispatch_intent(
                                crate::core::intent::DomainIntent::MoveSurface {
                                    source_surface_id: source,
                                    target_surface_id: surface_id,
                                }
                                .from_user_context_menu(),
                            );
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::Explorer {
                surface_id,
                paths,
                cwd,
                single_is_dir,
                x,
                y,
            } => {
                let multi = paths.len() > 1;
                let is_empty_target = paths.is_empty();
                let is_folder = paths.len() == 1 && single_is_dir;
                let has_clip = engine
                    .explorer_clipboard
                    .as_ref()
                    .map(|c| !c.paths.is_empty())
                    .unwrap_or(false);

                let copy_path_label = if multi {
                    crate::i18n::t("explorer.context_menu.copy_path_multi")
                } else {
                    crate::i18n::t("explorer.context_menu.copy_path")
                };
                let mut items = vec![MenuItem::new(1, copy_path_label)];
                // 즐겨찾기 추가는 단일 폴더 또는 빈 영역(cwd, 디렉토리)에서만 (design §3.3).
                if is_empty_target || is_folder {
                    items.push(MenuItem::new(
                        50,
                        crate::i18n::t("explorer.context_menu.add_to_favorites"),
                    ));
                }
                // 단일 폴더: 새 탭으로 열기 / 이 폴더로 루트 설정 (빈 영역=cwd 자기
                // 자신엔 무의미 → 제외).
                if is_folder {
                    items.push(MenuItem::new(
                        60,
                        crate::i18n::t("explorer.context_menu.open_in_new_tab"),
                    ));
                    items.push(MenuItem::new(
                        61,
                        crate::i18n::t("explorer.context_menu.set_as_root"),
                    ));
                }
                if is_empty_target {
                    // 빈 영역(cwd): 붙여넣기만 (클립보드가 있을 때).
                    if has_clip {
                        items.push(MenuItem::separator());
                        items.push(MenuItem::new(
                            12,
                            crate::i18n::t("explorer.context_menu.paste"),
                        ));
                    }
                } else {
                    // 파일/폴더/다중: 복사 · 잘라내기.
                    items.push(MenuItem::new(
                        10,
                        crate::i18n::t("explorer.context_menu.copy_files"),
                    ));
                    items.push(MenuItem::new(
                        11,
                        crate::i18n::t("explorer.context_menu.cut"),
                    ));
                    if is_folder && has_clip {
                        items.push(MenuItem::new(
                            12,
                            crate::i18n::t("explorer.context_menu.paste_into"),
                        ));
                    }
                    // 이름 변경 (단일 파일/폴더만).
                    if !multi {
                        items.push(MenuItem::new(
                            40,
                            crate::i18n::t("explorer.context_menu.rename"),
                        ));
                    }
                    items.push(MenuItem::separator());
                    // 휴지통으로 이동 (파일/폴더/다중 공통).
                    items.push(MenuItem::new(
                        30,
                        crate::i18n::t("explorer.context_menu.delete"),
                    ));
                    // "Open in system" 은 단일 폴더에서만 (design §3.3) — 메뉴 끝.
                    if is_folder {
                        items.push(MenuItem::new(
                            20,
                            crate::i18n::t("explorer.context_menu.open_in_system"),
                        ));
                    }
                }
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        let text = if is_empty_target {
                            cwd.display().to_string()
                        } else {
                            paths
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect::<Vec<_>>()
                                .join("\n")
                        };
                        if let Some(cb) = &mut self.clipboard {
                            cb.set_text(&text);
                        }
                        self.state.toasts.push_info(
                            crate::i18n::t("toast.copied"),
                            crate::adapters::ui::ToastScope::Surface(surface_id),
                        );
                    }
                    Some(10) => {
                        engine.explorer_clipboard = Some(crate::core::state::ExplorerClipboard {
                            paths: paths.clone(),
                            cut: false,
                        });
                    }
                    Some(11) => {
                        engine.explorer_clipboard = Some(crate::core::state::ExplorerClipboard {
                            paths: paths.clone(),
                            cut: true,
                        });
                    }
                    Some(12) => {
                        let dest = if is_folder {
                            paths.first().cloned().unwrap_or_else(|| cwd.clone())
                        } else {
                            cwd.clone()
                        };
                        if let Some(clip) = engine.explorer_clipboard.clone() {
                            let (ok, err) =
                                crate::explorer_ui::ops::paste_all(&clip.paths, &dest, clip.cut);
                            // 잘라내기는 이동 성공 시 클립보드 소진.
                            if clip.cut && err.is_none() {
                                engine.explorer_clipboard = None;
                            }
                            if let Some(v) = self.state.explorer_views.get_mut(surface_id) {
                                v.request_reload();
                            }
                            if let Some(e) = err {
                                tracing::warn!("explorer: paste error ({ok} ok): {e}");
                            }
                        }
                    }
                    Some(30) => {
                        // 휴지통으로 이동 (가역적이라 별도 확인 모달 없음).
                        if let Err(e) = trash::delete_all(&paths) {
                            tracing::warn!("explorer: move to trash failed: {e}");
                        }
                        if let Some(v) = self.state.explorer_views.get_mut(surface_id) {
                            v.selected.clear();
                            v.anchor = None;
                            v.request_reload();
                        }
                    }
                    Some(20) => {
                        let target = paths.first().cloned().unwrap_or_else(|| cwd.clone());
                        if let Err(e) = crate::platform::reveal::open_path(&target) {
                            tracing::warn!("explorer: open_path failed: {e}");
                        }
                    }
                    Some(40) => {
                        if let Some(path) = paths.first().cloned() {
                            let current_name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let target =
                                crate::state::RenameTarget::ExplorerEntry { surface_id, path };
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
                    }
                    Some(50) => {
                        // 즐겨찾기 추가 — 대상: 단일 폴더면 그 폴더, 빈 영역이면 cwd.
                        let path = if is_empty_target {
                            cwd.clone()
                        } else {
                            paths.first().cloned().unwrap_or_else(|| cwd.clone())
                        };
                        let seed = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let target = crate::state::RenameTarget::ExplorerAddFavorite { path };
                        let scope = target.popup_scope();
                        self.state.dialogs.rename = Some((target, seed));
                        self.state.dispatch_intent(
                            crate::intent::UiIntent::OpenPopup {
                                id: "rename",
                                mode: crate::intent::OpenPopupMode::WithScope(scope),
                            }
                            .from_user_context_menu(),
                        );
                    }
                    Some(60) => {
                        // 새 탭으로 열기 — 대상 폴더를 cwd 로 하는 새 explorer 를 우클릭
                        // 대상 surface 의 소유 pane 에 Pane 탭으로 연다(기존 surface 불변).
                        if let Some(folder) = paths.first().cloned() {
                            let params = serde_json::json!({ "path": folder.to_string_lossy() });
                            if let Err(e) = self
                                .state
                                .add_kind_tab_by_owner(engine, surface_id, "explorer", &params)
                            {
                                tracing::warn!("explorer: open in new tab failed: {e}");
                            }
                        }
                    }
                    Some(61) => {
                        // 이 폴더로 루트 설정 — 현재 explorer 의 cwd 를 그 폴더로 이동.
                        if let Some(folder) = paths.first().cloned() {
                            self.state.set_explorer_cwd(engine, surface_id, folder);
                        }
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::ExplorerFavorite {
                surface_id,
                path,
                x,
                y,
            } => {
                let items = [
                    MenuItem::new(60, crate::i18n::t("explorer.context_menu.open_in_new_tab")),
                    MenuItem::new(61, crate::i18n::t("explorer.context_menu.set_as_root")),
                    MenuItem::separator(),
                    MenuItem::new(
                        1,
                        crate::i18n::t("explorer.context_menu.remove_from_favorites"),
                    ),
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(60) => {
                        let params = serde_json::json!({ "path": path.to_string_lossy() });
                        if let Err(e) = self
                            .state
                            .add_kind_tab_by_owner(engine, surface_id, "explorer", &params)
                        {
                            tracing::warn!("explorer: open favorite in new tab failed: {e}");
                        }
                    }
                    Some(61) => {
                        self.state
                            .set_explorer_cwd(engine, surface_id, path.clone());
                    }
                    Some(1) => {
                        // 사이드바는 다음 프레임 스냅샷에서 갱신 — redraw 만 요청.
                        engine.explorer_favorites.remove(&path);
                        engine.explorer_favorites.save();
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
            PendingNativeMenu::NewWorkspaceButton { x, y } => {
                let items = [MenuItem::new(
                    1,
                    crate::i18n::t("preset.context.apply_workspace_preset"),
                )];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                if let Some(1) = result {
                    self.state.dialogs.preset_picker_selected = None;
                    self.state.dispatch_intent(
                        crate::intent::UiIntent::OpenPopup {
                            id: crate::adapters::ui::popup::preset_apply::APPLY_WORKSPACE_POPUP_ID,
                            mode: crate::intent::OpenPopupMode::CenteredFocused,
                        }
                        .from_user_context_menu(),
                    );
                }
                self.mark_dirty();
            }
            PendingNativeMenu::NewTabButton { pane_id, x, y } => {
                let items = [
                    MenuItem::new(1, crate::i18n::t("preset.context.apply_tab_preset")),
                    MenuItem::new(2, crate::i18n::t("preset.context.apply_pane_preset")),
                ];
                let result =
                    show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
                match result {
                    Some(1) => {
                        self.state.active_workspace_mut(engine).focused_pane = pane_id;
                        self.state.dialogs.preset_picker_selected = None;
                        self.state.dispatch_intent(
                            crate::intent::UiIntent::OpenPopup {
                                id: crate::adapters::ui::popup::preset_apply::APPLY_TAB_POPUP_ID,
                                mode: crate::intent::OpenPopupMode::CenteredFocused,
                            }
                            .from_user_context_menu(),
                        );
                    }
                    Some(2) => {
                        self.state.active_workspace_mut(engine).focused_pane = pane_id;
                        self.state.dialogs.preset_picker_selected = None;
                        self.state.dispatch_intent(
                            crate::intent::UiIntent::OpenPopup {
                                id: crate::adapters::ui::popup::preset_apply::APPLY_PANE_POPUP_ID,
                                mode: crate::intent::OpenPopupMode::CenteredFocused,
                            }
                            .from_user_context_menu(),
                        );
                    }
                    _ => {}
                }
                self.mark_dirty();
            }
        }
    }
}

/// 카테고리 헤더 우클릭 메뉴 항목 조립 (디자인 sidebar_context_menu.jsx category 분기
/// 전사, 2026-07-02). additive 선두: Add workspace(3) · ─ · [비-normal 한정:
/// Rename(1) · Delete(2) · ─] · New category(100). reserved normal 은 rename/delete
/// 만 금지 — add 는 노출한다. native 메뉴 조립은 순수 함수로 분리해 구성·순서를
/// 단위 테스트로 고정한다.
fn category_header_menu_items(is_normal: bool) -> Vec<crate::platform::native_menu::MenuItem> {
    use crate::platform::native_menu::MenuItem;
    let mut items = vec![
        MenuItem::new(3, crate::i18n::t("workspace_category.add_workspace")),
        MenuItem::separator(),
    ];
    if !is_normal {
        items.push(MenuItem::new(
            1,
            crate::i18n::t("workspace_category.rename_category"),
        ));
        items.push(MenuItem::new(
            2,
            crate::i18n::t("workspace_category.delete_category"),
        ));
        items.push(MenuItem::separator());
    }
    items.push(MenuItem::new(
        100,
        crate::i18n::t("workspace_category.new_category"),
    ));
    items
}

#[cfg(test)]
mod tests {
    use super::category_header_menu_items;

    /// 라벨은 i18n 상태에 좌우되므로 id·separator 위치로 구성·순서를 고정한다.
    fn shape(items: &[crate::platform::native_menu::MenuItem]) -> Vec<Option<u32>> {
        items
            .iter()
            .map(|i| (!i.is_separator()).then_some(i.id))
            .collect()
    }

    #[test]
    fn category_header_menu_non_normal_order() {
        // Add workspace · ─ · Rename · Delete · ─ · New category.
        let items = category_header_menu_items(false);
        assert_eq!(
            shape(&items),
            vec![Some(3), None, Some(1), Some(2), None, Some(100)]
        );
    }

    #[test]
    fn category_header_menu_normal_is_additive_only() {
        // reserved normal: Add workspace · ─ · New category (rename/delete 금지).
        let items = category_header_menu_items(true);
        assert_eq!(shape(&items), vec![Some(3), None, Some(100)]);
    }
}
