use super::*;
use tasty_core::{DomainEvent as E, Placement, Ratio, SplitSpec, SurfaceSpec};

/// 탭 바 높이. 시험의 pane 내용 영역은 pane 높이에서 이 값을 뺀 것이다.
const TAB_BAR: f32 = 30.0;

fn geometry(content_height: f32) -> SplitGeometry {
    SplitGeometry {
        terminal_rect: PhysicalRect {
            x: PhysicalPx(0.0),
            y: PhysicalPx(0.0),
            width: PhysicalPx(800.0),
            height: PhysicalPx(content_height + TAB_BAR),
        },
        tab_bar_height: PhysicalPx(TAB_BAR),
        scale: 1.0,
    }
}

fn spec(id: u32, kind: &str) -> SurfaceSpec {
    SurfaceSpec {
        id,
        kind: kind.into(),
        data: None,
    }
}

fn split(direction: SplitDirection) -> SplitSpec {
    SplitSpec {
        direction,
        ratio: Ratio::from_f32(0.5),
        placement: Placement::After,
    }
}

/// pane 10 하나, 탭 100 에 surface 1000(`first`). `stacked` 이면 그 아래에 surface 1001(`second`)을
/// 0.5 로 쌓는다.
fn session(first: &str, stacked: Option<&str>) -> crate::runtime::engine_session::EngineSession {
    let mut events = vec![
        E::CategoryCreated {
            id: 0,
            name: "normal".into(),
            index: 0,
        },
        E::WorkspaceCreated {
            id: 1,
            name: "w".into(),
            category: 0,
            index: 0,
            pane: 10,
        },
        E::TabCreated {
            id: 100,
            pane: 10,
            index: 0,
            name: "t".into(),
            surface: spec(1000, first),
        },
    ];
    if let Some(second) = stacked {
        events.push(E::SurfaceSplit {
            target: 1000,
            surface: spec(1001, second),
            split: split(SplitDirection::Horizontal),
        });
    }
    let mut model = crate::state::tests::test_model(events);
    model.applied.revision = Some(1);
    crate::state::tests::test_state_from_model(model).1
}

fn surface_split(target: u32, direction: SplitDirection) -> CreationDestination {
    CreationDestination::Split {
        target,
        split: split(direction),
    }
}

fn ratio(destination: &CreationDestination) -> f32 {
    match destination {
        CreationDestination::Split { split, .. } | CreationDestination::Pane { split, .. } => {
            split.ratio.to_f32()
        }
        _ => unreachable!("split destination"),
    }
}

/// 비율 `r` 로 높이 `h` 를 surface 분할했을 때 (첫째, 둘째) 높이.
fn halves(h: f32, r: f32) -> (f32, f32) {
    let (a, b) = PhysicalRect {
        x: PhysicalPx(0.0),
        y: PhysicalPx(0.0),
        width: PhysicalPx(800.0),
        height: PhysicalPx(h),
    }
    .split_with_gap(SplitDirection::Horizontal, r, SURFACE_BORDER_WIDTH);
    (a.height.value(), b.height.value())
}

fn reason(response: &JsonRpcResponse) -> (i32, String, serde_json::Value) {
    let error = response.error.as_ref().expect("refusal");
    (
        error.code,
        error.message.clone(),
        error.data.clone().unwrap_or_default()["reason"].clone(),
    )
}

/// 636px 열의 탐색기 칸을 두 번 나눈다. 첫 split 은 0.5 그대로 두 칸 모두 하한 이상이고,
/// 둘째 split 은 탐색기 칸이 160 을 지키도록 비율을 고친다.
#[test]
fn splitting_a_636px_explorer_column_twice_keeps_the_explorer_at_160() {
    let first = session("explorer", None);
    let mut destination = surface_split(1000, SplitDirection::Horizontal);
    hold(
        &mut destination,
        "terminal",
        &first.core_state,
        Some(&geometry(636.0)),
    )
    .unwrap();
    assert_eq!(ratio(&destination), 0.5);
    let (explorer, _) = halves(636.0, 0.5);
    assert!(explorer >= 160.0, "{explorer}");

    // 첫 split 의 결과(위 탐색기 317, 아래 터미널 318)에서 탐색기를 다시 나눈다.
    let second = session("explorer", Some("terminal"));
    let mut destination = surface_split(1000, SplitDirection::Horizontal);
    hold(
        &mut destination,
        "terminal",
        &second.core_state,
        Some(&geometry(636.0)),
    )
    .unwrap();
    let r = ratio(&destination);
    assert!(
        r > 0.5,
        "ratio stays at {r}, explorer would be {}",
        halves(explorer, 0.5).0
    );
    let (kept, sibling) = halves(explorer, r);
    assert!(kept >= 160.0, "explorer {kept}");
    assert!(
        kept < 161.0,
        "the divider moves only as far as the floor needs: {kept}"
    );
    assert!(
        sibling > 0.0 && r <= DIVIDER_RATIO_MAX,
        "sibling {sibling}, ratio {r}"
    );
}

/// 형제 칸도 탐색기라 둘 다 160 을 지킬 수 없으면 거절하고, 비율을 바꾸지 않는다.
#[test]
fn a_split_that_cannot_keep_both_explorers_is_refused() {
    let second = session("explorer", Some("terminal"));
    let mut destination = surface_split(1000, SplitDirection::Horizontal);
    let refused = hold(
        &mut destination,
        "explorer",
        &second.core_state,
        Some(&geometry(636.0)),
    )
    .unwrap_err();
    let (code, message, why) = reason(&refused);
    assert_eq!(code, -32602);
    assert_eq!(why, REFUSAL_REASON);
    assert!(message.contains("160px"), "{message}");
    assert_eq!(ratio(&destination), 0.5);
}

/// 새로 만드는 탐색기 칸(둘째 칸)도 하한을 지키도록 분할선을 위로 옮긴다.
#[test]
fn a_new_explorer_cell_gets_the_floor() {
    let second = session("explorer", Some("terminal"));
    let mut destination = surface_split(1001, SplitDirection::Horizontal);
    hold(
        &mut destination,
        "explorer",
        &second.core_state,
        Some(&geometry(636.0)),
    )
    .unwrap();
    let r = ratio(&destination);
    let (_, terminal_height) = halves(636.0, 0.5);
    let (_, explorer) = halves(terminal_height, r);
    assert!(
        r < 0.5 && explorer >= 160.0,
        "ratio {r}, explorer {explorer}"
    );
}

/// 옆으로 나누는 split 은 높이를 바꾸지 않는다. 창이 작아 이미 하한보다 낮은 탐색기라도 거절하지 않는다.
#[test]
fn a_side_by_side_split_of_a_short_explorer_is_left_alone() {
    let short = session("explorer", None);
    let mut destination = surface_split(1000, SplitDirection::Vertical);
    hold(
        &mut destination,
        "terminal",
        &short.core_state,
        Some(&geometry(120.0)),
    )
    .unwrap();
    assert_eq!(ratio(&destination), 0.5);
}

/// pane 분할은 대상 pane 안의 탐색기 칸 높이를 함께 줄이므로 같은 하한을 지킨다.
#[test]
fn a_pane_split_keeps_the_explorer_inside_the_target_pane() {
    let first = session("explorer", None);
    let mut destination = CreationDestination::Pane {
        target: 10,
        pane: 0,
        tab: 0,
        split: split(SplitDirection::Horizontal),
    };
    // pane 높이 330 → 0.5 면 위 pane 내용이 164 - 30 = 134 로 하한 아래다.
    hold(
        &mut destination,
        "terminal",
        &first.core_state,
        Some(&geometry(300.0)),
    )
    .unwrap();
    let r = ratio(&destination);
    let (top, _) = PhysicalRect {
        x: PhysicalPx(0.0),
        y: PhysicalPx(0.0),
        width: PhysicalPx(800.0),
        height: PhysicalPx(300.0 + TAB_BAR),
    }
    .split_with_gap(
        SplitDirection::Horizontal,
        r,
        PANE_BORDER_WIDTH.to_physical(1.0),
    );
    assert!(
        top.height.value() - TAB_BAR >= 160.0,
        "ratio {r}, top {:?}",
        top.height
    );
}

/// 창이 없는 엔진(헤드리스)은 칸을 그리지 않으므로 하한이 없다.
#[test]
fn without_a_window_the_ratio_is_left_alone() {
    let second = session("explorer", Some("terminal"));
    let mut destination = surface_split(1000, SplitDirection::Horizontal);
    hold(&mut destination, "explorer", &second.core_state, None).unwrap();
    assert_eq!(ratio(&destination), 0.5);
}

/// 탐색기가 아닌 칸끼리의 split 은 그대로 둔다.
#[test]
fn a_split_without_explorers_is_left_alone() {
    let terminals = session("terminal", Some("terminal"));
    let mut destination = surface_split(1000, SplitDirection::Horizontal);
    hold(
        &mut destination,
        "terminal",
        &terminals.core_state,
        Some(&geometry(100.0)),
    )
    .unwrap();
    assert_eq!(ratio(&destination), 0.5);
}
