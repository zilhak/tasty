//! Check selected View geometry/admission boundaries, not arbitrary transitive calls.
use super::{fn_body, repo_root, strip_comments};

fn read(path: &str) -> String {
    strip_comments(&std::fs::read_to_string(repo_root().join(path)).unwrap())
}
fn without_direct_ratio_write(source: &str) -> bool {
    ["update_ratio_for_rect(", "pane_layout_mut(", ".layout_mut("]
        .iter()
        .all(|call| !source.contains(call))
}

#[test]
fn divider_input_borrows_the_tree_and_finishes_through_a_command() {
    let input = read("src/state/mouse.rs");
    assert!(without_direct_ratio_write(&input));
    assert!(!without_direct_ratio_write(&format!(
        "{input}\nengine.pane_layout_mut().update_ratio_for_rect(rect, ratio, root, scale);"
    )));
    let source = read("src/view/main/divider_drag.rs");
    let finish = fn_body(&source, "fn finish_divider_drag(").unwrap();
    let boundary = |body: &str| {
        body.contains("dragging_divider.take()")
            && body.contains("layout_previews.finish(")
            && body.contains("Intent::CommitDivider(")
    };
    assert!(boundary(&finish));
    assert!(!boundary(
        &finish.replace("Intent::CommitDivider(", "bypass(")
    ));
}

#[test]
fn renderer_uses_the_same_preview_for_panes_and_surface_dividers() {
    let source = read("src/gfx/gpu.rs");
    let body = fn_body(&source, "fn prepare_layout(").unwrap();
    let boundary = |body: &str| {
        let compact: String = body.chars().filter(|ch| !ch.is_whitespace()).collect();
        [
            "state.pane_rects(",
            "state.pane_dividers(",
            "state.surface_dividers(",
        ]
        .iter()
        .all(|call| compact.contains(call))
    };
    assert!(boundary(&body));
    assert!(!boundary(
        &body.replace("state.pane_rects(", "pane_layout.compute_rects(")
    ));
    assert!(!boundary(&body.replace(
        "state.surface_dividers(",
        "surface.collect_dividers("
    )));
}
