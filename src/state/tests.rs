use super::*;
use crate::model::SplitDirection;

fn test_state() -> AppState {
    let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
    AppState::new(80, 24, waker).unwrap()
}

/// 현재 활성 워크스페이스의 모든 surface ID를 수집한다.
fn collect_surface_ids(state: &mut AppState) -> Vec<u32> {
    let mut ids = Vec::new();
    let ws = state.active_workspace_mut();
    ws.pane_layout_mut().for_each_terminal_mut(&mut |sid, _| {
        ids.push(sid);
    });
    ids
}

/// 모든 워크스페이스에 걸쳐 surface ID를 수집한다.
fn collect_all_surface_ids(state: &mut AppState) -> Vec<u32> {
    let mut ids = Vec::new();
    for ws in &mut state.engine.workspaces {
        ws.pane_layout_mut().for_each_terminal_mut(&mut |sid, _| {
            ids.push(sid);
        });
    }
    ids
}

// ---- find_terminal_by_id ----

#[test]
fn find_terminal_by_id_exists() {
    let mut state = test_state();
    let surface_ids = collect_surface_ids(&mut state);
    assert!(!surface_ids.is_empty());
    let first_id = surface_ids[0];
    assert!(state.find_terminal_by_id(first_id).is_some());
}

#[test]
fn find_terminal_by_id_nonexistent() {
    let state = test_state();
    assert!(state.find_terminal_by_id(9999).is_none());
}

#[test]
fn find_terminal_by_id_after_split() {
    let mut state = test_state();
    let original_ids = collect_surface_ids(&mut state);
    let original_id = original_ids[0];

    state.split_pane(SplitDirection::Vertical).unwrap();

    let all_ids = collect_surface_ids(&mut state);
    assert_eq!(all_ids.len(), 2);

    assert!(state.find_terminal_by_id(original_id).is_some());
    let new_id = *all_ids.iter().find(|&&id| id != original_id).unwrap();
    assert!(state.find_terminal_by_id(new_id).is_some());
}

#[test]
fn find_terminal_by_id_across_tabs() {
    let mut state = test_state();
    let original_ids = collect_surface_ids(&mut state);
    let first_id = original_ids[0];

    state.add_tab().unwrap();

    let all_ids = collect_all_surface_ids(&mut state);
    assert_eq!(all_ids.len(), 2);

    assert!(state.find_terminal_by_id(first_id).is_some());
    let second_id = *all_ids.iter().find(|&&id| id != first_id).unwrap();
    assert!(state.find_terminal_by_id(second_id).is_some());
}

// ---- focus_pane ----

#[test]
fn focus_pane_valid() {
    let mut state = test_state();
    state.split_pane(SplitDirection::Vertical).unwrap();

    let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
    assert_eq!(pane_ids.len(), 2);

    // 첫 번째 pane에 포커스
    let result = state.focus_pane(pane_ids[0]);
    assert!(result);
    assert_eq!(state.active_workspace().focused_pane, pane_ids[0]);
}

#[test]
fn focus_pane_invalid() {
    let mut state = test_state();
    let result = state.focus_pane(9999);
    assert!(!result);
}

#[test]
fn focus_pane_preserves_state() {
    let mut state = test_state();
    state.split_pane(SplitDirection::Vertical).unwrap();

    let ws_count_before = state.engine.workspaces.len();
    let tab_count_before = state.active_workspace().pane_layout().all_pane_ids().len();

    let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
    state.focus_pane(pane_ids[0]);

    assert_eq!(state.engine.workspaces.len(), ws_count_before);
    assert_eq!(
        state.active_workspace().pane_layout().all_pane_ids().len(),
        tab_count_before
    );
}

// ---- focus_surface ----

#[test]
fn focus_surface_valid() {
    let mut state = test_state();
    let surface_ids = collect_surface_ids(&mut state);
    let first_id = surface_ids[0];
    assert!(state.focus_surface(first_id));
}

#[test]
fn focus_surface_invalid() {
    let mut state = test_state();
    assert!(!state.focus_surface(9999));
}

#[test]
fn focus_surface_changes_pane_focus() {
    let mut state = test_state();

    // split 후 두 번째 pane의 surface ID를 구한다
    state.split_pane(SplitDirection::Vertical).unwrap();

    let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
    let first_pane_id = pane_ids[0];

    // 현재 포커스는 새로 생성된 두 번째 pane에 있다 (split 후 새 pane에 포커스)
    // 첫 번째 pane의 surface를 찾아 포커스한다
    let first_pane_surface: u32 = {
        let ws = state.active_workspace_mut();
        let pane = ws.pane_layout_mut().find_pane_mut(first_pane_id).unwrap();
        let mut sid = 0u32;
        for tab in &mut pane.tabs {
            tab.panel_mut().for_each_terminal_mut(&mut |id, _| {
                sid = id;
            });
        }
        sid
    };

    assert!(state.focus_surface(first_pane_surface));
    assert_eq!(state.active_workspace().focused_pane, first_pane_id);
}

// ---- close operations ----

#[test]
fn close_active_pane_single_fails() {
    let mut state = test_state();
    assert!(!state.close_active_pane());
}

#[test]
fn close_active_pane_after_split() {
    let mut state = test_state();
    state.split_pane(SplitDirection::Vertical).unwrap();

    assert_eq!(
        state.active_workspace().pane_layout().all_pane_ids().len(),
        2
    );
    assert!(state.close_active_pane());
    assert_eq!(
        state.active_workspace().pane_layout().all_pane_ids().len(),
        1
    );
}

#[test]
fn close_active_tab_single_fails() {
    let mut state = test_state();
    assert!(!state.close_active_tab());
}

#[test]
fn close_active_tab_after_add() {
    let mut state = test_state();
    state.add_tab().unwrap();

    let pane_id = state.active_workspace().focused_pane;
    let tab_count = state
        .active_workspace()
        .pane_layout()
        .find_pane(pane_id)
        .unwrap()
        .tabs
        .len();
    assert_eq!(tab_count, 2);

    assert!(state.close_active_tab());

    let tab_count_after = state
        .active_workspace()
        .pane_layout()
        .find_pane(pane_id)
        .unwrap()
        .tabs
        .len();
    assert_eq!(tab_count_after, 1);
}

// ---- workspace operations ----

#[test]
fn add_workspace_increments_count() {
    let mut state = test_state();
    assert_eq!(state.engine.workspaces.len(), 1);
    state.add_workspace().unwrap();
    assert_eq!(state.engine.workspaces.len(), 2);
}

#[test]
fn switch_workspace_valid() {
    let mut state = test_state();
    state.add_workspace().unwrap();
    assert_eq!(state.active_workspace, 1);

    state.switch_workspace(0);
    assert_eq!(state.active_workspace, 0);
}

#[test]
fn switch_workspace_out_of_range() {
    let mut state = test_state();
    state.switch_workspace(999);
    assert_eq!(state.active_workspace, 0);
}

// ---- focus movement ----

#[test]
fn move_focus_forward_single_pane() {
    let mut state = test_state();
    let before = state.active_workspace().focused_pane;
    state.move_focus_forward();
    let after = state.active_workspace().focused_pane;
    assert_eq!(before, after);
}

#[test]
fn move_focus_forward_two_panes() {
    let mut state = test_state();
    state.split_pane(SplitDirection::Vertical).unwrap();

    let pane_ids = state.active_workspace().pane_layout().all_pane_ids();
    assert_eq!(pane_ids.len(), 2);

    // split 후 새 pane(second)에 포커스가 있다
    let initial_focus = state.active_workspace().focused_pane;

    state.move_focus_forward();
    let after_first_move = state.active_workspace().focused_pane;
    assert_ne!(after_first_move, initial_focus);

    state.move_focus_forward();
    let after_second_move = state.active_workspace().focused_pane;
    // 두 번 이동하면 원래 위치로 돌아와야 한다
    assert_eq!(after_second_move, initial_focus);
}
