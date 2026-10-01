use super::*;
use crate::model::BinaryTree;
use crate::state::layout_preview::{DividerCommit, LayoutTarget};

fn split_layout() -> serde_json::Value {
    let leaf = serde_json::json!({"Leaf":{"Generic":{"kind":"empty","data":{}}}});
    let tab = serde_json::json!({"name":"split","explicit_name":null,"surface":{"Split":{"direction":"Vertical","ratio":0.5,"first":leaf,"second":leaf}}});
    let pane = serde_json::json!({"Leaf":{"tabs":[tab, {"name":"other","explicit_name":null,"surface":leaf}],"active_tab":0}});
    serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"ratios","subtitle":"","description":"","focused_pane_index":0,"pane_layout":{"Split":{"direction":"Vertical","ratio":0.5,"first":pane,"second":pane}}}]})
}

#[test]
fn divider_commit_publishes_once_and_stale_revision_rejects_unchanged_leaf_targets() {
    let (mut session, mut journal) = boot_with_layout(Some(split_layout()));
    let workspace = &session.core_state.local_workspaces()[0];
    let workspace_id = workspace.id;
    let pane_ids = workspace.pane_layout().all_pane_ids();
    let tab = &workspace.pane_layout().find_pane(pane_ids[0]).unwrap().tabs[0];
    let tab_id = tab.id;
    let surfaces = tab.all_surface_ids();
    let node_id = match tab.layout() {
        crate::model::SurfaceLayout::Split { node_id, .. } => *node_id,
        _ => panic!("split"),
    };
    let instance = session.runtime.surfaces.get(&surfaces[0]).unwrap().as_ref()
        as *const dyn crate::model::Surface as *const ();
    let revision = session.core_state.committed_structure_revision().unwrap();
    journal
        .admit_divider(
            &session,
            DividerCommit {
                sequence: 1,
                target: LayoutTarget::Workspace(workspace_id),
                path: Vec::new(),
                leaves: pane_ids.clone(),
                revision,
                ratio: 0.7,
            },
            &crate::intent::IntentOrigin::System,
        )
        .unwrap();
    assert_eq!(
        session.core_state.local_workspaces()[0]
            .pane_layout()
            .split_parts()
            .unwrap()
            .1,
        0.5
    );
    metadata::finish_intents(&mut journal, &mut session);
    assert!(
        journal
            .commands
            .completed_dividers
            .pop()
            .unwrap()
            .3
            .error
            .is_none()
    );
    assert_eq!(
        session.core_state.local_workspaces()[0]
            .pane_layout()
            .split_parts()
            .unwrap()
            .1,
        0.7
    );
    assert_eq!(
        session.core_state.committed_structure_revision(),
        session.journal_binding.as_ref().unwrap().revision
    );
    let mut change = DividerCommit {
        sequence: 2,
        target: LayoutTarget::Tab(tab_id),
        path: Vec::new(),
        leaves: surfaces.clone(),
        revision,
        ratio: 0.65,
    };
    journal
        .admit_divider(
            &session,
            change.clone(),
            &crate::intent::IntentOrigin::System,
        )
        .unwrap();
    metadata::finish_intents(&mut journal, &mut session);
    assert!(
        journal
            .commands
            .completed_dividers
            .pop()
            .unwrap()
            .3
            .error
            .is_some()
    );
    assert_eq!(
        session
            .core_state
            .find_pane_by_id(pane_ids[0])
            .unwrap()
            .tabs[0]
            .layout()
            .split_parts()
            .unwrap()
            .1,
        0.5
    );
    change.revision = session.core_state.committed_structure_revision().unwrap();
    journal
        .admit_divider(&session, change, &crate::intent::IntentOrigin::System)
        .unwrap();
    metadata::finish_intents(&mut journal, &mut session);
    assert!(
        journal
            .commands
            .completed_dividers
            .pop()
            .unwrap()
            .3
            .error
            .is_none()
    );
    let layout = session
        .core_state
        .find_pane_by_id(pane_ids[0])
        .unwrap()
        .tabs[0]
        .layout();
    assert_eq!(layout.split_parts().unwrap().1, 0.65);
    assert!(
        matches!(layout, crate::model::SurfaceLayout::Split { node_id: current, .. } if *current == node_id)
    );
    assert_eq!(
        instance,
        session.runtime.surfaces.get(&surfaces[0]).unwrap().as_ref()
            as *const dyn crate::model::Surface as *const ()
    );
}

#[test]
fn local_tab_reorder_keeps_the_fixed_tab_identity_between_queued_moves() {
    let (mut session, mut journal) = boot_with_layout(Some(split_layout()));
    let pane_id = session.core_state.local_workspaces()[0]
        .pane_layout()
        .all_pane_ids()[0];
    let tabs: Vec<_> = session
        .core_state
        .find_pane_by_id(pane_id)
        .unwrap()
        .tabs
        .iter()
        .map(|tab| tab.id)
        .collect();
    for tab_id in [tabs[0], tabs[1]] {
        assert!(journal.admit_metadata_intent(
            session.id,
            &session.core_state,
            &crate::app::command::DomainIntent::MoveTab {
                pane_id,
                tab_id,
                to_index: 1
            },
            &crate::intent::IntentOrigin::System,
            None,
        ));
    }
    metadata::finish_intents(&mut journal, &mut session);
    assert!(
        journal
            .commands
            .completed_intents
            .iter()
            .all(|result| result.response.error.is_none())
    );
    assert_eq!(
        session
            .core_state
            .find_pane_by_id(pane_id)
            .unwrap()
            .tabs
            .iter()
            .map(|tab| tab.id)
            .collect::<Vec<_>>(),
        tabs
    );
    let binding = session.journal_binding.as_ref().unwrap();
    let store = tasty_event_store::EventStore::open(
        &tasty_utils::path::tasty_home()
            .unwrap()
            .join("structure/journal.db"),
        &binding.journal_id,
    )
    .unwrap();
    let model =
        crate::runtime::journal::load(&store, &tasty_event_store::StreamId::new(&binding.stream))
            .unwrap();
    assert_eq!(model.panes[&pane_id].tabs, tabs);
}
