//! UI 배율을 바꿔 그린 도형으로 토큰 치수가 배율을 따르는지 확인한다.
use super::layout_tests::{deep_crumbs, painted_frames_themed, picker_state, text_shape};
use super::*;

/// Mocha 색과 `zoom` 배율의 Theme.
fn zoomed(zoom: f32) -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, zoom)
}

fn one_file() -> Vec<FilePickerEntryView> {
    vec![FilePickerEntryView {
        name: "file-0.toml".into(),
        is_dir: false,
        size_display: "1 KB".into(),
        modified_display: "2026-09-14".into(),
    }]
}

/// 배율 `zoom` 으로 열기 모드 피커를 그린 마지막 프레임.
fn painted_at(zoom: f32, size: egui::Vec2) -> (Theme, Vec<egui::epaint::ClippedShape>) {
    let th = zoomed(zoom);
    let (_, shapes) = painted_frames_themed(
        &th,
        &picker_state(size),
        (
            &deep_crumbs(2, "crumb"),
            FilePickerMode::Open { selection_text: "" },
            &one_file(),
            "",
        ),
        2,
        &|_, _| Vec::new(),
        &[],
    );
    (th, shapes)
}

/// 이름 칸의 오른쪽 끝 — 이름 galley 가 받은 말줄임 폭의 끝이다.
fn name_column_right(shapes: &[egui::epaint::ClippedShape], name: &str) -> f32 {
    let hits: Vec<f32> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == name => {
                Some(t.pos.x + t.galley.job.wrap.max_width)
            }
            _ => None,
        })
        .collect();
    assert_eq!(hits.len(), 1, "{name} 은 한 번 칠해진다");
    hits[0]
}

/// 크기 열(디자인 68)과 수정일 열(108)은 UI 배율을 따른다. 열 폭은 머리 열 이름의 오른쪽 끝과
/// 이름 칸 끝 사이에서 열 간격(`spacing_sm`)을 뺀 값이다.
#[test]
fn the_size_and_modified_columns_follow_the_ui_zoom() {
    for (zoom, size_w, modified_w) in [(1.0, 68.0, 108.0), (1.5, 102.0, 162.0)] {
        let (th, shapes) = painted_at(zoom, egui::vec2(640.0, 480.0) * zoom);
        let gap = th.spacing_sm.value();
        let (size_head, _) = text_shape(&shapes, "SIZE");
        let (modified_head, _) = text_shape(&shapes, "MODIFIED");
        let name_right = name_column_right(&shapes, "file-0.toml");
        let near = |got: f32, want: f32, what: &str| {
            assert!(
                (got - want).abs() <= 0.5,
                "배율 {zoom}: {what} {got}, 기대 {want}"
            );
        };
        near(
            modified_head.right() - size_head.right() - gap,
            modified_w,
            "수정일 열 폭",
        );
        near(size_head.right() - name_right - gap, size_w, "크기 열 폭");
    }
}
