//! Double-tap modifier (Shift+Shift / Ctrl+Ctrl / Alt+Alt) 단축키 처리.

use crate::intent::{Intent, OpenPopupMode};
use crate::model::SplitDirection;
use crate::window::main::MainWindow;

use super::send_app_event;

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
}
