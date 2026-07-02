use super::*;
use crate::model::SplitDirection;

fn test_state() -> (AppState, crate::core::CoreState) {
    let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
    let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
    // markdown surface kind는 com.tasty.markdown plugin 이 hello 시 egui-mesh
    // whitelist 로 등록한다. 테스트에서는 plugin manager 를 띄우지 않으므로 런타임과
    // 동형인 egui-mesh stand-in 등록을 직접 수행한다.
    let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(serde_json::json!({
        "kind": "markdown",
        "display_name_i18n_key": "surface.kind.markdown",
        "rendering": "egui-mesh",
    }))
    .expect("test SurfaceKindDecl");
    assert!(
        crate::engine::surface_registry::egui_mesh::register_egui_mesh_kind(
            &engine.surface_registry,
            "com.tasty.markdown",
            &decl,
            crate::plugin::manifest::HOST_API_VERSION,
        )
    );
    let preset_store = std::sync::Arc::new(std::sync::Mutex::new(
        tasty_presets::PresetStore::load_default(),
    ));
    let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
        std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::testing::InMemoryStorage::new(),
        ));
    let state = AppState::new(&mut engine, preset_store, memory);
    (state, engine)
}

/// 현재 활성 워크스페이스의 모든 surface ID를 수집한다.
fn collect_surface_ids(state: &mut AppState, engine: &mut crate::core::CoreState) -> Vec<u32> {
    let ws = state.active_workspace_mut(engine);
    let ws_ids: std::collections::HashSet<u32> = ws.all_surface_ids().into_iter().collect();
    engine
        .terminals
        .iter()
        .filter_map(|(sid, _)| ws_ids.contains(&sid).then_some(sid))
        .collect()
}

/// 모든 워크스페이스에 걸쳐 surface ID를 수집한다.
fn collect_all_surface_ids(_state: &mut AppState, engine: &mut crate::core::CoreState) -> Vec<u32> {
    engine.terminals.iter().map(|(sid, _)| sid).collect()
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
        .test_split_pane(&mut engine, SplitDirection::Vertical)
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
        .test_split_pane(&mut engine, SplitDirection::Vertical)
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

    assert!(state.close_surface_by_id_no_snapshot(&mut engine, sid, false));
    assert!(
        !engine.workspaces.is_empty(),
        "agent-initiated close must not leave the window with zero workspaces"
    );
    // 자동 재생성된 workspace 는 새 surface 를 갖는다.
    let new_surface_ids = collect_surface_ids(&mut state, &mut engine);
    assert_eq!(new_surface_ids.len(), 1);
    assert_ne!(new_surface_ids[0], sid);
}

// ---- deferred surface reify (display-point) ----

/// 포커스된 pane 에 deferred(lazy PTY) 탭을 하나 추가하고 그 surface_id 를 반환한다.
/// restore 경로가 만드는 `EmptySurface { deferred_spawn: Some(..) }` placeholder 와
/// 동등한 상태를 구성한다.
fn add_deferred_tab(state: &mut AppState, engine: &mut crate::core::CoreState) -> u32 {
    let tab_id = engine.next_ids.next_tab();
    let surface_id = engine.next_ids.next_surface();
    let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
    let waker = engine.make_waker(surface_id);
    // 복원 경로(restore.rs)가 만드는 deferred placeholder 와 동등하게 직접 구성한다.
    let spawn = crate::model::DeferredSpawn {
        shell: sh.shell_ref().map(|s| s.to_string()),
        shell_args: sh.args_ref().iter().map(|s| s.to_string()).collect(),
        cols: engine.default_cols,
        rows: engine.default_rows,
        waker,
        working_dir: None,
        restore_command: None,
        scrollback_persist_id: None,
    };
    let placeholder = crate::model::EmptySurface::new_deferred(surface_id, spawn);
    let tab =
        crate::model::Tab::new_named(tab_id, "Shell".to_string(), None, Box::new(placeholder));
    let pane = state.focused_pane_mut(engine).expect("focused pane");
    pane.tabs.push(tab);
    surface_id
}

#[test]
fn keyboard_tab_switch_reifies_deferred_surface() {
    // given: pane 에 tab0(활성, 즉시) + tab1(deferred placeholder)
    let (mut state, mut engine) = test_state();
    let sid = add_deferred_tab(&mut state, &mut engine);
    assert!(
        engine.is_surface_deferred(sid),
        "precondition: tab1 은 deferred"
    );

    // 키보드 next_tab 경로는 active_tab 만 바꾸고 reify 하지 않는다(설계상 분리).
    state.next_tab_in_pane(&mut engine);
    assert!(
        engine.is_surface_deferred(sid),
        "전환 핸들러 자체는 reify 하지 않는다(표시 지점에서 처리)"
    );

    // 표시 지점(매 프레임 렌더 직전)에서 reify 되어야 한다.
    state.reify_displayed_surfaces(&mut engine);
    assert!(
        !engine.is_surface_deferred(sid),
        "표시 즉시 reify 되어야 함"
    );
    assert!(
        engine.terminals.contains(sid),
        "PTY 가 store 에 insert 되어야 함"
    );
}

#[test]
fn close_active_tab_reifies_newly_active_deferred_surface() {
    // given: tab0(활성, 즉시) + tab1(deferred). close 시 active 가 tab1 로 이동.
    let (mut state, mut engine) = test_state();
    let sid = add_deferred_tab(&mut state, &mut engine);
    assert!(engine.is_surface_deferred(sid));

    // when: 활성 탭(tab0) close → tab1 이 새 활성 탭(deferred)
    assert!(state.close_active_tab(&mut engine));
    assert!(
        engine.is_surface_deferred(sid),
        "close 직후엔 아직 deferred"
    );

    // then: 표시 지점 reify 가 새 활성 deferred surface 를 살린다.
    state.reify_displayed_surfaces(&mut engine);
    assert!(
        !engine.is_surface_deferred(sid),
        "close 로 활성된 deferred 탭이 reify 되어야 함"
    );
    assert!(engine.terminals.contains(sid));
}

#[test]
fn reify_displayed_surfaces_is_noop_without_deferred() {
    // deferred 가 없으면 표시 지점 호출은 아무 것도 spawn 하지 않는다(이중 spawn 방지).
    let (mut state, mut engine) = test_state();
    let before = collect_all_surface_ids(&mut state, &mut engine);
    state.reify_displayed_surfaces(&mut engine);
    let after = collect_all_surface_ids(&mut state, &mut engine);
    assert_eq!(before.len(), after.len(), "deferred 없으면 no-op");
}

// ---- workspace operations ----

fn add_test_workspace(state: &mut AppState, engine: &mut crate::core::CoreState) {
    let event = crate::core::apply_create_workspace_inner(
        engine,
        None,
        "terminal".to_string(),
        serde_json::Value::Null,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } = event else {
        panic!("apply_create_workspace_inner 가 WorkspaceCreated 외 반환");
    };
    state.active_workspace = index;
}

#[test]
fn add_workspace_increments_count() {
    let (mut state, mut engine) = test_state();
    assert_eq!(engine.workspaces.len(), 1);
    add_test_workspace(&mut state, &mut engine);
    assert_eq!(engine.workspaces.len(), 2);
}

#[test]
fn switch_workspace_valid() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine);
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

// ---- resolve_inherit_cwd_from_surface ----

#[test]
fn resolve_inherit_cwd_from_markdown_surface() {
    // markdown(EguiMeshSurface) 의 source_cwd(파일 부모 디렉터리) 로 검증.
    let (mut state, mut engine) = test_state();
    #[cfg(windows)]
    let (root, file) = ("C:\\workspace\\proj", "C:\\workspace\\proj\\readme.md");
    #[cfg(not(windows))]
    let (root, file) = ("/workspace/proj", "/workspace/proj/readme.md");
    state
        .test_add_markdown_tab(&mut engine, file.to_string())
        .unwrap();

    let mut sid_opt = None;
    for ws in &engine.workspaces {
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(p) = ws.pane_layout().find_pane(pid) {
                for tab in &p.tabs {
                    if let Some(s) = tab.layout().find_surface(tab.focused_surface)
                        && s.kind() == "markdown"
                    {
                        sid_opt = s.surface_id();
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
        .test_add_markdown_tab(&mut engine, file.to_string())
        .unwrap();

    let mut sid_opt = None;
    for ws in &engine.workspaces {
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(p) = ws.pane_layout().find_pane(pid) {
                for tab in &p.tabs {
                    if let Some(s) = tab.layout().find_surface(tab.focused_surface)
                        && s.kind() == "markdown"
                    {
                        sid_opt = s.surface_id();
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

#[test]
fn surface_display_path_returns_workspace_and_tab_names() {
    let (mut state, mut engine) = test_state();
    let surface_ids = collect_surface_ids(&mut state, &mut engine);
    let sid = surface_ids[0];
    let path = engine
        .surface_display_path(sid)
        .expect("path for existing surface");
    let ws = state.active_workspace(&engine);
    assert_eq!(path.workspace_name, ws.name);
    assert!(path.tab_name.is_some());
}

#[test]
fn surface_display_path_unknown_surface_is_none() {
    let (_state, engine) = test_state();
    assert!(engine.surface_display_path(99999).is_none());
}

/// 활성 워크스페이스에서 `sid` 의 ExplorerPanel 을 찾아 반환(테스트 헬퍼).
fn explorer_of<'a>(
    engine: &'a crate::core::CoreState,
    state: &AppState,
    sid: u32,
) -> Option<&'a crate::model::ExplorerPanel> {
    let ws = state.active_workspace(engine);
    for pid in ws.pane_layout().all_pane_ids() {
        let Some(pane) = ws.pane_layout().find_pane(pid) else {
            continue;
        };
        for tab in &pane.tabs {
            if let Some(s) = tab.layout().find_surface(sid) {
                return s.as_any().downcast_ref::<crate::model::ExplorerPanel>();
            }
        }
    }
    None
}

#[test]
fn add_kind_tab_by_owner_opens_explorer_with_folder_cwd() {
    let (mut state, mut engine) = test_state();
    // 초기 pane 의 focused surface(터미널)를 owner 로 지정.
    let owner = state.focused_surface_id(&engine).expect("focused surface");
    let (_tab, sid) = state
        .add_kind_tab_by_owner(
            &mut engine,
            owner,
            "explorer",
            &serde_json::json!({ "path": "/proj/sub" }),
        )
        .expect("add explorer tab in owner pane");
    let ex = explorer_of(&engine, &state, sid).expect("explorer surface exists");
    // 새 explorer 는 cwd=current=folder (source_cwd 는 model 단위 테스트에서 검증).
    assert_eq!(ex.cwd(), std::path::Path::new("/proj/sub"));
    assert_eq!(ex.current_root(), std::path::Path::new("/proj/sub"));
}

#[test]
fn set_explorer_cwd_moves_root_and_clears_history() {
    let (mut state, mut engine) = test_state();
    let (_t, sid) = state
        .add_kind_tab(
            &mut engine,
            "explorer",
            &serde_json::json!({ "path": "/proj" }),
        )
        .expect("add explorer tab");
    state.set_explorer_cwd(&mut engine, sid, std::path::PathBuf::from("/other"));
    let ex = explorer_of(&engine, &state, sid).expect("explorer surface exists");
    assert_eq!(ex.cwd(), std::path::Path::new("/other"));
    assert_eq!(ex.current_root(), std::path::Path::new("/other"));
    assert!(!ex.active_tab().can_go_back());
}
