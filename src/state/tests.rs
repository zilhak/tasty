use super::*;
use crate::model::SplitDirection;

fn test_state() -> (AppState, crate::engine_state::CoreState) {
    let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
    let mut engine = crate::engine_state::CoreState::new(80, 24, waker).unwrap();
    let preset_store = std::sync::Arc::new(std::sync::Mutex::new(
        tasty_presets::PresetStore::load_default(),
    ));
    let state = AppState::new(&mut engine, preset_store);
    (state, engine)
}

/// 현재 활성 워크스페이스의 모든 surface ID를 수집한다.
fn collect_surface_ids(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
) -> Vec<u32> {
    let mut ids = Vec::new();
    let ws = state.active_workspace_mut(engine);
    ws.pane_layout_mut().for_each_terminal_mut(&mut |sid, _| {
        ids.push(sid);
    });
    ids
}

/// 모든 워크스페이스에 걸쳐 surface ID를 수집한다.
fn collect_all_surface_ids(
    _state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
) -> Vec<u32> {
    let mut ids = Vec::new();
    for ws in &mut engine.workspaces {
        ws.pane_layout_mut().for_each_terminal_mut(&mut |sid, _| {
            ids.push(sid);
        });
    }
    ids
}

// ---- find_terminal_by_id ----

#[test]
fn find_terminal_by_id_exists() {
    let (mut state, mut engine) = test_state();
    let surface_ids = collect_surface_ids(&mut state, &mut engine);
    assert!(!surface_ids.is_empty());
    let first_id = surface_ids[0];
    assert!(engine.find_terminal_by_id(first_id).is_some());
}

#[test]
fn find_terminal_by_id_nonexistent() {
    let (_state, engine) = test_state();
    assert!(engine.find_terminal_by_id(9999).is_none());
}

#[test]
fn find_terminal_by_id_after_split() {
    let (mut state, mut engine) = test_state();
    let original_ids = collect_surface_ids(&mut state, &mut engine);
    let original_id = original_ids[0];

    state
        .split_pane(&mut engine, SplitDirection::Vertical)
        .unwrap();

    let all_ids = collect_surface_ids(&mut state, &mut engine);
    assert_eq!(all_ids.len(), 2);

    assert!(engine.find_terminal_by_id(original_id).is_some());
    let new_id = *all_ids.iter().find(|&&id| id != original_id).unwrap();
    assert!(engine.find_terminal_by_id(new_id).is_some());
}

#[test]
fn find_terminal_by_id_across_tabs() {
    let (mut state, mut engine) = test_state();
    let original_ids = collect_surface_ids(&mut state, &mut engine);
    let first_id = original_ids[0];

    state.add_tab(&mut engine).unwrap();

    let all_ids = collect_all_surface_ids(&mut state, &mut engine);
    assert_eq!(all_ids.len(), 2);

    assert!(engine.find_terminal_by_id(first_id).is_some());
    let second_id = *all_ids.iter().find(|&&id| id != first_id).unwrap();
    assert!(engine.find_terminal_by_id(second_id).is_some());
}

// ---- focus_pane ----

#[test]
fn focus_pane_valid() {
    let (mut state, mut engine) = test_state();
    state
        .split_pane(&mut engine, SplitDirection::Vertical)
        .unwrap();

    let pane_ids = state.active_workspace(&engine).pane_layout().all_pane_ids();
    assert_eq!(pane_ids.len(), 2);

    // 첫 번째 pane에 포커스
    let result = state.focus_pane(&mut engine, pane_ids[0]);
    assert!(result);
    assert_eq!(state.active_workspace(&engine).focused_pane, pane_ids[0]);
}

#[test]
fn focus_pane_invalid() {
    let (mut state, mut engine) = test_state();
    let result = state.focus_pane(&mut engine, 9999);
    assert!(!result);
}

#[test]
fn focus_pane_preserves_state() {
    let (mut state, mut engine) = test_state();
    state
        .split_pane(&mut engine, SplitDirection::Vertical)
        .unwrap();

    let ws_count_before = engine.workspaces.len();
    let tab_count_before = state
        .active_workspace(&engine)
        .pane_layout()
        .all_pane_ids()
        .len();

    let pane_ids = state.active_workspace(&engine).pane_layout().all_pane_ids();
    state.focus_pane(&mut engine, pane_ids[0]);

    assert_eq!(engine.workspaces.len(), ws_count_before);
    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        tab_count_before
    );
}

// ---- focus_surface ----

#[test]
fn focus_surface_valid() {
    let (mut state, mut engine) = test_state();
    let surface_ids = collect_surface_ids(&mut state, &mut engine);
    let first_id = surface_ids[0];
    assert!(state.focus_surface(&mut engine, first_id));
}

#[test]
fn focus_surface_invalid() {
    let (mut state, mut engine) = test_state();
    assert!(!state.focus_surface(&mut engine, 9999));
}

#[test]
fn focus_surface_changes_pane_focus() {
    let (mut state, mut engine) = test_state();

    // split 후 두 번째 pane의 surface ID를 구한다
    state
        .split_pane(&mut engine, SplitDirection::Vertical)
        .unwrap();

    let pane_ids = state.active_workspace(&engine).pane_layout().all_pane_ids();
    let first_pane_id = pane_ids[0];

    // 현재 포커스는 새로 생성된 두 번째 pane에 있다 (split 후 새 pane에 포커스)
    // 첫 번째 pane의 surface를 찾아 포커스한다
    let first_pane_surface: u32 = {
        let ws = state.active_workspace_mut(&mut engine);
        let pane = ws.pane_layout_mut().find_pane_mut(first_pane_id).unwrap();
        let mut sid = 0u32;
        for tab in &mut pane.tabs {
            tab.for_each_terminal_mut(&mut |id, _| {
                sid = id;
            });
        }
        sid
    };

    assert!(state.focus_surface(&mut engine, first_pane_surface));
    assert_eq!(state.active_workspace(&engine).focused_pane, first_pane_id);
}

// ---- close operations ----

#[test]
fn close_active_pane_single_fails() {
    let (mut state, mut engine) = test_state();
    assert!(!state.close_active_pane(&mut engine));
}

#[test]
fn close_active_pane_after_split() {
    let (mut state, mut engine) = test_state();
    state
        .split_pane(&mut engine, SplitDirection::Vertical)
        .unwrap();

    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        2
    );
    assert!(state.close_active_pane(&mut engine));
    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        1
    );
}

#[test]
fn close_active_tab_single_fails() {
    let (mut state, mut engine) = test_state();
    assert!(!state.close_active_tab(&mut engine));
}

#[test]
fn close_active_tab_after_add() {
    let (mut state, mut engine) = test_state();
    state.add_tab(&mut engine).unwrap();

    let pane_id = state.active_workspace(&engine).focused_pane;
    let tab_count = state
        .active_workspace(&engine)
        .pane_layout()
        .find_pane(pane_id)
        .unwrap()
        .tabs
        .len();
    assert_eq!(tab_count, 2);

    assert!(state.close_active_tab(&mut engine));

    let tab_count_after = state
        .active_workspace(&engine)
        .pane_layout()
        .find_pane(pane_id)
        .unwrap()
        .tabs
        .len();
    assert_eq!(tab_count_after, 1);
}

#[test]
fn close_surface_by_id_no_snapshot_recreates_when_emptied() {
    // 마지막 workspace 의 유일 surface 까지 닫아도 workspaces 가 비면 안 된다.
    // 다음 redraw 의 active_workspace() 호출 패닉을 막기 위한 invariant 회복.
    let (mut state, mut engine) = test_state();
    assert_eq!(engine.workspaces.len(), 1);
    let surface_ids = collect_surface_ids(&mut state, &mut engine);
    assert_eq!(surface_ids.len(), 1);
    let sid = surface_ids[0];

    assert!(state.close_surface_by_id_no_snapshot(&mut engine, sid));
    assert!(
        !engine.workspaces.is_empty(),
        "agent-initiated close must not leave the window with zero workspaces"
    );
    // 자동 재생성된 workspace 는 새 surface 를 갖는다.
    let new_surface_ids = collect_surface_ids(&mut state, &mut engine);
    assert_eq!(new_surface_ids.len(), 1);
    assert_ne!(new_surface_ids[0], sid);
}

// ---- workspace operations ----

#[test]
fn add_workspace_increments_count() {
    let (mut state, mut engine) = test_state();
    assert_eq!(engine.workspaces.len(), 1);
    state.add_workspace(&mut engine).unwrap();
    assert_eq!(engine.workspaces.len(), 2);
}

#[test]
fn switch_workspace_valid() {
    let (mut state, mut engine) = test_state();
    state.add_workspace(&mut engine).unwrap();
    assert_eq!(state.active_workspace, 1);

    state.switch_workspace(&mut engine, 0);
    assert_eq!(state.active_workspace, 0);
}

#[test]
fn switch_workspace_out_of_range() {
    let (mut state, mut engine) = test_state();
    state.switch_workspace(&mut engine, 999);
    assert_eq!(state.active_workspace, 0);
}

// ---- focus movement ----

#[test]
fn move_focus_forward_single_pane() {
    let (mut state, mut engine) = test_state();
    let before = state.active_workspace(&engine).focused_pane;
    state.move_focus_forward(&mut engine);
    let after = state.active_workspace(&engine).focused_pane;
    assert_eq!(before, after);
}

#[test]
fn move_focus_forward_two_panes() {
    let (mut state, mut engine) = test_state();
    state
        .split_pane(&mut engine, SplitDirection::Vertical)
        .unwrap();

    let pane_ids = state.active_workspace(&engine).pane_layout().all_pane_ids();
    assert_eq!(pane_ids.len(), 2);

    // split 후 새 pane(second)에 포커스가 있다
    let initial_focus = state.active_workspace(&engine).focused_pane;

    state.move_focus_forward(&mut engine);
    let after_first_move = state.active_workspace(&engine).focused_pane;
    assert_ne!(after_first_move, initial_focus);

    state.move_focus_forward(&mut engine);
    let after_second_move = state.active_workspace(&engine).focused_pane;
    // 두 번 이동하면 원래 위치로 돌아와야 한다
    assert_eq!(after_second_move, initial_focus);
}

// ---- resolve_inherit_cwd_from_surface ----

#[test]
fn resolve_inherit_cwd_from_markdown_surface() {
    // explorer가 plugin으로 옮겨졌으므로 host-only kind인 markdown으로 동등 검증.
    let (mut state, mut engine) = test_state();
    #[cfg(windows)]
    let (root, file) = ("C:\\workspace\\proj", "C:\\workspace\\proj\\readme.md");
    #[cfg(not(windows))]
    let (root, file) = ("/workspace/proj", "/workspace/proj/readme.md");
    state
        .add_markdown_tab(&mut engine, file.to_string())
        .unwrap();

    let mut sid_opt = None;
    for ws in &engine.workspaces {
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(p) = ws.pane_layout().find_pane(pid) {
                for tab in &p.tabs {
                    if let Some(s) = tab.layout().find_surface(tab.focused_surface) {
                        if s.kind() == "markdown" {
                            sid_opt = s.surface_id();
                        }
                    }
                }
            }
        }
    }
    let sid = sid_opt.expect("markdown surface should exist");
    assert_eq!(
        state.resolve_inherit_cwd_from_surface(&engine, sid),
        Some(std::path::PathBuf::from(root))
    );
}

#[test]
fn resolve_inherit_cwd_from_surface_respects_toggle_off() {
    let (mut state, mut engine) = test_state();
    engine.settings.general.inherit_cwd = false;

    #[cfg(windows)]
    let file = "C:\\workspace\\proj\\readme.md";
    #[cfg(not(windows))]
    let file = "/workspace/proj/readme.md";
    state
        .add_markdown_tab(&mut engine, file.to_string())
        .unwrap();

    let mut sid_opt = None;
    for ws in &engine.workspaces {
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(p) = ws.pane_layout().find_pane(pid) {
                for tab in &p.tabs {
                    if let Some(s) = tab.layout().find_surface(tab.focused_surface) {
                        if s.kind() == "markdown" {
                            sid_opt = s.surface_id();
                        }
                    }
                }
            }
        }
    }
    let sid = sid_opt.expect("markdown surface should exist");
    assert_eq!(state.resolve_inherit_cwd_from_surface(&engine, sid), None);
}

#[test]
fn resolve_inherit_cwd_from_unknown_surface_is_none() {
    let (state, engine) = test_state();
    let _ = &engine;
    assert_eq!(state.resolve_inherit_cwd_from_surface(&engine, 99999), None);
}
