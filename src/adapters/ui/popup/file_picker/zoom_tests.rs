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
    let shapes = painted_mode(&th, size, FilePickerMode::Open { selection_text: "" });
    (th, shapes)
}

/// `th` 로 `mode` 피커를 그린 마지막 프레임. 경로는 `/` 와 `crumb-01` 두 칸이다.
fn painted_mode(
    th: &Theme,
    size: egui::Vec2,
    mode: FilePickerMode<'_>,
) -> Vec<egui::epaint::ClippedShape> {
    painted_frames_themed(
        th,
        &picker_state(size),
        (&deep_crumbs(2, "crumb"), mode, &one_file(), ""),
        2,
        &|_, _| Vec::new(),
        &[],
    )
    .1
}

/// 기존 파일 이름을 넣어 덮어쓰기 경고 줄이 뜬 저장 모드.
const OVERWRITE: FilePickerMode<'static> = FilePickerMode::Save {
    name: "file-0.toml",
    overwrite: true,
    can_confirm: true,
};

/// 덮어쓰기 경고 문구. 시험 하네스의 템플릿에 [`OVERWRITE`] 이름을 넣은 것이다.
const WARNING_TEXT: &str = "file-0.toml already exists in this folder. Saving replaces it.";

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

/// 경로 막대 chevron(디자인 13)은 UI 배율을 따른다. 두 crumb 글자 사이는 chevron 하나와
/// 그 양옆 `STRUCT_GAP_2` 간격이다.
#[test]
fn the_path_bar_chevron_follows_the_ui_zoom() {
    let gap = tasty_ui_widgets::tokens::STRUCT_GAP_2.value();
    for (zoom, glyph) in [(1.0, 13.0), (1.5, 20.0)] {
        let (_, shapes) = painted_at(zoom, egui::vec2(640.0, 480.0) * zoom);
        let (root, _) = text_shape(&shapes, "/");
        let (child, _) = text_shape(&shapes, "crumb-01");
        let between = child.left() - root.right() - gap * 2.0;
        assert!(
            (between - glyph).abs() <= 0.5,
            "배율 {zoom}: chevron 폭 {between}, 기대 {glyph}"
        );
    }
}

/// 덮어쓰기 경고 줄의 글리프(디자인 13)는 UI 배율을 따른다. 줄은 이름 라벨과 같은 왼쪽에서
/// 시작하고, 문구는 글리프와 `spacing_xs` 뒤에 선다.
#[test]
fn the_overwrite_line_glyph_follows_the_ui_zoom() {
    for (zoom, glyph) in [(1.0, 13.0), (1.5, 20.0)] {
        let th = zoomed(zoom);
        let shapes = painted_mode(&th, egui::vec2(640.0, 480.0) * zoom, OVERWRITE);
        let (label, _) = text_shape(&shapes, "File name");
        let (warning, _) = text_shape(&shapes, WARNING_TEXT);
        let before_text = warning.left() - label.left() - th.spacing_xs.value();
        assert!(
            (before_text - glyph).abs() <= 0.5,
            "배율 {zoom}: 경고 글리프 폭 {before_text}, 기대 {glyph}"
        );
    }
}
