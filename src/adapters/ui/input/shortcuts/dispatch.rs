//! 단축키와 명령 팔레트의 액션 실행.

use winit::keyboard::{Key, ModifiersState};

use crate::intent::{Intent, OpenPopupMode, UiIntent};
use crate::view::main::MainView;

use super::copy_paste::ExplorerAction;
use super::keybinding::PresetApplyScope;
use super::zoom::ZoomAction;

/// `&mut self` 가 필요해 match 뒤로 미루는 팔레트 액션.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredPaletteAction {
    Copy,
    Cut,
    Paste,
}

use super::keybinding::CellGeometry;
use super::matches_any_binding;
use super::{focused_explorer_surface_id, focused_workspace_category, send_app_event};

impl MainView {
    /// KeybindingSettings의 field_id로 액션을 실행한다. 알 수 없는 ID는 false다.
    /// 현재 명령 팔레트가 호출하며 에이전트 IPC에는 노출하지 않는다.
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: action_id 문자열→액션 평면 match 디스패치 — 단축키와 1:1, arm 나열
    pub(crate) fn dispatch_action_by_id(&mut self, action_id: &str) -> bool {
        use crate::adapters::ui::popup::PopupScope;
        use crate::model::SplitDirection;

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();
        let scale_factor = self.base.gpu.scale_factor();
        let proxy = self.proxy.clone();
        let proxy = &proxy;
        let mut pending_copy_text: Option<String> = None;
        let mut pending_copy_scope: Option<u32> = None;
        // state/engine 차용이 끝난 뒤 clipboard/selection 액션을 실행하도록 모아 둔다.
        let mut deferred: Option<DeferredPaletteAction> = None;
        let state = &mut self.state;
        let engine = &mut self.core_state;

        match action_id {
            "new_workspace" => {
                let category = focused_workspace_category(state, engine);
                state.dispatch_intent(
                    Intent::NewWorkspace {
                        kind: None,
                        params: serde_json::Value::Null,
                        category,
                    }
                    .from_user_shortcut("new_workspace"),
                );
                crate::core::Core::resize_all_terminals(
                    state.tab_bar_height,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                    scale_factor,
                );
            }
            "new_tab" => {
                if let Err(e) = state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
                crate::core::Core::resize_all_terminals(
                    state.tab_bar_height,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                    scale_factor,
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
                    state.tab_bar_height,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                    scale_factor,
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
                    state.tab_bar_height,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                    scale_factor,
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
                    state.tab_bar_height,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                    scale_factor,
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
                    state.tab_bar_height,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                    scale_factor,
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
            "fullscreen_stage_exit" => {
                // 키 경로는 앞단에서 닫지만 팔레트도 같은 액션 ID로 종료할 수 있어야 한다.
                state.close_fullscreen_stage();
            }
            "toggle_sidebar" => {
                state.sidebar_visible = !state.sidebar_visible;
            }
            "toggle_sidebar_collapse" => {
                state.sidebar_collapsed = !state.sidebar_collapsed;
            }
            "toggle_categories_collapsed" => {
                if engine.settings.general.workspace_categories_enabled {
                    engine.toggle_all_categories_collapsed();
                    engine.mark_layout_dirty();
                }
            }
            "close_workspace" => {
                state.close_active_workspace(engine);
                if engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    crate::core::Core::resize_all_terminals(
                        self.state.tab_bar_height,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                        scale_factor,
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
                        self.state.tab_bar_height,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                        scale_factor,
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
                        self.state.tab_bar_height,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                        scale_factor,
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
                        self.state.tab_bar_height,
                        engine,
                        terminal_rect,
                        cell_w,
                        cell_h,
                        scale_factor,
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
                    state.tab_bar_height,
                    engine,
                    terminal_rect,
                    cell_w,
                    cell_h,
                    scale_factor,
                );
            }
            "quit" => send_app_event(proxy, crate::AppEvent::QuitRequested),
            "quit_immediate" => send_app_event(proxy, crate::AppEvent::Shutdown),
            "quit_minimize" => send_app_event(proxy, crate::AppEvent::Minimize),
            "new_window" => send_app_event(
                proxy,
                crate::AppEvent::CreateWindow(crate::app::event::WindowRequestOrigin::User, None),
            ),
            "find" => {
                // 터미널 검색만 처리한다. 다른 종류의 자체 검색을 빈 터미널 검색창으로 가리지 않는다.
                if matches!(
                    state.focused_surface_type(engine),
                    crate::state::FocusedSurfaceType::Terminal
                ) {
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
            }
            "open_markdown" => {
                state.enqueue_convert_input_popup(engine, "markdown", None);
            }
            "open_explorer" => Self::open_explorer_tab(state),
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
                    state.enqueue_convert_input_popup(engine, "markdown", Some(sid));
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
            "select_all" => {
                if state.focused_surface_type(engine).is_kind("explorer")
                    && let Some(sid) = focused_explorer_surface_id(state, engine)
                    && let Some(view) = state.explorer_views.get_mut(sid)
                {
                    view.select_all();
                }
            }
            "enter_copy_mode" => {
                state.dialogs.pending_enter_copy_mode = true;
            }
            "copy_path" => {
                if state.focused_surface_type(engine).is_kind("explorer")
                    && let Some(sid) = focused_explorer_surface_id(state, engine)
                {
                    pending_copy_text = state
                        .explorer_views
                        .get(sid)
                        .and_then(|v| v.selected_paths_text());
                    if pending_copy_text.is_some() {
                        pending_copy_scope = Some(sid);
                    }
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
            "toggle_dag_list" => Self::toggle_dag_list_popup(state),
            "open_port_scanner" => Self::open_tool_popup(
                state,
                crate::adapters::ui::popup::port_scanner::PORT_SCANNER_POPUP_ID,
                "open_port_scanner",
            ),
            "open_remote_tool" => Self::open_tool_popup(
                state,
                crate::adapters::ui::popup::remote_tool::REMOTE_TOOL_POPUP_ID,
                "open_remote_tool",
            ),
            "open_tutorial" => Self::open_tool_popup(
                state,
                crate::adapters::ui::tutorial::topic_popup::TUTORIAL_TOPICS_POPUP_ID,
                "open_tutorial",
            ),
            "open_preset_window" => Self::open_preset_window(state),
            "open_file_picker" => Self::open_file_picker_tool(state, engine),
            "screenshot_to_clipboard" => Self::queue_screenshot_to_clipboard(state, engine),
            "apply_workspace_preset" => {
                Self::open_preset_apply_popup(state, PresetApplyScope::Workspace);
            }
            "apply_tab_preset" => Self::open_preset_apply_popup(state, PresetApplyScope::Tab),
            "apply_pane_preset" => Self::open_preset_apply_popup(state, PresetApplyScope::Pane),
            "zoom_in" => {
                Self::apply_zoom(state, engine, ZoomAction::In);
            }
            "zoom_out" => {
                Self::apply_zoom(state, engine, ZoomAction::Out);
            }
            "zoom_reset" => {
                Self::apply_zoom(state, engine, ZoomAction::Reset);
            }
            "copy" => deferred = Some(DeferredPaletteAction::Copy),
            "cut" => deferred = Some(DeferredPaletteAction::Cut),
            "paste" => deferred = Some(DeferredPaletteAction::Paste),
            "minimize_window" => {
                self.base.winit.set_minimized(true);
            }
            "maximize_window" => {
                let maximized = self.base.winit.is_maximized();
                self.base.winit.set_maximized(!maximized);
            }
            "close_window" => {
                send_app_event(proxy, crate::AppEvent::CloseWindow(self.base.winit.id()));
            }
            other => {
                tracing::warn!("dispatch_action_by_id: unknown action '{other}'");
                return false;
            }
        }
        // 키 경로와 같은 순서: 복사는 선택 텍스트→탐색기 파일, 붙여넣기는 탐색기→터미널이다.
        match deferred {
            Some(DeferredPaletteAction::Copy) => {
                if !self.run_copy() {
                    self.run_explorer_action(ExplorerAction::CopyFiles);
                }
            }
            Some(DeferredPaletteAction::Cut) => {
                self.run_explorer_action(ExplorerAction::CutFiles);
            }
            Some(DeferredPaletteAction::Paste) => {
                if !self.run_explorer_action(ExplorerAction::PasteFiles) {
                    self.run_paste();
                }
            }
            None => {}
        }
        if let Some(text) = pending_copy_text
            && let Some(cb) = self.clipboard.as_mut()
        {
            cb.set_text(&text);
            if let Some(sid) = pending_copy_scope {
                self.state.toasts.push_info(
                    crate::i18n::t("toast.copied_path"),
                    crate::adapters::ui::ToastScope::Surface(sid),
                );
            }
        }
        self.base.dirty = true;
        true
    }

    /// 창 제어는 공용 액션으로 실행한다. macOS는 NSMenu가 처리하므로 이 경로를 제외한다.
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

    /// 사용자 스크립트를 읽어 App의 Lua 워커에 실행을 요청한다.
    /// 등록이 없거나 파일을 못 읽어도 매칭된 키는 소비해 다른 액션으로 넘어가지 않게 한다.
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
        let name = if entry.name.is_empty() {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| script_id.clone())
        } else {
            entry.name.clone()
        };
        let stored_hash = entry.sha256.clone();
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(target: "tasty_lua", "script read failed {}: {e}", path.display());
                return true;
            }
        };
        // 등록 뒤 파일이 바뀌면 바로 실행하지 않고 사용자 확인을 받는다.
        let current_hash = tasty_settings::hash_bytes(source.as_bytes());
        if current_hash == stored_hash {
            send_app_event(&self.proxy, crate::AppEvent::RunLuaScript { source, name });
        } else {
            self.state.dialogs.pending_script_confirm = Some(crate::state::PendingScriptConfirm {
                script_id,
                name,
                source,
                new_hash: current_hash,
                result: None,
            });
            self.state.dispatch_intent(
                crate::intent::UiIntent::OpenPopup {
                    id: "script_changed_confirm",
                    mode: crate::intent::OpenPopupMode::CenteredFocused,
                }
                .from_user_menu("script_tofu_gate"),
            );
        }
        true
    }

    pub(crate) fn handle_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        // alt는 macOS의 Command, 다른 OS의 Alt다. option은 macOS에서만 사용한다.
        #[cfg(target_os = "macos")]
        let (alt, option) = (mods.super_key(), mods.alt_key());
        #[cfg(not(target_os = "macos"))]
        let (alt, option) = (mods.alt_key(), false);

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();

        if self.handle_copy_shortcut(key, mods) {
            return true;
        }

        if self.handle_explorer_shortcut(key, mods) {
            self.base.dirty = true;
            return true;
        }

        let kb = self.core_state.settings.keybindings.clone();

        // macOS는 AppKit의 메뉴 단축키가 처리하므로 winit에서 중복 실행하지 않는다.
        #[cfg(not(target_os = "macos"))]
        if self.handle_window_control_shortcuts(key, mods, &kb) {
            return true;
        }

        let cells = CellGeometry {
            w: crate::model::PhysicalPx(cell_w),
            h: crate::model::PhysicalPx(cell_h),
            scale_factor: self.base.gpu.scale_factor(),
        };
        if Self::handle_keybinding_shortcuts(
            &mut self.state,
            &mut self.core_state,
            &kb,
            key,
            mods,
            terminal_rect,
            cells,
            &self.proxy,
        ) {
            if self.core_state.workspaces.is_empty() {
                self.request_close();
            }
            self.base.dirty = true;
            return true;
        }

        if self.try_dispatch_script_shortcut(key, mods) {
            self.base.dirty = true;
            return true;
        }

        if Self::handle_numeric_switch_shortcuts(
            &mut self.state,
            &mut self.core_state,
            &kb,
            key,
            mods,
            ctrl,
            shift,
            alt,
            option,
        ) {
            if self.core_state.workspaces.is_empty() {
                self.request_close();
            }
            self.base.dirty = true;
            return true;
        }

        if self.handle_paste_shortcut(key, mods) {
            return true;
        }

        if Self::handle_zoom_shortcut(&mut self.state, &mut self.core_state, key, mods) {
            self.base.dirty = true;
            return true;
        }

        false
    }
}
