use super::*;
use crate::model::SplitDirection;

// `pub(crate)`: `state::popup_close_tests`(sibling module)와
// `adapters::ui::notification::on_close_drain_tests`(on_close 훅 drain 메커니즘
// 테스트)가 동일 구성을 재사용한다 — popup close 뒷정리/훅 테스트가 여기와
// 동형의 AppState/CoreState 를 필요로 함.
pub(crate) fn test_state() -> (AppState, crate::core::CoreState) {
    let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
        std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::testing::InMemoryStorage::new(),
        ));
    test_state_with_memory(memory)
}

/// `test_state` 와 같은 구성이되 memory 백엔드를 호출자가 넘긴다. 호출자가 concrete
/// `Arc<Mutex<InMemoryStorage>>` 를 따로 들고 있으면 close 이후 mock 의 호출 이력
/// (`purge_scope_call_count`)을 직접 검사할 수 있다.
pub(crate) fn test_state_with_memory(
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
) -> (AppState, crate::core::CoreState) {
    let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
    let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
    // markdown surface kind는 com.tasty.markdown plugin 이 hello 시 rendering="webview"
    // 로 등록한다(Stage B, register_plugin_surface_kinds 의 Webview 분기와 동형) —
    // register_webview_kind(overlay 플래그) + register_remote_kind(SurfaceKindDef) 둘 다
    // 필요하다. 테스트에서는 plugin manager 를 띄우지 않으므로 직접 재현한다.
    let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(serde_json::json!({
        "kind": "markdown",
        "display_name_i18n_key": "surface.kind.markdown",
        "rendering": "webview",
        // 실제 tasty-plugin.toml 의 `[[surface_kinds.preset_fields]]` 와 동형 —
        // `derive_cwd=true` 가 있어야 markdown 파일의 부모 디렉토리가 cwd 상속
        // 시발점으로 파생된다(`PresetFieldSpec::derive_cwd`, 이 test 픽스처가 없으면
        // remote_kind::register_remote_kind 의 create 클로저가 파생할 게 없어
        // 호출자가 넘긴 일반 inherited-cwd 로 fallback 해버린다).
        "preset_fields": [{
            "id": "file",
            "label_key": "preset.field.file",
            "param_key": "file",
            "input_type": "file_path",
            "required": true,
            "derive_cwd": true,
        }],
    }))
    .expect("test SurfaceKindDecl");
    crate::core::surface_registry::webview_kind::register_webview_kind(
        "com.tasty.markdown",
        &decl.kind,
    );
    let (host_cmd_tx, _host_cmd_rx) = std::sync::mpsc::channel();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &engine.surface_registry,
        "com.tasty.markdown",
        &decl,
        host_cmd_tx,
    );
    let preset_store = std::sync::Arc::new(std::sync::Mutex::new(
        tasty_presets::PresetStore::load_default(),
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

// ---- mirror 워크스페이스 구조 op forward (UI/키보드 경로) ----
// mirror 워크스페이스의 close/new-tab 은 로컬 트리를 건드리지 않고 원격으로
// forward 돼야 한다(split 이 Core::apply→forward 로 하는 것과 동형). UI-layer
// 직접 조작 경로(close_active_*·add_tab·close_tab)는 Core::apply 를 우회하므로
// forward 를 직접 얹는다. 아래 테스트는 각 경로가 (1) 올바른 StructuralOp 를
// pending_structural_forward 에 쌓고 (2) 로컬 트리는 그대로 두는지 검증한다.

#[test]
fn mirror_close_active_surface_forwards_close_surface() {
    use crate::ipc::stream::StructuralOp;
    let (mut state, mut engine) = test_state();
    let sid = state.focused_surface_id(&engine).unwrap();
    state.active_workspace_mut(&mut engine).mirror = true;
    assert!(engine.pending_structural_forward.is_empty());

    // 폴백 체인 정지(true) + 로컬 트리 불변.
    assert!(state.close_active_surface(&mut engine));
    assert!(
        engine.find_terminal_by_id(sid).is_some(),
        "mirror close 는 로컬 surface 를 지우면 안 된다"
    );
    assert_eq!(engine.pending_structural_forward.len(), 1);
    let queued = &engine.pending_structural_forward[0];
    assert!(
        queued.user_triggered,
        "AppState 직접 호출 경로는 항상 GUI 유래(08)"
    );
    match &queued.op {
        StructuralOp::CloseSurface { surface_id } => assert_eq!(*surface_id, sid),
        other => panic!("expected CloseSurface, got {other:?}"),
    }
}

#[test]
fn mirror_close_active_pane_forwards_close_pane() {
    use crate::ipc::stream::StructuralOp;
    let (mut state, mut engine) = test_state();
    let sid = state.focused_surface_id(&engine).unwrap();
    state.active_workspace_mut(&mut engine).mirror = true;

    assert!(state.close_active_pane(&mut engine));
    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        1,
        "mirror pane close 는 로컬 pane 을 제거하면 안 된다"
    );
    // mirror 는 forward 만 하고 로컬 흔적(스냅샷 포함)을 남기지 않는다:
    // 스냅샷 캡처 블록은 `forward_mirror_structural`의 이른 return **뒤**에
    // 있으므로 mirror 경로에선 애초에 실행되지 않는다.
    assert_eq!(
        engine.closed_items.len(),
        0,
        "mirror pane close 는 closed_items 스택에 아무것도 남기면 안 된다"
    );
    match &engine.pending_structural_forward[0].op {
        StructuralOp::ClosePane { anchor_surface_id } => assert_eq!(*anchor_surface_id, sid),
        other => panic!("expected ClosePane, got {other:?}"),
    }
}

#[test]
fn mirror_close_active_tab_forwards_close_tab() {
    use crate::ipc::stream::StructuralOp;
    let (mut state, mut engine) = test_state();
    let sid = state.focused_surface_id(&engine).unwrap();
    state.active_workspace_mut(&mut engine).mirror = true;

    assert!(state.close_active_tab(&mut engine));
    assert!(
        engine.find_terminal_by_id(sid).is_some(),
        "mirror tab close 는 로컬 surface 를 지우면 안 된다"
    );
    match &engine.pending_structural_forward[0].op {
        StructuralOp::CloseTab { anchor_surface_id } => assert_eq!(*anchor_surface_id, sid),
        other => panic!("expected CloseTab, got {other:?}"),
    }
}

#[test]
fn mirror_add_tab_forwards_new_tab() {
    use crate::ipc::stream::StructuralOp;
    let (mut state, mut engine) = test_state();
    let sid = state.focused_surface_id(&engine).unwrap();
    let pane_id = state.active_workspace(&engine).focused_pane;
    let tabs_before = state
        .active_workspace(&engine)
        .pane_layout()
        .find_pane(pane_id)
        .unwrap()
        .tabs
        .len();
    state.active_workspace_mut(&mut engine).mirror = true;

    state.add_tab(&mut engine).unwrap();
    let tabs_after = state
        .active_workspace(&engine)
        .pane_layout()
        .find_pane(pane_id)
        .unwrap()
        .tabs
        .len();
    assert_eq!(
        tabs_after, tabs_before,
        "mirror add_tab 은 로컬 탭을 만들면 안 된다"
    );
    match &engine.pending_structural_forward[0].op {
        StructuralOp::NewTab {
            anchor_surface_id,
            surface_kind,
            ..
        } => {
            assert_eq!(*anchor_surface_id, sid);
            assert_eq!(surface_kind, "terminal");
        }
        other => panic!("expected NewTab, got {other:?}"),
    }
}

// ---- 09: close 시 client-only 인접 focus 후보 계산 ----

/// 09 — 같은 pane 안 탭이 2개일 때 마지막 탭을 닫으면(닫히는 탭이 마지막이므로) 이전
/// 탭의 focused surface 가 1순위 인접 후보로 담긴다.
#[test]
fn mirror_close_active_tab_computes_sibling_candidate() {
    use crate::ipc::stream::StructuralOp;
    let (mut state, mut engine) = test_state();
    let sid_first = state.focused_surface_id(&engine).unwrap();
    // 두번째 탭 추가(아직 비-mirror — 로컬 실행돼 active_tab 이 새 탭으로 이동한다).
    state.add_tab(&mut engine).unwrap();
    let sid_second = state.focused_surface_id(&engine).unwrap();
    assert_ne!(sid_first, sid_second);

    state.active_workspace_mut(&mut engine).mirror = true;
    assert!(state.close_active_tab(&mut engine));
    let queued = &engine.pending_structural_forward[0];
    assert!(queued.user_triggered);
    assert_eq!(
        queued.close_focus_candidates,
        vec![sid_first],
        "닫히는 탭이 마지막이면 이전 탭의 focused surface 가 1순위 후보"
    );
    match &queued.op {
        StructuralOp::CloseTab { anchor_surface_id } => assert_eq!(*anchor_surface_id, sid_second),
        other => panic!("expected CloseTab, got {other:?}"),
    }
}

/// 09 — split 된 tab 안에서 focus 된 surface 를 닫으면, 같은 tab 안의 형제 surface
/// 가 인접 후보로 담긴다(pane 자체가 사라지지 않으므로 tab 레벨로 안 올라간다).
#[test]
fn mirror_close_active_surface_split_computes_sibling_candidate() {
    use crate::ipc::stream::StructuralOp;
    let (mut state, mut engine) = test_state();
    let sid_a = state.focused_surface_id(&engine).unwrap();
    let pane_id = state.active_workspace(&engine).focused_pane;
    let (ws_idx, _) = engine.find_workspace_index_for_surface(sid_a).unwrap();
    let sid_b = engine.next_ids.next_surface();
    engine.workspaces[ws_idx]
        .pane_layout_mut()
        .find_pane_mut(pane_id)
        .unwrap()
        .split_surface_by_id_marker(sid_a, SplitDirection::Horizontal, sid_b)
        .unwrap();
    engine
        .terminals
        .insert(sid_b, tasty_terminal::Terminal::new_detached(80, 24));
    // split_surface_by_id_marker 는 focused_surface 를 안 건드리므로 sid_a 가 여전히
    // focus — close_active_surface 가 그 surface 를 닫는다.
    assert_eq!(state.focused_surface_id(&engine), Some(sid_a));

    state.active_workspace_mut(&mut engine).mirror = true;
    assert!(state.close_active_surface(&mut engine));
    let queued = &engine.pending_structural_forward[0];
    assert!(queued.user_triggered);
    assert_eq!(
        queued.close_focus_candidates,
        vec![sid_b],
        "split tab 안 형제 surface 가 인접 후보"
    );
    match &queued.op {
        StructuralOp::CloseSurface { surface_id } => assert_eq!(*surface_id, sid_a),
        other => panic!("expected CloseSurface, got {other:?}"),
    }
}

/// split tab 안 surface 를 실제 로컬 close 경로(mirror 아님)로 닫으면
/// closed-item 스냅샷이 남아 Ctrl+Shift+T 로 복원 가능해야 한다.
#[test]
fn close_active_surface_split_saves_closed_item_snapshot() {
    let (mut state, mut engine) = test_state();
    let sid_a = state.focused_surface_id(&engine).unwrap();
    let pane_id = state.active_workspace(&engine).focused_pane;
    let (ws_idx, _) = engine.find_workspace_index_for_surface(sid_a).unwrap();
    let sid_b = engine.next_ids.next_surface();
    engine.workspaces[ws_idx]
        .pane_layout_mut()
        .find_pane_mut(pane_id)
        .unwrap()
        .split_surface_by_id_marker(sid_a, SplitDirection::Horizontal, sid_b)
        .unwrap();
    engine
        .terminals
        .insert(sid_b, tasty_terminal::Terminal::new_detached(80, 24));
    assert_eq!(state.focused_surface_id(&engine), Some(sid_a));

    assert_eq!(engine.closed_items.len(), 0);
    assert!(state.close_active_surface(&mut engine));

    assert_eq!(
        engine.closed_items.len(),
        1,
        "split surface 를 닫으면 ClosedItem 이 하나 쌓여야 한다"
    );
    match engine.closed_items.list().next().unwrap() {
        crate::model::ClosedItem::Surface { surface, .. } => {
            assert_eq!(surface.id, sid_a);
        }
        crate::model::ClosedItem::Tab(_) => panic!("expected ClosedItem::Surface, got Tab"),
        crate::model::ClosedItem::Pane { .. } => {
            panic!("expected ClosedItem::Surface, got Pane")
        }
        crate::model::ClosedItem::Workspace { .. } => {
            panic!("expected ClosedItem::Surface, got Workspace")
        }
    }
}

/// pane 을 전용 `close_pane` 단축키(`close_active_pane`)로 닫으면 closed-item
/// 스냅샷이 남아 `restore_closed`(Ctrl+Shift+T)로 복원 가능해야 한다. 이
/// 회귀를 잡아내려면 `close_case_pane`/`close_active_pane` 어느 경로도
/// `push_closed_item` 을 호출하지 않게 되는 상황(둘 다 있었던 실제 버그) —
/// pane close 가 복원 스택에 아예 기록되지 않아 직전에 다른 걸 안 닫았으면
/// no-op, 닫았으면 엉뚱한 항목이 복원되는 상황 — 을 구체적으로 검증해야 한다.
#[test]
fn close_pane_saves_closed_item_snapshot() {
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

    assert_eq!(engine.closed_items.len(), 0);
    assert!(state.close_active_pane(&mut engine));

    assert_eq!(
        engine.closed_items.len(),
        1,
        "pane 을 닫으면 ClosedItem 이 하나 쌓여야 한다"
    );
    match engine.closed_items.list().next().unwrap() {
        crate::model::ClosedItem::Pane { pane, .. } => {
            assert_eq!(pane.tabs.len(), 1, "split 로 만든 새 pane 은 tab 1개");
        }
        crate::model::ClosedItem::Surface { .. } => {
            panic!("expected ClosedItem::Pane, got Surface")
        }
        crate::model::ClosedItem::Tab(_) => panic!("expected ClosedItem::Pane, got Tab"),
        crate::model::ClosedItem::Workspace { .. } => {
            panic!("expected ClosedItem::Pane, got Workspace")
        }
    }
}

/// 스냅샷이 남는 것만으로는 부족하다 — pane close → restore 왕복이 실제로
/// 트리에 pane 을 되살리는지까지 end-to-end 검증한다
/// (`DomainIntent::RestoreClosedItem` 을 `Core::apply` 로 디스패치). 트리
/// 재삽입 위치(`insert_pane_beside` + 캡처된 split geometry)가 합리적인지 —
/// 복원 후 다시 pane 2개가 되고, cascade 이벤트가
/// `RestoredKind::PaneIntoWorkspace` 인지 — 를 함께 확인한다.
#[test]
fn close_pane_then_restore_reinserts_pane() {
    use crate::core::builder::CoreBuilder;
    use crate::core::intent::{CoreEvent, DomainIntent, RestoredKind};

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
    // `close_active_pane`는 *focused* pane(= test_split_pane 이 방금 만든 새
    // pane)을 닫는다 — target_pane_id 는 그 뒤에 남은(=닫힘 이후 focused) pane
    // 이어야 한다. 실제 `restore_closed` 흐름(`src/intent/closed_item.rs`)도
    // 복원 시점에 `state.focused_pane(engine)`을 읽으므로 close *이후* 값이다.
    assert!(state.close_active_pane(&mut engine));
    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        1,
        "pane close 직후엔 1개여야 한다"
    );
    assert_eq!(engine.closed_items.len(), 1);
    let remaining_pane_id = state.active_workspace(&engine).focused_pane;

    let mut core = CoreBuilder::new()
        .with_fs(std::sync::Arc::new(
            crate::adapters::test::mem_fs::MemFileSystem::new(),
        ))
        .with_clock(std::sync::Arc::new(
            crate::adapters::test::fake_clock::FakeClock::default(),
        ))
        .with_clipboard(std::sync::Arc::new(
            crate::adapters::test::mock_clipboard::MockClipboard::default(),
        ))
        .with_process(std::sync::Arc::new(
            crate::adapters::test::mock_process::MockProcessSpawner::default(),
        ))
        .with_home(std::sync::Arc::new(
            crate::adapters::test::tmp_home::TmpHome::new(tempfile::tempdir().expect("tmp").keep()),
        ))
        .with_sound_player(std::sync::Arc::new(
            crate::ports::notification_sound::NoopPlayer,
        ))
        .with_memory(std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::testing::InMemoryStorage::new(),
        )))
        .with_themes(std::sync::Arc::new(tasty_themes::ThemeStore::new()))
        .with_preset_store(std::sync::Arc::new(std::sync::Mutex::new(
            tasty_presets::PresetStore::load_default(),
        )))
        .with_settings_storage(std::sync::Arc::new(tasty_settings::FileSettingsStorage))
        .build()
        .expect("test Core");

    let events = core
        .apply(
            &mut engine,
            DomainIntent::RestoreClosedItem {
                target_pane_id: Some(remaining_pane_id),
            },
        )
        .expect("restore should not error");

    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        2,
        "restore 후 pane 이 다시 2개여야 한다"
    );
    let restored_kind_ok = events.iter().any(|ev| {
        matches!(
            ev,
            CoreEvent::ClosedItemRestored {
                restored: true,
                kind: RestoredKind::PaneIntoWorkspace { .. },
            }
        )
    });
    assert!(
        restored_kind_ok,
        "ClosedItemRestored{{restored:true, kind:PaneIntoWorkspace}} 이벤트가 있어야 한다: {events:?}"
    );
    assert_eq!(engine.closed_items.len(), 0, "복원 후 스택은 비어야 한다");
}

/// 09 — pane 레벨 close(`close_active_pane`)는 후보를 계산하지 않는다(로컬도
/// cascade 시 "워크스페이스 첫 pane" 으로 무조건 이동하는 것과 같은 스코프 결정).
#[test]
fn mirror_close_active_pane_has_no_focus_candidates() {
    let (mut state, mut engine) = test_state();
    state.active_workspace_mut(&mut engine).mirror = true;
    assert!(state.close_active_pane(&mut engine));
    assert!(
        engine.pending_structural_forward[0]
            .close_focus_candidates
            .is_empty()
    );
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

// ---- close_surface_by_id_inner cascade characterization (C3) ----
//
// `close_surface_by_id_inner` 의 *실제 부수효과* 를 고정한다: 닫힌 surface 의
// Terminal 이 store 에서 제거되는지(cleanup_surface 실행), 형제가 생존하는지,
// 구조(tab/pane)가 제거·재배정되는지, plugin lifecycle 큐에 close 이벤트가
// enqueue 되는지. Case4(마지막 workspace) 는 `..._recreates_when_emptied` 가
// 이미 커버하므로 Case1/2/3 만 신규 추가한다. save_snapshot=false 경로
// (`close_surface_by_id_no_snapshot`) 로 호출해 undo 스냅샷은 배제하고
// cleanup/enqueue 부수효과만 관측한다.

/// Case 1: split tab 내 다중 surface 중 하나 close → cleanup 실행(Terminal 제거),
/// 형제 surface 생존, lifecycle 이벤트 1건.
#[test]
fn c3_case1_split_surface_close_cleans_up_and_keeps_sibling() {
    let (mut state, mut engine) = test_state();
    let sid_a = collect_surface_ids(&mut state, &mut engine)[0];
    let pane_id = state.active_workspace(&engine).focused_pane;
    let (ws_idx, _) = engine.find_workspace_index_for_surface(sid_a).unwrap();
    let sid_b = engine.next_ids.next_surface();
    engine.workspaces[ws_idx]
        .pane_layout_mut()
        .find_pane_mut(pane_id)
        .unwrap()
        .split_surface_by_id_marker(sid_a, SplitDirection::Horizontal, sid_b)
        .unwrap();
    engine
        .terminals
        .insert(sid_b, tasty_terminal::Terminal::new_detached(80, 24));
    assert!(engine.terminals.contains(sid_a));

    let _ = state.take_pending_lifecycle_events();
    assert!(state.close_surface_by_id_no_snapshot(&mut engine, sid_a, false));

    assert!(
        !engine.terminals.contains(sid_a),
        "닫힌 surface 의 Terminal 이 cleanup 돼야 함"
    );
    assert!(engine.terminals.contains(sid_b), "형제 surface 는 생존");
    let events = state.take_pending_lifecycle_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].surface_id, sid_a);
}

/// Case 2: sole-surface tab & pane 에 tab >1 → tab 제거 + 해당 leaf cleanup,
/// 형제 tab 의 surface 생존.
#[test]
fn c3_case2_tab_close_removes_tab_and_cleans_surface() {
    let (mut state, mut engine) = test_state();
    let sid0 = collect_surface_ids(&mut state, &mut engine)[0];
    state.add_tab(&mut engine).unwrap();
    let pane_id = state.active_workspace(&engine).focused_pane;
    let sid1 = *collect_surface_ids(&mut state, &mut engine)
        .iter()
        .find(|&&s| s != sid0)
        .unwrap();
    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .find_pane(pane_id)
            .unwrap()
            .tabs
            .len(),
        2
    );

    let _ = state.take_pending_lifecycle_events();
    assert!(state.close_surface_by_id_no_snapshot(&mut engine, sid1, false));

    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .find_pane(pane_id)
            .unwrap()
            .tabs
            .len(),
        1
    );
    assert!(
        !engine.terminals.contains(sid1),
        "닫힌 tab 의 surface 가 cleanup 돼야 함"
    );
    assert!(
        engine.terminals.contains(sid0),
        "형제 tab 의 surface 는 생존"
    );
    let events = state.take_pending_lifecycle_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].surface_id, sid1);
}

/// Case 3: last tab in pane & ws 에 pane >1 → pane 제거 + focused_pane 재배정 +
/// leaf cleanup, 형제 pane 의 surface 생존.
#[test]
fn c3_case3_pane_close_removes_pane_and_reassigns_focus() {
    let (mut state, mut engine) = test_state();
    let sid0 = collect_surface_ids(&mut state, &mut engine)[0];
    state
        .test_split_pane(&mut engine, SplitDirection::Vertical)
        .unwrap();
    let sid1 = *collect_surface_ids(&mut state, &mut engine)
        .iter()
        .find(|&&s| s != sid0)
        .unwrap();
    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        2
    );

    let _ = state.take_pending_lifecycle_events();
    assert!(state.close_surface_by_id_no_snapshot(&mut engine, sid1, false));

    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .all_pane_ids()
            .len(),
        1
    );
    assert!(!engine.terminals.contains(sid1));
    assert!(
        engine.terminals.contains(sid0),
        "형제 pane 의 surface 는 생존"
    );
    let focused = state.active_workspace(&engine).focused_pane;
    assert!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .find_pane(focused)
            .is_some(),
        "focused_pane 이 생존 pane 으로 재배정돼야 함"
    );
    let events = state.take_pending_lifecycle_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].surface_id, sid1);
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
        extra_env: sh
            .envs_ref()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
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
        crate::core::WorkspaceCreationParams::terminal(),
    )
    .unwrap();
    let crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } = event else {
        panic!("apply_create_workspace_inner 가 WorkspaceCreated 외 반환");
    };
    state.active_workspace = index;
}

// ---- occupancy > completion 우선순위, NeedsInput > occupancy (ADR-0040, ADR-0062) ----

/// 점유(soft) 중 surface 는 Completion 하이라이트가 억제된다: `regions_from_state` 의
/// 해당 region `kind` 가 `None`. 점유 없이 attention 만 있으면 `Some(Completion)`(대조군).
#[test]
fn occupancy_suppresses_completion_highlight() {
    use crate::adapters::ui::divider::regions_from_state;
    use crate::core::AttentionKind;
    use crate::model::{PhysicalPx, PhysicalRect};

    let (state, mut engine) = test_state();
    let sids = engine
        .workspaces
        .get(state.active_workspace)
        .unwrap()
        .all_surface_ids();
    let sid = *sids.first().expect("기본 workspace 에 surface 하나");

    let term_rect = PhysicalRect {
        x: PhysicalPx(0.0),
        y: PhysicalPx(0.0),
        width: PhysicalPx(800.0),
        height: PhysicalPx(600.0),
    };

    // 대조군: attention 만 → kind Some(Completion).
    engine.raise_attention(sid, AttentionKind::Completion);
    let regions = regions_from_state(&state, &engine, term_rect);
    assert!(
        regions
            .iter()
            .any(|r| r.kind == Some(AttentionKind::Completion)),
        "점유 없이 attention 만이면 완료 테두리가 그려져야 한다"
    );

    // soft 점유 추가 → 억제(kind None).
    engine
        .attach
        .acquire_soft(sid, /* parent */ 9999, Some("agent".into()))
        .expect("soft 점유 획득");
    let regions = regions_from_state(&state, &engine, term_rect);
    assert!(
        regions.iter().all(|r| r.kind.is_none()),
        "점유 중이면 완료 테두리를 억제해야 한다(점유 > 완료)"
    );
}

/// NeedsInput 은 점유보다 우선순위가 높아 점유 중에도 억제되지 않는다 — "지금 답하지
/// 않으면 멈춘다"는 신호를 점유(정상적으로 잡혀 작업 중)가 가리면 안 되기 때문.
#[test]
fn needs_input_not_suppressed_by_occupancy() {
    use crate::adapters::ui::divider::regions_from_state;
    use crate::core::AttentionKind;
    use crate::model::{PhysicalPx, PhysicalRect};

    let (state, mut engine) = test_state();
    let sids = engine
        .workspaces
        .get(state.active_workspace)
        .unwrap()
        .all_surface_ids();
    let sid = *sids.first().expect("기본 workspace 에 surface 하나");

    let term_rect = PhysicalRect {
        x: PhysicalPx(0.0),
        y: PhysicalPx(0.0),
        width: PhysicalPx(800.0),
        height: PhysicalPx(600.0),
    };

    engine
        .attach
        .acquire_soft(sid, /* parent */ 9999, Some("agent".into()))
        .expect("soft 점유 획득");
    engine.raise_attention(sid, AttentionKind::NeedsInput);
    let regions = regions_from_state(&state, &engine, term_rect);
    assert!(
        regions
            .iter()
            .any(|r| r.kind == Some(AttentionKind::NeedsInput)),
        "점유 중이어도 NeedsInput 테두리는 억제되지 않아야 한다(NeedsInput > 점유)"
    );
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

// ---- next/prev workspace within active category ----

/// 단일 normal 카테고리에 워크스페이스 3개(A=0, B=1, C=2)일 때 next 는
/// A→B→C→A wrap, prev 는 역순으로 순환한다.
#[test]
fn next_prev_workspace_single_category_wraps() {
    let (mut state, mut engine) = test_state();
    // test_state 는 A(0) 하나로 시작 → B(1), C(2) 추가.
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    assert_eq!(engine.workspaces.len(), 3);

    state.switch_workspace(&mut engine, 0); // active = A
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 1); // B
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 2); // C
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 0); // wrap → A

    state.prev_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 2); // wrap → C
    state.prev_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 1); // B
}

/// normal 카테고리에 A(0)/C(2), work 카테고리에 B(1)/D(3) 일 때
/// 이동은 같은 카테고리 안에서만 wrap 하고 다른 카테고리 항목은 건너뛴다.
#[test]
fn next_workspace_in_active_category_wraps_within_category_only() {
    let (mut state, mut engine) = test_state();
    // A(0) 는 test_state 기본. B(1), C(2), D(3) 추가.
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    add_test_workspace(&mut state, &mut engine); // D=3
    assert_eq!(engine.workspaces.len(), 4);

    // add_test_workspace 는 카테고리를 안 붙이므로 work 카테고리를 만들어 B/D 재배정.
    let work = engine
        .create_category("work")
        .expect("create work category");
    engine.workspaces[1].set_category(work); // B
    engine.workspaces[3].set_category(work); // D

    // active = A (normal 카테고리): A(0) ↔ C(2) 사이에서만 순환.
    state.switch_workspace(&mut engine, 0);
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 2); // C (B=1 건너뜀)
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 0); // wrap → A
    state.prev_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 2); // wrap → C

    // active = B (work 카테고리): B(1) ↔ D(3) 사이에서만 순환.
    state.switch_workspace(&mut engine, 1);
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 3); // D (C=2 건너뜀)
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 1); // wrap → B
    state.prev_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 3); // wrap → D
}

// ---- workspace_switch_crosses_category (workspace 축 next/prev 의 카테고리 경계 넘기) ----

/// normal=A(0)/B(1), work=C(2)/D(3). 옵션 off(기본)면 카테고리 경계에서 로컬 wrap 만
/// 유지한다(회귀 없음).
#[test]
fn crosses_category_off_keeps_local_wrap() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    add_test_workspace(&mut state, &mut engine); // D=3
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[2].set_category(work); // C
    engine.workspaces[3].set_category(work); // D
    assert!(!engine.settings.general.workspace_switch_crosses_category);

    state.switch_workspace(&mut engine, 1); // active = B (normal 의 마지막)
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 0); // wrap → A (normal 의 첫), work 로 넘어가지 않음
}

/// 옵션 on 이면 카테고리 마지막 워크스페이스에서 next 가 다음 카테고리의 **첫**
/// 워크스페이스로 이동한다(last-active 착지 아님).
#[test]
fn crosses_category_on_next_lands_on_next_category_first() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    add_test_workspace(&mut state, &mut engine); // D=3
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[2].set_category(work); // C (work 의 first)
    engine.workspaces[3].set_category(work); // D

    // work 를 D(3) 로 마지막 방문해둬도 착지는 항상 first(C) 여야 한다(방향성 유지).
    state.switch_workspace(&mut engine, 3);
    state.switch_workspace(&mut engine, 1); // active = B (normal 의 마지막)
    engine.settings.general.workspace_switch_crosses_category = true;

    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 2); // work 의 first = C (D 의 last-active 아님)
}

/// 옵션 on 이면 카테고리 첫 워크스페이스에서 prev 가 이전 카테고리의 **마지막**
/// 워크스페이스로 이동한다.
#[test]
fn crosses_category_on_prev_lands_on_prev_category_last() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    add_test_workspace(&mut state, &mut engine); // D=3
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[2].set_category(work); // C
    engine.workspaces[3].set_category(work); // D (work 의 last)
    engine.settings.general.workspace_switch_crosses_category = true;

    state.switch_workspace(&mut engine, 2); // active = C (work 의 첫)
    state.prev_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 1); // normal 의 last = B
}

/// 옵션 on 이면 카테고리 목록 자체도 wrap 한다 — 마지막 카테고리의 마지막
/// 워크스페이스에서 next 는 첫 카테고리의 첫 워크스페이스로 돌아온다.
#[test]
fn crosses_category_on_wraps_across_full_category_list() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    add_test_workspace(&mut state, &mut engine); // D=3
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[2].set_category(work); // C (work 의 first)
    engine.workspaces[3].set_category(work); // D (work 의 last, 마지막 카테고리)
    engine.settings.general.workspace_switch_crosses_category = true;

    state.switch_workspace(&mut engine, 3); // active = D (마지막 카테고리의 마지막)
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 0); // wrap → normal 의 first = A
}

/// 옵션 on 이어도 카테고리가 1개뿐이면(카테고리 기능 off 포함) 넘어갈 인접 카테고리가
/// 없으므로 기존 로컬 wrap 과 동일하게 동작한다.
#[test]
fn crosses_category_on_single_category_falls_back_to_local_wrap() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    assert!(!engine.settings.general.workspace_categories_enabled);
    engine.settings.general.workspace_switch_crosses_category = true;

    state.switch_workspace(&mut engine, 2); // active = C (normal 의 마지막, 유일한 카테고리)
    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 0); // wrap → A, off 일 때와 동일
}

// ---- category quick-switch (T4WS ②⑤) ----

/// normal=A(0)/C(2), work=B(1)/D(3). 카테고리 전환은 대상 카테고리의 last-active 로 착지한다.
#[test]
fn switch_to_category_lands_on_last_active() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    add_test_workspace(&mut state, &mut engine); // D=3
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[1].set_category(work); // B
    engine.workspaces[3].set_category(work); // D

    // work 안에서 D(3)를 마지막으로 방문 → last-active[work]=3.
    state.switch_workspace(&mut engine, 3);
    // normal 로 이동.
    state.switch_workspace(&mut engine, 0);
    assert_eq!(state.active_workspace, 0);

    // section 1 = work 로 카테고리 전환 → 마지막 방문 D(3) 로 착지.
    state.switch_to_category(&mut engine, 1);
    assert_eq!(state.active_workspace, 3);
}

/// 한 번도 방문 안 한 카테고리로 전환하면 그 카테고리의 first 로 착지한다.
#[test]
fn switch_to_category_falls_back_to_first_when_never_visited() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    add_test_workspace(&mut state, &mut engine); // D=3
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[1].set_category(work); // B (work 의 first)
    engine.workspaces[3].set_category(work); // D

    // work 미방문 상태에서 active=A(0). section 1 로 전환 → work first = B(1).
    state.switch_workspace(&mut engine, 0);
    state.switch_to_category(&mut engine, 1);
    assert_eq!(state.active_workspace, 1);
}

/// 접힌 카테고리로 전환하면 auto-expand(collapsed=false) 되고 그 안으로 착지한다.
#[test]
fn switch_to_category_auto_expands_collapsed() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[1].set_category(work); // B
    engine.set_category_collapsed(work, true);
    assert!(engine.categories()[1].collapsed);

    state.switch_workspace(&mut engine, 0); // active=A(normal)
    state.switch_to_category(&mut engine, 1); // → work
    assert!(!engine.categories()[1].collapsed); // auto-expand
    assert_eq!(state.active_workspace, 1); // work first = B
}

/// 존재하지 않는 섹션 인덱스는 no-op.
#[test]
fn switch_to_category_out_of_range_noop() {
    let (mut state, mut engine) = test_state();
    state.switch_workspace(&mut engine, 0);
    state.switch_to_category(&mut engine, 99);
    assert_eq!(state.active_workspace, 0);
}

// ---- category axis next/prev (S-9) ----

/// normal=A(0), work=B(1), play=C(2) — 각 카테고리에 워크스페이스 1개씩. next_category
/// 는 카테고리 리스트(0=normal, 1.., 등록 순서) 를 wrap-around 순회하며 각 카테고리의
/// (미방문이므로) first 워크스페이스로 착지한다.
#[test]
fn next_prev_category_wraps_across_categories() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    let work = engine.create_category("work").expect("create work");
    let play = engine.create_category("play").expect("create play");
    engine.workspaces[1].set_category(work); // B
    engine.workspaces[2].set_category(play); // C

    state.switch_workspace(&mut engine, 0); // active = A (normal)
    state.next_category(&mut engine);
    assert_eq!(state.active_workspace, 1); // work → B
    state.next_category(&mut engine);
    assert_eq!(state.active_workspace, 2); // play → C
    state.next_category(&mut engine);
    assert_eq!(state.active_workspace, 0); // wrap → normal → A

    state.prev_category(&mut engine);
    assert_eq!(state.active_workspace, 2); // wrap → play → C
    state.prev_category(&mut engine);
    assert_eq!(state.active_workspace, 1); // work → B
}

/// 카테고리가 normal 하나뿐이면 next/prev_category 가 no-op.
#[test]
fn next_prev_category_noop_when_single_category() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // 같은 normal 카테고리에 워크스페이스 추가.
    assert_eq!(state.active_workspace, 1);

    state.next_category(&mut engine);
    assert_eq!(state.active_workspace, 1);
    state.prev_category(&mut engine);
    assert_eq!(state.active_workspace, 1);
}

/// 카테고리 전환은 대상 카테고리의 last-active 로 착지(switch_to_category 재사용 확인).
#[test]
fn next_category_lands_on_last_active() {
    let (mut state, mut engine) = test_state();
    add_test_workspace(&mut state, &mut engine); // B=1
    add_test_workspace(&mut state, &mut engine); // C=2
    let work = engine.create_category("work").expect("create work");
    engine.workspaces[1].set_category(work); // B
    engine.workspaces[2].set_category(work); // C

    // work 안에서 C(2)를 마지막으로 방문.
    state.switch_workspace(&mut engine, 2);
    state.switch_workspace(&mut engine, 0); // normal 로 복귀.

    state.next_category(&mut engine); // → work, last-active = C.
    assert_eq!(state.active_workspace, 2);
}

/// 카테고리 내 워크스페이스가 자기 자신 하나뿐이면 next/prev 가 no-op.
#[test]
fn next_prev_workspace_in_active_category_noop_when_alone() {
    let (mut state, mut engine) = test_state();
    assert_eq!(engine.workspaces.len(), 1);
    assert_eq!(state.active_workspace, 0);

    state.next_workspace_in_active_category(&mut engine);
    assert_eq!(state.active_workspace, 0);
    state.prev_workspace_in_active_category(&mut engine);
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
    // explorer root 는 절대경로만 채택하므로 경로 리터럴도 플랫폼 절대경로로 만든다.
    let folder = crate::test_support::abs_path("proj/sub");
    let (_tab, sid) = state
        .add_kind_tab_by_owner(
            &mut engine,
            owner,
            "explorer",
            &serde_json::json!({ "path": folder.to_string_lossy() }),
        )
        .expect("add explorer tab in owner pane");
    let ex = explorer_of(&engine, &state, sid).expect("explorer surface exists");
    // 새 explorer 는 cwd=current=folder (source_cwd 는 model 단위 테스트에서 검증).
    assert_eq!(ex.cwd(), folder.as_path());
    assert_eq!(ex.current_root(), folder.as_path());
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

// ---- 19: 탭바 클릭 → 비-focused pane 으로 focus 이동 ----
//
// 2-pane 구성(pane_a=원래 pane, pane_b=split 로 새로 생긴 pane — split 직후
// focused)에서, pane_a(비-focused) 유래 탭바 액션을 `apply_tab_bar_actions` 로
// 적용했을 때 `focused_pane` 이 pane_a 로 옮겨가는지 검증한다.

/// pane 이 2개(pane_a, pane_b)인 워크스페이스를 만들고, split 직후 focus 인
/// pane_b 와 비-focused pane_a 의 ID를 반환한다.
fn two_pane_setup(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> (
    u32, /* pane_a: 비focused */
    u32, /* pane_b: focused */
) {
    let pane_a = state.active_workspace(engine).focused_pane;
    state
        .test_split_pane(engine, SplitDirection::Vertical)
        .unwrap();
    let pane_b = state.active_workspace(engine).focused_pane;
    assert_ne!(pane_a, pane_b, "split 은 새 pane 을 만들어야 한다");
    (pane_a, pane_b)
}

#[test]
fn switch_tab_on_other_pane_moves_focus() {
    use crate::adapters::ui::tab_bar::{TabBarAction, apply_tab_bar_actions};

    let (mut state, mut engine) = test_state();
    let (pane_a, pane_b) = two_pane_setup(&mut state, &mut engine);
    assert_eq!(state.active_workspace(&engine).focused_pane, pane_b);

    apply_tab_bar_actions(
        &mut state,
        &mut engine,
        vec![TabBarAction::SwitchTab {
            pane_id: pane_a,
            tab_index: 0,
        }],
        &[],
        160.0,
        1.0,
    );

    assert_eq!(
        state.active_workspace(&engine).focused_pane,
        pane_a,
        "비-focused pane 의 탭 클릭은 그 pane 으로 focus 를 옮겨야 한다"
    );
}

#[test]
fn focus_pane_action_moves_focus_without_switching_tab() {
    use crate::adapters::ui::tab_bar::{TabBarAction, apply_tab_bar_actions};

    let (mut state, mut engine) = test_state();
    let (pane_a, pane_b) = two_pane_setup(&mut state, &mut engine);

    // pane_a 에 탭을 하나 더 추가하고 두번째 탭을 활성으로 만든다(빈 영역 클릭이
    // active_tab 을 건드리지 않는지 확인하기 위한 대조군).
    state.active_workspace_mut(&mut engine).focused_pane = pane_a;
    state.add_tab(&mut engine).unwrap();
    let active_before = state
        .active_workspace(&engine)
        .pane_layout()
        .find_pane(pane_a)
        .unwrap()
        .active_tab;
    assert_eq!(active_before, 1);
    state.active_workspace_mut(&mut engine).focused_pane = pane_b;

    apply_tab_bar_actions(
        &mut state,
        &mut engine,
        vec![TabBarAction::FocusPane { pane_id: pane_a }],
        &[],
        160.0,
        1.0,
    );

    assert_eq!(
        state.active_workspace(&engine).focused_pane,
        pane_a,
        "탭바 빈 영역 클릭은 그 pane 으로 focus 를 옮겨야 한다"
    );
    let active_after = state
        .active_workspace(&engine)
        .pane_layout()
        .find_pane(pane_a)
        .unwrap()
        .active_tab;
    assert_eq!(
        active_after, active_before,
        "빈 영역 클릭은 focus 만 이동하고 active_tab 은 건드리지 않아야 한다"
    );
}

#[test]
fn scroll_left_and_right_on_other_pane_move_focus() {
    use crate::adapters::ui::tab_bar::{TabBarAction, apply_tab_bar_actions};

    for action_of in [
        (|pane_id| TabBarAction::ScrollLeft { pane_id }) as fn(u32) -> TabBarAction,
        (|pane_id| TabBarAction::ScrollRight { pane_id }) as fn(u32) -> TabBarAction,
    ] {
        let (mut state, mut engine) = test_state();
        let (pane_a, pane_b) = two_pane_setup(&mut state, &mut engine);
        assert_eq!(state.active_workspace(&engine).focused_pane, pane_b);

        apply_tab_bar_actions(
            &mut state,
            &mut engine,
            vec![action_of(pane_a)],
            &[],
            160.0,
            1.0,
        );

        assert_eq!(
            state.active_workspace(&engine).focused_pane,
            pane_a,
            "스크롤 화살표 클릭도 그 pane 으로 focus 를 옮겨야 한다"
        );
    }
}

#[test]
fn close_tab_on_other_pane_moves_focus() {
    use crate::adapters::ui::tab_bar::{TabBarAction, apply_tab_bar_actions};

    let (mut state, mut engine) = test_state();
    let (pane_a, pane_b) = two_pane_setup(&mut state, &mut engine);

    // close_tab 은 마지막 탭을 보호하므로(pane.rs::close_tab) pane_a 에 탭을 하나
    // 더 추가해 close 가 실제로 일어나게 한다 — pane 자체는 사라지지 않는다.
    state.active_workspace_mut(&mut engine).focused_pane = pane_a;
    state.add_tab(&mut engine).unwrap();
    state.active_workspace_mut(&mut engine).focused_pane = pane_b;

    apply_tab_bar_actions(
        &mut state,
        &mut engine,
        vec![TabBarAction::CloseTab {
            pane_id: pane_a,
            tab_index: 0,
        }],
        &[],
        160.0,
        1.0,
    );

    assert_eq!(
        state.active_workspace(&engine).focused_pane,
        pane_a,
        "다른 pane 의 탭을 close 해도 그 pane 으로 focus 를 옮겨야 한다"
    );
    assert_eq!(
        state
            .active_workspace(&engine)
            .pane_layout()
            .find_pane(pane_a)
            .unwrap()
            .tabs
            .len(),
        1,
        "탭이 실제로 닫혀야 한다"
    );
}

#[test]
fn context_menu_actions_do_not_move_focus() {
    use crate::adapters::ui::tab_bar::{TabBarAction, apply_tab_bar_actions};

    let (mut state, mut engine) = test_state();
    let (pane_a, pane_b) = two_pane_setup(&mut state, &mut engine);
    assert_eq!(state.active_workspace(&engine).focused_pane, pane_b);

    let actions = vec![
        TabBarAction::OpenContextMenu {
            pane_id: pane_a,
            tab_index: 0,
            pos: egui::Pos2::ZERO,
        },
        TabBarAction::OpenPaneContextMenu {
            pane_id: pane_a,
            pos: egui::Pos2::ZERO,
        },
        TabBarAction::OpenNewTabButtonContextMenu {
            pane_id: pane_a,
            pos: egui::Pos2::ZERO,
        },
    ];
    apply_tab_bar_actions(&mut state, &mut engine, actions, &[], 160.0, 1.0);

    assert_eq!(
        state.active_workspace(&engine).focused_pane,
        pane_b,
        "우클릭 컨텍스트 메뉴는 focus 를 옮기면 안 된다(대상은 pending_native_menu 의 pane_id 로 이미 결정됨)"
    );
}

// ── cleanup_surface 의 memory scope purge ──────────────────────────────────

/// `test_state_with_memory` 로 concrete mock 을 들고 AppState 를 만든다.
/// 반환한 `Arc<Mutex<InMemoryStorage>>` 로 close 이후 호출 이력을 검사한다.
fn test_state_with_mock_memory() -> (
    AppState,
    crate::core::CoreState,
    std::sync::Arc<std::sync::Mutex<tasty_memory::testing::InMemoryStorage>>,
) {
    let mock = std::sync::Arc::new(std::sync::Mutex::new(
        tasty_memory::testing::InMemoryStorage::new(),
    ));
    let (state, engine) = test_state_with_memory(mock.clone());
    (state, engine, mock)
}

#[test]
fn cleanup_surface_purges_surface_scope_exactly_once() {
    let (mut state, mut engine, mock) = test_state_with_mock_memory();
    let sid = collect_surface_ids(&mut state, &mut engine)[0];
    let scope = tasty_memory::Scope::Surface(sid);

    // seed 단계의 put 은 purge 이력에 잡히지 않는다 — 카운터는 purge_scope 전용.
    state.with_memory(|m| {
        crate::surface_meta::SurfaceMetaStore::set(m, sid, "nickname", "before-close").unwrap();
    });
    assert_eq!(
        mock.lock().unwrap().purge_scope_call_count(&scope),
        0,
        "close 전에는 purge 가 없어야 한다"
    );

    state.cleanup_surface(&mut engine, sid, None);

    // 회귀 대상: 과거엔 SurfaceMetaStore::remove 와 purge_surface_memory_scope 가
    // 같은 인자로 같은 함수를 불러 2였다. purge_scope 는 매 호출 끝에 memory 테이블
    // 풀스캔을 하므로 이 중복이 그대로 close 비용이 된다.
    // 락은 한 번만 잡아 지역으로 뽑는다 — 어서션 실패 메시지에서 다시 `mock.lock()`
    // 하면 첫 guard 가 살아 있는 채로 재진입해 std Mutex 가 데드락한다(성공 경로에선
    // 포맷 인자가 평가되지 않아 드러나지 않고, 회귀가 났을 때만 멈춘다).
    let (count, calls) = {
        let guard = mock.lock().unwrap();
        (
            guard.purge_scope_call_count(&scope),
            guard.purge_scope_calls().to_vec(),
        )
    };
    assert_eq!(
        count, 1,
        "surface close 당 purge_scope(Scope::Surface) 는 1회여야 한다 (호출 이력: {calls:?})"
    );
}

#[test]
fn cleanup_surface_still_clears_surface_scope_entries() {
    let (mut state, mut engine, _mock) = test_state_with_mock_memory();
    let sid = collect_surface_ids(&mut state, &mut engine)[0];

    state.with_memory(|m| {
        crate::surface_meta::SurfaceMetaStore::set(m, sid, "nickname", "doomed").unwrap();
        crate::surface_meta::SurfaceMetaStore::set(m, sid, "restore.command", "claude -r x")
            .unwrap();
    });
    assert_eq!(
        state
            .with_memory(|m| crate::surface_meta::SurfaceMetaStore::list(m, sid))
            .len(),
        2
    );

    state.cleanup_surface(&mut engine, sid, None);

    // 중복 제거가 "삭제를 통째로 빼먹는" 형태로 잘못 구현되지 않았는지 — 남은 1회가
    // 실제로 scope 를 비워야 한다.
    assert!(
        state
            .with_memory(|m| crate::surface_meta::SurfaceMetaStore::list(m, sid))
            .is_empty(),
        "close 후 surface scope 에 키가 남으면 안 된다"
    );
    assert_eq!(
        state.with_memory(|m| crate::surface_meta::SurfaceMetaStore::get(m, sid, "nickname")),
        None
    );
}

#[test]
fn workspace_close_purges_each_surface_scope_once() {
    let (mut state, mut engine, mock) = test_state_with_mock_memory();
    // 탭을 늘려 N-surface 워크스페이스를 만든다 — 중복이 남아 있으면 2N 이 된다.
    state.add_tab(&mut engine).expect("add_tab");
    state.add_tab(&mut engine).expect("add_tab");
    let sids = collect_surface_ids(&mut state, &mut engine);
    assert!(sids.len() >= 3, "탭 3개 이상이어야 의미 있다: {sids:?}");

    for sid in &sids {
        state.cleanup_surface(&mut engine, *sid, None);
    }

    let guard = mock.lock().unwrap();
    for sid in &sids {
        assert_eq!(
            guard.purge_scope_call_count(&tasty_memory::Scope::Surface(*sid)),
            1,
            "surface {sid} 의 purge 가 1회가 아니다"
        );
    }
    assert_eq!(
        guard.purge_scope_calls().len(),
        sids.len(),
        "surface 수만큼만 purge 해야 한다 (2N 이면 중복 회귀)"
    );
}
