//! 설정 가능한 키바인딩 (사이드바/탭/팝업 등) 의 매칭/분기.
//!
//! 본 fn 은 `handle_shortcut` 의 메인 분기에서 호출된다. 키 + 모디파이어와
//! `KeybindingSettings` 의 각 액션 바인딩 목록을 순서대로 비교하여 첫 매칭에서
//! 액션을 수행한다.

use winit::keyboard::{Key, ModifiersState};

use crate::intent::{Intent, OpenPopupMode, UiIntent};
use crate::model::SplitDirection;
use crate::view::main::MainView;

use super::{focused_explorer_surface_id, matches_any_binding, send_app_event};

impl MainView {
    #[allow(clippy::too_many_arguments)] // reason: keybinding dispatch context
    pub(super) fn handle_keybinding_shortcuts(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
        terminal_rect: crate::model::PhysicalRect,
        cell_w: f32,
        cell_h: f32,
        proxy: &winit::event_loop::EventLoopProxy<crate::AppEvent>,
    ) -> bool {
        if matches_any_binding(&kb.new_workspace, key, mods) {
            state.dispatch_intent(
                Intent::NewWorkspace {
                    kind: None,
                    params: serde_json::Value::Null,
                    category: None,
                }
                .from_user_shortcut("new_workspace"),
            );
            return true;
        }
        if matches_any_binding(&kb.new_tab, key, mods) {
            if let Err(e) = state.add_tab(engine) {
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
            state.resize_all(engine, terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_pane_horizontal, key, mods) {
            state.dispatch_intent(
                Intent::SplitPane {
                    direction: SplitDirection::Horizontal,
                }
                .from_user_shortcut("split_pane_horizontal"),
            );
            state.resize_all(engine, terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_surface_vertical, key, mods) {
            state.dispatch_intent(
                Intent::SplitSurface {
                    direction: SplitDirection::Vertical,
                }
                .from_user_shortcut("split_surface_vertical"),
            );
            state.resize_all(engine, terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_surface_horizontal, key, mods) {
            state.dispatch_intent(
                Intent::SplitSurface {
                    direction: SplitDirection::Horizontal,
                }
                .from_user_shortcut("split_surface_horizontal"),
            );
            state.resize_all(engine, terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.toggle_settings, key, mods) {
            send_app_event(proxy, crate::AppEvent::OpenSettings);
            return true;
        }
        if matches_any_binding(&kb.toggle_notifications, key, mods) {
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
            return true;
        }
        if matches_any_binding(&kb.find, key, mods) {
            // 이 winit 경로는 검색창이 포커스되지 않은 상태(터미널 포커스)에서만 도달한다
            // — 검색창 포커스 상태의 find 는 overlay 게이트에 막혀 egui 경로(search_bar)
            //   가 처리한다. 따라서 여기서는 항상 "검색창으로 포커스 이동"이다.
            if state.popups.is_open("search_bar") {
                // 이미 떠 있으면 닫지 않고 포커스만 검색창으로 옮긴다.
                state.popups.set_focused("search_bar", true);
            } else if let Some(sid) = state.focused_surface_id(engine) {
                state.search.surface_id = sid;
                state.dispatch_intent(
                    UiIntent::OpenPopup {
                        id: "search_bar",
                        mode: OpenPopupMode::AtTopOfScope(
                            crate::adapters::ui::popup::PopupScope::Surface(sid),
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
            state.close_active_workspace(engine);
            if !engine.workspaces.is_empty() {
                state.resize_all(engine, terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.close_pane, key, mods) {
            if !state.close_active_pane(engine) {
                state.close_active_workspace(engine);
            }
            if !engine.workspaces.is_empty() {
                state.resize_all(engine, terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.close_surface, key, mods) {
            let closed = state.close_active_surface(engine);
            if !closed && !state.close_active_pane(engine) {
                state.close_active_workspace(engine);
            }
            if !engine.workspaces.is_empty() {
                state.resize_all(engine, terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.focus_pane_next, key, mods) {
            state.move_pane_focus_forward(engine);
            return true;
        }
        if matches_any_binding(&kb.focus_pane_prev, key, mods) {
            state.move_pane_focus_backward(engine);
            return true;
        }
        if matches_any_binding(&kb.focus_surface_next, key, mods) {
            state.move_surface_focus_forward(engine);
            return true;
        }
        if matches_any_binding(&kb.focus_surface_prev, key, mods) {
            state.move_surface_focus_backward(engine);
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
        // 카테고리 토글이 켜져 있을 때만 매칭·consume — 꺼져 있으면 키를 다른 binding 으로
        // 흘려보낸다(표시=동작: 비활성 기능은 단축키도 비활성).
        if engine.settings.general.workspace_categories_enabled
            && matches_any_binding(&kb.toggle_categories_collapsed, key, mods)
        {
            engine.toggle_all_categories_collapsed();
            engine.mark_layout_dirty();
            return true;
        }
        if matches_any_binding(&kb.restore_closed, key, mods) {
            state.dispatch_intent(
                crate::intent::Intent::RestoreClosedItem.from_user_shortcut("restore_closed"),
            );
            state.resize_all(engine, terminal_rect, cell_w, cell_h);
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
            return true;
        }
        if matches_any_binding(&kb.convert_surface, key, mods) {
            if let Some(sid) = state.focused_surface_id(engine) {
                state.dialogs.convert_popup = Some(sid);
                state.dialogs.convert_popup_selected = None;
                state.dispatch_intent(
                    UiIntent::OpenPopup {
                        id: "convert_surface",
                        mode: OpenPopupMode::WithScope(
                            crate::adapters::ui::popup::PopupScope::Surface(sid),
                        ),
                    }
                    .from_user_shortcut("convert_surface"),
                );
            }
            return true;
        }
        if matches_any_binding(&kb.convert_to_markdown, key, mods) {
            if let Some(sid) = state.focused_surface_id(engine) {
                let pane_id = state.active_workspace(engine).focused_pane;
                state.dialogs.markdown_convert_surface_id = Some(sid);
                state.dialogs.file_open_pane_id = Some(pane_id);
                state.dialogs.markdown_open_buffer.clear();
                state.dispatch_intent(
                    UiIntent::OpenPopup {
                        id: "markdown_open",
                        mode: OpenPopupMode::WithScope(
                            crate::adapters::ui::popup::PopupScope::Surface(sid),
                        ),
                    }
                    .from_user_shortcut("convert_to_markdown"),
                );
            }
            return true;
        }
        if matches_any_binding(&kb.convert_to_explorer, key, mods) {
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
            return true;
        }
        if matches_any_binding(&kb.new_window, key, mods) {
            send_app_event(proxy, crate::AppEvent::CreateWindow);
            return true;
        }
        if matches_any_binding(&kb.close_active, key, mods) {
            if !state.close_active_tab(engine) && !state.close_active_pane(engine) {
                state.close_active_workspace(engine);
            }
            if !engine.workspaces.is_empty() {
                state.resize_all(engine, terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.next_tab, key, mods) {
            state.next_tab_in_pane(engine);
            return true;
        }
        if matches_any_binding(&kb.prev_tab, key, mods) {
            state.prev_tab_in_pane(engine);
            return true;
        }
        if matches_any_binding(&kb.rename_tab, key, mods) {
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
            return true;
        }
        if matches_any_binding(&kb.rename_workspace, key, mods) {
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
            return true;
        }
        if matches_any_binding(&kb.explorer_refresh, key, mods)
            && state.focused_surface_type(engine).is_kind("explorer")
        {
            if let Some(sid) = focused_explorer_surface_id(state, engine) {
                crate::adapters::ui::egui_panels::apply_explorer_action(
                    state,
                    engine,
                    sid,
                    crate::explorer_ui::ExplorerAction::Refresh,
                );
            }
            return true;
        }
        if matches_any_binding(&kb.explorer_go_up, key, mods)
            && state.focused_surface_type(engine).is_kind("explorer")
        {
            if let Some(sid) = focused_explorer_surface_id(state, engine) {
                crate::adapters::ui::egui_panels::apply_explorer_action(
                    state,
                    engine,
                    sid,
                    crate::explorer_ui::ExplorerAction::GoUp,
                );
                // 경로 변경은 ExplorerView 가 다음 draw 에서 자동 감지해 reload 한다
                // (toolbar GoUp 버튼과 동일 경로).
            }
            return true;
        }
        if matches_any_binding(&kb.toggle_command_palette, key, mods) {
            state.command_palette.reset();
            state.dispatch_intent(
                UiIntent::TogglePopup {
                    id: crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("toggle_command_palette"),
            );
            return true;
        }
        if matches_any_binding(&kb.apply_workspace_preset, key, mods) {
            state.dialogs.preset_picker_selected = None;
            state.dispatch_intent(
                UiIntent::OpenPopup {
                    id: crate::adapters::ui::popup::preset_apply::APPLY_WORKSPACE_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("apply_workspace_preset"),
            );
            return true;
        }
        if matches_any_binding(&kb.apply_tab_preset, key, mods) {
            state.dialogs.preset_picker_selected = None;
            state.dispatch_intent(
                UiIntent::OpenPopup {
                    id: crate::adapters::ui::popup::preset_apply::APPLY_TAB_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("apply_tab_preset"),
            );
            return true;
        }
        if matches_any_binding(&kb.apply_pane_preset, key, mods) {
            state.dialogs.preset_picker_selected = None;
            state.dispatch_intent(
                UiIntent::OpenPopup {
                    id: crate::adapters::ui::popup::preset_apply::APPLY_PANE_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("apply_pane_preset"),
            );
            return true;
        }
        if matches_any_binding(&kb.enter_copy_mode, key, mods) {
            state.dialogs.pending_enter_copy_mode = true;
            return true;
        }
        if matches_any_binding(&kb.rename_workspace_subtitle, key, mods) {
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
            return true;
        }
        false
    }
}
