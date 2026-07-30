//! 설정 가능한 키바인딩 (사이드바/탭/팝업 등) 의 매칭/분기.
//!
//! 본 fn 은 `handle_shortcut` 의 메인 분기에서 호출된다. 키 + 모디파이어와
//! `KeybindingSettings` 의 각 액션 바인딩 목록을 순서대로 비교하여 첫 매칭에서
//! 액션을 수행한다.
//!
//! 분기 본체는 카테고리별 그룹 매처(`match_*_bindings`)로 나뉘어 있으나 매칭
//! 순서는 원본 나열 순서를 그대로 보존한다 — 그룹은 연속 블록(contiguous run)
//! 단위이며, 본체는 그룹을 원본 순서대로 순차 호출한다(먼저 매칭되는 binding 이
//! 이긴다는 우선순위 규칙 불변).

use winit::keyboard::{Key, ModifiersState};

use crate::intent::{Intent, OpenPopupMode, UiIntent};
use crate::model::SplitDirection;
use crate::view::main::MainView;

use super::{
    focused_explorer_surface_id, focused_workspace_category, matches_any_binding, send_app_event,
};

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
        // 그룹 호출 순서 = 원본 블록 나열 순서. 순서 변경 금지(단축키 우선순위 영향).
        if Self::match_create_bindings(state, engine, kb, key, mods) {
            return true;
        }
        if Self::match_split_bindings(state, engine, kb, key, mods, terminal_rect, cell_w, cell_h) {
            return true;
        }
        if Self::match_panel_bindings(state, engine, kb, key, mods, proxy) {
            return true;
        }
        if Self::match_close_bindings(state, engine, kb, key, mods, terminal_rect, cell_w, cell_h) {
            return true;
        }
        if Self::match_focus_bindings(state, engine, kb, key, mods) {
            return true;
        }
        if Self::match_sidebar_bindings(state, engine, kb, key, mods) {
            return true;
        }
        if Self::match_restore_quit_bindings(
            state,
            engine,
            kb,
            key,
            mods,
            terminal_rect,
            cell_w,
            cell_h,
            proxy,
        ) {
            return true;
        }
        if Self::match_convert_bindings(state, engine, kb, key, mods) {
            return true;
        }
        if Self::match_capture_bindings(state, engine, kb, key, mods) {
            return true;
        }
        if Self::match_window_tab_bindings(
            state,
            engine,
            kb,
            key,
            mods,
            terminal_rect,
            cell_w,
            cell_h,
            proxy,
        ) {
            return true;
        }
        if Self::match_rename_bindings(state, engine, kb, key, mods) {
            return true;
        }
        if Self::match_explorer_bindings(state, engine, kb, key, mods) {
            return true;
        }
        if Self::match_preset_bindings(state, kb, key, mods) {
            return true;
        }
        if Self::match_copy_rename_bindings(state, engine, kb, key, mods) {
            return true;
        }
        false
    }

    /// 생성 계열: new_workspace / new_tab.
    ///
    /// `pub(super)` — `focused_workspace_category` 계승 여부를 실제 dispatch 경로로
    /// 검증하는 `shortcuts::tests` 단위 테스트가 직접 호출한다(zoom/numeric 과 동일한
    /// 테스트 가시성 패턴).
    pub(super) fn match_create_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
        if matches_any_binding(&kb.new_workspace, key, mods) {
            // 현재 활성 워크스페이스의 카테고리를 계승 — 마우스 경로(레일 팝업/카테고리
            // 메뉴)와 동일하게 카테고리 인지형 생성으로 맞춘다(항상 normal 로 고정되던
            // 결함 수정).
            let category = focused_workspace_category(state, engine);
            state.dispatch_intent(
                Intent::NewWorkspace {
                    kind: None,
                    params: serde_json::Value::Null,
                    category,
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
        false
    }

    /// 분할 계열: split_pane_{vertical,horizontal} / split_surface_{vertical,horizontal}.
    #[allow(clippy::too_many_arguments)] // reason: keybinding dispatch context
    fn match_split_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
        terminal_rect: crate::model::PhysicalRect,
        cell_w: f32,
        cell_h: f32,
    ) -> bool {
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
        false
    }

    /// 패널/오버레이 토글 계열: toggle_settings / toggle_notifications / find.
    fn match_panel_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
        proxy: &winit::event_loop::EventLoopProxy<crate::AppEvent>,
    ) -> bool {
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
        false
    }

    /// 닫기 계열: close_workspace / close_pane / close_surface.
    #[allow(clippy::too_many_arguments)] // reason: keybinding dispatch context
    fn match_close_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
        terminal_rect: crate::model::PhysicalRect,
        cell_w: f32,
        cell_h: f32,
    ) -> bool {
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
        false
    }

    /// 포커스 이동 계열: focus_{pane,surface}_{next,prev}.
    fn match_focus_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
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
        false
    }

    /// 사이드바 계열: toggle_sidebar / toggle_sidebar_collapse /
    /// toggle_categories_collapsed.
    fn match_sidebar_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
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
        false
    }

    /// 복구/종료 계열: restore_closed / quit_immediate / quit_minimize / quit.
    #[allow(clippy::too_many_arguments)] // reason: keybinding dispatch context
    fn match_restore_quit_bindings(
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
        false
    }

    /// 변환 계열: open_markdown / convert_surface / convert_to_markdown /
    /// convert_to_explorer.
    fn match_convert_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
        if matches_any_binding(&kb.open_markdown, key, mods) {
            // 새 탭으로 markdown 파일 열기 — surface_id 없이 file-open 팝업을 연다
            // (plugin 이 file_handler.dispatch 로 새 탭). host 는 kind 이름을 몰라도
            // registry `convert_input_popup` 데이터로 그 kind plugin 팝업을 연다.
            state.enqueue_convert_input_popup(engine, "markdown", None);
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
                // 포커스 surface 를 제자리 markdown 변환 — surface_id 를 실어 file-open
                // 팝업을 연다(plugin 이 markdown.navigate 로 제자리 변환).
                state.enqueue_convert_input_popup(engine, "markdown", Some(sid));
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
        false
    }

    /// (03) 스크린샷→클립보드: `screenshot_to_clipboard`. 포커스된 surface 기준으로
    /// 로컬/원격(mirror) 을 **여기서** 판별해 `engine.pending_screenshot_captures`
    /// 에 push 만 한다 — 실제 OS 캡처(블로킹)는 `App::poll_screenshot_captures` 가
    /// 백그라운드 스레드에서 수행(메인 루프 무블록). 판별을 트리거 시점에 끝내 두는
    /// 이유: 캡처가 끝나기 전에 포커스가 바뀌어도 판정이 흔들리지 않게.
    fn match_capture_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
        if matches_any_binding(&kb.screenshot_to_clipboard, key, mods) {
            let mirror_ws_id = state.focused_surface_id(engine).and_then(|sid| {
                let (idx, _pane_id) = engine.find_workspace_index_for_surface(sid)?;
                let ws = engine.workspaces.get(idx)?;
                ws.mirror.then_some(ws.id)
            });
            engine.pending_screenshot_captures.push(mirror_ws_id);
            return true;
        }
        false
    }

    /// 윈도우/탭 계열: new_window / close_active / next_tab / prev_tab.
    #[allow(clippy::too_many_arguments)] // reason: keybinding dispatch context
    fn match_window_tab_bindings(
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
        false
    }

    /// 이름 변경 계열: rename_tab / rename_workspace.
    fn match_rename_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
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
        false
    }

    /// 탐색기 계열(탐색기 포커스일 때만): explorer_refresh / explorer_go_up.
    fn match_explorer_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
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
        false
    }

    /// 커맨드 팔레트/프리셋 계열: toggle_command_palette / apply_{workspace,tab,pane}_preset.
    fn match_preset_bindings(
        state: &mut crate::state::AppState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
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
            // 방어적 리셋 — 카테고리 헤더 메뉴에서 열었다가 취소한 뒤 이 단축키로
            // 재오픈해도 이전 카테고리가 누출되지 않도록 명시(원인 분석 3 참고).
            state.dialogs.preset_apply_target_category = None;
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
            state.dialogs.preset_apply_target_category = None;
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
            state.dialogs.preset_apply_target_category = None;
            state.dispatch_intent(
                UiIntent::OpenPopup {
                    id: crate::adapters::ui::popup::preset_apply::APPLY_PANE_POPUP_ID,
                    mode: OpenPopupMode::CenteredFocused,
                }
                .from_user_shortcut("apply_pane_preset"),
            );
            return true;
        }
        false
    }

    /// 복사 모드/워크스페이스 부제 이름변경 계열: enter_copy_mode /
    /// rename_workspace_subtitle.
    fn match_copy_rename_bindings(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
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
