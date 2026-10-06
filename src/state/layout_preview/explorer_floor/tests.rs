use crate::model::{DividerInfo, PhysicalPx, PhysicalRect, SplitDirection};
use crate::state::layout_preview::LayoutTarget;

fn rect() -> PhysicalRect {
    PhysicalRect {
        x: PhysicalPx(0.0),
        y: PhysicalPx(0.0),
        width: PhysicalPx(800.0),
        height: PhysicalPx(600.0),
    }
}

/// 위 pane 은 `top` 종류, 아래 pane 은 터미널인 위아래 분할.
fn stacked(
    top: &str,
) -> (
    crate::state::RequestContext,
    crate::runtime::engine_session::EngineSession,
) {
    use tasty_core::{DomainEvent as E, Placement, Ratio, SplitSpec, SurfaceSpec};
    let spec = |id, kind: &str| SurfaceSpec {
        id,
        kind: kind.into(),
        data: None,
    };
    let events = vec![
        E::CategoryCreated {
            id: 0,
            name: "normal".into(),
            index: 0,
        },
        E::WorkspaceCreated {
            id: 1,
            name: "split".into(),
            category: 0,
            index: 0,
            pane: 10,
        },
        E::TabCreated {
            id: 100,
            pane: 10,
            index: 0,
            name: "top".into(),
            surface: spec(1000, top),
        },
        E::PaneSplit {
            target: 10,
            pane: 20,
            split: SplitSpec {
                direction: SplitDirection::Horizontal,
                ratio: Ratio::from_f32(0.5),
                placement: Placement::After,
            },
        },
        E::TabCreated {
            id: 200,
            pane: 20,
            index: 0,
            name: "bottom".into(),
            surface: spec(2000, "terminal"),
        },
    ];
    let mut model = crate::state::tests::test_model(events);
    model.applied.revision = Some(1);
    crate::state::tests::test_state_from_model(model)
}

/// 위아래 분할선을 `ratio` 까지 끌었을 때 위 pane 의 높이.
fn drag_top_to(top: &str, ratio: f32) -> f32 {
    let (mut state, session) = stacked(top);
    let core = &session.core_state;
    state.reconcile_presentation(core);
    let workspace = &core.local_workspaces()[0];
    let sequence = state
        .layout_previews
        .begin(
            core,
            LayoutTarget::Workspace(1),
            workspace.pane_layout(),
            DividerInfo {
                direction: SplitDirection::Horizontal,
                split_rect: rect(),
            },
            rect(),
            1.0,
        )
        .unwrap();
    assert!(state.update_divider_preview(core, sequence, ratio, rect(), 1.0));
    state.pane_rects(core, workspace, rect(), 1.0)[0]
        .1
        .height
        .value()
}

#[test]
fn the_split_drag_stops_at_the_explorer_floor() {
    let floor = crate::theme::theme().explorer_min_height().value();
    let top = drag_top_to("explorer", 0.1);
    assert!(
        top >= floor && top < floor + 1.0,
        "explorer pane {top} should stop just at the {floor} floor"
    );
}

#[test]
fn a_terminal_cell_has_no_explorer_floor() {
    let top = drag_top_to("terminal", 0.1);
    assert!(top < 100.0, "terminal pane follows the drag, got {top}");
}

#[test]
fn growing_the_explorer_is_never_held_back() {
    let top = drag_top_to("explorer", 0.8);
    assert!(top > 400.0, "explorer pane grows with the drag, got {top}");
}
