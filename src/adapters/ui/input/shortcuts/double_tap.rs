//! Double-tap modifier (Shift+Shift / Ctrl+Ctrl / Alt+Alt) 단축키 처리.

use crate::intent::{Intent, OpenPopupMode, UiIntent};
use crate::model::{PhysicalRect, SplitDirection};
use crate::view::main::MainView;

use super::{focused_workspace_category, send_app_event};

impl MainView {
    /// 마지막 workspace가 닫혔으면 창을 닫고, 남아 있으면 레이아웃을 다시 계산한다.
    fn finish_after_possible_close(
        &mut self,
        terminal_rect: PhysicalRect,
        cell_w: f32,
        cell_h: f32,
    ) {
        if self.core_state.workspaces.is_empty() {
            self.request_close();
        } else {
            let scale_factor = self.base.gpu.scale_factor();
            let engine = &mut self.core_state;
            self.state
                .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
        }
    }

    pub(crate) fn handle_double_tap_shortcut(
        &mut self,
        dt: crate::double_tap::DoubleTapKey,
    ) -> bool {
        let kb = self.core_state.settings.keybindings.clone();
        let dt_str = dt.binding_str();

        let has_dt = |bindings: &[String]| bindings.iter().any(|b| b == dt_str);
        if has_dt(&kb.toggle_settings) {
            send_app_event(&self.proxy, crate::AppEvent::OpenSettings);
            return true;
        }
        if has_dt(&kb.toggle_notifications) {
            // intent 적용은 다음 프레임이므로 현재 상태에서 열릴지를 판단해 읽음 처리한다.
            let will_open = !self.state.popups.is_open("notifications");
            self.state.dispatch_intent(
                UiIntent::TogglePopup {
                    id: "notifications",
                    mode: OpenPopupMode::Default,
                }
                .from_user_shortcut("toggle_notifications_double_tap"),
            );
            if will_open {
                self.state.dispatch_intent(
                    crate::core::intent::DomainIntent::MarkAllNotificationsRead
                        .from_user_shortcut("toggle_notifications_double_tap"),
                );
            }
            return true;
        }

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
            (&kb.restore_closed, "restore_closed"),
            (&kb.quit, "quit"),
            (&kb.quit_immediate, "quit_immediate"),
            (&kb.quit_minimize, "quit_minimize"),
        ];

        // 매칭뿐 아니라 실제 실행한 경우에만 입력을 소비한다.
        for (bindings, action) in &bindings_to_check {
            if has_dt(bindings) && self.run_double_tap_action(action) {
                return true;
            }
        }

        false
    }

    /// 매칭된 액션을 실행하고 처리 여부를 반환한다.
    fn run_double_tap_action(&mut self, action: &str) -> bool {
        if self.run_double_tap_layout_action(action)
            || self.run_double_tap_focus_action(action)
            || self.run_double_tap_open_action(action)
            || self.run_double_tap_app_action(action)
        {
            return true;
        }
        tracing::warn!("double-tap: registered action '{action}' has no execution arm");
        false
    }

    fn run_double_tap_layout_action(&mut self, action: &str) -> bool {
        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();
        let scale_factor = self.base.gpu.scale_factor();

        let engine = &mut self.core_state;
        match action {
            "new_workspace" => {
                let category = focused_workspace_category(&self.state, engine);
                self.state.dispatch_intent(
                    Intent::NewWorkspace {
                        kind: None,
                        params: serde_json::Value::Null,
                        category,
                    }
                    .from_user_shortcut("new_workspace"),
                );
                self.state
                    .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
            }
            "close_workspace" => {
                self.state.close_active_workspace(engine);
                self.finish_after_possible_close(terminal_rect, cell_w, cell_h);
            }
            "new_tab" => {
                if let Err(e) = self.state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
                self.state
                    .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
            }
            "close_pane" => {
                if !self.state.close_active_pane(engine) {
                    self.state.close_active_workspace(engine);
                }
                self.finish_after_possible_close(terminal_rect, cell_w, cell_h);
            }
            "split_pane_vertical" => {
                self.state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_pane_vertical"),
                );
                self.state
                    .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
            }
            "split_pane_horizontal" => {
                self.state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Horizontal,
                    }
                    .from_user_shortcut("split_pane_horizontal"),
                );
                self.state
                    .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
            }
            "split_surface_vertical" => {
                self.state.dispatch_intent(
                    Intent::SplitSurface {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_surface_vertical"),
                );
                self.state
                    .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
            }
            "split_surface_horizontal" => {
                self.state.dispatch_intent(
                    Intent::SplitSurface {
                        direction: SplitDirection::Horizontal,
                    }
                    .from_user_shortcut("split_surface_horizontal"),
                );
                self.state
                    .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
            }
            "close_surface" => {
                let closed = self.state.close_active_surface(engine);
                if !closed && !self.state.close_active_pane(engine) {
                    self.state.close_active_workspace(engine);
                }
                self.finish_after_possible_close(terminal_rect, cell_w, cell_h);
            }
            "close_active" => {
                if !self.state.close_active_tab(engine) && !self.state.close_active_pane(engine) {
                    self.state.close_active_workspace(engine);
                }
                self.finish_after_possible_close(terminal_rect, cell_w, cell_h);
            }
            "restore_closed" => {
                self.state.dispatch_intent(
                    crate::intent::Intent::RestoreClosedItem.from_user_shortcut("restore_closed"),
                );
                self.state
                    .resize_all(engine, terminal_rect, cell_w, cell_h, scale_factor);
            }
            _ => return false,
        }
        true
    }

    fn run_double_tap_focus_action(&mut self, action: &str) -> bool {
        let engine = &mut self.core_state;
        match action {
            "focus_pane_next" => {
                self.state.move_pane_focus_forward(engine);
            }
            "focus_pane_prev" => {
                self.state.move_pane_focus_backward(engine);
            }
            "focus_surface_next" => {
                self.state.move_surface_focus_forward(engine);
            }
            "focus_surface_prev" => {
                self.state.move_surface_focus_backward(engine);
            }
            "next_tab" => {
                self.state.next_tab_in_pane(engine);
            }
            "prev_tab" => {
                self.state.prev_tab_in_pane(engine);
            }
            _ => return false,
        }
        true
    }

    fn run_double_tap_open_action(&mut self, action: &str) -> bool {
        let engine = &mut self.core_state;
        match action {
            "open_markdown" => {
                self.state
                    .enqueue_convert_input_popup(engine, "markdown", None);
            }
            "open_explorer" => {
                Self::open_explorer_tab(&mut self.state);
            }
            "convert_surface" => {
                if let Some(sid) = self.state.focused_surface_id(engine) {
                    self.state.dialogs.convert_popup = Some(sid);
                    self.state.dialogs.convert_popup_selected = None;
                    self.state.dispatch_intent(
                        UiIntent::OpenPopup {
                            id: "convert_surface",
                            mode: OpenPopupMode::WithScope(
                                crate::adapters::ui::popup::PopupScope::Surface(sid),
                            ),
                        }
                        .from_user_shortcut("convert_surface_double_tap"),
                    );
                }
            }
            "convert_to_markdown" => {
                if let Some(sid) = self.state.focused_surface_id(engine) {
                    self.state
                        .enqueue_convert_input_popup(engine, "markdown", Some(sid));
                }
            }
            "convert_to_explorer" => {
                if let Some(sid) = self.state.focused_surface_id(engine) {
                    self.state.dispatch_intent(
                        crate::intent::Intent::ConvertSurface {
                            surface_id: sid,
                            target: crate::intent::ConvertTarget::Kind {
                                cwd: None,
                                kind: "explorer".to_string(),
                                params: serde_json::json!({}),
                            },
                        }
                        .from_user_shortcut("convert_to_explorer_double_tap"),
                    );
                }
            }
            _ => return false,
        }
        true
    }

    fn run_double_tap_app_action(&mut self, action: &str) -> bool {
        match action {
            "quit" => {
                send_app_event(&self.proxy, crate::AppEvent::QuitRequested);
            }
            "quit_immediate" => {
                send_app_event(&self.proxy, crate::AppEvent::Shutdown);
            }
            "quit_minimize" => {
                send_app_event(&self.proxy, crate::AppEvent::Minimize);
            }
            _ => return false,
        }
        true
    }
}
