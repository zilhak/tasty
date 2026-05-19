mod binding;
mod copy_paste;
mod numeric;
#[cfg(test)]
mod tests;
mod zoom;

use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, KeyCode, ModifiersState, PhysicalKey};

use crate::intent::{Intent, OpenPopupMode};
use crate::model::SplitDirection;
use crate::window::main::MainWindow;

pub(crate) use binding::matches_any_binding;

/// Best-effort `EventLoopProxy::send_event` dispatch.
///
/// `send_event`는 event loop가 이미 종료된 뒤에만 Err를 돌려준다. quit/shutdown
/// 단축키 직후의 자투리 입력 race에서만 발생하며, 이미 종료 중인 상황이라 무해
/// — 다만 디버깅에 도움 되도록 trace 레벨로 흔적은 남긴다.
pub(crate) fn send_app_event(proxy: &EventLoopProxy<crate::AppEvent>, event: crate::AppEvent) {
    if let Err(e) = proxy.send_event(event) {
        tracing::trace!("AppEvent send dropped (event loop closing): {e}");
    }
}

/// Convert a physical key code to a Key::Character for shortcut matching.
/// On macOS, when IME is composing (e.g. Korean), logical_key may contain
/// the composed character (e.g. "ㅇ" instead of "d"). This function extracts
/// the intended key from the physical key code.
pub(crate) fn physical_key_to_logical(physical: &PhysicalKey) -> Option<Key> {
    let code = match physical {
        PhysicalKey::Code(c) => c,
        _ => return None,
    };
    let ch: &str = match code {
        KeyCode::KeyA => "a",
        KeyCode::KeyB => "b",
        KeyCode::KeyC => "c",
        KeyCode::KeyD => "d",
        KeyCode::KeyE => "e",
        KeyCode::KeyF => "f",
        KeyCode::KeyG => "g",
        KeyCode::KeyH => "h",
        KeyCode::KeyI => "i",
        KeyCode::KeyJ => "j",
        KeyCode::KeyK => "k",
        KeyCode::KeyL => "l",
        KeyCode::KeyM => "m",
        KeyCode::KeyN => "n",
        KeyCode::KeyO => "o",
        KeyCode::KeyP => "p",
        KeyCode::KeyQ => "q",
        KeyCode::KeyR => "r",
        KeyCode::KeyS => "s",
        KeyCode::KeyT => "t",
        KeyCode::KeyU => "u",
        KeyCode::KeyV => "v",
        KeyCode::KeyW => "w",
        KeyCode::KeyX => "x",
        KeyCode::KeyY => "y",
        KeyCode::KeyZ => "z",
        KeyCode::Digit0 => "0",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Minus => "-",
        KeyCode::Equal => "=",
        KeyCode::BracketLeft => "[",
        KeyCode::BracketRight => "]",
        KeyCode::Semicolon => ";",
        KeyCode::Quote => "'",
        KeyCode::Backquote => "`",
        KeyCode::Backslash => "\\",
        KeyCode::Comma => ",",
        KeyCode::Period => ".",
        KeyCode::Slash => "/",
        _ => return None,
    };
    Some(Key::Character(ch.into()))
}

/// 바인딩 목록 중 하나라도 매칭되면 true.
/// Returns the surface ID of the focused image surface, if any.
fn focused_image_surface_id(state: &crate::state::AppState) -> Option<u32> {
    let pane = state.focused_pane()?;
    let tab = pane.tabs.get(pane.active_tab)?;
    let focused = tab.focused_surface;
    let surface = tab.layout().find_surface(focused)?;
    surface
        .as_any()
        .downcast_ref::<crate::model::ImagePanel>()
        .map(|p| p.id)
}


impl MainWindow {
    /// Handle double-tap modifier shortcuts. Returns true if consumed.
    pub(crate) fn handle_double_tap_shortcut(
        &mut self,
        dt: crate::double_tap::DoubleTapKey,
    ) -> bool {
        let kb = self.state.engine.settings.keybindings.clone();
        let dt_str = dt.binding_str();

        let has_dt = |bindings: &[String]| bindings.iter().any(|b| b == dt_str);
        if has_dt(&kb.toggle_settings) {
            send_app_event(&self.proxy, crate::AppEvent::OpenSettings);
            return true;
        }
        if has_dt(&kb.toggle_notifications) {
            // Intent 통과 시 다음 프레임에 처리되므로 is_open 체크는 즉시 수행
            // 후 toggle Intent 발화. mark_all_read 는 현재 상태 기준 — 다음 프레임
            // 에 popup 이 열릴 예정이면 미리 읽음 처리.
            let will_open = !self.state.popups.is_open("notifications");
            self.state.dispatch_intent(
                Intent::TogglePopup {
                    id: "notifications",
                    mode: OpenPopupMode::Default,
                }
                .from_user_shortcut("toggle_notifications_double_tap"),
            );
            if will_open {
                self.state.engine.notifications.mark_all_read();
            }
            return true;
        }

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();

        // Check all configurable bindings for double-tap matches
        let bindings_to_check: Vec<(&[String], &str)> = vec![
            (&kb.new_workspace, "new_workspace"),
            (&kb.close_workspace, "close_workspace"),
            (&kb.new_tab, "new_tab"),
            (&kb.close_pane, "close_pane"),
            (&kb.split_pane_vertical, "split_pane_vertical"),
            (&kb.split_pane_horizontal, "split_pane_horizontal"),
            (&kb.split_surface_vertical, "split_surface_vertical"),
            (&kb.split_surface_horizontal, "split_surface_horizontal"),
            (&kb.focus_pane_next, "focus_pane_next"),
            (&kb.focus_pane_prev, "focus_pane_prev"),
            (&kb.focus_surface_next, "focus_surface_next"),
            (&kb.focus_surface_prev, "focus_surface_prev"),
            (&kb.close_surface, "close_surface"),
            (&kb.open_markdown, "open_markdown"),
            (&kb.open_explorer, "open_explorer"),
            (&kb.convert_surface, "convert_surface"),
            (&kb.convert_to_markdown, "convert_to_markdown"),
            (&kb.convert_to_explorer, "convert_to_explorer"),
            (&kb.close_active, "close_active"),
            (&kb.next_tab, "next_tab"),
            (&kb.prev_tab, "prev_tab"),
        ];

        for (bindings, action) in &bindings_to_check {
            if has_dt(bindings) {
                match *action {
                    "new_workspace" => {
                        self.state.dispatch_intent(
                            Intent::NewWorkspace {
                                kind: None,
                                params: serde_json::Value::Null,
                            }
                            .from_user_shortcut("new_workspace"),
                        );
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "close_workspace" => {
                        self.state.close_active_workspace();
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "new_tab" => {
                        if let Err(e) = self.state.add_tab() {
                            tracing::warn!("add_tab failed: {e}");
                        }
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "close_pane" => {
                        if !self.state.close_active_pane() {
                            self.state.close_active_workspace();
                        }
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "split_pane_vertical" => {
                        self.state.dispatch_intent(
                            Intent::SplitPane {
                                direction: SplitDirection::Vertical,
                            }
                            .from_user_shortcut("split_pane_vertical"),
                        );
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "split_pane_horizontal" => {
                        self.state.dispatch_intent(
                            Intent::SplitPane {
                                direction: SplitDirection::Horizontal,
                            }
                            .from_user_shortcut("split_pane_horizontal"),
                        );
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "split_surface_vertical" => {
                        self.state.dispatch_intent(
                            Intent::SplitSurface {
                                direction: SplitDirection::Vertical,
                            }
                            .from_user_shortcut("split_surface_vertical"),
                        );
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "split_surface_horizontal" => {
                        self.state.dispatch_intent(
                            Intent::SplitSurface {
                                direction: SplitDirection::Horizontal,
                            }
                            .from_user_shortcut("split_surface_horizontal"),
                        );
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "focus_pane_next" => {
                        self.state.move_pane_focus_forward();
                    }
                    "focus_pane_prev" => {
                        self.state.move_pane_focus_backward();
                    }
                    "focus_surface_next" => {
                        self.state.move_surface_focus_forward();
                    }
                    "focus_surface_prev" => {
                        self.state.move_surface_focus_backward();
                    }
                    "close_surface" => {
                        let target_sid = self.state.focused_surface_id();
                        let target_kind = target_sid.and_then(|s| self.state.surface_kind(s));
                        let closed = self.state.close_active_surface();
                        if closed {
                            if let (Some(sid), Some(k)) = (target_sid, target_kind) {
                                self.state.enqueue_surface_closed(sid, k, true);
                            }
                        } else if !self.state.close_active_pane() {
                            self.state.close_active_workspace();
                        }
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "restore_closed" => {
                        self.state.restore_closed_item();
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "quit" => {
                        send_app_event(&self.proxy, crate::AppEvent::QuitRequested);
                    }
                    "quit_immediate" => {
                        send_app_event(&self.proxy, crate::AppEvent::Shutdown);
                    }
                    "quit_minimize" => {
                        send_app_event(&self.proxy, crate::AppEvent::Minimize);
                    }
                    "open_markdown" => {
                        let pane_id = self.state.active_workspace().focused_pane;
                        self.state.dialogs.file_open_pane_id = Some(pane_id);
                        self.state.dialogs.markdown_open_buffer.clear();
                        self.state.dispatch_intent(
                            Intent::OpenPopup {
                                id: "markdown_open",
                                mode: OpenPopupMode::CenteredFocused,
                            }
                            .from_user_shortcut("open_markdown_double_tap"),
                        );
                    }
                    "convert_surface" => {
                        if let Some(sid) = self.state.focused_surface_id() {
                            self.state.dialogs.convert_popup = Some(sid);
                            self.state.dialogs.convert_popup_selected = None;
                            self.state.dispatch_intent(
                                Intent::OpenPopup {
                                    id: "convert_surface",
                                    mode: OpenPopupMode::WithScope(
                                        crate::ui::popup::PopupScope::Surface(sid),
                                    ),
                                }
                                .from_user_shortcut("convert_surface_double_tap"),
                            );
                        }
                    }
                    "convert_to_markdown" => {
                        if let Some(sid) = self.state.focused_surface_id() {
                            let pane_id = self.state.active_workspace().focused_pane;
                            self.state.dialogs.markdown_convert_surface_id = Some(sid);
                            self.state.dialogs.file_open_pane_id = Some(pane_id);
                            self.state.dialogs.markdown_open_buffer.clear();
                            self.state.dispatch_intent(
                                Intent::OpenPopup {
                                    id: "markdown_open",
                                    mode: OpenPopupMode::WithScope(
                                        crate::ui::popup::PopupScope::Surface(sid),
                                    ),
                                }
                                .from_user_shortcut("convert_to_markdown_double_tap"),
                            );
                        }
                    }
                    "convert_to_explorer" => {
                        // explorer는 com.tasty.explorer plugin이 제공하므로
                        // 호스트 측 즉시 변환은 더 이상 동작하지 않는다. 단축키
                        // 자체는 보존하되 동작은 후속 단계에서 plugin RemoteSurface
                        // swap으로 복구할 예정.
                    }
                    "close_active" => {
                        if !self.state.close_active_tab() {
                            if !self.state.close_active_pane() {
                                self.state.close_active_workspace();
                            }
                        }
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "next_tab" => {
                        self.state.next_tab_in_pane();
                    }
                    "prev_tab" => {
                        self.state.prev_tab_in_pane();
                    }
                    _ => {}
                }
                return true;
            }
        }

        false
    }

    /// Dispatch a keybinding action by its stable `field_id` (예: `"new_workspace"`).
    /// 단축키와 정확히 같은 효과를 내며, Command Palette / 외부 자동화에서 호출한다.
    ///
    /// Returns true if the action was recognized and dispatched. Unknown action_id는 false.
    pub(crate) fn dispatch_action_by_id(&mut self, action_id: &str) -> bool {
        use crate::model::SplitDirection;
        use crate::ui::popup::PopupScope;

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();
        let proxy = &self.proxy;
        let state = &mut self.state;

        match action_id {
            "new_workspace" => {
                state.dispatch_intent(
                    Intent::NewWorkspace {
                        kind: None,
                        params: serde_json::Value::Null,
                    }
                    .from_user_shortcut("new_workspace"),
                );
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            "new_tab" => {
                if let Err(e) = state.add_tab() {
                    tracing::warn!("add_tab failed: {e}");
                }
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            "split_pane_vertical" => {
                state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_pane_vertical"),
                );
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            "split_pane_horizontal" => {
                state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Horizontal,
                    }
                    .from_user_shortcut("split_pane_horizontal"),
                );
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            "split_surface_vertical" => {
                state.dispatch_intent(
                    Intent::SplitSurface {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_surface_vertical"),
                );
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            "split_surface_horizontal" => {
                state.dispatch_intent(
                    Intent::SplitSurface {
                        direction: SplitDirection::Horizontal,
                    }
                    .from_user_shortcut("split_surface_horizontal"),
                );
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            "toggle_settings" => {
                send_app_event(proxy, crate::AppEvent::OpenSettings);
            }
            "toggle_notifications" => {
                let will_open = !state.popups.is_open("notifications");
                state.dispatch_intent(
                    Intent::TogglePopup {
                        id: "notifications",
                        mode: OpenPopupMode::Default,
                    }
                    .from_user_shortcut("toggle_notifications"),
                );
                if will_open {
                    state.engine.notifications.mark_all_read();
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
                state.close_active_workspace();
                if state.engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    self.state
                        .resize_all(terminal_rect, cell_w, cell_h);
                }
                return true;
            }
            "close_pane" => {
                if !state.close_active_pane() {
                    state.close_active_workspace();
                }
                if state.engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    self.state.resize_all(terminal_rect, cell_w, cell_h);
                }
                return true;
            }
            "close_surface" => {
                let target_sid = state.focused_surface_id();
                let target_kind = target_sid.and_then(|s| state.surface_kind(s));
                let closed = state.close_active_surface();
                if closed {
                    if let (Some(sid), Some(k)) = (target_sid, target_kind) {
                        state.enqueue_surface_closed(sid, k, true);
                    }
                } else if !state.close_active_pane() {
                    state.close_active_workspace();
                }
                if state.engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    self.state.resize_all(terminal_rect, cell_w, cell_h);
                }
                return true;
            }
            "close_active" => {
                if !state.close_active_tab()
                    && !state.close_active_pane()
                {
                    state.close_active_workspace();
                }
                if state.engine.workspaces.is_empty() {
                    self.request_close();
                } else {
                    self.state.resize_all(terminal_rect, cell_w, cell_h);
                }
                return true;
            }
            "focus_pane_next" => state.move_pane_focus_forward(),
            "focus_pane_prev" => state.move_pane_focus_backward(),
            "focus_surface_next" => state.move_surface_focus_forward(),
            "focus_surface_prev" => state.move_surface_focus_backward(),
            "next_tab" => state.next_tab_in_pane(),
            "prev_tab" => state.prev_tab_in_pane(),
            "restore_closed" => {
                state.restore_closed_item();
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            "quit" => send_app_event(proxy, crate::AppEvent::QuitRequested),
            "quit_immediate" => send_app_event(proxy, crate::AppEvent::Shutdown),
            "quit_minimize" => send_app_event(proxy, crate::AppEvent::Minimize),
            "new_window" => send_app_event(proxy, crate::AppEvent::CreateWindow),
            "find" => {
                if state.popups.is_open("search_bar") {
                    state.search.clear();
                    state.dispatch_intent(
                        Intent::ClosePopup { id: "search_bar" }
                            .from_user_shortcut("find_close"),
                    );
                } else if let Some(sid) = state.focused_surface_id() {
                    state.search.surface_id = sid;
                    state.dispatch_intent(
                        Intent::OpenPopup {
                            id: "search_bar",
                            mode: OpenPopupMode::AtTopOfScope(PopupScope::Surface(sid)),
                        }
                        .from_user_shortcut("find"),
                    );
                }
            }
            "open_markdown" => {
                let pane_id = state.active_workspace().focused_pane;
                state.dialogs.file_open_pane_id = Some(pane_id);
                state.dialogs.markdown_open_buffer.clear();
                state.dispatch_intent(
                    Intent::OpenPopup {
                        id: "markdown_open",
                        mode: OpenPopupMode::CenteredFocused,
                    }
                    .from_user_shortcut("open_markdown"),
                );
            }
            "convert_surface" => {
                if let Some(sid) = state.focused_surface_id() {
                    state.dialogs.convert_popup = Some(sid);
                    state.dialogs.convert_popup_selected = None;
                    state.dispatch_intent(
                        Intent::OpenPopup {
                            id: "convert_surface",
                            mode: OpenPopupMode::WithScope(PopupScope::Surface(sid)),
                        }
                        .from_user_shortcut("convert_surface"),
                    );
                }
            }
            "convert_to_markdown" => {
                if let Some(sid) = state.focused_surface_id() {
                    let pane_id = state.active_workspace().focused_pane;
                    state.dialogs.markdown_convert_surface_id = Some(sid);
                    state.dialogs.file_open_pane_id = Some(pane_id);
                    state.dialogs.markdown_open_buffer.clear();
                    state.dispatch_intent(
                        Intent::OpenPopup {
                            id: "markdown_open",
                            mode: OpenPopupMode::WithScope(PopupScope::Surface(sid)),
                        }
                        .from_user_shortcut("convert_to_markdown"),
                    );
                }
            }
            "rename_tab" => {
                let pane_id = state.active_workspace().focused_pane;
                if let Some(pane) =
                    state.active_workspace().pane_layout().find_pane(pane_id)
                {
                    let tab_index = pane.active_tab;
                    if let Some(tab) = pane.tabs.get(tab_index) {
                        let current_name = tab.display_name();
                        let target = crate::state::RenameTarget::TabName { pane_id, tab_index };
                        let scope = target.popup_scope();
                        state.dialogs.rename = Some((target, current_name));
                        state.dispatch_intent(
                            Intent::OpenPopup {
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
                if let Some(ws) = state.engine.workspaces.get(ws_idx) {
                    let target = crate::state::RenameTarget::WorkspaceName { ws_idx };
                    let scope = target.popup_scope();
                    state.dialogs.rename = Some((target, ws.name.clone()));
                    state.dispatch_intent(
                        Intent::OpenPopup {
                            id: "rename",
                            mode: OpenPopupMode::WithScope(scope),
                        }
                        .from_user_shortcut("rename_workspace"),
                    );
                }
            }
            "rename_workspace_subtitle" => {
                let ws_idx = state.active_workspace;
                if let Some(ws) = state.engine.workspaces.get(ws_idx) {
                    let target = crate::state::RenameTarget::WorkspaceSubtitle { ws_idx };
                    let scope = target.popup_scope();
                    state.dialogs.rename = Some((target, ws.subtitle.clone()));
                    state.dispatch_intent(
                        Intent::OpenPopup {
                            id: "rename",
                            mode: OpenPopupMode::WithScope(scope),
                        }
                        .from_user_shortcut("rename_workspace_subtitle"),
                    );
                }
            }
            "image_undo" => {
                if state.focused_surface_type().is_kind("image") {
                    if let Some(sid) = focused_image_surface_id(state) {
                        if let Some(view) = state.image_views.get_mut(sid) {
                            view.undo();
                        }
                    }
                }
            }
            "image_redo" => {
                if state.focused_surface_type().is_kind("image") {
                    if let Some(sid) = focused_image_surface_id(state) {
                        if let Some(view) = state.image_views.get_mut(sid) {
                            view.redo();
                        }
                    }
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

        let kb = self.state.engine.settings.keybindings.clone();

        // Configurable keybinding shortcuts
        if Self::handle_keybinding_shortcuts(
            &mut self.state,
            &kb,
            key,
            mods,
            terminal_rect,
            cell_w,
            cell_h,
            &self.proxy,
        ) {
            if self.state.engine.workspaces.is_empty() {
                self.request_close();
            }
            self.base.dirty = true;
            return true;
        }

        // Numeric tab/workspace switching (Ctrl+1..9 / Alt+1..9)
        if Self::handle_numeric_switch_shortcuts(&mut self.state, &kb, key, ctrl, shift, alt) {
            if self.state.engine.workspaces.is_empty() {
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
        if Self::handle_zoom_shortcut(&mut self.state, key, mods) {
            self.base.dirty = true;
            return true;
        }

        false
    }

    fn handle_keybinding_shortcuts(
        state: &mut crate::state::AppState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
        terminal_rect: crate::model::Rect,
        cell_w: f32,
        cell_h: f32,
        proxy: &winit::event_loop::EventLoopProxy<crate::AppEvent>,
    ) -> bool {
        if matches_any_binding(&kb.new_workspace, key, mods) {
            state.dispatch_intent(
                Intent::NewWorkspace {
                    kind: None,
                    params: serde_json::Value::Null,
                }
                .from_user_shortcut("new_workspace"),
            );
            return true;
        }
        if matches_any_binding(&kb.new_tab, key, mods) {
            if let Err(e) = state.add_tab() {
                tracing::warn!("add_tab failed: {e}");
            }
            return true;
        }
        if matches_any_binding(&kb.split_pane_vertical, key, mods) {
            state.dispatch_intent(
                Intent::SplitPane {
                    direction: SplitDirection::Vertical,
                }
                .from_user_shortcut("split_pane_vertical"),
            );
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_pane_horizontal, key, mods) {
            state.dispatch_intent(
                Intent::SplitPane {
                    direction: SplitDirection::Horizontal,
                }
                .from_user_shortcut("split_pane_horizontal"),
            );
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_surface_vertical, key, mods) {
            state.dispatch_intent(
                Intent::SplitSurface {
                    direction: SplitDirection::Vertical,
                }
                .from_user_shortcut("split_surface_vertical"),
            );
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_surface_horizontal, key, mods) {
            state.dispatch_intent(
                Intent::SplitSurface {
                    direction: SplitDirection::Horizontal,
                }
                .from_user_shortcut("split_surface_horizontal"),
            );
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.toggle_settings, key, mods) {
            send_app_event(proxy, crate::AppEvent::OpenSettings);
            return true;
        }
        if matches_any_binding(&kb.toggle_notifications, key, mods) {
            let will_open = !state.popups.is_open("notifications");
            state.dispatch_intent(
                Intent::TogglePopup {
                    id: "notifications",
                    mode: OpenPopupMode::Default,
                }
                .from_user_shortcut("toggle_notifications"),
            );
            if will_open {
                state.engine.notifications.mark_all_read();
            }
            return true;
        }
        if matches_any_binding(&kb.find, key, mods) {
            if state.popups.is_open("search_bar") {
                state.search.clear();
                state.dispatch_intent(
                    Intent::ClosePopup { id: "search_bar" }.from_user_shortcut("find_close"),
                );
            } else if let Some(sid) = state.focused_surface_id() {
                state.search.surface_id = sid;
                state.dispatch_intent(
                    Intent::OpenPopup {
                        id: "search_bar",
                        mode: OpenPopupMode::AtTopOfScope(
                            crate::ui::popup::PopupScope::Surface(sid),
                        ),
                    }
                    .from_user_shortcut("find_open"),
                );
            }
            return true;
        }
        if matches_any_binding(&kb.toggle_clipboard_viewer, key, mods) {
            // 호스트는 단축키 신호만 publish하고 viewer는 clipboard-history plugin이 책임.
            state.enqueue_host_event(crate::state::PendingHostEvent::Raw {
                key: "shortcut.toggle_clipboard_viewer".into(),
                payload: serde_json::Value::Null,
            });
            return true;
        }
        if matches_any_binding(&kb.close_workspace, key, mods) {
            state.close_active_workspace();
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.close_pane, key, mods) {
            if !state.close_active_pane() {
                state.close_active_workspace();
            }
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.close_surface, key, mods) {
            let target_sid = state.focused_surface_id();
            let target_kind = target_sid.and_then(|s| state.surface_kind(s));
            let closed = state.close_active_surface();
            if closed {
                if let (Some(sid), Some(k)) = (target_sid, target_kind) {
                    state.enqueue_surface_closed(sid, k, true);
                }
            } else if !state.close_active_pane() {
                state.close_active_workspace();
            }
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.focus_pane_next, key, mods) {
            state.move_pane_focus_forward();
            return true;
        }
        if matches_any_binding(&kb.focus_pane_prev, key, mods) {
            state.move_pane_focus_backward();
            return true;
        }
        if matches_any_binding(&kb.focus_surface_next, key, mods) {
            state.move_surface_focus_forward();
            return true;
        }
        if matches_any_binding(&kb.focus_surface_prev, key, mods) {
            state.move_surface_focus_backward();
            return true;
        }
        if matches_any_binding(&kb.toggle_sidebar, key, mods) {
            state.sidebar_visible = !state.sidebar_visible;
            return true;
        }
        if matches_any_binding(&kb.toggle_sidebar_collapse, key, mods) {
            state.sidebar_collapsed = !state.sidebar_collapsed;
            return true;
        }
        if matches_any_binding(&kb.restore_closed, key, mods) {
            state.restore_closed_item();
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.quit_immediate, key, mods) {
            send_app_event(proxy, crate::AppEvent::Shutdown);
            return true;
        }
        if matches_any_binding(&kb.quit_minimize, key, mods) {
            send_app_event(proxy, crate::AppEvent::Minimize);
            return true;
        }
        if matches_any_binding(&kb.quit, key, mods) {
            send_app_event(proxy, crate::AppEvent::QuitRequested);
            return true;
        }
        if matches_any_binding(&kb.open_markdown, key, mods) {
            let pane_id = state.active_workspace().focused_pane;
            state.dialogs.file_open_pane_id = Some(pane_id);
            state.dialogs.markdown_open_buffer.clear();
            state.dispatch_intent(
                Intent::OpenPopup {
                    id: "markdown_open",
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("open_markdown"),
            );
            return true;
        }
        if matches_any_binding(&kb.convert_surface, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                state.dialogs.convert_popup = Some(sid);
                state.dialogs.convert_popup_selected = None;
                state.dispatch_intent(
                    Intent::OpenPopup {
                        id: "convert_surface",
                        mode: OpenPopupMode::WithScope(
                            crate::ui::popup::PopupScope::Surface(sid),
                        ),
                    }
                    .from_user_shortcut("convert_surface"),
                );
            }
            return true;
        }
        if matches_any_binding(&kb.convert_to_markdown, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                let pane_id = state.active_workspace().focused_pane;
                state.dialogs.markdown_convert_surface_id = Some(sid);
                state.dialogs.file_open_pane_id = Some(pane_id);
                state.dialogs.markdown_open_buffer.clear();
                state.dispatch_intent(
                    Intent::OpenPopup {
                        id: "markdown_open",
                        mode: OpenPopupMode::WithScope(
                            crate::ui::popup::PopupScope::Surface(sid),
                        ),
                    }
                    .from_user_shortcut("convert_to_markdown"),
                );
            }
            return true;
        }
        if matches_any_binding(&kb.new_window, key, mods) {
            send_app_event(proxy, crate::AppEvent::CreateWindow);
            return true;
        }
        if matches_any_binding(&kb.close_active, key, mods) {
            if !state.close_active_tab() {
                if !state.close_active_pane() {
                    state.close_active_workspace();
                }
            }
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.next_tab, key, mods) {
            state.next_tab_in_pane();
            return true;
        }
        if matches_any_binding(&kb.prev_tab, key, mods) {
            state.prev_tab_in_pane();
            return true;
        }
        if matches_any_binding(&kb.rename_tab, key, mods) {
            let pane_id = state.active_workspace().focused_pane;
            if let Some(pane) = state
                .active_workspace()
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
                        Intent::OpenPopup {
                            id: "rename",
                            mode: OpenPopupMode::WithScope(scope),
                        }
                        .from_user_shortcut("rename_tab"),
                    );
                }
            }
            return true;
        }
        if matches_any_binding(&kb.rename_workspace, key, mods) {
            let ws_idx = state.active_workspace;
            if let Some(ws) = state.engine.workspaces.get(ws_idx) {
                let target = crate::state::RenameTarget::WorkspaceName { ws_idx };
                let scope = target.popup_scope();
                state.dialogs.rename = Some((target, ws.name.clone()));
                state.dispatch_intent(
                    Intent::OpenPopup {
                        id: "rename",
                        mode: OpenPopupMode::WithScope(scope),
                    }
                    .from_user_shortcut("rename_workspace"),
                );
            }
            return true;
        }
        if matches_any_binding(&kb.image_undo, key, mods) {
            if state.focused_surface_type().is_kind("image") {
                if let Some(sid) = focused_image_surface_id(state) {
                    if let Some(view) = state.image_views.get_mut(sid) {
                        view.undo();
                    }
                }
                return true;
            }
        }
        if matches_any_binding(&kb.image_redo, key, mods) {
            if state.focused_surface_type().is_kind("image") {
                if let Some(sid) = focused_image_surface_id(state) {
                    if let Some(view) = state.image_views.get_mut(sid) {
                        view.redo();
                    }
                }
                return true;
            }
        }
        if matches_any_binding(&kb.toggle_command_palette, key, mods) {
            state.command_palette.reset();
            state.dispatch_intent(
                Intent::TogglePopup {
                    id: crate::ui::command_palette_popup::COMMAND_PALETTE_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("toggle_command_palette"),
            );
            return true;
        }
        if matches_any_binding(&kb.apply_workspace_preset, key, mods) {
            state.dialogs.preset_picker_selected = None;
            state.dispatch_intent(
                Intent::OpenPopup {
                    id: crate::ui::preset_apply_popup::APPLY_WORKSPACE_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("apply_workspace_preset"),
            );
            return true;
        }
        if matches_any_binding(&kb.apply_tab_preset, key, mods) {
            state.dialogs.preset_picker_selected = None;
            state.dispatch_intent(
                Intent::OpenPopup {
                    id: crate::ui::preset_apply_popup::APPLY_TAB_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("apply_tab_preset"),
            );
            return true;
        }
        if matches_any_binding(&kb.apply_pane_preset, key, mods) {
            state.dialogs.preset_picker_selected = None;
            state.dispatch_intent(
                Intent::OpenPopup {
                    id: crate::ui::preset_apply_popup::APPLY_PANE_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("apply_pane_preset"),
            );
            return true;
        }
        if matches_any_binding(&kb.rename_workspace_subtitle, key, mods) {
            let ws_idx = state.active_workspace;
            if let Some(ws) = state.engine.workspaces.get(ws_idx) {
                let target = crate::state::RenameTarget::WorkspaceSubtitle { ws_idx };
                let scope = target.popup_scope();
                state.dialogs.rename = Some((target, ws.subtitle.clone()));
                state.dispatch_intent(
                    Intent::OpenPopup {
                        id: "rename",
                        mode: OpenPopupMode::WithScope(scope),
                    }
                    .from_user_shortcut("rename_workspace_subtitle"),
                );
            }
            return true;
        }
        false
    }

}

