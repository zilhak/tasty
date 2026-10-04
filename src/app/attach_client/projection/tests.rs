use super::*;
use crate::app::attach_client::tests::test_ids;
use crate::model::{PaneNode, SplitDirection, SurfaceLayout};
use std::collections::{HashMap, HashSet};
use tasty_remote::client_session::MirrorStructureIds;

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
