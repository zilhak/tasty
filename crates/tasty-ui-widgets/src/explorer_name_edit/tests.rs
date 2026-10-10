use super::*;

fn theme() -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
}

/// 편집 줄을 두 프레임 그려 (줄, 입력 칸) rect 를 돌려준다.
fn rects(layout: ExplorerNameLayout) -> (egui::Rect, egui::Rect) {
    rects_in(layout, ExplorerNameOptions::default())
}

fn rects_in(
    layout: ExplorerNameLayout,
    options: ExplorerNameOptions<'_>,
) -> (egui::Rect, egui::Rect) {
    let theme = theme();
    let ctx = egui::Context::default();
    let mut edit = ExplorerNameEdit::new("New folder".into(), 0..10);
    let mut out = None;
    for _ in 0..2 {
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    // 넓은 입력이 왼쪽으로 나올 자리를 두고 그린다.
                    let at = ui.max_rect().min + egui::Vec2::X * 120.0;
                    let ui = &mut ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_size(at, egui::vec2(400.0, 300.0))),
                    );
                    let (row, field, _) = explorer_name_row_in(
                        ui,
                        &theme,
                        layout,
                        options,
                        tasty_icons::FOLDER,
                        &mut edit,
                        false,
                    );
                    out = Some((row, field));
                });
        }));
    }
    out.expect("drawn")
}

#[test]
fn a_detail_editor_is_one_table_row_and_stays_in_the_name_column() {
    let theme = theme();
    let (row, field) = rects(ExplorerNameLayout::Detail {
        inset: 10.0,
        name_width: 200.0,
    });
    assert_eq!(row.height(), theme.table_cell_height().value());
    assert!(field.right() <= row.left() + 200.0, "{field:?}");
}

#[test]
fn a_list_editor_grows_the_row_to_the_table_row_height() {
    let theme = theme();
    let (row, field) = rects(ExplorerNameLayout::List);
    assert!(theme.table_cell_height().value() > theme.tree_row_height().value());
    assert_eq!(row.height(), theme.table_cell_height().value());
    // 글리프 가운데가 아래 tree_row 아이콘 가운데와 같다. 입력 칸은 글리프 오른쪽 간격 뒤에서 시작한다.
    let glyph_right = crate::tree_row_icon_center(&theme) + theme.icon_glyph_size_md.value() * 0.5;
    assert_eq!(
        field.left() - row.left(),
        glyph_right + theme.spacing_sm.value()
    );
}

#[test]
fn a_field_with_the_in_folder_caption_stops_at_the_create_field_width() {
    let theme = theme();
    let options = ExplorerNameOptions {
        caption: Some("in mockup-exports"),
        ..Default::default()
    };
    let max = theme.explorer_create_field_max_width().value();
    for layout in [
        ExplorerNameLayout::List,
        ExplorerNameLayout::Detail {
            inset: 10.0,
            name_width: 400.0,
        },
    ] {
        let (_, field) = rects_in(layout, options);
        assert_eq!(field.width(), max, "{layout:?}");
        let (_, plain) = rects(layout);
        assert!(plain.width() > max, "{layout:?}: no caption, no cap");
    }
}

#[test]
fn a_grid_editor_centres_a_field_wider_than_the_cell() {
    let theme = theme();
    let cell = egui::vec2(80.0, 100.0);
    let (rect, field) = rects(ExplorerNameLayout::Grid { cell, slot: 16.0 });
    assert_eq!(rect.size(), cell);
    assert_eq!(field.width(), theme.field_width_md.value());
    assert!((field.center().x - rect.center().x).abs() < 0.5);
}

#[test]
fn the_first_frame_takes_the_initial_selection() {
    let theme = theme();
    let ctx = egui::Context::default();
    let mut edit = ExplorerNameEdit::new("untitled.txt".into(), 0..8);
    let mut field_id = None;
    for _ in 0..3 {
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                explorer_name_row(
                    ui,
                    &theme,
                    ExplorerNameLayout::List,
                    tasty_icons::FILE,
                    &mut edit,
                    false,
                );
                field_id = ctx.memory(|m| m.focused());
            });
        }));
    }
    assert!(edit.initial_selection.is_none());
    let id = field_id.expect("the field has focus");
    let state = egui::TextEdit::load_state(&ctx, id).expect("state");
    let range = state.cursor.char_range().expect("range");
    assert_eq!(
        (
            range.primary.index.min(range.secondary.index),
            range.primary.index.max(range.secondary.index)
        ),
        (0, 8)
    );
}

#[test]
fn a_grid_field_at_the_list_edge_moves_inside_the_visible_area() {
    let r = |a: f32, b: f32| egui::Rangef::new(a, b);
    assert_eq!(inside_shift(r(10.0, 50.0), r(0.0, 100.0)), 0.0);
    assert_eq!(inside_shift(r(-20.0, 140.0), r(0.0, 300.0)), 20.0);
    assert_eq!(inside_shift(r(200.0, 360.0), r(0.0, 300.0)), -60.0);
    // 영역이 입력보다 좁으면 왼쪽 끝을 맞춘다.
    assert_eq!(inside_shift(r(-20.0, 140.0), r(0.0, 100.0)), 20.0);
    assert_eq!(inside_shift(r(30.0, 190.0), r(0.0, 100.0)), -30.0);
}
