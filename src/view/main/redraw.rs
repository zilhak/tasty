use winit::event_loop::ActiveEventLoop;

use crate::plugin::PluginManager;
use crate::view::ui::View;

use super::MainView;

impl MainView {
    pub(super) fn handle_redraw(
        &mut self,
        _event_loop: &ActiveEventLoop,
        plugin_manager: Option<&PluginManager>,
        stream_hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        self.dispatch_pending_modal_opens();

        // PTY drain 은 전적으로 AppEvent::TerminalOutput 핸들러 몫이다. 과거의
        // per-frame process_all safety net 은 제거됨 — 코얼레싱 게이트의
        // early-reset(drain 전 게이트 해제)과 reader 의 EOF 최종 wake 가
        // 스킵된 wake 의 데이터까지 커버한다 (형식적 메모리모델 잔여 윈도우는
        // 실하드웨어에서 사실상 0 — ns 급 store 가시화 지연 vs µs 급 핸들러 경로).

        // D.3.C.C.8: TerminalEvent → CoreEvent 변환은 Core::process_pty_output 이
        // event_handler 의 AppEvent::TerminalOutput 처리 안에서 수행한다.
        // redraw 는 더 이상 collect_events 분기를 가지지 않는다.

        self.resync_scale_factor();

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
        self.render_if_dirty(plugin_manager, stream_hub);

        self.dispatch_pending_command_palette();

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
        self.sync_webviews(plugin_manager);

        if self.base.dirty {
            self.base.winit.request_redraw();
        }
    }

    /// settings/plugins 모달 오픈 요청 dispatch. `ui.rs`가 `state.settings_open`/
    /// `state.plugins_open`을 true로 세팅하면 여기서 소비해 `AppEvent`로 변환한다.
    fn dispatch_pending_modal_opens(&mut self) {
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
    }

    /// Re-sync scale factor before render — macOS may not fire
    /// ScaleFactorChanged reliably during monitor hot-swap or sleep/wake.
    fn resync_scale_factor(&mut self) {
        if self.base.gpu.sync_scale_factor(&self.base.winit) {
            let new_size = self.base.winit.inner_size();
            self.base.gpu.resize(new_size);
            let terminal_rect = self.compute_terminal_rect();
            let (cols, rows) = self.base.gpu.grid_size_for_rect(&terminal_rect);
            self.core_state.update_grid_size(cols, rows);
            // Schedule another redraw to verify scale factor has stabilized.
            self.base.dirty = true;
        }
    }

    /// `self.base.dirty`일 때만 실제 프레임을 그린다 — egui-mesh forward,
    /// `gpu.render`, full-textures 재전송 요청 drain(로컬 3종 + attach mesh mirror)
    /// 을 한 트랜잭션으로 묶는다.
    fn render_if_dirty(
        &mut self,
        plugin_manager: Option<&PluginManager>,
        stream_hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        if !self.base.dirty {
            return;
        }
        self.base.dirty = false;
        self.update_ime_cursor_area();
        // egui-mesh surface 에 렌더 컨텍스트(크기/ppp/입력) forward (A1-S7) — 합성
        // (gpu.render) 직전. plugin 이 PaintFrame 으로 회신하면 합성기가 그린다.
        // link_hover 등 self 불변 차용을 잡기 *전*에 호출한다(&mut self).
        if let Some(mgr) = plugin_manager {
            self.forward_egui_mesh_context(mgr);
            // GUI가 attach 서버인 경우의 mesh mirror forward — 로컬 redraw가
            // 방금 만든(또는 위 호출로 이미 있던) EguiMeshFrame 을 attach 구독자에게
            // 중계한다. 로컬 set_context 송신 이후에 불러 최신 프레임을 relay한다.
            self.forward_mesh_to_attach_subscribers(mgr, stream_hub);
        }
        // attach mesh mirror surface — 위와 동형이되 목적지가 원격이라
        // PluginManager 가 필요 없다(로컬에 plugin 프로세스가 없다).
        self.forward_attach_mesh_context();
        self.submit_gpu_frame(plugin_manager);
        self.drain_full_texture_requests();
    }

    /// 실제 GPU 프레임 제출 + surface 에러 분기 처리.
    fn submit_gpu_frame(&mut self, plugin_manager: Option<&PluginManager>) {
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
            Ok(()) => {
                // T7 (부팅 계측): 첫 present 성공 시각. Lost/Outdated 재시도
                // 프레임은 present 가 안 되므로 Ok 분기에서만 기록 (원샷).
                crate::boot::trace::mark_first_paint();
                if self.base.gpu.take_terminal_cursor_restore_pending() {
                    self.base.dirty = true;
                }
            }
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

    /// egui-mesh(로컬 plugin 대상) + attach mesh mirror(원격 대상) full 재전송
    /// 요청 drain — 렌더 prepare 가 textures_delta 체인 단절을 감지한 대상들.
    /// surface 는 forward 추적 상태에, popup/banner 는 AppState 에 옮겨 두면
    /// 다음 tick 의 forward 가 need_full_textures `set_context`/`MeshFullResendRequest`
    /// 를 보낸다. plugin/원격은 스스로 재송신하지 않으므로 다음 tick 을 dirty 로
    /// 보장한다.
    fn drain_full_texture_requests(&mut self) {
        let full_reqs = self.base.gpu.take_egui_mesh_full_requests();
        let popup_full_reqs = self.base.gpu.take_egui_mesh_popup_full_requests();
        let banner_full_reqs = self.base.gpu.take_egui_mesh_banner_full_requests();
        if !full_reqs.is_empty() || !popup_full_reqs.is_empty() || !banner_full_reqs.is_empty() {
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

        // attach mesh mirror(`docs/dev-guide/attach-behavior.md#mesh-mirror-채널` 참고)
        // full 재전송 요청 drain — 위와 동형이되 대상이
        // 로컬 plugin 이 아니라 원격이므로, `about_to_wait`(`dispatch_pending_mesh_full_resend_forwards`)
        // 가 세션을 통해 `MeshFullResendRequest` 로 forward 하도록 큐에 옮긴다.
        let attach_full_reqs = self.base.gpu.take_attach_mesh_full_requests();
        if !attach_full_reqs.is_empty() {
            self.core_state
                .pending_mesh_full_resend_forward
                .extend(attach_full_reqs);
            self.base.dirty = true;
        }
    }

    /// Command palette pending dispatch — popup writes `pending_run` when
    /// user hits Enter or clicks a row. We drain after render so the popup
    /// is already closed by the time the action fires (avoids racing with
    /// any window state the action might mutate).
    ///
    /// 호스트 명령은 이 자리에서 바로 dispatch(기존 동작 유지). Plugin 명령은
    /// `PluginManager`에 접근할 수 없는 이 스코프(`MainView`) 대신
    /// `pending_plugin_command_invokes` 큐에 enqueue해 App 메인 루프가 drain하게
    /// 한다 (`pending_tool_events`와 동형).
    fn dispatch_pending_command_palette(&mut self) {
        if let Some(cmd) = self.state.command_palette.pending_run.take() {
            match cmd {
                crate::state::command_palette::PaletteCommand::Host { id, .. } => {
                    self.dispatch_action_by_id(id);
                }
                crate::state::command_palette::PaletteCommand::Plugin {
                    plugin_id,
                    command_id,
                    ..
                } => {
                    self.state
                        .pending_plugin_command_invokes
                        .push((plugin_id, command_id));
                }
            }
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

    /// 블록 A: 전 워크스페이스 순회로 html webview surface 수집(순수 계산, native 부수효과 없음).
    /// all_html_ids = 살아있는 모든 html surface, active_html = 활성 ws·활성 tab 만 inset bounds 포함.
    fn collect_html_surfaces(
        &self,
        scale_factor: f64,
    ) -> (
        std::collections::HashMap<u32, crate::webview::WebViewBounds>,
        Vec<u32>,
    ) {
        let terminal_rect = self.compute_terminal_rect();
        let tab_bar_h = self.state.tab_bar_height.value() as f64;

        // Collect all Html surface IDs and their visibility/bounds
        let active_ws = self.state.active_workspace;
        let mut active_html: std::collections::HashMap<u32, crate::webview::WebViewBounds> =
            std::collections::HashMap::new();
        let mut all_html_ids: Vec<u32> = Vec::new();

        for (ws_idx, ws) in self.core_state.workspaces.iter().enumerate() {
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

        (active_html, all_html_ids)
    }

    /// 블록 B: webview 없는 html surface 마다 native PlatformWebView 생성 + URL 로드 + 설정 적용 +
    /// 비활성 숨김. &self 해석(find_webview_url/resolve_webview_settings) → 소유값 → &mut insert.
    fn create_missing_webviews(
        &mut self,
        all_html_ids: &[u32],
        active_html: &std::collections::HashMap<u32, crate::webview::WebViewBounds>,
        scale_factor: f64,
    ) {
        // Create new webviews for Html panels that don't have one yet
        for &sid in all_html_ids {
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
                            self.webview_loaded_urls.insert(sid, url.clone());
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
    }

    /// 이미 생성된 webview 마다 `surface.webview_url()` 최신값과 마지막으로 로드한
    /// URL(`webview_loaded_urls`)을 비교해, 달라졌으면 기존 인스턴스에 재로드를
    /// 트리거한다(파괴·재생성 없음). `load_url`/`load_html` 은 호출 즉시 native
    /// nav_state 를 `Loading` 으로 세팅하므로(각 플랫폼 구현 공통), 이 함수를
    /// `sync_webviews` 의 reveal 판정(nav_state 기준) 이전에 호출하면 재로드가
    /// 트리거된 프레임에 곧바로 반영된다 — 이전 페이지가 한 프레임이라도 다시
    /// 노출되는 일이 없다.
    fn resync_webview_urls(&mut self, all_html_ids: &[u32]) {
        for &sid in all_html_ids {
            let Some(url) = self.find_webview_url(sid) else {
                continue;
            };
            if self.webview_loaded_urls.get(&sid) == Some(&url) {
                continue;
            }
            if let Some(wv) = self.webviews.get(&sid) {
                if url.starts_with("file://")
                    || url.starts_with("http://")
                    || url.starts_with("https://")
                {
                    wv.load_url(&url);
                } else {
                    wv.load_html(&url);
                }
                self.webview_loaded_urls.insert(sid, url);
            }
        }
    }

    /// Synchronize native WebView instances with the current state.
    /// Creates webviews for new Html panels, destroys removed ones,
    /// updates bounds and visibility based on active workspace/tab.
    fn sync_webviews(&mut self, plugin_manager: Option<&PluginManager>) {
        let scale_factor = self.base.gpu.scale_factor() as f64;
        let (active_html, all_html_ids) = self.collect_html_surfaces(scale_factor);
        self.create_missing_webviews(&all_html_ids, &active_html, scale_factor);
        self.resync_webview_urls(&all_html_ids);

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
        self.webview_loaded_urls
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

        // navigation 시도 통지(host→plugin `webview.navigation_attempt`) — native backend
        // decide-policy/NavigationStarting 콜백이 캡처해 쌓아둔 URL 큐를 매 프레임 drain 해
        // 소유 plugin 에 forward. "원격 http(s) 차단" 판정(위 native 레벨에서 독립 처리)과
        // 무관하게 항상 통지한다. `plugin_manager`가 없어도(headless 등) 큐는 그대로
        // drain 해 무한정 쌓이지 않게 한다.
        for (sid, wv) in &self.webviews {
            for url in wv.take_pending_navigations() {
                if let Some(mgr) = plugin_manager {
                    crate::adapters::ipc::handler::webview::notify_navigation_attempt(
                        mgr,
                        &self.core_state,
                        *sid,
                        &url,
                    );
                }
            }
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
        use crate::state::PendingNativeMenu;

        let pending = match self.state.dialogs.pending_native_menu.take() {
            Some(p) => p,
            None => return,
        };

        // debug: egui 프레임이 세우는 메뉴(explorer 우클릭 등)를 블로킹 native 팝업
        // 없이 관찰하기 위한 격리 훅. winit 경로(mesh inject)는 핸들러가 즉시 메뉴를
        // 세워 `debug_captured_menu` 로 포획되지만, egui 경로는 이 redraw 프레임에서
        // 메뉴를 세우므로 여기서 가로채야 headless 회귀 테스트가 가능하다. release 미노출.
        #[cfg(debug_assertions)]
        if std::env::var_os("TASTY_DEBUG_SUPPRESS_NATIVE_MENU").is_some() {
            self.debug_captured_menu = Some(pending);
            return;
        }

        match pending {
            PendingNativeMenu::Tab {
                pane_id,
                tab_index,
                x,
                y,
            } => self.handle_tab_native_menu(pane_id, tab_index, x, y),
            PendingNativeMenu::Pane { pane_id, x, y } => {
                self.handle_pane_native_menu(pane_id, x, y)
            }
            PendingNativeMenu::Workspace { ws_idx, x, y } => {
                self.handle_workspace_native_menu(ws_idx, x, y)
            }
            PendingNativeMenu::WorkspaceCategoryHeader { cat_id, x, y } => {
                self.handle_workspace_category_header_native_menu(cat_id, x, y)
            }
            PendingNativeMenu::SidebarBackground { x, y } => {
                self.handle_sidebar_background_native_menu(x, y)
            }
            PendingNativeMenu::TerminalSurface { surface_id, x, y } => {
                self.handle_terminal_surface_native_menu(surface_id, x, y)
            }
            PendingNativeMenu::Surface { surface_id, x, y } => {
                self.handle_surface_native_menu(surface_id, x, y)
            }
            PendingNativeMenu::Explorer {
                surface_id,
                paths,
                cwd,
                single_is_dir,
                x,
                y,
            } => self.handle_explorer_native_menu(surface_id, paths, cwd, single_is_dir, x, y),
            PendingNativeMenu::ExplorerFavorite {
                surface_id,
                path,
                x,
                y,
            } => self.handle_explorer_favorite_native_menu(surface_id, path, x, y),
            PendingNativeMenu::NewWorkspaceButton { x, y } => {
                self.handle_new_workspace_button_native_menu(x, y)
            }
            PendingNativeMenu::NewTabButton { pane_id, x, y } => {
                self.handle_new_tab_button_native_menu(pane_id, x, y)
            }
        }
    }

    fn handle_tab_native_menu(&mut self, pane_id: u32, tab_index: usize, x: f32, y: f32) {
        use crate::platform::native_menu::show_context_menu;
        let items = self.build_tab_context_menu_items(pane_id, tab_index);
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
        match result {
            Some(1) => self.rename_tab(pane_id, tab_index),
            Some(2) => {
                // Close tab (모든 surface 포함). 이전엔 첫 surface 만 닫아 split
                // 상태에서 surface 하나만 사라지던 버그가 있었음.
                if self
                    .state
                    .close_tab(&mut self.core_state, pane_id, tab_index)
                    && self.core_state.workspaces.is_empty()
                {
                    self.request_close();
                }
            }
            Some(3) => {
                // Move Left — mirror 워크스페이스는 로컬 탭 순서 변경 대신 MoveTab 을
                // 원격으로 forward 한다(로컬 실행은 원격 트리와 어긋남).
                if tab_index > 0 {
                    self.move_tab_via_mirror_or_local(pane_id, tab_index, tab_index - 1);
                }
            }
            Some(4) => {
                // Move Right — mirror 워크스페이스는 로컬 탭 순서 변경 대신 MoveTab 을
                // 원격으로 forward 한다(로컬 실행은 원격 트리와 어긋남).
                self.move_tab_via_mirror_or_local(pane_id, tab_index, tab_index + 1);
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

    /// tab 우클릭 컨텍스트 메뉴 항목 8개 구성. move left/right 는 인접 위치
    /// 존재 여부로 활성/비활성을 미리 계산한다.
    fn build_tab_context_menu_items(
        &mut self,
        pane_id: u32,
        tab_index: usize,
    ) -> [crate::platform::native_menu::MenuItem; 8] {
        use crate::platform::native_menu::MenuItem;
        let engine = &mut self.core_state;
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

        [
            MenuItem::new(1, crate::i18n::t("tab_context_menu.rename")),
            MenuItem::new(2, crate::i18n::t("tab_context_menu.close")),
            MenuItem::separator(),
            move_left,
            move_right,
            MenuItem::separator(),
            MenuItem::new(5, crate::i18n::t("preset.context.save_as_tab_preset")),
            MenuItem::new(6, crate::i18n::t("preset.context.save_as_pane_preset")),
        ]
    }

    /// tab 을 `from_index` → `to_index` 로 이동. mirror 워크스페이스는 로컬 탭
    /// 순서를 직접 바꾸지 않고 `MoveTab` 을 원격으로 forward 한다(로컬 실행은
    /// 원격 트리와 어긋남) — forward 가 안 먹힌(비-mirror) 워크스페이스에서만
    /// 로컬 `pane.move_tab` 을 수행한다. Move Left/Right 양쪽에서 동일 로직이라
    /// 공용화(과거엔 두 곳에 중복).
    fn move_tab_via_mirror_or_local(&mut self, pane_id: u32, from_index: usize, to_index: usize) {
        let mirror_op = self
            .core_state
            .find_pane_by_id(pane_id)
            .and_then(|p| p.tabs.get(p.active_tab))
            .and_then(|t| t.focused_surface_id())
            .map(|sid| crate::ipc::stream::StructuralOp::MoveTab {
                anchor_surface_id: sid,
                from_index,
                to_index,
            });
        if !self
            .state
            .forward_mirror_structural(&mut self.core_state, mirror_op, Vec::new())
            && let Some(pane) = self
                .state
                .active_workspace_mut(&mut self.core_state)
                .pane_layout_mut()
                .find_pane_mut(pane_id)
        {
            pane.move_tab(from_index, to_index);
        }
    }

    /// tab rename 팝업을 연다 — 현재 표시명을 prefill 하고 `RenameTarget::TabName`
    /// scope 로 `rename` 팝업을 dispatch.
    fn rename_tab(&mut self, pane_id: u32, tab_index: usize) {
        let engine = &mut self.core_state;
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

    fn handle_pane_native_menu(&mut self, pane_id: u32, x: f32, y: f32) {
        let engine = &mut self.core_state;
        use crate::platform::native_menu::{MenuItem, show_context_menu};
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
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
        match result {
            Some(1) => {
                self.state.active_workspace_mut(engine).focused_pane = pane_id;
                if let Err(e) = self.state.add_tab(engine) {
                    tracing::warn!("add_tab from context menu failed: {e}");
                }
            }
            Some(2) => {
                // 빈 탭을 먼저 만들고, 그 surface 를 제자리 markdown 변환. surface_id 를
                // 실어 file-open 팝업을 연다(plugin 이 markdown.navigate 로 제자리 변환).
                self.state.active_workspace_mut(engine).focused_pane = pane_id;
                if let Some((_tab_id, surface_id)) = self.state.add_empty_tab(engine) {
                    // intent-exempt: surface_id 결과 의존 (후속 convert)
                    self.state
                        .enqueue_convert_input_popup(engine, "markdown", Some(surface_id));
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

    fn handle_workspace_native_menu(&mut self, ws_idx: usize, x: f32, y: f32) {
        use crate::platform::native_menu::show_context_menu;
        let (items, move_targets) = self.build_workspace_context_menu_items(ws_idx);
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
        let engine = &mut self.core_state;
        if ws_idx < engine.workspaces.len() {
            match result {
                Some(1) => {
                    let name = engine.workspaces[ws_idx].name.clone();
                    self.open_rename_workspace_dialog(
                        crate::state::RenameTarget::WorkspaceName { ws_idx },
                        name,
                    );
                }
                Some(2) => {
                    let subtitle = engine.workspaces[ws_idx].subtitle.clone();
                    self.open_rename_workspace_dialog(
                        crate::state::RenameTarget::WorkspaceSubtitle { ws_idx },
                        subtitle,
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
                    if self.state.close_workspace_at(engine, ws_idx) && engine.workspaces.is_empty()
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
                    self.move_workspace_to_category(ws_idx, &move_targets, id);
                }
                _ => {}
            }
        }
        self.mark_dirty();
    }

    /// workspace 우클릭 컨텍스트 메뉴 항목 구성 + "카테고리로 이동" 대상 목록.
    /// 반환된 `Vec<WorkspaceCategoryId>` 는 인덱스 i 가 메뉴 항목 id `200+i` 에
    /// 대응한다(카테고리 토글이 꺼져 있으면 빈 벡터).
    fn build_workspace_context_menu_items(
        &mut self,
        ws_idx: usize,
    ) -> (
        Vec<crate::platform::native_menu::MenuItem>,
        Vec<crate::model::WorkspaceCategoryId>,
    ) {
        use crate::platform::native_menu::MenuItem;
        let engine = &mut self.core_state;
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
        ];

        // 카테고리 토글 on — "카테고리로 이동"(현재 소속 제외 평면 나열, 선택지 B)
        // + "새 카테고리". move_targets[i] = (cat_id) 로 결과 id(200+i) 매핑.
        let mut move_targets: Vec<crate::model::WorkspaceCategoryId> = Vec::new();
        if engine.settings.general.workspace_categories_enabled && ws_idx < engine.workspaces.len()
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

        // 카테고리 토글 상태와 무관하게 "닫기"는 항상 최하단.
        items.push(MenuItem::separator());
        items.push(MenuItem::new(
            6,
            crate::i18n::t("context_menu.close_workspace"),
        ));

        (items, move_targets)
    }

    /// workspace 이름/부제 rename 팝업을 연다 — 현재 값을 prefill 하고 `target`
    /// scope 로 `rename` 팝업을 dispatch(제목/부제 공용 — 값과 target 만 다르다).
    fn open_rename_workspace_dialog(
        &mut self,
        target: crate::state::RenameTarget,
        current_value: String,
    ) {
        let scope = target.popup_scope();
        self.state.dialogs.rename = Some((target, current_value));
        self.state.dispatch_intent(
            crate::intent::UiIntent::OpenPopup {
                id: "rename",
                mode: crate::intent::OpenPopupMode::WithScope(scope),
            }
            .from_user_context_menu(),
        );
    }

    /// workspace 를 `move_targets[id-200]` 카테고리로 이동. `id` 는 컨텍스트
    /// 메뉴가 회신한 원본 값(200 이상 — `build_workspace_context_menu_items`
    /// 참고) 그대로 받는다.
    fn move_workspace_to_category(
        &mut self,
        ws_idx: usize,
        move_targets: &[crate::model::WorkspaceCategoryId],
        id: u32,
    ) {
        let engine = &mut self.core_state;
        if let Some(&cat_id) = move_targets.get((id - 200) as usize) {
            let ws_id = engine.workspaces[ws_idx].id;
            if let Err(e) = engine.set_workspace_category(ws_id, cat_id) {
                tracing::warn!("set_workspace_category failed: {e:?}");
            }
            engine.mark_layout_dirty();
        }
    }

    fn handle_workspace_category_header_native_menu(
        &mut self,
        cat_id: crate::model::WorkspaceCategoryId,
        x: f32,
        y: f32,
    ) {
        let engine = &mut self.core_state;
        use crate::platform::native_menu::show_context_menu;
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
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
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
            Some(4) => {
                // 원격 워크스페이스 추가 팝업 — 카테고리 헤더 우클릭 진입(원칙 1).
                self.state.dispatch_intent(
                    crate::intent::UiIntent::OpenPopup {
                        id: crate::adapters::ui::popup::remote_attach::REMOTE_ATTACH_POPUP_ID,
                        mode: crate::intent::OpenPopupMode::CenteredFocused,
                    }
                    .from_user_context_menu(),
                );
            }
            Some(5) => {
                // 이 카테고리 소속으로 프리셋 적용 — "+" 버튼 메뉴와 동일 팝업
                // (APPLY_WORKSPACE_POPUP_ID) 재사용, 대상 카테고리만 임시 상태로 기억.
                self.state.dialogs.preset_apply_target_category = Some(cat_id);
                self.state.dialogs.preset_picker_selected = None;
                self.state.dispatch_intent(
                    crate::intent::UiIntent::OpenPopup {
                        id: crate::adapters::ui::popup::preset_apply::APPLY_WORKSPACE_POPUP_ID,
                        mode: crate::intent::OpenPopupMode::CenteredFocused,
                    }
                    .from_user_context_menu(),
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
                crate::adapters::ui::category_actions::open_new_category_dialog(&mut self.state);
            }
            _ => {}
        }
        self.mark_dirty();
    }

    fn handle_sidebar_background_native_menu(&mut self, x: f32, y: f32) {
        use crate::platform::native_menu::{MenuItem, show_context_menu};
        // 빈 배경 우클릭 — 새 카테고리 · 원격 워크스페이스 추가. 그룹모드 배경(카테고리
        // ON)·flat모드 배경(카테고리 OFF, `docs/features/workspace-category/index.md`
        // 참고) 양쪽에서 이 핸들러로 라우팅되므로
        // 카테고리 상태 분기 없이 공통 메뉴로 노출한다.
        let items = [
            MenuItem::new(100, crate::i18n::t("workspace_category.new_category")),
            MenuItem::separator(),
            MenuItem::new(2, crate::i18n::t("context_menu.add_remote_workspace")),
        ];
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
        match result {
            Some(100) => {
                crate::adapters::ui::category_actions::open_new_category_dialog(&mut self.state);
            }
            Some(2) => {
                self.state.dispatch_intent(
                    crate::intent::UiIntent::OpenPopup {
                        id: crate::adapters::ui::popup::remote_attach::REMOTE_ATTACH_POPUP_ID,
                        mode: crate::intent::OpenPopupMode::CenteredFocused,
                    }
                    .from_user_context_menu(),
                );
            }
            _ => {}
        }
        self.mark_dirty();
    }

    fn handle_terminal_surface_native_menu(&mut self, surface_id: u32, x: f32, y: f32) {
        let engine = &mut self.core_state;
        use crate::platform::native_menu::{MenuItem, show_context_menu};
        // Show copy items only when there is an active (non-empty) selection.
        let has_selection = self.text_selection.as_ref().is_some_and(|s| !s.is_empty());
        // "경로 열기" 항목은 has_selection 과 별도로, 우클릭한 surface 와 selection 이
        // 속한 surface 가 같을 때만 노출한다 — surface 별로 독립적인 드래그 상태를
        // 가질 수 있어 다른 surface 의 선택을 그대로 노출하면 혼동을 유발한다(복사
        // 항목은 기존부터 surface 무관하게 전역 selection 을 대상으로 동작하는 별개
        // 관례라 그대로 둔다).
        let selection_open_path = if has_selection {
            self.text_selection
                .as_ref()
                .filter(|s| s.surface_id == surface_id)
                .and_then(|s| Self::resolve_selection_open_path(engine, s))
        } else {
            None
        };
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
            if let Some(target) = &selection_open_path {
                items.push(MenuItem::new(
                    20,
                    if target.is_dir() {
                        crate::i18n::t("terminal_context_menu.open_folder")
                    } else {
                        crate::i18n::t("terminal_context_menu.open_file")
                    },
                ));
            }
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
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
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
            Some(20) => {
                if let Some(target) = &selection_open_path
                    && let Err(e) = crate::platform::reveal::open_path(target)
                {
                    tracing::warn!("terminal: open selected path failed: {e}");
                }
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

    /// selection 이 가리키는 실재 파일/폴더 경로를 찾는다(우클릭 대상과 selection 의
    /// surface 일치는 호출부가 미리 검사한다). mirror(원격 attach) surface 는 로컬
    /// 파일 관리자로 원격 경로를 여는 배선이 아직 없어 이번 범위에서 제외한다 — 그
    /// 배선이 생기면 이 조건을 재검토한다.
    fn resolve_selection_open_path(
        engine: &crate::core::CoreState,
        sel: &crate::selection::TextSelection,
    ) -> Option<std::path::PathBuf> {
        let terminal = engine.visible_terminal(sel.surface_id)?;
        terminal.process_id()?;
        let raw_text = crate::selection::extract_selected_text(terminal, sel);
        let cwd = terminal.get_cwd();
        crate::adapters::ui::terminal_link::longest_existing_selection_path(
            &raw_text,
            cwd.as_deref(),
            false,
        )
    }

    fn handle_surface_native_menu(&mut self, surface_id: u32, x: f32, y: f32) {
        let engine = &mut self.core_state;
        use crate::platform::native_menu::{MenuItem, show_context_menu};
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
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
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

    fn handle_explorer_native_menu(
        &mut self,
        surface_id: u32,
        paths: Vec<std::path::PathBuf>,
        cwd: std::path::PathBuf,
        single_is_dir: bool,
        x: f32,
        y: f32,
    ) {
        use crate::platform::native_menu::show_context_menu;
        let multi = paths.len() > 1;
        let is_empty_target = paths.is_empty();
        let is_folder = paths.len() == 1 && single_is_dir;
        let has_clip = self
            .core_state
            .explorer_clipboard
            .as_ref()
            .map(|c| !c.paths.is_empty())
            .unwrap_or(false);

        let items = Self::build_explorer_context_menu(multi, is_empty_target, is_folder, has_clip);
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
        match result {
            Some(1) => self.explorer_menu_copy_path(surface_id, &paths, &cwd, is_empty_target),
            Some(10) => self.explorer_menu_set_clipboard(&paths, false),
            Some(11) => self.explorer_menu_set_clipboard(&paths, true),
            Some(12) => self.explorer_menu_paste(surface_id, &paths, &cwd, is_folder),
            Some(30) => self.explorer_menu_trash(surface_id, &paths),
            Some(20) => self.explorer_menu_open_in_system(&paths, &cwd),
            Some(40) => self.explorer_menu_rename(surface_id, &paths),
            Some(50) => self.explorer_menu_add_favorite(&paths, &cwd, is_empty_target),
            Some(60) => self.explorer_menu_open_in_new_tab(surface_id, &paths),
            Some(61) => self.explorer_menu_set_root(surface_id, &paths),
            _ => {}
        }
        self.mark_dirty();
    }

    /// explorer 컨텍스트 메뉴 아이템 목록 구성 (design §3.3).
    fn build_explorer_context_menu(
        multi: bool,
        is_empty_target: bool,
        is_folder: bool,
        has_clip: bool,
    ) -> Vec<crate::platform::native_menu::MenuItem> {
        use crate::platform::native_menu::MenuItem;
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
        items
    }

    /// 경로 복사 (아이템 1).
    fn explorer_menu_copy_path(
        &mut self,
        surface_id: u32,
        paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
        is_empty_target: bool,
    ) {
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
            crate::i18n::t("toast.copied_path"),
            crate::adapters::ui::ToastScope::Surface(surface_id),
        );
    }

    /// 복사(cut=false, 아이템 10) / 잘라내기(cut=true, 아이템 11) 클립보드 설정.
    /// 컨텍스트 메뉴와 키보드 단축키(`handle_explorer_shortcut`) 양쪽에서 공유한다.
    pub(crate) fn explorer_menu_set_clipboard(&mut self, paths: &[std::path::PathBuf], cut: bool) {
        self.core_state.explorer_clipboard = Some(crate::core::state::ExplorerClipboard {
            paths: paths.to_vec(),
            cut,
        });
    }

    /// 붙여넣기 (아이템 12). 컨텍스트 메뉴와 키보드 단축키 양쪽에서 공유한다.
    pub(crate) fn explorer_menu_paste(
        &mut self,
        surface_id: u32,
        paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
        is_folder: bool,
    ) {
        let engine = &mut self.core_state;
        let dest = if is_folder {
            paths.first().cloned().unwrap_or_else(|| cwd.to_path_buf())
        } else {
            cwd.to_path_buf()
        };
        if let Some(clip) = engine.explorer_clipboard.clone() {
            let (ok, err) = crate::explorer_ui::ops::paste_all(&clip.paths, &dest, clip.cut);
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

    /// 휴지통으로 이동 (아이템 30, 가역적이라 별도 확인 모달 없음).
    fn explorer_menu_trash(&mut self, surface_id: u32, paths: &[std::path::PathBuf]) {
        if let Err(e) = trash::delete_all(paths) {
            tracing::warn!("explorer: move to trash failed: {e}");
        }
        if let Some(v) = self.state.explorer_views.get_mut(surface_id) {
            v.selected.clear();
            v.anchor = None;
            v.request_reload();
        }
    }

    /// 시스템에서 열기 (아이템 20).
    fn explorer_menu_open_in_system(
        &mut self,
        paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
    ) {
        let target = paths.first().cloned().unwrap_or_else(|| cwd.to_path_buf());
        if let Err(e) = crate::platform::reveal::open_path(&target) {
            tracing::warn!("explorer: open_path failed: {e}");
        }
    }

    /// 이름 변경 (아이템 40).
    fn explorer_menu_rename(&mut self, surface_id: u32, paths: &[std::path::PathBuf]) {
        if let Some(path) = paths.first().cloned() {
            let current_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let target = crate::state::RenameTarget::ExplorerEntry { surface_id, path };
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

    /// 즐겨찾기 추가 (아이템 50) — 대상: 단일 폴더면 그 폴더, 빈 영역이면 cwd.
    fn explorer_menu_add_favorite(
        &mut self,
        paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
        is_empty_target: bool,
    ) {
        let path = if is_empty_target {
            cwd.to_path_buf()
        } else {
            paths.first().cloned().unwrap_or_else(|| cwd.to_path_buf())
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

    /// 새 탭으로 열기 (아이템 60) — 대상 폴더를 cwd 로 하는 새 explorer 를
    /// 우클릭 대상 surface 의 소유 pane 에 Pane 탭으로 연다(기존 surface 불변).
    fn explorer_menu_open_in_new_tab(&mut self, surface_id: u32, paths: &[std::path::PathBuf]) {
        if let Some(folder) = paths.first().cloned() {
            let params = serde_json::json!({ "path": folder.to_string_lossy() });
            let engine = &mut self.core_state;
            if let Err(e) = self
                .state
                .add_kind_tab_by_owner(engine, surface_id, "explorer", &params)
            {
                tracing::warn!("explorer: open in new tab failed: {e}");
            }
        }
    }

    /// 이 폴더로 루트 설정 (아이템 61) — 현재 explorer 의 cwd 를 그 폴더로 이동.
    fn explorer_menu_set_root(&mut self, surface_id: u32, paths: &[std::path::PathBuf]) {
        if let Some(folder) = paths.first().cloned() {
            let engine = &mut self.core_state;
            self.state.set_explorer_cwd(engine, surface_id, folder);
        }
    }

    fn handle_explorer_favorite_native_menu(
        &mut self,
        surface_id: u32,
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    ) {
        let engine = &mut self.core_state;
        use crate::platform::native_menu::{MenuItem, show_context_menu};
        let items = [
            MenuItem::new(60, crate::i18n::t("explorer.context_menu.open_in_new_tab")),
            MenuItem::new(61, crate::i18n::t("explorer.context_menu.set_as_root")),
            MenuItem::separator(),
            MenuItem::new(
                1,
                crate::i18n::t("explorer.context_menu.remove_from_favorites"),
            ),
        ];
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
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

    fn handle_new_workspace_button_native_menu(&mut self, x: f32, y: f32) {
        use crate::platform::native_menu::{MenuItem, show_context_menu};
        let items = [
            MenuItem::new(1, crate::i18n::t("preset.context.apply_workspace_preset")),
            MenuItem::separator(),
            MenuItem::new(2, crate::i18n::t("context_menu.add_remote_workspace")),
        ];
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
        match result {
            Some(1) => {
                self.state.dialogs.preset_picker_selected = None;
                self.state.dispatch_intent(
                    crate::intent::UiIntent::OpenPopup {
                        id: crate::adapters::ui::popup::preset_apply::APPLY_WORKSPACE_POPUP_ID,
                        mode: crate::intent::OpenPopupMode::CenteredFocused,
                    }
                    .from_user_context_menu(),
                );
            }
            Some(2) => {
                self.state.dispatch_intent(
                    crate::intent::UiIntent::OpenPopup {
                        id: crate::adapters::ui::popup::remote_attach::REMOTE_ATTACH_POPUP_ID,
                        mode: crate::intent::OpenPopupMode::CenteredFocused,
                    }
                    .from_user_context_menu(),
                );
            }
            _ => {}
        }
        self.mark_dirty();
    }

    fn handle_new_tab_button_native_menu(&mut self, pane_id: u32, x: f32, y: f32) {
        let engine = &mut self.core_state;
        use crate::platform::native_menu::{MenuItem, show_context_menu};
        let items = [
            MenuItem::new(1, crate::i18n::t("preset.context.apply_tab_preset")),
            MenuItem::new(2, crate::i18n::t("preset.context.apply_pane_preset")),
        ];
        let result = show_context_menu(self.base.winit.as_ref(), x as f64, y as f64, &items);
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

/// 카테고리 헤더 우클릭 메뉴 항목 조립 (디자인 sidebar_context_menu.jsx category 분기
/// 전사, 2026-07-02). additive 선두: Add workspace(3) · Create from preset(5,
/// `docs/features/workspace-category/index.md` 참고)
/// · ─ · Add remote workspace(4) · ─ · [비-normal 한정: Rename(1) · Delete(2)
/// · ─] · New category(100). reserved normal 은 rename/delete 만 금지 — add(로컬/
/// 프리셋/원격) 는 노출한다. native 메뉴 조립은 순수 함수로 분리해 구성·순서를 단위
/// 테스트로 고정한다.
fn category_header_menu_items(is_normal: bool) -> Vec<crate::platform::native_menu::MenuItem> {
    use crate::platform::native_menu::MenuItem;
    let mut items = vec![
        MenuItem::new(3, crate::i18n::t("workspace_category.add_workspace")),
        // "+" 버튼 메뉴와 동일 라벨/액션(프리셋 선택 → 워크스페이스 생성) 재사용 —
        // 신규 i18n 키 불필요.
        MenuItem::new(5, crate::i18n::t("preset.context.apply_workspace_preset")),
        MenuItem::separator(),
        MenuItem::new(4, crate::i18n::t("context_menu.add_remote_workspace")),
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
        // Add workspace · Create from preset · ─ · Add remote workspace · ─ ·
        // Rename · Delete · ─ · New category.
        let items = category_header_menu_items(false);
        assert_eq!(
            shape(&items),
            vec![
                Some(3),
                Some(5),
                None,
                Some(4),
                None,
                Some(1),
                Some(2),
                None,
                Some(100)
            ]
        );
    }

    #[test]
    fn category_header_menu_normal_is_additive_only() {
        // reserved normal: Add workspace · Create from preset · ─ · Add remote
        // workspace · ─ · New category (rename/delete 금지).
        let items = category_header_menu_items(true);
        assert_eq!(
            shape(&items),
            vec![Some(3), Some(5), None, Some(4), None, Some(100)]
        );
    }

    // build_explorer_context_menu(multi, is_empty_target, is_folder, has_clip) 의
    // 위치별 메뉴 구성을 id·separator 위치로 고정한다. id: 1=copy_path, 10=copy_files,
    // 11=cut, 12=paste/paste_into, 20=open_in_system, 30=delete, 40=rename,
    // 50=add_to_favorites, 60=open_in_new_tab, 61=set_as_root. None=separator.
    fn explorer_menu_shape(
        multi: bool,
        is_empty_target: bool,
        is_folder: bool,
        has_clip: bool,
    ) -> Vec<Option<u32>> {
        shape(&super::MainView::build_explorer_context_menu(
            multi,
            is_empty_target,
            is_folder,
            has_clip,
        ))
    }

    #[test]
    fn explorer_menu_empty_no_clip() {
        // 빈 영역, 클립보드 없음: 경로복사 · 즐겨찾기추가.
        assert_eq!(
            explorer_menu_shape(false, true, false, false),
            vec![Some(1), Some(50)]
        );
    }

    #[test]
    fn explorer_menu_empty_with_clip() {
        // 빈 영역, 클립보드 있음: + ─ · 붙여넣기.
        assert_eq!(
            explorer_menu_shape(false, true, false, true),
            vec![Some(1), Some(50), None, Some(12)]
        );
    }

    #[test]
    fn explorer_menu_single_file() {
        // 단일 파일(클립보드 무관): 경로복사 · 복사 · 잘라내기 · 이름변경 · ─ · 휴지통.
        assert_eq!(
            explorer_menu_shape(false, false, false, false),
            vec![Some(1), Some(10), Some(11), Some(40), None, Some(30)]
        );
        // has_clip 은 단일 파일 shape 에 영향 없음.
        assert_eq!(
            explorer_menu_shape(false, false, false, true),
            vec![Some(1), Some(10), Some(11), Some(40), None, Some(30)]
        );
    }

    #[test]
    fn explorer_menu_single_folder_no_clip() {
        // 단일 폴더, 클립보드 없음: 경로복사 · 즐겨찾기 · 새탭 · 루트설정 · 복사 ·
        // 잘라내기 · 이름변경 · ─ · 휴지통 · 시스템열기.
        assert_eq!(
            explorer_menu_shape(false, false, true, false),
            vec![
                Some(1),
                Some(50),
                Some(60),
                Some(61),
                Some(10),
                Some(11),
                Some(40),
                None,
                Some(30),
                Some(20)
            ]
        );
    }

    #[test]
    fn explorer_menu_single_folder_with_clip() {
        // 단일 폴더, 클립보드 있음: 위 + 붙여넣기(12, cut/paste 그룹 안).
        assert_eq!(
            explorer_menu_shape(false, false, true, true),
            vec![
                Some(1),
                Some(50),
                Some(60),
                Some(61),
                Some(10),
                Some(11),
                Some(12),
                Some(40),
                None,
                Some(30),
                Some(20)
            ]
        );
    }

    #[test]
    fn explorer_menu_multi() {
        // 다중 선택(클립보드 무관): 경로복사 · 복사 · 잘라내기 · ─ · 휴지통.
        assert_eq!(
            explorer_menu_shape(true, false, false, false),
            vec![Some(1), Some(10), Some(11), None, Some(30)]
        );
        assert_eq!(
            explorer_menu_shape(true, false, false, true),
            vec![Some(1), Some(10), Some(11), None, Some(30)]
        );
    }
}
