//! 목록 행 글리프 잉크를 행 상태(쉼 · hover · 선택 · 선택+hover)마다 그린 도형으로 본다. 정본은 kit `FpRow`다.
use super::layout_tests::{deep_crumbs, painted_frames_tweaked, picker_state};
use super::*;

const FOLDER: &str = "dir-a";
const FILE: &str = "file-b.txt";

/// 내장 Latte. raised·on-active 잉크가 기본 잉크와 갈리는 테마다.
fn latte() -> Theme {
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT).expect("latte");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    Theme::with_colors(colors, is_light.unwrap_or(true))
}

fn rows() -> Vec<FilePickerEntryView> {
    [(FOLDER, true), (FILE, false)]
        .into_iter()
        .map(|(name, is_dir)| FilePickerEntryView {
            name: name.into(),
            is_dir,
            size_display: "1 KB".into(),
            modified_display: "2026-09-14".into(),
        })
        .collect()
}

fn paint(
    th: &Theme,
    selection: &str,
    pointer: Option<egui::Pos2>,
) -> Vec<egui::epaint::ClippedShape> {
    paint_focused(th, selection, pointer, None)
}

/// `focus` 가 `Some((행, 링))` 이면 그 행에 행 포커스를 두고 링 표시를 정한다.
fn paint_focused(
    th: &Theme,
    selection: &str,
    pointer: Option<egui::Pos2>,
    focus: Option<(&'static str, bool)>,
) -> Vec<egui::epaint::ClippedShape> {
    let events = move |_: usize, _: egui::Rect| {
        pointer
            .map(|p| vec![egui::Event::PointerMoved(p)])
            .unwrap_or_default()
    };
    painted_frames_tweaked(
        th,
        &picker_state(egui::vec2(640.0, 480.0)),
        (
            &deep_crumbs(2, "crumb"),
            FilePickerMode::Open { selection_text: "" },
            &rows(),
            selection,
        ),
        3,
        &events,
        &[],
        &|p| {
            if let Some((row, ring)) = focus {
                p.focus_row = Some(row);
                p.ring = ring;
            }
        },
    )
    .1
}

/// `name` 행 이름 글자의 사각형.
fn name_rect(shapes: &[egui::epaint::ClippedShape], name: &str) -> egui::Rect {
    shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{name} 이 칠해진다"))
}

/// `name` 행의 글리프 tint. egui 는 이미지를 텍스처 사각형으로 그리고 채움에 tint 를 싣는다.
fn glyph_ink(th: &Theme, shapes: &[egui::epaint::ClippedShape], name: &str) -> egui::Color32 {
    let text = name_rect(shapes, name);
    let size = egui::Vec2::splat(th.icon_glyph_size_md.value());
    let hits: Vec<egui::Color32> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r)
                if r.rect.size() == size
                    && r.rect.right() <= text.left()
                    && (r.rect.center().y - text.center().y).abs() < 1.0 =>
            {
                Some(r.fill)
            }
            _ => None,
        })
        .collect();
    assert_eq!(hits.len(), 1, "{name} 행 글리프 하나: {hits:?}");
    hits[0]
}

/// (쉼, hover, 선택, 선택+hover) 네 상태의 `name` 행 글리프 잉크.
fn four_states(th: &Theme, name: &str) -> [egui::Color32; 4] {
    let at = name_rect(&paint(th, "", None), name).center();
    [
        glyph_ink(th, &paint(th, "", None), name),
        glyph_ink(th, &paint(th, "", Some(at)), name),
        glyph_ink(th, &paint(th, name, None), name),
        glyph_ink(th, &paint(th, name, Some(at)), name),
    ]
}

/// 폴더 글리프: 쉼·선택 accent-primary-raised, hover accent-primary, 선택+hover accent-primary-on-active.
#[test]
fn the_folder_glyph_reads_the_primary_glyph_cell_of_each_row_state() {
    let th = latte();
    let (raised, plain, on_active) = (
        th.accent_primary_raised().to_egui(),
        th.accent_primary().to_egui(),
        th.accent_primary_on_active().to_egui(),
    );
    assert!(
        raised != plain && raised != on_active && plain != on_active,
        "Latte 에서 세 잉크가 갈려야 시험이 뜻을 갖는다"
    );
    assert_eq!(
        four_states(&th, FOLDER),
        [raised, plain, raised, on_active],
        "쉼 · hover · 선택 · 선택+hover"
    );
}

/// 파일 글리프: 쉼·선택 text-muted, hover `fp-row-glyph-fg-hover`(text-secondary), 선택+hover text-on-active.
/// Latte 는 text-muted 와 text-secondary 가 같은 subtext1 이라 세 잉크가 갈리는 Mocha 로 잰다.
#[test]
fn the_file_glyph_reads_the_kit_ink_of_each_row_state() {
    let th = Theme::with_colors(tasty_themes::mocha_fallback_colors(), false);
    let (muted, hover, on_active) = (
        th.text_muted().to_egui(),
        th.fp_row_glyph_fg_hover().to_egui(),
        th.text_on_active().to_egui(),
    );
    assert!(
        muted != hover && muted != on_active && hover != on_active,
        "Mocha 에서 세 잉크가 갈려야 시험이 뜻을 갖는다"
    );
    assert_eq!(
        four_states(&th, FILE),
        [muted, hover, muted, on_active],
        "쉼 · hover · 선택 · 선택+hover"
    );
}

/// 행 사각형 안쪽에 그린 테두리 선(링 후보): (도형 순번, 사각형, 반경, 선).
fn strokes_on_row(
    shapes: &[egui::epaint::ClippedShape],
    name: &str,
) -> Vec<(usize, egui::Rect, f32, egui::Stroke)> {
    let text = name_rect(shapes, name);
    shapes
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match &c.shape {
            egui::Shape::Rect(r)
                if r.stroke.width > 0.0
                    && r.rect.contains_rect(text)
                    && r.rect.height() < text.height() * 4.0 =>
            {
                Some((i, r.rect, f32::from(r.corner_radius.nw), r.stroke))
            }
            _ => None,
        })
        .collect()
}

/// 이름 글자를 그린 도형 순번.
fn name_index(shapes: &[egui::epaint::ClippedShape], name: &str) -> usize {
    shapes
        .iter()
        .position(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == name))
        .unwrap_or_else(|| panic!("{name} 이 칠해진다"))
}

/// 링: 행 포커스 행에만, 행을 다 그린 뒤, 행 높이 그대로 안쪽 `fp-row-focus-ring-width` 선과 `radius-sm` 모서리로
/// 그린다. 색은 바탕 칸(쉼·hover `fp-row-focus-ring`, 선택 `-selected`, 선택+hover `-selected-hover`)이다.
#[test]
fn the_row_focus_ring_is_an_inset_line_inked_by_the_row_ground() {
    for th in [
        latte(),
        Theme::with_colors(tasty_themes::mocha_fallback_colors(), false),
    ] {
        let (rest, selected, selected_hover) = (
            th.fp_row_focus_ring().to_egui(),
            th.fp_row_focus_ring_selected().to_egui(),
            th.fp_row_focus_ring_selected_hover().to_egui(),
        );
        assert!(rest != selected && selected != selected_hover && rest != selected_hover);
        let at = name_rect(&paint(&th, "", None), FOLDER).center();
        let row_h = row_height(&th).value();
        for (case, selection, pointer, ink) in [
            ("쉼", "", None, rest),
            ("hover", "", Some(at), rest),
            ("선택", FOLDER, None, selected),
            ("선택+hover", FOLDER, Some(at), selected_hover),
        ] {
            let shapes = paint_focused(&th, selection, pointer, Some((FOLDER, true)));
            let rings = strokes_on_row(&shapes, FOLDER);
            assert_eq!(rings.len(), 1, "{case}: 링 하나 {rings:?}");
            let (i, rect, radius, stroke) = rings[0];
            assert_eq!(stroke.color, ink, "{case}: 링 색");
            assert_eq!(
                stroke.width,
                th.fp_row_focus_ring_width().value(),
                "{case}: 선 굵기"
            );
            assert_eq!(radius, th.corner_radius_sm.value(), "{case}: 모서리");
            // 행 사각형은 픽셀 격자에 맞춰 반올림돼 계산값과 1px 안에서 다르다.
            assert!(
                (rect.height() - row_h).abs() < 0.5,
                "{case}: 행 높이 그대로 {} vs {row_h}",
                rect.height()
            );
            assert!(
                i > name_index(&shapes, FOLDER),
                "{case}: 행 내용 뒤에 그린다"
            );
            // 같은 행 상태에서 링을 끄면 레이아웃이 그대로다.
            let off = paint_focused(&th, selection, pointer, Some((FOLDER, false)));
            assert!(
                strokes_on_row(&off, FOLDER).is_empty(),
                "{case}: 링을 끄면 없다"
            );
            assert_eq!(
                name_rect(&off, FOLDER),
                name_rect(&shapes, FOLDER),
                "{case}: 위치 불변"
            );
            assert!(
                strokes_on_row(&shapes, FILE).is_empty(),
                "{case}: 다른 행엔 없다"
            );
        }
    }
}

/// hover 행의 크기·날짜는 `fp-row-meta-fg-hover` 잉크다. Latte 는 text-muted 와 text-secondary 가 같은 값이라
/// 두 잉크가 갈리는 Mocha 에서도 잰다.
#[test]
fn the_hovered_row_meta_reads_the_hover_meta_token() {
    for th in [
        latte(),
        Theme::with_colors(tasty_themes::mocha_fallback_colors(), false),
    ] {
        let at = name_rect(&paint(&th, "", None), FILE).center();
        let shapes = paint(&th, "", Some(at));
        let text = name_rect(&shapes, FILE);
        let inks: Vec<egui::Color32> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == "1 KB"
                        && (t.pos.y + t.galley.size().y * 0.5 - text.center().y).abs() < 1.0 =>
                {
                    Some(t.fallback_color)
                }
                _ => None,
            })
            .collect();
        assert_eq!(inks, vec![th.fp_row_meta_fg_hover().to_egui()]);
    }
    let mocha = Theme::with_colors(tasty_themes::mocha_fallback_colors(), false);
    assert_ne!(
        mocha.fp_row_meta_fg_hover().to_egui(),
        mocha.text_secondary().to_egui(),
        "Mocha 에서 hover 메타와 text-secondary 가 갈린다"
    );
}

/// 선택 막대: 선택 행마다 `accent-primary-on-active`, 폭 `selection-edge-width`, 행 높이 그대로, 행 바탕(선택 · 선택+hover)
/// 위에 그린다.
#[test]
fn the_selection_bar_is_on_active_over_the_row_ground() {
    for th in [
        latte(),
        Theme::with_colors(tasty_themes::mocha_fallback_colors(), false),
    ] {
        let at = name_rect(&paint(&th, "", None), FOLDER).center();
        let row_h = row_height(&th).value();
        for (case, pointer) in [("선택", None), ("선택+hover", Some(at))] {
            let shapes = paint(&th, FOLDER, pointer);
            let text = name_rect(&shapes, FOLDER);
            let on_row = |r: &egui::Rect| {
                (r.height() - row_h).abs() < 0.5 && r.y_range().contains(text.center().y)
            };
            let bars: Vec<(usize, egui::Color32)> = shapes
                .iter()
                .enumerate()
                .filter_map(|(i, c)| match &c.shape {
                    egui::Shape::Rect(r)
                        if r.brush.is_none()
                            && on_row(&r.rect)
                            && (r.rect.width() - th.selection_edge_width.value()).abs() < 0.01 =>
                    {
                        Some((i, r.fill))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                bars,
                vec![(
                    bars.first().map_or(0, |b| b.0),
                    th.accent_primary_on_active().to_egui()
                )],
                "{case}: 막대 하나"
            );
            let ground = shapes
                .iter()
                .position(|c| {
                    matches!(&c.shape, egui::Shape::Rect(r)
                        if r.brush.is_none() && on_row(&r.rect) && r.rect.width() > text.width()
                            && r.fill != egui::Color32::TRANSPARENT)
                })
                .expect("행 바탕");
            assert!(bars[0].0 > ground, "{case}: 막대는 바탕 위");
            let other = paint(&th, FILE, pointer);
            assert!(
                other
                    .iter()
                    .all(|c| !matches!(&c.shape, egui::Shape::Rect(r)
                    if r.fill == th.accent_primary_on_active().to_egui()
                        && (r.rect.width() - th.selection_edge_width.value()).abs() < 0.01
                        && r.rect.y_range().contains(name_rect(&other, FOLDER).center().y))),
                "{case}: 고르지 않은 행엔 막대가 없다"
            );
        }
    }
}

/// hover 행 이름은 text-secondary-raised, 선택+hover 행 이름은 text-on-active-hover 다.
#[test]
fn the_hovered_row_name_reads_the_raised_label_ink() {
    for th in [
        latte(),
        Theme::with_colors(tasty_themes::mocha_fallback_colors(), false),
    ] {
        let at = name_rect(&paint(&th, "", None), FILE).center();
        for (case, selection, want) in [
            ("hover", "", th.text_secondary_raised()),
            ("선택+hover", FILE, th.text_on_active_hover()),
        ] {
            let shapes = paint(&th, selection, Some(at));
            let inks: Vec<egui::Color32> = shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == FILE => Some(t.fallback_color),
                    _ => None,
                })
                .collect();
            assert_eq!(inks, vec![want.to_egui()], "{case}");
        }
    }
}
