//! Tab bar actions → application and core state.

use super::{PaneTabBarView, TabBarAction, compute_drop_index};
use crate::state::AppState;
use egui::emath::GuiRounding as _;

/// 탭바 유래 액션을 상태에 반영한다. egui `Context` 비의존이라 단위 테스트 가능.
///
/// primary-click 계열 액션은 개별 처리 전에 그 pane 으로 `focused_pane` 을 먼저
/// 옮긴다([`TabBarAction::focus_target_pane`]) — 탭바 클릭은 그 pane 을 직접
/// 조작하는 사용자 행위이므로, 콘텐츠 영역 클릭(경로 B)과 대칭으로 focus 가 따라가는
/// 것이 일관된 동작이다.
pub fn apply_tab_bar_actions(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    actions: Vec<TabBarAction>,
    panes: &[PaneTabBarView],
    tab_w: f32,
    scale_factor: f32,
) {
    let separator_w: f32 = 1.0;

    for action in actions {
        if let Some(pane_id) = action.focus_target_pane() {
            state.active_workspace_mut(engine).focused_pane = pane_id;
        }
        match action {
            TabBarAction::SwitchTab { pane_id, tab_index } => {
                let before = state.tutorial_tab_snapshot(engine);
                let mut to_wake: Vec<u32> = Vec::new();
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.active_tab = tab_index;
                    if let Some(tab) = pane.tabs.get(tab_index) {
                        to_wake = tab.deferred_surface_ids();
                    }
                }
                for sid in to_wake {
                    engine.ensure_surface_initialized(sid);
                }
                state.observe_tutorial_tab_switch(engine, before);
            }
            TabBarAction::CloseTab { pane_id, tab_index } => {
                state.close_tab(engine, pane_id, tab_index);
            }
            TabBarAction::AddTab { pane_id: _ } => {
                if let Err(e) = state.add_tab(engine) {
                    tracing::warn!("add_tab failed: {e}");
                }
            }
            TabBarAction::RequestSplit { pane_id: _ } => {
                // 단축키(`split_pane_vertical`)와 동일 경로. focus 는 위에서 이미 대상
                // pane 으로 이동했다(cascade 가 새 pane 으로 다시 focus 이동).
                use crate::intent::Intent;
                use crate::model::SplitDirection;
                state.dispatch_intent(
                    Intent::SplitPane {
                        direction: SplitDirection::Vertical,
                    }
                    .from_user_shortcut("split_pane_vertical"),
                );
            }
            TabBarAction::OpenSearch { pane_id: _ } => {
                // 단축키(`find`)와 동일 경로 — 대상 pane 활성 surface 에 검색창을 연다.
                // focus 는 위에서 이미 대상 pane 으로 이동했다.
                open_search_for_focused_terminal(state, engine);
            }
            TabBarAction::ScrollLeft { pane_id } => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset = (pane.tab_scroll_offset - tab_w).max(0.0);
                }
            }
            TabBarAction::ScrollRight { pane_id } => {
                if let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
                {
                    pane.tab_scroll_offset += tab_w;
                }
            }
            TabBarAction::AutoScrollToActiveTab { pane_id, offset } => {
                apply_auto_scroll(state, engine, pane_id, offset);
            }
            // 빈 영역 클릭은 focus 이동이 전부다(탭 전환 없음) — 위 pre-match 에서 이미
            // 처리됐으므로 여기선 추가 작업이 없다.
            TabBarAction::FocusPane { pane_id: _ } => {}
            TabBarAction::OpenContextMenu {
                pane_id,
                tab_index,
                pos,
            } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Tab {
                    pane_id,
                    tab_index,
                    x: pos.x,
                    y: pos.y,
                });
            }
            TabBarAction::OpenPaneContextMenu { pane_id, pos } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::Pane {
                    pane_id,
                    x: pos.x,
                    y: pos.y,
                });
            }
            TabBarAction::OpenNewTabButtonContextMenu { pane_id, pos } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::NewTabButton {
                        pane_id,
                        x: pos.x,
                        y: pos.y,
                    });
            }
            TabBarAction::DragStart { pane_id, tab_index } => {
                state.dialogs.tab_drag = Some(crate::state::TabDragState {
                    pane_id,
                    tab_index,
                    current_x: 0.0,
                });
            }
            TabBarAction::DragUpdate { pane_id, mouse_x } => {
                if let Some(ref mut drag) = state.dialogs.tab_drag
                    && drag.pane_id == pane_id
                {
                    drag.current_x = mouse_x;
                }
            }
            TabBarAction::DragEnd { pane_id } => {
                apply_drag_end(
                    state,
                    engine,
                    panes,
                    tab_w,
                    scale_factor,
                    separator_w,
                    pane_id,
                );
            }
        }
    }
}

/// [`TabBarAction::OpenSearch`] 적용 — `apply_tab_bar_actions`의 cognitive complexity 를
/// 낮추기 위해 분리.
///
/// `keybinding.rs`/`dispatch.rs`의 `kb.find` 게이트와 동일한 이유로 focused surface 가
/// Terminal 일 때만 처리한다 — `search_bar` popup 은 `find_terminal_by_id` 로만 동작해
/// 다른 kind 에서는 항상 빈 0/0 오버레이가 된다. 이 버튼은 활성 탭의 kind 와 무관하게
/// 항상 렌더되므로(pane 마다 고정 노출), 단축키 경로만 고치고 이 경로를 놓치면 같은
/// 버그가 마우스 클릭으로 그대로 재현된다.
fn open_search_for_focused_terminal(state: &mut AppState, engine: &mut crate::core::CoreState) {
    use crate::adapters::ui::popup::PopupScope;
    use crate::intent::{OpenPopupMode, UiIntent};
    if !matches!(
        state.focused_surface_type(engine),
        crate::state::FocusedSurfaceType::Terminal
    ) {
        return;
    }
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

/// [`TabBarAction::AutoScrollToActiveTab`] 적용 — view 가 계산한 보정 오프셋을
/// 그대로 pane 에 반영한다. `apply_tab_bar_actions` 의 cognitive complexity 를
/// 낮추기 위해 분리.
fn apply_auto_scroll(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_id: u32,
    offset: f32,
) {
    if let Some(pane) = state
        .active_workspace_mut(engine)
        .pane_layout_mut()
        .find_pane_mut(pane_id)
    {
        pane.tab_scroll_offset = offset;
    }
}

/// [`TabBarAction::DragEnd`] 적용 — drag 중이던 탭을 실제 drop 위치로 옮긴다.
/// `apply_tab_bar_actions` 의 cognitive complexity 를 낮추기 위해 분리.
fn apply_drag_end(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    panes: &[PaneTabBarView],
    tab_w: f32,
    scale_factor: f32,
    separator_w: f32,
    pane_id: u32,
) {
    if let Some(drag) = state.dialogs.tab_drag.take()
        && drag.pane_id == pane_id
        && let Some(pane_info) = panes.iter().find(|i| i.pane_id == pane_id)
    {
        let pane_rect = pane_info.rect;
        let pane_logical_x = pane_rect.x.to_logical(scale_factor).value().round_ui();
        let pane_logical_w = pane_rect.width.to_logical(scale_factor).value().round_ui();
        let target = compute_drop_index(
            drag.current_x,
            pane_logical_x,
            pane_info.scroll_offset,
            pane_info.tab_names.len(),
            tab_w,
            separator_w,
            pane_logical_w,
        );
        // mirror 워크스페이스는 로컬 탭 순서 변경 대신 MoveTab 을 원격으로
        // forward 한다(로컬 실행은 원격 트리와 어긋남).
        if target != drag.tab_index {
            let mirror_op = engine
                .find_pane_by_id(pane_id)
                .and_then(|p| p.tabs.get(p.active_tab))
                .and_then(|t| t.focused_surface_id())
                .map(|sid| crate::ipc::stream::StructuralOp::MoveTab {
                    anchor_surface_id: sid,
                    from_index: drag.tab_index,
                    to_index: target,
                });
            if !state.forward_mirror_structural(engine, mirror_op, Vec::new())
                && let Some(pane) = state
                    .active_workspace_mut(engine)
                    .pane_layout_mut()
                    .find_pane_mut(pane_id)
            {
                pane.move_tab(drag.tab_index, target);
            }
        }
    }
}
