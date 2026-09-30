use super::*;

#[derive(Default)]
struct Fixture {
    navigation: crate::state::navigation::NavigationState,
    structure_ids: MirrorStructureIds,
    ids: crate::core::state::IdGenerator,
}

impl Fixture {
    fn rebuild(&mut self, tree: Value) -> Workspace {
        let map = (1..=30).map(|id| (id, id + 100)).collect();
        build_mirror_workspace(
            &mut self.structure_ids,
            &mut self.navigation,
            90,
            "mirror",
            &tree,
            &self.ids,
            &map,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        )
    }
}

fn tab(id: u32, surface: u32, active: bool) -> Value {
    serde_json::json!({
        "id": id, "name": "leaf", "active": active, "focused_surface": surface,
        "layout": {"type": "Leaf", "id": surface, "kind": "empty"}
    })
}

fn pane(id: u32, tabs: Vec<Value>) -> Value {
    serde_json::json!({"id": id, "tabs": tabs})
}

#[test]
fn removed_local_pane_uses_remote_default_and_mapping_drops_deleted_ids() {
    let mut f = Fixture::default();
    let before = f.rebuild(serde_json::json!({"focused_pane": 10, "panes": [
        pane(10, vec![tab(100, 1, true)]),
        pane(20, vec![tab(200, 2, true)]),
        pane(30, vec![tab(300, 3, true)])
    ]}));
    assert!(set_focus_to_surface(&mut f.navigation, &before, 102));
    let a_id = f.structure_ids.panes[&10];
    let c_id = f.structure_ids.panes[&30];
    let after = f.rebuild(serde_json::json!({"focused_pane": 30, "panes": [
        pane(10, vec![tab(100, 1, true)]),
        pane(30, vec![tab(300, 3, true)])
    ]}));
    assert_eq!(f.navigation.pane_id(&after), Some(c_id));
    assert_eq!(f.structure_ids.panes[&10], a_id);
    assert_eq!(f.structure_ids.panes.len(), 2);
    assert_eq!(f.structure_ids.remote_tabs.len(), 2);
    assert!(!f.structure_ids.panes.contains_key(&20));
    assert!(!f.structure_ids.remote_tabs.contains_key(&200));
    // Reusing a remote ID after deletion must not reuse stale local identity.
    let replacement = f.rebuild(serde_json::json!({"focused_pane": 20, "panes": [
        pane(20, vec![tab(200, 2, true)])
    ]}));
    assert_eq!(f.structure_ids.panes.len(), 1);
    assert_eq!(f.structure_ids.remote_tabs.len(), 1);
    assert_eq!(
        f.navigation.pane_id(&replacement),
        Some(f.structure_ids.panes[&20])
    );
}

#[test]
fn removed_tab_and_surface_use_wire_defaults_but_live_choices_survive() {
    let mut f = Fixture::default();
    let before = f.rebuild(
        serde_json::json!({"focused_pane": 10, "panes": [pane(10, vec![
            tab(100, 1, true), tab(200, 2, false), tab(300, 3, false)
        ])]}),
    );
    set_focus_to_surface(&mut f.navigation, &before, 102);
    let after = f.rebuild(
        serde_json::json!({"focused_pane": 10, "panes": [pane(10, vec![
            tab(100, 1, false), tab(300, 3, true)
        ])]}),
    );
    let p = after.pane_layout().first_pane().unwrap();
    assert_eq!(
        f.navigation.tab_id(p),
        Some(f.structure_ids.remote_tabs[&300])
    );
    assert_eq!(f.structure_ids.remote_tabs.len(), 2);
    let split_tab = |first, second, focused| {
        serde_json::json!({
            "id": 300, "name": "split", "active": true, "focused_surface": focused,
            "layout": {"type": "Split", "direction": "horizontal", "ratio": 0.5,
                "focus_second": false,
                "first": {"type": "Leaf", "id": first, "kind": "empty"},
                "second": {"type": "Leaf", "id": second, "kind": "empty"}}
        })
    };
    let split = f.rebuild(
        serde_json::json!({"focused_pane": 10, "panes": [pane(10, vec![
            split_tab(3, 4, 4)
        ])]}),
    );
    let t = &split.pane_layout().first_pane().unwrap().tabs[0];
    assert_eq!(
        f.navigation.surface_id(t),
        Some(103),
        "live local leaf wins over wire"
    );
    let deleted = f.rebuild(
        serde_json::json!({"focused_pane": 10, "panes": [pane(10, vec![
            split_tab(4, 5, 5)
        ])]}),
    );
    let t = &deleted.pane_layout().first_pane().unwrap().tabs[0];
    assert_eq!(
        f.navigation.surface_id(t),
        Some(105),
        "deleted local leaf uses wire, not first"
    );
}
