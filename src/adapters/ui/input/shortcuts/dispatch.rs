//! 단축키 디스패치 — `handle_shortcut` 의 키 입력 → 액션 분기 + `dispatch_action_by_id`
//! 의 액션 ID 직접 호출 (Command Palette / 자동화 진입점).

use winit::keyboard::{Key, ModifiersState};

use crate::intent::{Intent, OpenPopupMode, UiIntent};
use crate::view::main::MainView;

use super::matches_any_binding;
use super::{focused_explorer_surface_id, focused_image_surface_id, send_app_event};

impl MainView {
    /// Dispatch a keybinding action by its stable `field_id` (예: `"new_workspace"`).
    /// 단축키와 정확히 같은 효과를 내며, Command Palette / 외부 자동화에서 호출한다.
    ///
    /// Returns true if the action was recognized and dispatched. Unknown action_id는 false.
    pub(crate) fn dispatch_action_by_id(&mut self, action_id: &str) -> bool {
        use crate::adapters::ui::popup::PopupScope;
        use crate::model::SplitDirection;

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();
        let proxy = self.proxy.clone();
        let proxy = &proxy;
        // copy_path 등 clipboard 쓰기는 self.clipboard 차용이 필요해 match(=state/engine
        // 차용) 종료 후 처리하도록 텍스트만 모아둔다.
        let mut pending_copy_text: Option<String> = None;
        let state = &mut self.state;
        let engine = &mut self.core_state;

        match action_id {
            "new_workspace" => {
                state.dispatch_intent(
                    Intent::NewWorkspace {
                        kind: None,
                        params: serde_json::Value::Null,
                        category: None,
                    }
                    .from_user_shortcut("new_workspace"),
                );
                crate::core::Core::resize_all_terminals(
                    state,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                );
            }
            "new_tab" => {
                if let Err(e) = state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
                crate::core::Core::resize_all_terminals(
                    state,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                );
            }
            "split_pane_vertical" => {
                state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_pane_vertical"),
                );
                crate::core::Core::resize_all_terminals(
                    state,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                );
            }
            "split_pane_horizontal" => {
                state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Horizontal,
                    }
                    .from_user_shortcut("split_pane_horizontal"),
                );
                crate::core::Core::resize_all_terminals(
                    state,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                );
            }
            "split_surface_vertical" => {
                state.dispatch_intent(
                    Intent::SplitSurface {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_surface_vertical"),
                );
                crate::core::Core::resize_all_terminals(
                    state,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                );
            }
            "split_surface_horizontal" => {
                state.dispatch_intent(
                    Intent::SplitSurface {
                        direction: SplitDirection::Horizontal,
                    }
                    .from_user_shortcut("split_surface_horizontal"),
                );
                crate::core::Core::resize_all_terminals(
                    state,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                );
            }
            "toggle_settings" => {
                send_app_event(proxy, crate::AppEvent::OpenSettings);
            }
            "toggle_notifications" => {
                let will_open = !state.popups.is_open("notifications");
                state.dispatch_intent(
                    UiIntent::TogglePopup {
                        id: "notifications",
                        mode: OpenPopupMode::Default,
                    }
                    .from_user_shortcut("toggle_notifications"),
                );
                if will_open {
                    state.dispatch_intent(
                        crate::core::intent::DomainIntent::MarkAllNotificationsRead
                            .from_user_shortcut("toggle_notifications"),
                    );
                }
            }
            "toggle_clipboard_viewer" => {
                state.enqueue_host_event(crate::state::PendingHostEvent::Raw {
                    key: "shortcut.toggle_clipboard_viewer".into(),
                    payload: serde_json::Value::Null,
                });
            }
            "toggle_sidebar" => {
                state.sidebar_visible = !state.sidebar_visible;
            }
            "toggle_sidebar_collapse" => {
                state.sidebar_collapsed = !state.sidebar_collapsed;
            }
            "close_workspace" => {
                state.close_active_workspace(engine);
                if engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    crate::core::Core::resize_all_terminals(
                        &self.state,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                    );
                }
                return true;
            }
            "close_pane" => {
                if !state.close_active_pane(engine) {
                    state.close_active_workspace(engine);
                }
                if engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    crate::core::Core::resize_all_terminals(
                        &self.state,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                    );
                }
                return true;
            }
            "close_surface" => {
                let closed = state.close_active_surface(engine);
                if !closed && !state.close_active_pane(engine) {
                    state.close_active_workspace(engine);
                }
                if engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    crate::core::Core::resize_all_terminals(
                        &self.state,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                    );
                }
                return true;
            }
            "close_active" => {
                if !state.close_active_tab(engine) && !state.close_active_pane(engine) {
                    state.close_active_workspace(engine);
                }
                if engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    crate::core::Core::resize_all_terminals(
                        &self.state,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                    );
                }
                return true;
            }
            "focus_pane_next" => state.move_pane_focus_forward(engine),
            "focus_pane_prev" => state.move_pane_focus_backward(engine),
            "focus_surface_next" => state.move_surface_focus_forward(engine),
            "focus_surface_prev" => state.move_surface_focus_backward(engine),
            "next_tab" => state.next_tab_in_pane(engine),
            "prev_tab" => state.prev_tab_in_pane(engine),
            "restore_closed" => {
                state.dispatch_intent(
                    crate::intent::Intent::RestoreClosedItem.from_user_shortcut("restore_closed"),
                );
                crate::core::Core::resize_all_terminals(
                    state,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                );
            }
            "quit" => send_app_event(proxy, crate::AppEvent::QuitRequested),
            "quit_immediate" => send_app_event(proxy, crate::AppEvent::Shutdown),
            "quit_minimize" => send_app_event(proxy, crate::AppEvent::Minimize),
            "new_window" => send_app_event(proxy, crate::AppEvent::CreateWindow),
            "find" => {
                // winit 경로는 검색창 비포커스(터미널 포커스) 상태에서만 도달한다.
                // 검색창 포커스 상태의 find 는 egui 경로(search_bar)가 처리한다.
                // 여기서는 항상 "검색창으로 포커스 이동".
                if state.popups.is_open("search_bar") {
                    state.popups.set_focused("search_bar", true);
                } else if let Some(sid) = state.focused_surface_id(engine) {
                    state.search.surface_id = sid;
                    state.dispatch_intent(
                        UiIntent::OpenPopup {
                            id: "search_bar",
                            mode: OpenPopupMode::AtTopOfScope(PopupScope::Surface(sid)),
                        }
                        .from_user_shortcut("find"),
                    );
                }
            }
            "open_markdown" => {
                let pane_id = state.active_workspace(engine).focused_pane;
                state.dialogs.file_open_pane_id = Some(pane_id);
                state.dialogs.markdown_open_buffer.clear();
                state.dispatch_intent(
                    UiIntent::OpenPopup {
                        id: "markdown_open",
                        mode: OpenPopupMode::CenteredFocused,
                    }
                    .from_user_shortcut("open_markdown"),
                );
            }
            "convert_surface" => {
                if let Some(sid) = state.focused_surface_id(engine) {
                    state.dialogs.convert_popup = Some(sid);
                    state.dialogs.convert_popup_selected = None;
                    state.dispatch_intent(
                        UiIntent::OpenPopup {
                            id: "convert_surface",
                            mode: OpenPopupMode::WithScope(PopupScope::Surface(sid)),
                        }
                        .from_user_shortcut("convert_surface"),
                    );
                }
            }
            "convert_to_markdown" => {
                if let Some(sid) = state.focused_surface_id(engine) {
                    let pane_id = state.active_workspace(engine).focused_pane;
                    state.dialogs.markdown_convert_surface_id = Some(sid);
                    state.dialogs.file_open_pane_id = Some(pane_id);
                    state.dialogs.markdown_open_buffer.clear();
                    state.dispatch_intent(
                        UiIntent::OpenPopup {
                            id: "markdown_open",
                            mode: OpenPopupMode::WithScope(PopupScope::Surface(sid)),
                        }
                        .from_user_shortcut("convert_to_markdown"),
                    );
                }
            }
            "convert_to_explorer" => {
                if let Some(sid) = state.focused_surface_id(engine) {
                    state.dispatch_intent(
                        crate::intent::Intent::ConvertSurface {
                            surface_id: sid,
                            target: crate::intent::ConvertTarget::Kind {
                                cwd: None,
                                kind: "explorer".to_string(),
                                params: serde_json::json!({}),
                            },
                        }
                        .from_user_shortcut("convert_to_explorer"),
                    );
                }
            }
            "rename_tab" => {
                let pane_id = state.active_workspace(engine).focused_pane;
                if let Some(pane) = state
                    .active_workspace(engine)
                    .pane_layout()
                    .find_pane(pane_id)
                {
                    let tab_index = pane.active_tab;
                    if let Some(tab) = pane.tabs.get(tab_index) {
                        let current_name = tab.display_name();
                        let target = crate::state::RenameTarget::TabName { pane_id, tab_index };
                        let scope = target.popup_scope();
                        state.dialogs.rename = Some((target, current_name));
                        state.dispatch_intent(
                            UiIntent::OpenPopup {
                                id: "rename",
                                mode: OpenPopupMode::WithScope(scope),
                            }
                            .from_user_shortcut("rename_tab"),
                        );
                    }
                }
            }
            "rename_workspace" => {
                let ws_idx = state.active_workspace;
                if let Some(ws) = engine.workspaces.get(ws_idx) {
                    let target = crate::state::RenameTarget::WorkspaceName { ws_idx };
                    let scope = target.popup_scope();
                    state.dialogs.rename = Some((target, ws.name.clone()));
                    state.dispatch_intent(
                        UiIntent::OpenPopup {
                            id: "rename",
                            mode: OpenPopupMode::WithScope(scope),
                        }
                        .from_user_shortcut("rename_workspace"),
                    );
                }
            }
            "rename_workspace_subtitle" => {
                let ws_idx = state.active_workspace;
                if let Some(ws) = engine.workspaces.get(ws_idx) {
                    let target = crate::state::RenameTarget::WorkspaceSubtitle { ws_idx };
                    let scope = target.popup_scope();
                    state.dialogs.rename = Some((target, ws.subtitle.clone()));
                    state.dispatch_intent(
                        UiIntent::OpenPopup {
                            id: "rename",
                            mode: OpenPopupMode::WithScope(scope),
                        }
                        .from_user_shortcut("rename_workspace_subtitle"),
                    );
                }
            }
            "image_undo" => {
                if state.focused_surface_type(engine).is_kind("image")
                    && let Some(sid) = focused_image_surface_id(state, engine)
                    && let Some(view) = state.image_views.get_mut(sid)
                {
                    view.undo();
                }
            }
            "image_redo" => {
                if state.focused_surface_type(engine).is_kind("image")
                    && let Some(sid) = focused_image_surface_id(state, engine)
                    && let Some(view) = state.image_views.get_mut(sid)
                {
                    view.redo();
                }
            }
            "select_all" => {
                if state.focused_surface_type(engine).is_kind("explorer")
                    && let Some(sid) = focused_explorer_surface_id(state, engine)
                    && let Some(view) = state.explorer_views.get_mut(sid)
                {
                    view.select_all();
                }
            }
            "copy_path" => {
                if state.focused_surface_type(engine).is_kind("explorer")
                    && let Some(sid) = focused_explorer_surface_id(state, engine)
                {
                    // clipboard 쓰기는 self.clipboard 가 필요해 match 밖에서 처리.
                    pending_copy_text = state
                        .explorer_views
                        .get(sid)
                        .and_then(|v| v.selected_paths_text());
                }
            }
            "explorer_refresh" => {
                if state.focused_surface_type(engine).is_kind("explorer")
                    && let Some(sid) = focused_explorer_surface_id(state, engine)
                {
                    crate::adapters::ui::egui_panels::apply_explorer_action(
                        state,
                        engine,
                        sid,
                        crate::explorer_ui::ExplorerAction::Refresh,
                    );
                }
            }
            "explorer_go_up" => {
                if state.focused_surface_type(engine).is_kind("explorer")
                    && let Some(sid) = focused_explorer_surface_id(state, engine)
                {
                    crate::adapters::ui::egui_panels::apply_explorer_action(
                        state,
                        engine,
                        sid,
                        crate::explorer_ui::ExplorerAction::GoUp,
                    );
                }
            }
            // 윈도우 컨트롤 — CSD 캡션 버튼(P5)/Linux DE 버튼(P6)/macOS 네이티브
            // 신호등과 동일한 winit window 조작을 그대로 수행한다(단일 동작 경로).
            "minimize_window" => {
                self.base.winit.set_minimized(true);
            }
            "maximize_window" => {
                let maximized = self.base.winit.is_maximized();
                self.base.winit.set_maximized(!maximized);
            }
            "close_window" => {
                // CSD close 버튼과 동일 라이프사이클(quit/close 라우팅)로 보낸다.
                send_app_event(proxy, crate::AppEvent::CloseWindow(self.base.winit.id()));
            }
            other => {
                tracing::warn!("dispatch_action_by_id: unknown action '{other}'");
                return false;
            }
        }
        if let Some(text) = pending_copy_text
            && let Some(cb) = self.clipboard.as_mut()
        {
            cb.set_text(&text);
        }
        self.base.dirty = true;
        true
    }

    /// 윈도우 컨트롤 단축키(minimize/maximize/close)를 현재 window 에 적용한다.
    /// 매칭 시 [`dispatch_action_by_id`](Self::dispatch_action_by_id) 로 위임해 CSD 버튼과
    /// 동일 경로를 탄다. macOS 는 NSMenu 가 처리하므로 비활성(`cfg(not(macos))`).
    #[cfg(not(target_os = "macos"))]
    fn handle_window_control_shortcuts(
        &mut self,
        key: &Key,
        mods: ModifiersState,
        kb: &crate::settings::KeybindingSettings,
    ) -> bool {
        for (binding, action_id) in [
            (&kb.minimize_window, "minimize_window"),
            (&kb.maximize_window, "maximize_window"),
            (&kb.close_window, "close_window"),
        ] {
            if matches_any_binding(binding, key, mods) {
                return self.dispatch_action_by_id(action_id);
            }
        }
        false
    }

    /// 사용자 스크립트 단축키 매칭 → Lua 워커 실행 요청 (ADR-0031, TODO 04).
    ///
    /// combo 가 매칭되면 등록 스크립트를 조회해 소스를 읽고 `AppEvent::RunLuaScript` 로
    /// App(lua_engine 소유)에 넘긴다. 매칭됐으나 스크립트/파일이 없으면 이벤트는 소비하되
    /// 실행하지 않는다(다른 핸들러로 새지 않게). release 는 사용자 키 입력에서만 이 경로를 탄다.
    fn try_dispatch_script_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let kb = &self.core_state.settings.keybindings;
        let Some(script_id) = kb
            .script_bindings
            .iter()
            .find(|b| matches_any_binding(std::slice::from_ref(&b.combo), key, mods))
            .map(|b| b.script_id.clone())
        else {
            return false;
        };
        let Some(entry) = self.core_state.settings.scripts.get(&script_id) else {
            tracing::warn!(
                target: "tasty_lua",
                "script shortcut matched but script '{script_id}' not registered — ignoring"
            );
            return true;
        };
        let path = entry.path.clone();
        let name = entry.name.clone();
        match std::fs::read_to_string(&path) {
            Ok(source) => {
                send_app_event(&self.proxy, crate::AppEvent::RunLuaScript { source, name });
            }
            Err(e) => {
                tracing::warn!(target: "tasty_lua", "script read failed {}: {e}", path.display());
            }
        }
        true
    }

    /// Handle keyboard shortcuts. Returns true if the event was consumed by a shortcut.
    pub(crate) fn handle_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        #[cfg(target_os = "macos")]
        let alt = mods.super_key();
        #[cfg(not(target_os = "macos"))]
        let alt = mods.alt_key();

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();

        // Clipboard copy (needs &self before state borrow)
        if self.handle_copy_shortcut(key, mods) {
            return true;
        }

        // Explorer 선택/경로복사 (clipboard 차용 필요 → keybinding free-fn 이전에 처리)
        if self.handle_explorer_shortcut(key, mods) {
            self.base.dirty = true;
            return true;
        }

        let kb = self.core_state.settings.keybindings.clone();

        // 윈도우 컨트롤(minimize/maximize/close)은 현재 winit window 를 직접 조작한다.
        // macOS 는 이 액션을 NSMenu(performMiniaturize:/performZoom:/performClose:) 의
        // key equivalent 로 처리하므로(AppKit 가 키를 소비) winit 경로를 끈다 — 이중 처리 방지.
        #[cfg(not(target_os = "macos"))]
        if self.handle_window_control_shortcuts(key, mods, &kb) {
            return true;
        }

        // Configurable keybinding shortcuts
        if Self::handle_keybinding_shortcuts(
            &mut self.state,
            &mut self.core_state,
            &kb,
            key,
            mods,
            terminal_rect,
            cell_w,
            cell_h,
            &self.proxy,
        ) {
            if self.core_state.workspaces.is_empty() {
                self.request_close();
            }
            self.base.dirty = true;
            return true;
        }

        // 사용자 스크립트 단축키 (ADR-0031, TODO 04) — 사용자 키 입력 경로에서만 발화.
        if self.try_dispatch_script_shortcut(key, mods) {
            self.base.dirty = true;
            return true;
        }

        // Numeric tab/workspace switching (Ctrl+1..9 / Alt+1..9)
        if Self::handle_numeric_switch_shortcuts(
            &mut self.state,
            &mut self.core_state,
            &kb,
            key,
            ctrl,
            shift,
            alt,
        ) {
            if self.core_state.workspaces.is_empty() {
                self.request_close();
            }
            self.base.dirty = true;
            return true;
        }

        // Clipboard paste
        if self.handle_paste_shortcut(key, mods) {
            return true;
        }

        // Zoom
        if Self::handle_zoom_shortcut(&mut self.state, &mut self.core_state, key, mods) {
            self.base.dirty = true;
            return true;
        }

        false
    }
}
