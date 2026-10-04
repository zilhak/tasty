use super::*;

pub(super) fn supply_ids(engine: &crate::runtime::engine_access::EngineMut<'_>) {
    engine
        .runtime
        .ids
        .supply(
            [
                tasty_core::IdKind::Workspace,
                tasty_core::IdKind::Pane,
                tasty_core::IdKind::Tab,
                tasty_core::IdKind::Surface,
            ]
            .map(|kind| tasty_event_store::IdRange {
                kind: kind.label().into(),
                start: 20000,
                end: 30000,
            })
            .to_vec(),
        )
        .unwrap();
}

pub(super) fn test_ids() -> crate::runtime::id_reservations::ReservedIds {
    let bank = crate::runtime::id_reservations::IdReservations::default();
    let kinds = [
        tasty_core::IdKind::Workspace,
        tasty_core::IdKind::Pane,
        tasty_core::IdKind::Tab,
        tasty_core::IdKind::Surface,
    ];
    bank.supply(
        kinds
            .iter()
            .map(|kind| tasty_event_store::IdRange {
                kind: kind.label().into(),
                start: 10000,
                end: 20000,
            })
            .collect(),
    )
    .unwrap();
    bank.lease(&kinds.map(|kind| (kind, 10000))).unwrap()
}

use crate::app::engine_registry::EngineRegistry;

use crate::runtime::engine_session::EngineSession;

/// 순수 함수 시험용 parked 항목. registry 없이 id·View 복원 자료·engine만 묶는다.
struct ParkedEngine {
    view_restore: crate::state::MainViewState,
    session: EngineSession,
}

impl ParkedEngine {
    fn from_test_state(
        (view_restore, core_state): (crate::state::MainViewState, EngineSession),
    ) -> Self {
        Self {
            view_restore,
            session: core_state,
        }
    }
}

#[test]
fn an_agent_close_is_forwarded_with_the_agent_origin() {
    let origin_on_wire = |user_triggered| {
        let payload = structural_op_payload(
            3,
            tasty_ipc::stream::StructuralOp::CloseSurface { surface_id: 9 },
            user_triggered,
        );
        let v: Value = serde_json::from_slice(&payload).expect("json");
        assert_eq!(v["event"], "structural_op");
        v["origin"].clone()
    };
    assert_eq!(origin_on_wire(false), "agent");
    assert_eq!(origin_on_wire(true), "user");
}
use crate::ipc::stream::SplitAxis;

/// 공용 정리 본문을 검사한다. 두 호출 경로의 연결 여부까지 검증하는 시험은 아니다.
#[test]
fn remove_mirror_workspace_clears_terminal_busy_and_mesh() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let ws_id = 9_000u32;
    let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
    let remote_surface = 42u32;

    let mut mirror_ws = Workspace::new_with_terminal_marker(
        ws_id,
        "mirror".to_string(),
        pane_id,
        tab_id,
        local_surface,
    );
    mirror_ws.mirror = true;
    engine.push_mirror_workspace(mirror_ws);
    engine
        .runtime
        .terminals
        .insert(local_surface, Terminal::new_detached(80, 24), None);
    engine.set_mirror_surface_busy(local_surface, true);
    engine.set_mirror_surface_cwd(local_surface, Some("/srv/remote".to_string()));
    engine
        .remote
        .attach_mesh_frames
        .update(local_surface, vec![1, 2, 3], 0, 0, true);
    state.set_active_workspace_index(&engine, engine.workspaces().len() - 1);

    let remote_to_local = HashMap::from([(remote_surface, local_surface)]);
    assert!(remove_mirror_workspace_from_engine(
        &mut engine,
        &mut state,
        ws_id,
        &remote_to_local
    ));

    assert!(!engine.has_workspace(ws_id), "mirror 워크스페이스 행 제거");
    assert!(
        !engine.runtime.terminals.contains(local_surface),
        "mirror 터미널 제거"
    );
    assert!(
        !engine.is_surface_busy(local_surface),
        "mirror busy 엔트리 제거"
    );
    assert!(
        engine
            .remote
            .attach_mesh_frames
            .get(local_surface)
            .is_none(),
        "mesh 프레임 캐시 제거"
    );
    assert!(
        engine.remote.mirror_surface_cwd.is_empty(),
        "mirror cwd 엔트리 제거"
    );
    assert_eq!(
        state.active_workspace_index(&engine),
        engine.workspaces().len() - 1,
        "제거로 out-of-range 가 된 active_workspace 클램프"
    );
}

#[test]
fn remove_mirror_workspace_leaves_unrelated_engine_untouched() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let local_surface = 9_003u32;
    engine
        .runtime
        .terminals
        .insert(local_surface, Terminal::new_detached(80, 24), None);
    let before = engine.workspaces().len();

    let remote_to_local = HashMap::from([(42u32, local_surface)]);
    assert!(!remove_mirror_workspace_from_engine(
        &mut engine,
        &mut state,
        9_000,
        &remote_to_local
    ));
    assert_eq!(engine.workspaces().len(), before);
    assert!(engine.runtime.terminals.contains(local_surface));
}

/// mirror가 두 번째 parked engine에 있어야 첫 항목만 검사하는 오류를 잡을 수 있다.
#[test]
fn cleanup_scans_all_parked_engines_for_the_mirror_workspace() {
    let ws_id = 9_000u32;
    let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
    let remote_to_local = HashMap::from([(42u32, local_surface)]);

    let mut reg = EngineRegistry::default();
    let ids: Vec<EngineId> = (0..2)
        .map(|_| {
            let (state, engine_session) = crate::state::tests::test_state();
            reg.park_for_test(state, engine_session)
        })
        .collect();
    let untouched_ws_count = reg.get(ids[0]).expect("engine").workspaces().len();

    let mut mirror_ws = Workspace::new_with_terminal_marker(
        ws_id,
        "mirror".to_string(),
        pane_id,
        tab_id,
        local_surface,
    );
    mirror_ws.mirror = true;
    {
        let (state, mut engine) = reg.parked_session_mut(ids[1]).expect("parked 항목");
        engine.push_mirror_workspace(mirror_ws);
        engine
            .runtime
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24), None);
        engine.set_mirror_surface_busy(local_surface, true);
        engine
            .remote
            .attach_mesh_frames
            .update(local_surface, vec![1, 2, 3], 0, 0, true);
        state.set_active_workspace_index(engine.core, engine.workspaces().len() - 1);
    }

    assert!(remove_mirror_workspace_from_parked(
        EngineScanMut::from_fields(&mut HashMap::new(), &mut reg),
        ws_id,
        &remote_to_local
    ));

    let (state, engine) = reg.parked_session_mut(ids[1]).expect("parked 항목");
    assert!(!engine.has_workspace(ws_id), "mirror 워크스페이스 행 제거");
    assert!(
        !engine.runtime.terminals.contains(local_surface),
        "mirror 터미널 제거"
    );
    assert!(
        !engine.is_surface_busy(local_surface),
        "mirror busy 엔트리 제거"
    );
    assert!(
        engine
            .remote
            .attach_mesh_frames
            .get(local_surface)
            .is_none(),
        "mesh 프레임 캐시 제거"
    );
    assert_eq!(
        state.active_workspace_index(engine.core),
        engine.workspaces().len() - 1
    );
    assert_eq!(
        reg.get(ids[0]).expect("engine").workspaces().len(),
        untouched_ws_count,
        "무관한 parked engine 은 건드리지 않는다"
    );
}

#[test]
fn cleanup_parked_scan_reports_false_when_absent() {
    let mut reg = EngineRegistry::default();
    for _ in 0..2 {
        let (state, engine_session) = crate::state::tests::test_state();
        reg.park_for_test(state, engine_session);
    }
    let remote_to_local = HashMap::from([(42u32, 9_003u32)]);
    assert!(!remove_mirror_workspace_from_parked(
        EngineScanMut::from_fields(&mut HashMap::new(), &mut reg),
        9_000,
        &remote_to_local
    ));
}

#[test]
fn bulk_chunk_frames_roundtrip_and_reassembly() {
    let transfer_id = 0xABCD_1234_5678_9F01u64;
    // 여러 프레임에 걸쳐 다시 조립되는지 확인할 크기다.
    let total = BULK_CHUNK_RAW_LEN * 2 + 777;
    let data: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();

    let frames = bulk_chunk_frames(transfer_id, &data);
    assert_eq!(frames.len(), 3, "2.5 청크 = 3 파트");

    let mut reassembled = Vec::new();
    for (expected_seq, framed) in frames.iter().enumerate() {
        assert!(framed.len() <= stream::MAX_FRAME_LEN as usize);
        let (tid, seq, part) = stream::decode_bulk_chunk(framed).expect("valid bulk chunk header");
        assert_eq!(tid, transfer_id);
        assert_eq!(seq as usize, expected_seq);
        reassembled.extend_from_slice(part);
    }
    assert_eq!(reassembled, data, "재조립 바이트가 원본과 동일");
}

#[test]
fn bulk_chunk_frames_empty_is_zero_chunks() {
    assert!(bulk_chunk_frames(1, &[]).is_empty());
}

#[test]
fn bulk_chunk_frames_exact_boundary_is_single_chunk() {
    let data = vec![7u8; BULK_CHUNK_RAW_LEN];
    let frames = bulk_chunk_frames(9, &data);
    assert_eq!(frames.len(), 1);
    let (_, seq, part) = stream::decode_bulk_chunk(&frames[0]).unwrap();
    assert_eq!(seq, 0);
    assert_eq!(part.len(), BULK_CHUNK_RAW_LEN);
}

#[test]
fn build_layout_preserves_split_and_remaps_ids() {
    let mut navigation = crate::state::navigation::NavigationState::default();

    let ids = test_ids();
    let mut map = HashMap::new();
    map.insert(100u32, 5u32); // 100 → local 5 (terminal)
    map.insert(101u32, 6u32); // 101 → local 6 (placeholder)
    let mut term = HashSet::new();
    term.insert(5u32);
    let node = serde_json::json!({
        "type": "Split",
        "direction": "vertical",
        "ratio": 0.3,
        "focus_second": true,
        "first": { "type": "Leaf", "id": 100, "kind": "terminal" },
        "second": { "type": "Leaf", "id": 101, "kind": "empty" },
    });
    let layout = build_layout(
        &mut navigation,
        &node,
        &MirrorLayoutSources {
            ids: &ids,
            map: &map,
            term: &term,
            mesh: &HashMap::new(),
            explorer: &HashMap::new(),
        },
        &mut HashMap::new(),
    )
    .expect("fixture construction")
    .expect("layout");
    match layout {
        SurfaceLayout::Split {
            direction,
            ratio,
            node_id,
            first,
            second,
        } => {
            assert_eq!(direction, SplitDirection::Vertical);
            assert!((ratio - 0.3).abs() < 1e-6);
            assert!(navigation.split_hints.get(&node_id).copied().unwrap());
            assert_eq!(first.first_surface_id(), Some(5));
            assert_eq!(second.first_surface_id(), Some(6));
            assert_eq!(first.find_surface(5).unwrap().kind(), "terminal");
            assert_ne!(second.find_surface(6).unwrap().kind(), "terminal");
        }
        _ => panic!("expected Split"),
    }
}

#[test]
fn build_layout_preserves_explorer_descriptor_from_explorer_map() {
    let mut navigation = crate::state::navigation::NavigationState::default();

    let ids = test_ids();
    let map = HashMap::from([(200u32, 9u32)]);
    let term = HashSet::new();
    let mesh = HashMap::new();
    let explorer = HashMap::from([(9u32, std::path::PathBuf::from("/remote/project"))]);
    let node = serde_json::json!({ "type": "Leaf", "id": 200, "kind": "explorer" });
    let layout = build_layout(
        &mut navigation,
        &node,
        &MirrorLayoutSources {
            ids: &ids,
            map: &map,
            term: &term,
            mesh: &mesh,
            explorer: &explorer,
        },
        &mut HashMap::new(),
    )
    .expect("fixture construction")
    .expect("layout");
    let SurfaceLayout::Leaf(surface) = layout else {
        panic!("expected Leaf");
    };
    assert_eq!(surface.kind(), "explorer");
    assert_eq!(surface.id, 9);
    assert_eq!(
        explorer[&surface.id],
        std::path::PathBuf::from("/remote/project")
    );
}

#[test]
fn build_mirror_workspace_single_pane_tab() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let mut map = HashMap::new();
    map.insert(1u32, 50u32);
    let mut term = HashSet::new();
    term.insert(50u32);
    let tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 7,
        "panes": [ {
            "id": 7,
            "tabs": [ {
                "id": 3, "name": "Shell", "active": true, "focused_surface": 1,
                "layout": { "type": "Leaf", "id": 1, "kind": "terminal" }
            } ]
        } ]
    });
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    assert_eq!(ws.id, 99);
    assert_eq!(ws.all_surface_ids(), vec![50]);
}

#[test]
fn build_mirror_workspace_preserves_survivor_and_inserts_new_leaf() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let survivor_local = 50u32;
    let mut map = HashMap::new();
    map.insert(1u32, survivor_local);
    let new_local = ids.next_surface().expect("fixture ID lease"); // 역반영이 신규에 발급하는 것과 동형.
    map.insert(2u32, new_local);
    let mut term = HashSet::new();
    term.insert(survivor_local);
    term.insert(new_local);
    let tree = serde_json::json!({
        "id": 9, "focused_pane": 7,
        "panes": [ {
            "id": 7,
            "tabs": [ {
                "id": 3, "name": "Shell", "active": true, "focused_surface": 1,
                "layout": {
                    "type": "Split", "direction": "vertical", "ratio": 0.5,
                    "focus_second": false,
                    "first": { "type": "Leaf", "id": 1, "kind": "terminal" },
                    "second": { "type": "Leaf", "id": 2, "kind": "terminal" }
                }
            } ]
        } ]
    });
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    let sids = ws.all_surface_ids();
    assert!(
        sids.contains(&survivor_local),
        "survivor local id({survivor_local}) 가 유지돼야 한다: {sids:?}"
    );
    assert!(
        sids.contains(&new_local),
        "신규 leaf local id({new_local}) 가 트리에 삽입돼야 한다: {sids:?}"
    );
    assert_eq!(sids.len(), 2, "survivor + 신규 = 2개 leaf");
}

#[test]
fn build_mirror_workspace_empty_tree_fallback() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let map = HashMap::new();
    let term = HashSet::new();
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        1,
        "remote",
        &serde_json::Value::Null,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    assert_eq!(ws.id, 1);
    assert_eq!(ws.all_surface_ids().len(), 1);
}

#[test]
fn build_mirror_workspace_preserves_vertical_pane_split() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let map = HashMap::new(); // 이 테스트는 focused_surface 매핑 불필요(pane 레벨 검증 목적)
    let term = HashSet::new();
    let tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 8,
        "panes": [],
        "pane_layout": {
            "type": "Split",
            "direction": "vertical",
            "ratio": 0.3,
            "first": { "type": "Leaf", "id": 7, "tabs": [] },
            "second": { "type": "Leaf", "id": 8, "tabs": [] }
        }
    });
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    match ws.pane_layout() {
        PaneNode::Split {
            direction,
            ratio,
            second,
            ..
        } => {
            assert_eq!(*direction, SplitDirection::Vertical);
            assert!((*ratio - 0.3).abs() < 0.001);
            if let PaneNode::Leaf(p) = second.as_ref() {
                assert_eq!(navigation.pane_id(&ws).unwrap(), p.id);
            } else {
                panic!("expected second to be Leaf");
            }
        }
        _ => panic!("expected Split, got Leaf"),
    }
}

#[test]
fn build_mirror_workspace_falls_back_to_horizontal_chain_without_pane_layout_field() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let mut map = HashMap::new();
    map.insert(1u32, 50u32);
    map.insert(2u32, 51u32);
    let mut term = HashSet::new();
    term.insert(50u32);
    term.insert(51u32);
    let tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 2,
        "panes": [
            { "id": 1, "tabs": [ { "id": 3, "name": "Shell", "active": true,
                "focused_surface": 1, "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } } ] },
            { "id": 2, "tabs": [ { "id": 4, "name": "Shell", "active": true,
                "focused_surface": 2, "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } } ] }
        ]
    });
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    match ws.pane_layout() {
        PaneNode::Split {
            direction, ratio, ..
        } => {
            assert_eq!(*direction, SplitDirection::Horizontal);
            assert!((*ratio - 0.5).abs() < 1e-6);
        }
        _ => panic!("expected Split (2 panes → horizontal chain fallback)"),
    }
}

/// pane B의 두 번째 탭에 있는 surface를 원격 ID로 되찾는다.
#[test]
fn capture_focused_remote_finds_remote_id_of_locally_focused_surface() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let mut map = HashMap::new();
    map.insert(1u32, 50u32); // pane A 의 surface
    map.insert(2u32, 51u32); // pane B, tab1 의 surface
    map.insert(3u32, 52u32); // pane B, tab2 의 surface — 사용자가 보고 있는 곳
    let mut term = HashSet::new();
    term.insert(50u32);
    term.insert(51u32);
    term.insert(52u32);
    let tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 11,
        "panes": [],
        "pane_layout": {
            "type": "Split", "direction": "horizontal", "ratio": 0.5,
            "first": { "type": "Leaf", "id": 10, "tabs": [
                { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                  "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
            ] },
            "second": { "type": "Leaf", "id": 11, "tabs": [
                { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                  "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                  "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
            ] }
        }
    });
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    assert_eq!(
        capture_focused_remote(&navigation, &ws, &map),
        Some(3),
        "focused_pane=pane B, active_tab=tab2(remote 3) 를 정확히 되짚어야 한다"
    );
}

/// 원격 포커스와 다른 로컬 포커스를 기억해 구조 재구성 뒤 복원한다.
#[test]
fn focus_restore_keeps_client_on_pane_b_after_structural_delta_from_pane_a() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();

    let mut map = HashMap::new();
    map.insert(1u32, 50u32);
    map.insert(2u32, 51u32);
    map.insert(3u32, 52u32);
    let mut term = HashSet::new();
    term.insert(50u32);
    term.insert(51u32);
    term.insert(52u32);
    let before_tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 10,
        "panes": [],
        "pane_layout": {
            "type": "Split", "direction": "horizontal", "ratio": 0.5,
            "first": { "type": "Leaf", "id": 10, "tabs": [
                { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                  "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
            ] },
            "second": { "type": "Leaf", "id": 11, "tabs": [
                { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                  "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                  "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
            ] }
        }
    });
    let mut before_ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &before_tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");

    let pane_b_surface3_local = *map.get(&3).unwrap();
    let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&before_ws, pane_b_surface3_local)
        .expect("pane B tab2 surface must exist");
    navigation.select_pane(&before_ws, pane_b_id);
    let pane_b = before_ws
        .pane_layout_mut()
        .find_pane_mut(pane_b_id)
        .expect("pane B exists");
    let tab_index = pane_b
        .tabs
        .iter()
        .position(|t| t.id == tab_id)
        .expect("tab exists");
    navigation.goto_tab(pane_b, tab_index);
    navigation.select_surface(&pane_b.tabs[tab_index], pane_b_surface3_local);

    let old_focused_remote = capture_focused_remote(&navigation, &before_ws, &map);
    assert_eq!(old_focused_remote, Some(3));

    let mut after_map = map.clone();
    after_map.insert(4u32, 53u32);
    term.insert(53u32);
    let after_tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 10,
        "panes": [],
        "pane_layout": {
            "type": "Split", "direction": "horizontal", "ratio": 0.5,
            "first": { "type": "Leaf", "id": 10, "tabs": [
                { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                  "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } },
                { "id": 101, "name": "Shell", "active": false, "focused_surface": 4,
                  "layout": { "type": "Leaf", "id": 4, "kind": "terminal" } }
            ] },
            "second": { "type": "Leaf", "id": 11, "tabs": [
                { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                  "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                  "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
            ] }
        }
    });
    let mut after_ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &after_tree,
        &ids,
        &after_map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");

    // Stable local pane/tab IDs preserve the live selection during rebuild.
    assert_eq!(navigation.pane_id(&after_ws), Some(pane_b_id));
    assert_eq!(
        capture_focused_remote(&navigation, &after_ws, &after_map),
        Some(3)
    );

    restore_focus_after_delta(
        &mut navigation,
        &mut after_ws,
        old_focused_remote,
        &after_map,
    );

    let pane_b_surface3_local = *after_map.get(&3).unwrap();
    let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&after_ws, pane_b_surface3_local)
        .expect("pane B tab2 surface must exist in rebuilt tree");
    assert_eq!(
        navigation.pane_id(&after_ws).unwrap(),
        pane_b_id,
        "복원 후 focus 는 pane A 가 아니라 사용자가 실제로 보던 pane B 에 있어야 한다"
    );
    let pane_b = after_ws
        .pane_layout()
        .find_pane(pane_b_id)
        .expect("pane B exists");
    assert_eq!(
        pane_b.tabs[navigation.tab_index(pane_b)].id,
        tab_id,
        "pane B 의 active_tab 도 사용자가 보던 두 번째 탭이어야 한다"
    );
    assert_eq!(
        navigation
            .surface_id(&pane_b.tabs[navigation.tab_index(pane_b)])
            .unwrap(),
        pane_b_surface3_local,
        "그 탭의 focused_surface 도 정확히 그 surface 를 가리켜야 한다"
    );
}

#[test]
fn focus_restore_is_noop_when_captured_surface_no_longer_exists() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let mut map = HashMap::new();
    map.insert(1u32, 50u32);
    map.insert(2u32, 51u32);
    let mut term = HashSet::new();
    term.insert(50u32);
    term.insert(51u32);
    let tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 10,
        "panes": [],
        "pane_layout": {
            "type": "Split", "direction": "horizontal", "ratio": 0.5,
            "first": { "type": "Leaf", "id": 10, "tabs": [
                { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                  "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
            ] },
            "second": { "type": "Leaf", "id": 11, "tabs": [
                { "id": 110, "name": "Shell", "active": true, "focused_surface": 2,
                  "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } }
            ] }
        }
    });
    let mut ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    let untouched_focused_pane = navigation.pane_id(&ws).unwrap();

    restore_focus_after_delta(&mut navigation, &mut ws, Some(3), &map);

    assert_eq!(
        navigation.pane_id(&ws).unwrap(),
        untouched_focused_pane,
        "캡처된 surface 가 없으면 원격이 보낸 focused_pane 그대로 둬야 한다"
    );
}

#[test]
fn set_focus_to_surface_updates_pane_tab_surface_or_reports_false() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let ids = test_ids();
    let mut map = HashMap::new();
    map.insert(1u32, 50u32);
    map.insert(2u32, 51u32);
    let mut term = HashSet::new();
    term.insert(50u32);
    term.insert(51u32);
    let tree = serde_json::json!({
        "id": 9, "name": "remote", "focused_pane": 10,
        "panes": [],
        "pane_layout": {
            "type": "Split", "direction": "horizontal", "ratio": 0.5,
            "first": { "type": "Leaf", "id": 10, "tabs": [
                { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                  "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
            ] },
            "second": { "type": "Leaf", "id": 11, "tabs": [
                { "id": 110, "name": "Shell", "active": true, "focused_surface": 2,
                  "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } }
            ] }
        }
    });
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        99,
        "remote",
        &tree,
        &ids,
        &map,
        &term,
        &HashMap::new(),
        &HashMap::new(),
        &mut HashMap::new(),
    )
    .expect("fixture construction");
    let local_b = *map.get(&2).unwrap();

    assert!(set_focus_to_surface(&mut navigation, &ws, local_b));
    let (pane_b_id, tab_b_id) = find_pane_and_tab_for_surface(&ws, local_b).expect("pane B exists");
    assert_eq!(navigation.pane_id(&ws).unwrap(), pane_b_id);
    let pane_b = ws.pane_layout().find_pane(pane_b_id).unwrap();
    assert_eq!(pane_b.tabs[navigation.tab_index(pane_b)].id, tab_b_id);
    assert_eq!(
        navigation
            .surface_id(&pane_b.tabs[navigation.tab_index(pane_b)])
            .unwrap(),
        local_b
    );

    assert!(
        !set_focus_to_surface(&mut navigation, &ws, 12345),
        "존재하지 않는 surface 는 false"
    );
}

#[test]
fn remote_structural_op_maps_the_move_target_to_its_remote_id() {
    // remote → local. 로컬 ID가 다른 원격 surface ID와 겹치는 배치다.
    let map: HashMap<u32, u32> = [(40, 3), (41, 4), (3, 9)].into_iter().collect();
    let local = StructuralOp::MoveSurface {
        source_surface_id: 4,
        target_surface_id: 3,
    };
    let wire = remote_structural_op(&local, 41, &map).expect("target mapped");
    assert_eq!(
        wire,
        StructuralOp::MoveSurface {
            source_surface_id: 41,
            target_surface_id: 40,
        },
        "target 도 원격 ID 로 보내야 한다 — 로컬 3 을 그대로 보내면 원격 surface 3 을 가리킨다"
    );
}

#[test]
fn remote_structural_op_drops_a_move_whose_target_is_not_mirrored() {
    let map: HashMap<u32, u32> = [(41, 4)].into_iter().collect();
    let local = StructuralOp::MoveSurface {
        source_surface_id: 4,
        target_surface_id: 7,
    };
    assert_eq!(remote_structural_op(&local, 41, &map), None);
}

#[test]
fn remote_structural_op_only_swaps_the_anchor_for_other_ops() {
    let local = StructuralOp::CloseSurface { surface_id: 4 };
    assert_eq!(
        remote_structural_op(&local, 41, &HashMap::new()),
        Some(StructuralOp::CloseSurface { surface_id: 41 })
    );
}

#[test]
fn pending_op_focus_for_new_tab_and_split_is_new_resource() {
    let map = HashMap::new();
    for op in [
        StructuralOp::NewTab {
            anchor_surface_id: 1,
            surface_kind: "terminal".to_string(),
            params: serde_json::Value::Null,
        },
        StructuralOp::SplitSurface {
            surface_id: 1,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::Value::Null,
        },
        StructuralOp::SplitPane {
            anchor_surface_id: 1,
            direction: SplitAxis::Vertical,
            surface_kind: "terminal".to_string(),
            params: serde_json::Value::Null,
        },
        StructuralOp::RestoreClosedItem {
            anchor_surface_id: 1,
        },
    ] {
        assert!(matches!(
            pending_op_focus_for(&op, &[], &map),
            Some(PendingOpFocus::NewResource)
        ));
    }
}

#[test]
fn pending_op_focus_for_close_translates_candidates_or_none() {
    let mut map = HashMap::new();
    map.insert(7u32, 70u32); // remote 7 -> local 70
    map.insert(8u32, 71u32); // remote 8 -> local 71

    let op = StructuralOp::CloseSurface { surface_id: 1 };
    match pending_op_focus_for(&op, &[70, 71], &map) {
        Some(PendingOpFocus::Close { candidates }) => {
            assert_eq!(candidates, vec![7, 8]);
        }
        other => panic!("expected Close{{candidates}}, got {other:?}"),
    }

    assert!(pending_op_focus_for(&op, &[999], &map).is_none());
    assert!(pending_op_focus_for(&op, &[], &map).is_none());
}

#[test]
fn pending_op_focus_for_non_target_ops_is_none() {
    let mut map = HashMap::new();
    map.insert(7u32, 70u32);
    let op = StructuralOp::MoveTab {
        anchor_surface_id: 1,
        from_index: 0,
        to_index: 1,
    };
    assert!(pending_op_focus_for(&op, &[70], &map).is_none());
}

#[test]
fn merge_survivor_mapping_prefers_server_display_name_and_falls_back_to_kind() {
    let ids = test_ids();
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let (tx, _rx) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;

    let surfaces = vec![
        serde_json::json!({
            "remote_id": 10,
            "role": "mesh",
            "kind": "markdown",
            "plugin_id": "com.tasty.markdown",
            "display_name": "README.md",
        }),
        serde_json::json!({
            "remote_id": 11,
            "role": "mesh",
            "kind": "image",
            "plugin_id": "com.tasty.image",
        }),
    ];

    let mapping = merge_survivor_mapping(&HashMap::new(), &surfaces, &ids, &frame_tx, &mut engine)
        .expect("fixture construction");
    let mesh = &mapping.mesh;

    let local_10 = mapping.remote_to_local[&10];
    let local_11 = mapping.remote_to_local[&11];
    assert_eq!(mesh[&local_10].display_name, "README.md");
    assert_eq!(
        mesh[&local_11].display_name, "image",
        "display_name 필드가 없으면 kind 로 fallback 해야 한다"
    );
}

#[test]
fn merge_survivor_mapping_cleans_up_stale_terminal_on_convert_to_mesh() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    // 별도 발급기를 만들면 기본 workspace의 ID와 충돌하므로 engine의 발급기를 공유한다.
    let ids = test_ids();
    let (tx, _rx) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;

    let surfaces_v1 = vec![serde_json::json!({
        "remote_id": 10, "role": "terminal", "cols": 80, "rows": 24,
    })];
    let mut m1 =
        merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine)
            .expect("fixture construction");
    let map1 = m1.remote_to_local.clone();
    let local_10 = map1[&10];
    assert!(
        engine.runtime.terminals.get(local_10).is_some(),
        "최초 terminal survivor 는 Terminal 을 만들어야 한다"
    );
    engine
        .remote
        .attach_mesh_frames
        .update(local_10, vec![1, 2, 3], 1, 1, true);

    // 다음 병합이 이전 kind를 조회할 수 있도록 먼저 실제 트리에 반영한다.
    let tree = serde_json::json!({
        "id": 9, "name": "mirror", "focused_pane": 7,
        "panes": [ {
            "id": 7,
            "tabs": [ {
                "id": 3, "name": "Shell", "active": true, "focused_surface": 10,
                "layout": { "type": "Leaf", "id": 10, "kind": "terminal" }
            } ]
        } ]
    });
    let mut ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        999,
        "mirror",
        &tree,
        &ids,
        &map1,
        &m1.terminals,
        &m1.mesh,
        &m1.explorer,
        &mut m1.markdown,
    )
    .expect("fixture construction");
    ws.mirror = true;
    engine.push_mirror_workspace(ws);

    let surfaces_v2 = vec![serde_json::json!({
        "remote_id": 10,
        "role": "mesh",
        "kind": "markdown",
        "plugin_id": "com.tasty.markdown",
        "display_name": "a.md",
    })];
    let m2 = merge_survivor_mapping(&map1, &surfaces_v2, &ids, &frame_tx, &mut engine)
        .expect("fixture construction");
    let (map2, term2, mesh2, new2) = (
        &m2.remote_to_local,
        &m2.terminals,
        &m2.mesh,
        &m2.newly_created_remote_ids,
    );

    assert_eq!(
        map2[&10], local_10,
        "local id 는 convert 후에도 유지돼야 한다"
    );
    assert!(new2.is_empty(), "survivor 는 신규 취급되면 안 된다");
    assert!(
        !term2.contains(&local_10),
        "markdown 으로 바뀐 뒤에는 더 이상 terminal_locals 에 없어야 한다"
    );
    assert!(
        mesh2.contains_key(&local_10),
        "mesh_locals 에는 새로 등록돼야 한다"
    );
    assert!(
        engine.runtime.terminals.get(local_10).is_none(),
        "옛 Terminal 객체는 즉시 제거돼야 한다"
    );
    assert!(
        engine.remote.attach_mesh_frames.get(local_10).is_none(),
        "옛(terminal 시절의 무의미한) mesh frame 캐시도 제거돼야 한다"
    );
}

/// 원격이 닫은 mirror surface는 로컬 닫기처럼 attach 점유 기록도 남기지 않는다.
/// forwarded terminal.kill이 남긴 soft 점유가 이 경로로 사라져야 한다.
#[test]
fn merge_survivor_mapping_forgets_the_occupancy_of_a_remotely_closed_surface() {
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let ids = test_ids();
    let (tx, _rx) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;
    let parent = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];

    let surfaces_v1 = vec![serde_json::json!({
        "remote_id": 10, "role": "terminal", "cols": 80, "rows": 24,
    })];
    let m1 = merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine)
        .expect("fixture construction");
    let local_10 = m1.remote_to_local[&10];
    engine
        .occupy_soft(local_10, parent, None)
        .expect("soft 점유");

    let m2 = merge_survivor_mapping(&m1.remote_to_local, &[], &ids, &frame_tx, &mut engine)
        .expect("fixture construction");
    assert!(m2.remote_to_local.is_empty());
    assert!(
        engine.live.occupancy.occupancy_of(local_10).is_none(),
        "원격이 닫은 surface 의 soft 점유가 남았다"
    );
}

#[test]
fn removing_a_mirror_workspace_forgets_the_occupancy_of_its_surfaces() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let parent = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    let ws_id = 9000;
    let local = 9003;
    let mut workspace =
        Workspace::new_with_terminal_marker(ws_id, "mirror".into(), 9001, 9002, local);
    workspace.mirror = true;
    engine.push_mirror_workspace(workspace);
    engine.occupy_soft(local, parent, None).expect("soft 점유");

    let map = HashMap::from([(99u32, local)]);
    assert!(remove_mirror_workspace_from_engine(
        &mut engine,
        &mut state,
        ws_id,
        &map
    ));
    assert!(
        engine.live.occupancy.occupancy_of(local).is_none(),
        "정리된 mirror surface 의 soft 점유가 남았다"
    );
}

#[test]
fn merge_survivor_mapping_creates_terminal_when_mesh_survivor_converts_to_terminal() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    // 기본 workspace와 ID가 충돌하지 않도록 engine의 발급기를 공유한다.
    let ids = test_ids();
    let (tx, _rx) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;

    let surfaces_v1 = vec![serde_json::json!({
        "remote_id": 20,
        "role": "mesh",
        "kind": "markdown",
        "plugin_id": "com.tasty.markdown",
        "display_name": "a.md",
    })];
    let mut m1 =
        merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine)
            .expect("fixture construction");
    let map1 = m1.remote_to_local.clone();
    let local_20 = map1[&20];
    assert!(
        engine.runtime.terminals.get(local_20).is_none(),
        "mesh survivor 는 애초에 Terminal 이 없어야 한다"
    );

    let tree = serde_json::json!({
        "id": 9, "name": "mirror", "focused_pane": 7,
        "panes": [ {
            "id": 7,
            "tabs": [ {
                "id": 3, "name": "a.md", "active": true, "focused_surface": 20,
                "layout": { "type": "Leaf", "id": 20, "kind": "markdown" }
            } ]
        } ]
    });
    let mut ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        999,
        "mirror",
        &tree,
        &ids,
        &map1,
        &m1.terminals,
        &m1.mesh,
        &m1.explorer,
        &mut m1.markdown,
    )
    .expect("fixture construction");
    ws.mirror = true;
    engine.push_mirror_workspace(ws);
    // terminal이 아니었던 surface의 이전 cwd도 지워야 한다.
    engine.set_mirror_surface_cwd(local_20, Some("/srv/remote/docs".to_string()));

    let surfaces_v2 = vec![serde_json::json!({
        "remote_id": 20, "role": "terminal", "cols": 80, "rows": 24,
    })];
    let m2 = merge_survivor_mapping(&map1, &surfaces_v2, &ids, &frame_tx, &mut engine)
        .expect("fixture construction");
    let (map2, term2, new2) = (
        &m2.remote_to_local,
        &m2.terminals,
        &m2.newly_created_remote_ids,
    );

    assert_eq!(
        map2[&20], local_20,
        "local id 는 convert 후에도 유지돼야 한다"
    );
    assert!(new2.is_empty(), "survivor 는 신규 취급되면 안 된다");
    assert!(term2.contains(&local_20));
    assert!(
        !engine.remote.mirror_surface_cwd.contains_key(&local_20),
        "kind 전환은 비-terminal 출발이어도 옛 cwd 를 버린다"
    );
    assert!(
        engine.runtime.terminals.get(local_20).is_some(),
        "mesh → terminal convert 는 새 Terminal 을 만들어야 한다(안 그러면 입력이 안 감)"
    );
}

#[test]
fn cwd_push_applies_as_remote_origin_and_null_clears_it() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let ws_id = 9_000u32;
    let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
    let remote_surface = 42u32;
    let mut mirror_ws = Workspace::new_with_terminal_marker(
        ws_id,
        "mirror".to_string(),
        pane_id,
        tab_id,
        local_surface,
    );
    mirror_ws.mirror = true;
    engine.push_mirror_workspace(mirror_ws);
    engine
        .runtime
        .terminals
        .insert(local_surface, Terminal::new_detached(80, 24), None);
    let mut sess = test_session(ws_id, HashMap::from([(remote_surface, local_surface)]));
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

    {
        let mut host = MirrorHost::parked(&mut state, &mut engine);
        apply_mirror_events(
            &mut sess,
            &mut host,
            &mut plugin_manager,
            vec![MirrorEvent::Cwd(
                remote_surface,
                Some("/srv/remote/proj".to_string()),
            )],
        );
    }
    assert_eq!(
        engine.surface_cwd(local_surface),
        Some(crate::core::state::SurfaceCwd::Remote(
            crate::core::state::RemoteCwd::new("/srv/remote/proj")
        ))
    );
    assert_eq!(
        state.resolve_inherit_cwd_from_surface(&engine.as_ref().read(), local_surface),
        None,
        "원격 cwd를 로컬 실행 경로로 사용하면 안 된다"
    );

    {
        let mut host = MirrorHost::parked(&mut state, &mut engine);
        apply_mirror_events(
            &mut sess,
            &mut host,
            &mut plugin_manager,
            vec![MirrorEvent::Cwd(remote_surface, None)],
        );
    }
    assert!(
        engine.remote.mirror_surface_cwd.is_empty(),
        "null push 는 옛 원격 경로를 남기지 않는다"
    );
}

#[test]
fn a_loss_notice_becomes_a_desync_event() {
    let payload = serde_json::to_vec(&StreamControl::Loss { frames: 7 }).unwrap();
    assert!(
        matches!(
            mirror_event_from_control(&payload),
            Some(MirrorEvent::Desynced { frames: 7 })
        ),
        "Loss 가 재동기화 이벤트로 옮겨지지 않았다"
    );
}

/// wire에서 파싱한 실패 사유가 사용자 안내까지 유지되는지 확인한다.
#[test]
fn a_structural_failure_reason_reaches_the_toast_verbatim() {
    // 다른 시험의 전역 번역 초기화와 경쟁하지 않도록 기준 문구를 읽기 전에 초기화한다.
    crate::i18n::init("en");
    let toast_for = |reason: Option<&str>| {
        let payload = serde_json::to_vec(&StreamControl::StructuralResult {
            op_id: 0,
            ok: false,
            reason: reason.map(str::to_string),
        })
        .unwrap();
        let ev = mirror_event_from_control(&payload).expect("실패 회신은 이벤트가 된다");
        let mut sess = test_session(9_000, HashMap::new());
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        supply_ids(&engine);
        {
            let mut host = MirrorHost::windowed(&mut state, &mut engine);
            apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, vec![ev]);
        }
        let messages: Vec<String> = state
            .toasts
            .messages()
            .into_iter()
            .map(str::to_string)
            .collect();
        messages
    };
    let base = crate::i18n::t("attach.toast.mirror_structural_forward_failed").to_string();

    assert_eq!(
        toast_for(Some("unknown surface kind: definitely-not-registered")),
        vec![format!(
            "{base} (unknown surface kind: definitely-not-registered)"
        )],
        "원격 사유가 원문 그대로 괄호 안에 실려야 한다"
    );
    assert_eq!(
        toast_for(None),
        vec![base],
        "wire 에 사유가 없으면 괄호 없는 기본 문구다"
    );
}

#[test]
fn a_desync_detaches_once_renews_the_stream_and_sums_later_notices() {
    use tasty_terminal::{OUTPUT_RETENTION_MAX_BYTES, OutputCursor, OutputReadRequest};

    let _home = crate::test_support::TastyHomeGuard::new();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let (remote_surface, local_surface) = (42u32, 9_003u32);
    engine
        .runtime
        .terminals
        .insert(local_surface, Terminal::new_detached(80, 24), None);
    let mut sess = test_session(9_000, HashMap::from([(remote_surface, local_surface)]));
    let (tx, frames_out) = tasty_remote::connection::channel();
    sess.transport.frame_tx = tx;
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

    let read = |engine: &crate::runtime::engine_access::EngineRef<'_>, expect: Option<String>| {
        engine
            .runtime
            .terminals
            .get(local_surface)
            .expect("mirror terminal")
            .read_output(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: OUTPUT_RETENTION_MAX_BYTES,
                strip_ansi: false,
                expect_stream: expect,
            })
    };
    {
        let mut host = MirrorHost::parked(&mut state, &mut engine);
        apply_mirror_events(
            &mut sess,
            &mut host,
            &mut plugin_manager,
            vec![MirrorEvent::Data(remote_surface, b"before".to_vec())],
        );
    }
    let before = read(&engine.as_ref(), None).expect("read").stream;

    {
        let mut host = MirrorHost::windowed(&mut state, &mut engine);
        apply_mirror_events(
            &mut sess,
            &mut host,
            &mut plugin_manager,
            vec![
                MirrorEvent::Desynced { frames: 3 },
                MirrorEvent::Desynced { frames: 2 },
            ],
        );
    }
    assert_eq!(
        sess.state.resync_pending,
        Some(5),
        "기다리는 동안의 통지는 합산한다"
    );
    let sent: Vec<OutFrame> = frames_out.try_iter().map(|queued| queued.frame).collect();
    assert_eq!(
        sent.len(),
        1,
        "재attach 를 위해 옛 연결을 놓는 것은 한 번이다"
    );
    assert_eq!(sent[0].tag, StreamTag::Detach);
    assert!(
        read(&engine.as_ref(), Some(before)).is_err(),
        "손실 전 표지로 읽으면 stream 불일치여야 한다"
    );
    assert_eq!(
        sess.state.phase,
        SessionState::Connected,
        "재attach 는 EOF 를 본 뒤에 건다 — 통지만으로 상태를 바꾸지 않는다"
    );
}

/// 창 없는 수동 mirror는 재attach할 창이 생길 때까지 옛 연결을 유지한다.
#[test]
fn a_loss_on_a_parked_mirror_without_an_anchor_waits_for_a_window_instead_of_closing() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let (remote_surface, local_surface) = (42u32, 9_003u32);
    engine
        .runtime
        .terminals
        .insert(local_surface, Terminal::new_detached(80, 24), None);
    let mut sess = test_session(9_000, HashMap::from([(remote_surface, local_surface)]));
    assert!(
        sess.state.anchor_ws_id.is_none(),
        "전제: 수동 attach(anchor 없음)"
    );
    let (tx, frames_out) = tasty_remote::connection::channel();
    sess.transport.frame_tx = tx;
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

    {
        let mut host = MirrorHost::parked(&mut state, &mut engine);
        apply_mirror_events(
            &mut sess,
            &mut host,
            &mut plugin_manager,
            vec![MirrorEvent::Desynced { frames: 4 }],
        );
    }
    assert_eq!(sess.state.resync_pending, Some(4));
    assert!(sess.state.resync_awaiting_window);
    assert!(
        frames_out.try_iter().next().is_none(),
        "parked 에서 옛 연결을 놓으면 재attach 가 창을 못 찾아 mirror 가 정리된다"
    );
    assert_eq!(
        disconnect_disposition(
            sess.transport.disconnected.load(Ordering::SeqCst),
            sess.state.phase,
            sess.resync_released(),
            sess.state.anchor_ws_id.is_some(),
        ),
        DisconnectDisposition::None
    );
    assert_eq!(
        disconnect_disposition(
            true,
            sess.state.phase,
            sess.resync_released(),
            sess.state.anchor_ws_id.is_some(),
        ),
        DisconnectDisposition::Cleanup
    );

    let toasts_before = state.toasts.len();
    {
        let mut host = MirrorHost::parked(&mut state, &mut engine);
        resume_resync_in_window(&mut sess, &mut host);
    }
    assert!(
        frames_out.try_iter().next().is_none(),
        "아직 창이 없으면 놓지 않는다"
    );
    {
        let mut host = MirrorHost::windowed(&mut state, &mut engine);
        resume_resync_in_window(&mut sess, &mut host);
    }
    let sent: Vec<OutFrame> = frames_out.try_iter().map(|queued| queued.frame).collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].tag, StreamTag::Detach);
    assert!(!sess.state.resync_awaiting_window);
    assert_eq!(state.toasts.len(), toasts_before + 1, "재동기화 안내 toast");
    assert_eq!(
        disconnect_disposition(
            true,
            sess.state.phase,
            sess.resync_released(),
            sess.state.anchor_ws_id.is_some(),
        ),
        DisconnectDisposition::Resync,
        "Detach를 보낸 뒤 EOF를 받으면 재attach한다"
    );
}

#[test]
fn a_bulk_transfer_declares_loss_notify_and_aborts_on_a_loss_notice() {
    use std::io::{BufRead, BufReader};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        let (sock, _) = listener.accept().expect("accept");
        let mut writer = sock.try_clone().expect("clone");
        let mut reader = BufReader::new(sock);
        let mut line = String::new();
        reader.read_line(&mut line).expect("stream.open line");
        let ack = serde_json::to_vec(&tasty_ipc::stream::StreamAck {
            ok: true,
            client_id: Some(5),
            proto: STREAM_PROTO,
            error: None,
        })
        .expect("ack");
        stream::write_frame(&mut writer, StreamTag::Control, &ack).expect("write ack");
        let declared = stream::read_frame(&mut reader).expect("declaration");
        let loss = serde_json::to_vec(&StreamControl::Loss { frames: 1 }).expect("loss");
        stream::write_frame(&mut writer, StreamTag::Control, &loss).expect("write loss");
        declared
    });

    let mut conn = open_bulk_connection(port, 3).expect("bulk connection");
    let err = await_bulk_result(
        &mut conn,
        11,
        &tasty_remote::connection::channel().0.epoch(),
    )
    .expect_err("결과를 모르면 성공이 아니다");
    let declared = server.join().expect("server");
    assert_eq!(declared.tag, StreamTag::Control);
    assert!(
        matches!(
            serde_json::from_slice::<StreamControl>(&declared.payload),
            Ok(StreamControl::ClientLossNotify {})
        ),
        "bulk 연결이 손실 통지를 선언해야 한다"
    );
    let msg = err.to_string();
    assert!(msg.contains("aborted"), "중단으로 보고해야 한다: {msg}");
    assert!(
        !msg.starts_with(BULK_REJECT_PREFIX),
        "원격 거부로 보이면 재시도가 막힌다: {msg}"
    );
}

/// writer 없는 시험 세션. 입력 전송은 실패해도 forwarder가 다음 입력을 기다린다.
pub(super) fn test_session(
    local_workspace: u32,
    remote_to_local: HashMap<u32, u32>,
) -> AttachClientSession {
    let (tx, _rx) = tasty_remote::connection::channel();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let control = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_peer, _) = listener.accept().unwrap();
    AttachClientSession {
        transport: ClientTransport {
            workers: tasty_remote::transport::ConnectionWorkers::new(control, Vec::new()),
            output: MirrorOutbox::new(tx.epoch()),
            disconnected: Arc::new(AtomicBool::new(false)),
            frame_tx: tx,
            tunnel: None,
        },
        state: ClientSessionState {
            structure_ids: Default::default(),
            local_workspace,
            remote_to_local,
            phase: SessionState::Connected,
            client_id: 1,
            remote_workspace: 7,
            bulk_port: 0,
            anchor_ws_id: None,
            op_seq: 0,
            pending_op_focus: HashMap::new(),
            agent_requests: Default::default(),
            next_delta_focus: None,
            last_forwarded_resize: HashMap::new(),
            remote_label: "127.0.0.1:0".to_string(),
            pending_list_dir_consumers: HashMap::new(),
            markdown_locals: HashSet::new(),
            resync_pending: None,
            resync_awaiting_window: false,
        },
    }
}

/// 실제 kind 등록 경로를 사용하며 플러그인 프로세스 대신 채널 수신자로 명령을 확인한다.
fn register_markdown_kind(
    engine: &crate::runtime::engine_access::EngineMut<'_>,
    plugin_id: &str,
) -> std::sync::mpsc::Receiver<crate::plugin_bridge::host_cmd::HostCmd> {
    let decl: crate::plugin::manifest::SurfaceKindDecl =
        serde_json::from_value(serde_json::json!({
            "kind": "markdown",
            "display_name_i18n_key": "surface.kind.markdown",
            "rendering": "webview",
        }))
        .expect("test SurfaceKindDecl");
    let (tx, rx) = std::sync::mpsc::channel();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &engine.runtime.surface_registry,
        plugin_id,
        &decl,
        tx,
    );
    rx
}

fn created_surfaces(
    rx: &std::sync::mpsc::Receiver<crate::plugin_bridge::host_cmd::HostCmd>,
) -> Vec<(u32, Value)> {
    rx.try_iter()
        .filter_map(|cmd| match cmd {
            crate::plugin_bridge::host_cmd::HostCmd::RemoteSurfaceCreated {
                surface_id,
                params,
                ..
            } => Some((surface_id, params)),
            _ => None,
        })
        .collect()
}

fn markdown_descriptor(remote_id: u32) -> Value {
    serde_json::json!({
        "remote_id": remote_id,
        "role": "markdown",
        "file": "/remote/docs/README.md",
        "display_name": "README.md",
    })
}

fn single_leaf_tree(remote_id: u32) -> Value {
    serde_json::json!({
        "id": 9, "name": "mirror", "focused_pane": 7,
        "panes": [ {
            "id": 7,
            "tabs": [ {
                "id": 3, "name": "README.md", "active": true, "focused_surface": remote_id,
                "layout": { "type": "Leaf", "id": remote_id, "kind": "markdown" }
            } ]
        } ]
    })
}

#[test]
fn merge_survivor_mapping_builds_local_markdown_surface_for_markdown_role() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let rx = register_markdown_kind(&engine, MARKDOWN_PLUGIN_ID);
    let ids = test_ids();
    let (tx, _frames) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;

    let mut mapping = merge_survivor_mapping(
        &HashMap::new(),
        &[markdown_descriptor(30)],
        &ids,
        &frame_tx,
        &mut engine,
    )
    .expect("fixture construction");
    let local = mapping.remote_to_local[&30];
    assert_eq!(mapping.markdown_ids(), HashSet::from([local]));

    let created = created_surfaces(&rx);
    assert_eq!(created.len(), 1, "plugin 에 surface.create 가 한 번 간다");
    assert_eq!(created[0].0, local);
    assert_eq!(created[0].1["remote"]["file"], "/remote/docs/README.md");
    assert!(
        created[0].1.get("file").is_none(),
        "원격 경로를 `file` 로 실으면 plugin 이 client 로컬 파일을 읽는다"
    );

    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        999,
        "mirror",
        &single_leaf_tree(30),
        &ids,
        &mapping.remote_to_local,
        &mapping.terminals,
        &mapping.mesh,
        &mapping.explorer,
        &mut mapping.markdown,
    )
    .expect("fixture construction");
    let pane = ws.pane_layout().first_pane().expect("pane");
    let leaf = pane.tabs[0]
        .layout_if_initialized()
        .and_then(|l| l.find_surface(local))
        .expect("markdown leaf");
    assert_eq!(
        leaf.kind(),
        "markdown",
        "빈 surface 가 아니라 markdown surface"
    );
}

/// 다른 소유자의 markdown kind는 사용하지 않고 빈 surface로 둔다.
#[test]
fn markdown_role_stays_empty_when_another_plugin_owns_the_kind() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let rx = register_markdown_kind(&engine, "com.example.other-markdown");
    let ids = test_ids();
    let (tx, _frames) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;

    let mut mapping = merge_survivor_mapping(
        &HashMap::new(),
        &[markdown_descriptor(30)],
        &ids,
        &frame_tx,
        &mut engine,
    )
    .expect("fixture construction");
    assert!(mapping.markdown.is_empty());
    assert!(created_surfaces(&rx).is_empty());
    let local = mapping.remote_to_local[&30];
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        999,
        "mirror",
        &single_leaf_tree(30),
        &ids,
        &mapping.remote_to_local,
        &mapping.terminals,
        &mapping.mesh,
        &mapping.explorer,
        &mut mapping.markdown,
    )
    .expect("fixture construction");
    let pane = ws.pane_layout().first_pane().expect("pane");
    let leaf = pane.tabs[0]
        .layout_if_initialized()
        .and_then(|l| l.find_surface(local))
        .expect("leaf");
    assert_eq!(leaf.kind(), "empty");
    assert!(
        !engine
            .runtime
            .surfaces
            .get(&local)
            .and_then(|surface| surface.as_any().downcast_ref::<EmptySurface>())
            .is_some_and(|empty| empty.deferred.is_some())
    );
}

#[test]
fn markdown_role_waits_for_the_plugin_kind_and_reifies_as_a_mirror_document() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    // The common fixture registers markdown; this scenario starts with no live kind.
    assert_eq!(
        engine
            .runtime
            .surface_registry
            .withdraw_plugin(MARKDOWN_PLUGIN_ID),
        vec![MARKDOWN_MIRROR_KIND],
    );
    assert!(
        engine
            .runtime
            .surface_registry
            .get_live(MARKDOWN_MIRROR_KIND)
            .is_none()
    );
    supply_ids(&engine);
    let ids = test_ids();
    let (tx, _frames) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;

    let mut mapping = merge_survivor_mapping(
        &HashMap::new(),
        &[markdown_descriptor(30)],
        &ids,
        &frame_tx,
        &mut engine,
    )
    .expect("fixture construction");
    let local = mapping.remote_to_local[&30];
    assert_eq!(
        mapping.markdown_ids(),
        HashSet::from([local]),
        "placeholder 도 이 세션의 markdown leaf 로 센다 — destroy·끊김 통지 대상"
    );
    let ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        999,
        "mirror",
        &single_leaf_tree(30),
        &ids,
        &mapping.remote_to_local,
        &mapping.terminals,
        &mapping.mesh,
        &mapping.explorer,
        &mut mapping.markdown,
    )
    .expect("fixture construction");
    assert!(
        engine
            .runtime
            .surfaces
            .get(&local)
            .and_then(|surface| surface.as_any().downcast_ref::<EmptySurface>())
            .is_some_and(|empty| empty.deferred.is_some()),
        "kind 가 없으면 kind 대기 placeholder"
    );
    engine.push_mirror_workspace(ws);

    engine.reify_displayed_mirror_resources(&[local]);
    assert_eq!(engine.find_surface_by_id(local).unwrap().kind(), "empty");
    let rx = register_markdown_kind(&engine, MARKDOWN_PLUGIN_ID);
    engine.reify_displayed_mirror_resources(&[local]);

    let leaf = engine.find_surface_by_id(local).expect("leaf");
    assert_eq!(leaf.kind(), "markdown");
    let restored: Vec<Value> = rx
        .try_iter()
        .filter_map(|cmd| match cmd {
            crate::plugin_bridge::host_cmd::HostCmd::RemoteSurfaceRestored {
                surface_id,
                data,
                ..
            } if surface_id == local => Some(data),
            _ => None,
        })
        .collect();
    assert_eq!(restored.len(), 1, "plugin 에 surface.restore 가 한 번 간다");
    assert_eq!(restored[0]["remote"]["file"], "/remote/docs/README.md");
    assert_eq!(restored[0]["display_name"], "README.md");
    assert!(restored[0].get("file").is_none());
}

#[test]
fn structural_delta_reuses_markdown_survivor_and_reports_removed_ones() {
    let mut navigation = crate::state::navigation::NavigationState::default();
    let mut structure_ids = MirrorStructureIds::default();
    let (_, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let rx = register_markdown_kind(&engine, MARKDOWN_PLUGIN_ID);
    let ids = test_ids();
    let (tx, _frames) = tasty_remote::connection::channel();
    let frame_tx: SharedFrameSender = tx;

    let mut m1 = merge_survivor_mapping(
        &HashMap::new(),
        &[markdown_descriptor(30)],
        &ids,
        &frame_tx,
        &mut engine,
    )
    .expect("fixture construction");
    let local = m1.remote_to_local[&30];
    let ws_id = 999;
    let mut ws = build_mirror_workspace(
        &mut structure_ids,
        &mut navigation,
        ws_id,
        "mirror",
        &single_leaf_tree(30),
        &ids,
        &m1.remote_to_local,
        &m1.terminals,
        &m1.mesh,
        &m1.explorer,
        &mut m1.markdown,
    )
    .expect("fixture construction");
    ws.mirror = true;
    engine.push_mirror_workspace(ws);
    assert_eq!(created_surfaces(&rx).len(), 1);
    let webview_url = engine
        .find_surface_by_id(local)
        .and_then(|s| {
            s.as_any()
                .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
        })
        .map(|rs| Arc::clone(&rs.webview_url))
        .expect("RemoteSurface");

    let mut sess = test_session(ws_id, m1.remote_to_local.clone());
    sess.state.markdown_locals = HashSet::from([local]);

    let removed = apply_mirror_structural_delta(
        &mut navigation,
        &mut sess,
        &mut engine,
        7,
        &single_leaf_tree(30),
        &[markdown_descriptor(30)],
        None,
    )
    .expect("mirror delta");
    assert!(removed.is_empty());
    assert!(
        created_surfaces(&rx).is_empty(),
        "survivor 에 surface.create 를 다시 보내면 문서가 로딩부터 다시 시작한다"
    );
    let shared = engine
        .find_surface_by_id(local)
        .and_then(|s| {
            s.as_any()
                .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
        })
        .expect("still a RemoteSurface");
    assert!(Arc::ptr_eq(&shared.webview_url, &webview_url));

    let removed = apply_mirror_structural_delta(
        &mut navigation,
        &mut sess,
        &mut engine,
        7,
        &single_leaf_tree(30),
        &[serde_json::json!({ "remote_id": 30, "role": "terminal", "cols": 80, "rows": 24 })],
        None,
    )
    .expect("mirror delta");
    assert_eq!(removed, vec![local]);
    assert!(sess.state.markdown_locals.is_empty());
}

#[test]
fn parse_markdown_content_result_reads_both_shapes_and_ignores_other_events() {
    let ok = serde_json::json!({
        "event": "markdown_content_result", "request_id": 4, "surface_id": 30,
        "ok": true, "file": "/r/a.md", "source": "# hi", "truncated": true,
    });
    match parse_markdown_content_result(&serde_json::to_vec(&ok).unwrap()) {
        Some(MirrorEvent::MarkdownContentResult {
            request_id: 4,
            surface_id: 30,
            ok: true,
            file,
            source,
            truncated: true,
            reason: None,
        }) => {
            assert_eq!(file.as_deref(), Some("/r/a.md"));
            assert_eq!(source.as_deref(), Some("# hi"));
        }
        _ => panic!("success shape not parsed"),
    }
    let failed = serde_json::json!({
        "event": "markdown_content_result", "request_id": 5, "surface_id": 30,
        "ok": false, "reason": "permission denied",
    });
    match parse_markdown_content_result(&serde_json::to_vec(&failed).unwrap()) {
        Some(MirrorEvent::MarkdownContentResult {
            ok: false, reason, ..
        }) => assert_eq!(reason.as_deref(), Some("permission denied")),
        _ => panic!("failure shape not parsed"),
    }
    let other = serde_json::json!({ "event": "git_query_result", "request_id": 1 });
    assert!(parse_markdown_content_result(&serde_json::to_vec(&other).unwrap()).is_none());
}

#[test]
fn parse_markdown_changed_reads_the_remote_id_and_ignores_other_events() {
    let changed = serde_json::json!({ "event": "markdown_changed", "surface_id": 30 });
    assert!(matches!(
        parse_markdown_changed(&serde_json::to_vec(&changed).unwrap()),
        Some(MirrorEvent::MarkdownChanged { surface_id: 30 })
    ));
    let result = serde_json::json!({
        "event": "markdown_content_result", "request_id": 4, "surface_id": 30, "ok": true,
    });
    assert!(parse_markdown_changed(&serde_json::to_vec(&result).unwrap()).is_none());
    assert!(parse_markdown_content_result(&serde_json::to_vec(&changed).unwrap()).is_none());
}

#[test]
fn markdown_mirror_local_maps_only_markdown_leaves() {
    let mut sess = test_session(1, HashMap::from([(30, 300), (31, 310)]));
    sess.state.markdown_locals.insert(300);
    assert_eq!(markdown_mirror_local(&sess, 30), Some(300));
    assert_eq!(markdown_mirror_local(&sess, 31), None, "터미널 leaf");
    assert_eq!(
        markdown_mirror_local(&sess, 99),
        None,
        "이 세션이 mirror 하지 않는 문서"
    );
}

fn parked_ids(
    parked: &[ParkedEngine],
) -> impl Iterator<Item = (EngineId, &crate::core::CoreState)> {
    parked.iter().map(|p| (p.session.id, &p.session.core_state))
}

/// 첫 항목만 보는 오류를 잡도록 mirror는 두 번째 parked engine에만 둔다.
fn parked_with_mirror(ws_id: u32, local_surface: u32) -> Vec<ParkedEngine> {
    let mut parked: Vec<ParkedEngine> = (0..2)
        .map(|_| ParkedEngine::from_test_state(crate::state::tests::test_state()))
        .collect();
    let mut mirror_ws = Workspace::new_with_terminal_marker(
        ws_id,
        "mirror".to_string(),
        9_001,
        9_002,
        local_surface,
    );
    mirror_ws.mirror = true;
    let mut engine = parked[1].session.borrow_mut();
    engine.push_mirror_workspace(mirror_ws);
    engine.runtime.surfaces.insert(
        local_surface,
        Box::new(crate::model::TerminalSurface { id: local_surface }),
    );
    engine
        .runtime
        .terminals
        .insert(local_surface, Terminal::new_detached(80, 24), None);
    parked
}

#[test]
fn mirror_output_host_prefers_window_then_parked_then_none() {
    let ws_id = 9_000u32;
    let parked = parked_with_mirror(ws_id, 9_003);
    let wid = winit::window::WindowId::from(7u64);
    assert_eq!(
        mirror_output_host(Some(wid), parked_ids(&parked), ws_id),
        Some(MirrorOutputHost::Window(wid)),
        "창 있는 engine 이 있으면 그쪽"
    );
    assert_eq!(
        mirror_output_host(None, parked_ids(&parked), ws_id),
        Some(MirrorOutputHost::Parked(parked[1].session.id)),
        "창이 없으면 mirror 를 든 parked engine(두 번째)"
    );
    assert_eq!(
        mirror_output_host(None, parked_ids(&parked), 424_242),
        None,
        "어느 engine 에도 없으면 None — drain 하지 않는다"
    );
}

#[test]
fn parked_engine_receives_mirror_data_and_structural_delta() {
    let ws_id = 9_000u32;
    let (survivor_remote, survivor_local, new_remote) = (42u32, 9_003u32, 43u32);
    let mut parked = parked_with_mirror(ws_id, survivor_local);
    let untouched_ws_count = parked[0].session.core_state.workspaces().len();
    let original_generation = parked[1]
        .session
        .runtime
        .terminals
        .generation(survivor_local)
        .expect("original mirror terminal");
    let mut sess = test_session(ws_id, HashMap::from([(survivor_remote, survivor_local)]));

    let tree = serde_json::json!({
        "id": 7, "name": "mirror", "focused_pane": 70,
        "panes": [ {
            "id": 70,
            "tabs": [ {
                "id": 30, "name": "Shell", "active": true, "focused_surface": survivor_remote,
                "layout": {
                    "type": "Split", "direction": "vertical", "ratio": 0.5,
                    "focus_second": false,
                    "first": { "type": "Leaf", "id": survivor_remote, "kind": "terminal" },
                    "second": { "type": "Leaf", "id": new_remote, "kind": "terminal" }
                }
            } ]
        } ]
    });
    let surfaces = vec![
        serde_json::json!({ "remote_id": survivor_remote, "role": "terminal", "cols": 80, "rows": 24 }),
        serde_json::json!({ "remote_id": new_remote, "role": "terminal", "cols": 80, "rows": 24 }),
    ];
    let events = vec![
        MirrorEvent::Data(survivor_remote, b"hello-parked".to_vec()),
        MirrorEvent::StructuralDelta {
            workspace_id: 7,
            tree,
            surfaces,
        },
        MirrorEvent::Data(new_remote, b"world-new".to_vec()),
    ];

    let id =
        find_parked_with_workspace(parked_ids(&parked), ws_id).expect("mirror 를 든 parked engine");
    let pidx = parked
        .iter()
        .position(|p| p.session.id == id)
        .expect("찾은 id의 항목");
    {
        let ParkedEngine {
            view_restore: state,
            session,
        } = &mut parked[pidx];
        let mut engine = session.borrow_mut();
        supply_ids(&engine);
        let mut host = MirrorHost::parked(state, &mut engine);
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
        apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, events);
    }

    let engine = parked[pidx].session.as_ref();
    assert_eq!(
        engine.runtime.terminals.generation(survivor_local),
        Some(original_generation),
        "a surviving mirror keeps its original terminal owner"
    );
    let survivor = engine
        .runtime
        .terminals
        .get(survivor_local)
        .expect("survivor mirror 터미널은 delta 뒤에도 같은 local id 로 남는다");
    assert!(
        survivor.screen_text(false).contains("hello-parked"),
        "parked 동안 도착한 Data 가 mirror grid 에 남아야 한다: {:?}",
        survivor.screen_text(false)
    );
    let new_local = *sess
        .state
        .remote_to_local
        .get(&new_remote)
        .expect("delta 가 새 remote surface 를 매핑에 넣어야 한다(desync 방지)");
    let fresh = engine
        .runtime
        .terminals
        .get(new_local)
        .expect("delta 가 새 mirror 터미널을 만들어야 한다");
    assert!(
        fresh.screen_text(false).contains("world-new"),
        "delta 이후의 Data 가 갱신된 매핑으로 새 터미널에 라우팅돼야 한다: {:?}",
        fresh.screen_text(false)
    );
    let ws = engine
        .workspaces()
        .into_iter()
        .find(|w| w.id == ws_id)
        .expect("mirror 워크스페이스는 같은 local id 로 교체된다");
    let sids = ws.all_surface_ids();
    assert!(
        sids.contains(&survivor_local) && sids.contains(&new_local),
        "{sids:?}"
    );
    assert_eq!(
        parked[0].session.core_state.workspaces().len(),
        untouched_ws_count,
        "무관한 parked engine 은 건드리지 않는다"
    );
}

/// None 분기와 아래 실제 적용 시험을 함께 검사한다.
/// host 없이 take_for를 호출할 수 없다는 API 제약은 이 실행 시험의 검출 범위와 별개다.
#[test]
fn no_host_leaves_the_mirror_buffer_untouched() {
    let ws_id = 9_000u32;
    let mut sess = test_session(ws_id, HashMap::new());
    for event in [
        MirrorEvent::Data(1, b"a".to_vec()),
        MirrorEvent::Resize(1, 10, 5),
    ] {
        assert!(sess.transport.output.push(event));
    }
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

    let applied = apply_pending_mirror_output(&mut sess, None, &mut plugin_manager);

    assert!(!applied, "적용 대상이 없으면 적용했다고 보고하지 않는다");
    let buf = sess.transport.output.drain();
    assert_eq!(
        buf.len(),
        2,
        "host 가 없으면 버퍼는 그대로 남아 다음 호출이 다시 시도한다"
    );
    assert!(matches!(buf[0], MirrorEvent::Data(1, ref b) if b == b"a"));
    assert!(matches!(buf[1], MirrorEvent::Resize(1, 10, 5)));
}

#[test]
fn a_host_drains_and_applies_the_mirror_buffer() {
    let ws_id = 9_000u32;
    let local_surface = 9_003u32;
    let remote_surface = 42u32;
    let mut parked = parked_with_mirror(ws_id, local_surface);
    let mut sess = test_session(ws_id, HashMap::from([(remote_surface, local_surface)]));
    sess.transport
        .output
        .push(MirrorEvent::Data(remote_surface, b"applied-here".to_vec()));
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

    let id =
        find_parked_with_workspace(parked_ids(&parked), ws_id).expect("mirror 를 든 parked engine");
    let pidx = parked
        .iter()
        .position(|p| p.session.id == id)
        .expect("찾은 id의 항목");
    let applied = {
        let ParkedEngine {
            view_restore: state,
            session,
        } = &mut parked[pidx];
        let mut engine = session.borrow_mut();
        supply_ids(&engine);
        apply_pending_mirror_output(
            &mut sess,
            Some(MirrorHost::parked(state, &mut engine)),
            &mut plugin_manager,
        )
    };

    assert!(applied);
    assert!(
        sess.transport.output.drain().is_empty(),
        "적용했으면 버퍼는 비워진다"
    );
    let term = parked[pidx]
        .session
        .runtime
        .terminals
        .get(local_surface)
        .expect("mirror 터미널");
    assert!(term.screen_text(false).contains("applied-here"));
}

#[test]
fn parked_host_does_not_stack_toasts_but_windowed_does() {
    let ws_id = 9_000u32;
    let mut sess = test_session(ws_id, HashMap::new());
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
    let failure = || vec![MirrorEvent::StructuralFailed(0, Some("nope".to_string()))];

    let (mut parked_state, mut parked_engine_session) = crate::state::tests::test_state();
    let mut parked_engine = parked_engine_session.borrow_mut();
    {
        let mut host = MirrorHost::parked(&mut parked_state, &mut parked_engine);
        apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, failure());
    }
    assert_eq!(
        parked_state.toasts.len(),
        0,
        "창이 없는 engine 에는 toast 를 쌓지 않는다"
    );

    let (mut win_state, mut win_engine_session) = crate::state::tests::test_state();
    let mut win_engine = win_engine_session.borrow_mut();
    {
        let mut host = MirrorHost::windowed(&mut win_state, &mut win_engine);
        apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, failure());
    }
    assert_eq!(
        win_state.toasts.len(),
        1,
        "창이 있으면 같은 이벤트가 toast 를 낸다 — 게이트가 창 유무로만 갈린다"
    );
}

/// resize 전후의 출력 순서를 보존하며 버퍼를 비워야 한다.
#[test]
fn outbox_drain_keeps_output_resize_arrival_order() {
    let buf = MirrorOutbox::new(tasty_remote::connection::channel().0.epoch());
    for event in [
        MirrorEvent::Data(1, b"a".to_vec()),
        MirrorEvent::Resize(1, 10, 5),
        MirrorEvent::Data(1, b"b".to_vec()),
    ] {
        assert!(buf.push(event));
    }
    let drained = buf.drain();
    assert!(matches!(drained[0], MirrorEvent::Data(1, ref b) if b == b"a"));
    assert!(matches!(drained[1], MirrorEvent::Resize(1, 10, 5)));
    assert!(matches!(drained[2], MirrorEvent::Data(1, ref b) if b == b"b"));
    assert_eq!(drained.len(), 3);
    assert!(buf.drain().is_empty(), "꺼낸 뒤 버퍼는 비어 있다");
}
