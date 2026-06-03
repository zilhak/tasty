//! 단축키 디스패치 — `handle_shortcut` 의 키 입력 → 액션 분기 + `dispatch_action_by_id`
//! 의 액션 ID 직접 호출 (Command Palette / 자동화 진입점).

use winit::keyboard::{Key, ModifiersState};

use crate::intent::{Intent, OpenPopupMode, UiIntent};
use crate::view::main::MainView;

use super::{focused_image_surface_id, send_app_event};

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
        let state = &mut self.state;
        let engine = &mut self.core_state;

        match action_id {
            "new_workspace" => {
                state.dispatch_intent(
                    Intent::NewWorkspace {
                        kind: None,
                        params: serde_json::Value::Null,
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
                let target_sid = state.focused_surface_id(engine);
                let target_kind = target_sid.and_then(|s| state.surface_kind(engine, s));
                let closed = state.close_active_surface(engine);
                if closed {
                    if let (Some(sid), Some(k)) = (target_sid, target_kind) {
                        state.enqueue_surface_closed(sid, k, true);
                    }
                } else if !state.close_active_pane(engine) {
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
                if state.popups.is_open("search_bar") {
                    state.search.clear();
                    state.dispatch_intent(
                        UiIntent::ClosePopup { id: "search_bar" }.from_user_shortcut("find_close"),
                    );
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
            other => {
                tracing::warn!("dispatch_action_by_id: unknown action '{other}'");
                return false;
            }
        }
        self.base.dirty = true;
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

        let kb = self.core_state.settings.keybindings.clone();

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
